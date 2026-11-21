//! 场景树按场景根及地形清单顺序索引已载入画布，只缓存各来源的行数区间。

use super::catalog::{PrivateVegetationCatalog, VegetationCatalogAssets, parked_vegetation};
use bevy::asset::AssetId;
use crate::authoring::{EditorAuthoringSession, EditorSceneDraft, terrain_documents::{root_vegetation, root_water}};
use feathrise_scene_content::{SceneId, SceneTerrainObject, SceneVegetationAsset, TerrainId};

#[derive(Clone, Copy)]
struct SourceEnd {
    terrain_index: Option<usize>,
    end: usize,
}

#[derive(Default)]
struct Sources {
    water: Vec<SourceEnd>,
    vegetation: Vec<SourceEnd>,
}

impl Sources {
    fn add(&mut self, draft: &EditorSceneDraft, terrain_index: Option<usize>,
        vegetation: Option<&SceneVegetationAsset>) {
        let terrain = terrain_index.map(|index| &draft.manifest.terrains.as_ref().unwrap()[index]);
        let id = terrain.map(|terrain| &terrain.id);
        let append = |ends: &mut Vec<SourceEnd>, count| {
            if count > 0 {
                let end = ends.last().map_or(0, |last| last.end) + count;
                ends.push(SourceEnd { terrain_index, end });
            }
        };
        if terrain.is_none_or(|terrain| complete_canvas(draft, terrain)) {
            append(&mut self.water, root_water::listed_source(draft, id).map_or(0, root_water::feature_count));
        }
        append(&mut self.vegetation, vegetation.map_or(0, |vegetation| {
            vegetation.authoring.as_ref().map_or(vegetation.layers.len(), |authoring| authoring.layers.len())
        }));
    }
}

fn complete_canvas(draft: &EditorSceneDraft, terrain: &SceneTerrainObject) -> bool {
    let store = &draft.terrain_documents;
    !store.detached && (store.active.as_ref().is_some_and(|active| active.definition.as_ref() == terrain)
        || store.values.get(&terrain.id).filter(|canvas| !canvas.vegetation_only)
            .and_then(|canvas| canvas.identity.as_ref()).is_some_and(|identity| identity.definition.as_ref() == terrain))
}

/// 根植被可能已停放，私有图层则由其稳定地形 ID 解引用。
pub(in crate::ui) fn vegetation_source<'a>(
    draft: &'a EditorSceneDraft, terrain: Option<&TerrainId>,
) -> Option<&'a SceneVegetationAsset> {
    let Some(id) = terrain else { return root_vegetation::source(draft)?.vegetation; };
    let store = &draft.terrain_documents;
    if store.detached { return None; }
    if store.active.as_ref().is_some_and(|active| &active.definition.id == id) {
        draft.vegetation.as_ref()
    } else {
        store.values.get(id).filter(|canvas| !canvas.vegetation_only)?.vegetation.as_ref()
    }
}

#[derive(PartialEq, Eq)]
struct Identity {
    manifest: Option<(usize, usize)>,
    revisions: (u64, u64, u64, u64),
    private_canvases: usize,
    catalog_revision: u64,
}

#[derive(Default)]
struct Build {
    sources: Sources,
    next: usize,
}

#[derive(Default)]
pub(super) struct LayerInventory {
    scene: Option<SceneId>,
    identity: Option<Identity>,
    published: Sources,
    build: Option<Build>,
    catalog: PrivateVegetationCatalog,
}

impl LayerInventory {
    /// 只按作者数据修订换代，逐帧处理来源元数据。换画布期间沿用上一完整区间，行身份按来源重新核对。
    pub(super) fn sync(&mut self, session: &EditorAuthoringSession, draft: &EditorSceneDraft,
        assets: Option<&VegetationCatalogAssets>) {
        let manifest = draft.manifest.terrains.as_ref().map(|objects| (objects.as_ptr() as usize, objects.len()));
        let scene_changed = self.scene.as_ref() != Some(&draft.scene_id);
        let scope_changed = scene_changed || self.identity.as_ref().is_none_or(|old| old.manifest != manifest);
        if scope_changed {
            self.published = Sources::default();
            self.published.add(draft, None, vegetation_source(draft, None));
            if !self.catalog.reset() { return; }
        }
        if let Some(assets) = assets { self.catalog.sync(draft, assets); }
        let identity = Identity {
            manifest,
            revisions: (session.terrain_revision, session.water_revision,
                session.vegetation_structure_revision, session.vegetation_compiled_structure_revision),
            private_canvases: draft.terrain_documents.values.len()
                + usize::from(draft.terrain_documents.active.is_some()),
            catalog_revision: self.catalog.revision,
        };
        if scene_changed || self.identity.as_ref() != Some(&identity) {
            if scene_changed { self.scene = Some(draft.scene_id.clone()); }
            self.identity = Some(identity);
            self.build = Some(Build::default());
        }
        let Some(build) = self.build.as_mut() else { return; };
        let source_count = 1 + draft.manifest.terrains.as_ref().map_or(0, |objects| objects.len());
        // 沿用场景树轻量行投影额度。每项只核对一个地形并读数组长度，像素及层级数据不进入缓存。
        let end = source_count.min(build.next + crate::ui::gltf_browser::MAX_GLTF_ENTRY_ROWS_PER_FRAME);
        for index in build.next..end {
            let (vegetation, _) = vegetation_at(&self.catalog, draft, index.checked_sub(1), assets);
            build.sources.add(draft, index.checked_sub(1), vegetation);
        }
        build.next = end;
        if end == source_count {
            self.published = self.build.take().unwrap().sources;
        }
    }

    pub(super) fn vegetation_at<'a>(&self, draft: &'a EditorSceneDraft, source: Option<usize>,
        assets: Option<&'a VegetationCatalogAssets<'_>>)
        -> (Option<&'a SceneVegetationAsset>, Option<(usize, AssetId<SceneVegetationAsset>)>) {
        vegetation_at(&self.catalog, draft, source, assets)
    }

    pub(super) fn count(&self, water: bool) -> usize {
        self.sources(water).last().map_or(0, |source| source.end)
    }

    pub(super) fn source(&self, water: bool, flat_index: usize) -> Option<(Option<usize>, usize)> {
        let sources = self.sources(water);
        let index = sources.partition_point(|source| source.end <= flat_index);
        Some((sources.get(index)?.terrain_index,
            flat_index - index.checked_sub(1).map_or(0, |prev| sources[prev].end)))
    }

    fn sources(&self, water: bool) -> &[SourceEnd] {
        if water { &self.published.water } else { &self.published.vegetation }
    }
}

fn vegetation_at<'a>(
    catalog: &PrivateVegetationCatalog, draft: &'a EditorSceneDraft, index: Option<usize>,
    assets: Option<&'a VegetationCatalogAssets<'_>>,
) -> (Option<&'a SceneVegetationAsset>, Option<(usize, AssetId<SceneVegetationAsset>)>) {
    let Some(index) = index else { return (vegetation_source(draft, None), None); };
    let Some(terrain) = draft.manifest.terrains.as_ref().and_then(|objects| objects.get(index))
        .filter(|_| !draft.terrain_documents.detached) else { return (None, None); };
    if complete_canvas(draft, terrain) {
        return (vegetation_source(draft, Some(&terrain.id)), None);
    }
    let Some((id, definition)) = assets.and_then(|assets| catalog.get(draft, terrain, index, assets))
        else { return (None, None); };
    (Some(parked_vegetation(draft, terrain).unwrap_or(definition)), Some((index, id)))
}
