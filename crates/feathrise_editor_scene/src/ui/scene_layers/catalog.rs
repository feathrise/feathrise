//! 场景树借用正式植被定义列出冷来源，不读取作者密度、几何或换入画布。

use crate::{authoring::EditorSceneDraft, ui::scene_context_menu::SceneObjectKey};
use bevy::{asset::{AssetPath, LoadState}, prelude::*, tasks::{AsyncComputeTaskPool, Task, futures::check_ready}};
use feathrise_scene_content::{SceneTerrainObject, SceneVegetationAsset, SCENE_SCHEMA_VERSION};

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct VegetationCatalogAssets<'w> {
    pub(crate) server: Option<Res<'w, AssetServer>>,
    pub(super) definitions: Option<Res<'w, Assets<SceneVegetationAsset>>>,
}

impl VegetationCatalogAssets<'_> {
    pub(crate) fn row_definition<'a>(&'a self, draft: &'a EditorSceneDraft, key: &SceneObjectKey) -> Option<&'a SceneVegetationAsset> {
        let SceneObjectKey::Vegetation { terrain, cold, .. } = key else { return None; };
        let Some((index, asset)) = cold else { return super::inventory::vegetation_source(draft, terrain.as_ref()); };
        let owner = draft.manifest.terrains.as_ref()?.get(*index)
            .filter(|owner| terrain.as_ref() == Some(&owner.id))?;
        let catalog = self.definition(draft, owner, *asset)?;
        Some(parked_vegetation(draft, owner).unwrap_or(catalog))
    }

    /// 冻结的句柄必须仍由当前来源路径持有，加载期间的旧定义不能启动写侧。
    pub(super) fn definition(
        &self, draft: &EditorSceneDraft, source: &SceneTerrainObject, id: AssetId<SceneVegetationAsset>,
    ) -> Option<&SceneVegetationAsset> {
        let path = AssetPath::try_parse(source.vegetation_path.as_deref()?).ok()?;
        let handle = self.server.as_ref()?.get_handle::<SceneVegetationAsset>(path)?;
        if handle.id() != id { return None; }
        self.get(draft, id)
    }

    fn get(&self, draft: &EditorSceneDraft, id: AssetId<SceneVegetationAsset>) -> Option<&SceneVegetationAsset> {
        self.definitions.as_ref()?.get(id)
            .filter(|asset| asset.scene_id == draft.scene_id && asset.schema_version == SCENE_SCHEMA_VERSION)
    }
}

pub(super) fn parked_vegetation<'a>(draft: &'a EditorSceneDraft, terrain: &SceneTerrainObject) -> Option<&'a SceneVegetationAsset> {
    draft.terrain_documents.values.get(&terrain.id)
        .filter(|canvas| canvas.vegetation_only && canvas.identity.as_ref()
            .is_some_and(|identity| identity.definition.as_ref() == terrain))?.vegetation.as_ref()
}

#[derive(Default)]
pub(super) struct PrivateVegetationCatalog {
    handles: Vec<Option<Handle<SceneVegetationAsset>>>,
    pending: Option<Handle<SceneVegetationAsset>>,
    retiring: Option<Task<()>>,
    pub(super) revision: u64,
}

impl PrivateVegetationCatalog {
    pub(super) fn reset(&mut self) -> bool {
        if self.retiring.as_mut().and_then(check_ready).is_some() {
            self.retiring = None;
        }
        if self.retiring.is_some() { return false; }
        if !self.handles.is_empty() {
            let old = std::mem::take(&mut self.handles);
            self.retiring = Some(AsyncComputeTaskPool::get().spawn(async move { drop(old) }));
        }
        // 上一场景未完成的文件仍占用请求槽，直到资产系统报告真实完成。
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub(super) fn sync(
        &mut self, draft: &EditorSceneDraft, assets: &VegetationCatalogAssets,
    ) {
        let (Some(server), Some(definitions), Some(objects)) =
            (&assets.server, &assets.definitions, &draft.manifest.terrains) else { return; };
        if definitions.is_changed() { self.revision = self.revision.wrapping_add(1); }
        if let Some(handle) = self.pending.as_ref() {
            if !matches!(server.get_load_state(handle.id()), Some(LoadState::Loaded | LoadState::Failed(_))) {
                return;
            }
            self.pending = None;
        }
        let start = self.handles.len();
        // 正式地形已有的定义句柄直接共用。未请求的文件只保留一份在途加载。
        let end = (start + crate::ui::gltf_browser::MAX_GLTF_ASSET_DEMAND_PATHS_PER_FRAME).min(objects.len());
        for object in &objects[start..end] {
            let handle = object.vegetation_path.as_deref().and_then(|path| AssetPath::try_parse(path).ok()).map(|path| {
                server.get_handle(path.clone()).unwrap_or_else(|| server.load(path.into_owned()))
            });
            if let Some(handle) = &handle
                && definitions.get(handle).is_none()
                && !matches!(server.get_load_state(handle.id()), Some(LoadState::Failed(_)))
            {
                self.pending = Some(handle.clone());
            }
            self.handles.push(handle);
            if self.pending.is_some() { break; }
        }
        if start != self.handles.len() { self.revision = self.revision.wrapping_add(1); }
    }

    pub(super) fn get<'a>(
        &self, draft: &EditorSceneDraft, terrain: &SceneTerrainObject, index: usize, assets: &'a VegetationCatalogAssets,
    ) -> Option<(AssetId<SceneVegetationAsset>, &'a SceneVegetationAsset)> {
        let id = self.handles.get(index)?.as_ref()?.id();
        Some((id, assets.definition(draft, terrain, id)?))
    }
}
