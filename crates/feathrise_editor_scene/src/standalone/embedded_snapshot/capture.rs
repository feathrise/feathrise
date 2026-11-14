//! 元数据按固定预算跨帧冻结；像素、网格、多地形清单和高度树只共享不可变 Arc。

use crate::authoring::{
    EditorAuthoringSession, EditorSceneDraft, GlobalEnvironmentAuthoringState,
    terrain_documents::TerrainCanvas,
};
use bevy::prelude::*;
use feathrise_scene_content::*;
use std::{
    collections::BTreeMap,
    ops::Bound::{Excluded, Unbounded},
    sync::Arc,
};

const RECORDS_PER_FRAME: usize = 64;
const CANVASES_PER_FRAME: usize = 4;
const HEIGHT_SOURCES_PER_FRAME: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourceIdentity {
    pub scene_id: SceneId,
    pub active: Option<TerrainId>,
    revisions: [u64; 15],
}

pub(super) fn source_identity(world: &World) -> Option<SourceIdentity> {
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    Some(SourceIdentity {
        scene_id: draft.scene_id.clone(),
        active: draft
            .terrain_documents
            .active
            .as_ref()
            .map(|active| active.definition.id.clone()),
        revisions: [
            session.revision,
            session.placement_derived_pose_revision,
            session.scene_objects_revision,
            session.portal_revision,
            session.terrain_revision,
            session.surface_revision,
            session.vegetation_revision,
            session.vegetation_compiled_revision,
            session.vegetation_compiled_structure_revision,
            session.water_revision,
            session.environment_revision,
            session.boundary_revision,
            draft.terrain_documents.root_water_revision,
            world
                .get_resource::<GlobalEnvironmentAuthoringState>()
                .map_or(0, |global| global.revision),
            world
                .get_resource::<SparseHeightLiveOverride>()
                .map_or(0, |height| height.revision()),
        ],
    })
}

pub(super) struct Capture {
    pub source: SourceIdentity,
    pub entry: SceneCatalogEntry,
    pub manifest: SceneManifestAsset,
    pub placements: ScenePlacementsAsset,
    pub portals: ScenePortalsAsset,
    pub boundaries: Option<SceneBoundariesAsset>,
    pub active: CanvasCapture,
    pub root_water: WaterCapture,
    pub root_vegetation: Option<(SceneVegetationAsset, BTreeMap<String, Arc<Vec<u8>>>)>,
    pub stored: Vec<Arc<TerrainCanvas>>,
    pub heights: Vec<(Option<TerrainId>, SparseHeightLiveSnapshot)>,
    pub global: Option<WorldEnvironmentAsset>,
    pub extra_files: Vec<(String, Arc<Vec<u8>>)>,
    boundary_capture: SceneBoundaryCapture,
    boundaries_done: bool,
    stored_cursor: Option<TerrainId>,
    stored_done: bool,
    height_cursor: Option<Entity>,
    heights_done: bool,
}

impl Capture {
    pub fn new(world: &World, source: SourceIdentity) -> Result<Self, String> {
        let draft = world
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .ok_or("作者草稿已关闭")?;
        if !draft.manifest.is_data_driven() {
            return Err(
                "工程视口当前需要数据驱动场景；此场景仍可用编辑器 3D 与保存后运行".into(),
            );
        }
        if draft.domain_dirty() {
            return Err("当前场景含尚未接入内存快照的外部作者草稿，请先保存该领域文档".into());
        }
        let root = crate::authoring::terrain_documents::root_water::source(draft);
        let container = draft.manifest.terrains.is_some();
        let root_vegetation = crate::authoring::terrain_documents::root_vegetation::source(draft)
            .filter(|source| !container || source.has_document());
        let entry = SceneCatalogEntry {
            id: draft.scene_id.clone(),
            display_name: draft.manifest.display_name.clone(),
            folder: draft.folder.clone(),
            published: true,
            placements_glb: false, // 跨进程快照是内存覆盖，正式 GLB 网格继续按原资产路径加载。
            surface_path: (!container && draft.surface.is_some())
                .then(|| format!("{}/{}", draft.folder, SCENE_SURFACE_FILE_NAME)),
            vegetation_path: root_vegetation.as_ref().is_some_and(|source| source.vegetation.is_some())
                .then(|| format!("{}/{}", draft.folder, SCENE_VEGETATION_FILE_NAME)),
            water_path: root.water.map(|_| root.asset_path.to_owned()),
            boundary_path: draft
                .boundaries
                .as_ref()
                .map(|_| format!("{}/{}", draft.folder, SCENE_BOUNDARIES_FILE_NAME)),
            characters_path: world
                .get_resource::<SceneContentRegistry>()
                .and_then(|registry| registry.handles(&draft.scene_id))
                .and_then(|handles| handles.characters_path.clone()),
        };
        // terrain/sub-map/paint-group 集合已有 Arc；这里只克隆固定清单头，asset_roots 单独按预算推进。
        let manifest = manifest_without_roots(&draft.manifest);
        let mut heights = Vec::new();
        // 空树仍携带合法初始live修订；只发修改页会让工程端无法绑定未雕刻场景的首个刷圈。
        if let Some(live) = world
            .get_resource::<SparseHeightLiveOverride>()
            .filter(|live| live.scene_id() == Some(&draft.scene_id))
        {
            heights.push((source.active.clone(), live.snapshot()));
        }
        let extra_files = world
            .get_resource::<crate::authoring::volume_sculpt::VolumeAuthoringState>()
            .map(|volume| {
                volume
                    .save_files()
                    .into_iter()
                    .map(|file| (file.path, file.bytes))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            source,
            entry,
            manifest,
            placements: ScenePlacementsAsset {
                schema_version: draft.placements.schema_version,
                scene_id: draft.scene_id.clone(),
                placements: Vec::new(),
            },
            portals: ScenePortalsAsset {
                schema_version: draft.portals.schema_version,
                scene_id: draft.scene_id.clone(),
                portals: Vec::new(),
            },
            boundaries: None,
            boundary_capture: SceneBoundaryCapture::default(),
            boundaries_done: false,
            active: CanvasCapture::new(draft),
            root_water: WaterCapture::new(root.water),
            root_vegetation: container.then(|| root_vegetation.and_then(|source|
                source.vegetation.map(|definition| (definition.clone(), source.densities.clone())))).flatten(),
            stored: Vec::new(),
            stored_cursor: None,
            stored_done: false,
            heights,
            height_cursor: None,
            heights_done: false,
            global: world
                .get_resource::<GlobalEnvironmentAuthoringState>()
                .map(|global| global.draft.clone()),
            extra_files,
        })
    }

    pub fn step(&mut self, world: &World) -> Result<bool, String> {
        let draft = world
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .ok_or("作者草稿已关闭")?;
        append(
            &mut self.manifest.asset_roots,
            &draft.manifest.asset_roots,
            RECORDS_PER_FRAME,
        );
        // 整网格水体覆盖没有数量上限；记录头与覆盖分别限额，不能整条 clone 可变长记录。
        let placements_done = append_placements(
            &mut self.placements.placements,
            &draft.placements.placements,
        )?;
        append(
            &mut self.portals.portals,
            &draft.portals.portals,
            RECORDS_PER_FRAME,
        );
        if !self.boundaries_done {
            self.boundaries_done = draft
                .boundaries
                .as_ref()
                .is_none_or(|source| self.boundary_capture.step(source, 512, 16));
            if self.boundaries_done {
                self.boundaries = self.boundary_capture.take();
            }
        }
        // 容器的根槽可能仍携带初始化或切换后的植被描述，但没有active时不会被编码。
        // 不得要求这个非来源槽拥有完整密度；根水始终由下面的独立路径捕获。
        let active_done = if self.manifest.terrains.is_none() || self.source.active.is_some() {
            self.active.step(draft)?
        } else {
            true
        };
        let root = crate::authoring::terrain_documents::root_water::source(draft);
        let water_done = self.root_water.step(root.water, root.masks)?;
        if !self.stored_done {
            for _ in 0..CANVASES_PER_FRAME {
                let next = self.stored_cursor.as_ref().map_or_else(
                    || draft.terrain_documents.values.iter().next(),
                    |after| {
                        draft
                            .terrain_documents
                            .values
                            .range((Excluded(after), Unbounded))
                            .next()
                    },
                );
                let Some((id, canvas)) = next else {
                    self.stored_done = true;
                    break;
                };
                self.stored_cursor = Some(id.clone());
                if self.source.active.as_ref() != Some(id) {
                    self.stored.push(canvas.clone());
                }
            }
        }
        if !self.heights_done {
            type Heights =
                feathrise_scene_core::residency::InstanceResourceTable<SparseHeightLiveOverride>;
            if let Some(table) = world.get_resource::<Heights>() {
                for _ in 0..HEIGHT_SOURCES_PER_FRAME {
                    let next = self.height_cursor.as_ref().map_or_else(
                        || table.values.iter().next(),
                        |after| table.values.range((Excluded(after), Unbounded)).next(),
                    );
                    let Some((entity, live)) = next else {
                        self.heights_done = true;
                        break;
                    };
                    self.height_cursor = Some(*entity);
                    let Some(instance) = world.get::<feathrise_scene::SubsceneInstance>(*entity)
                    else {
                        continue;
                    };
                    let Some(id) = instance.path.terrain_id.as_ref() else {
                        continue;
                    };
                    if instance.path.root_scene_id == draft.scene_id
                        && instance.path.instances.is_empty()
                        && self.source.active.as_ref() != Some(id)
                        && world
                            .get::<feathrise_scene::SubsceneActive>(*entity)
                            .is_some()
                        && live.scene_id() == Some(&draft.scene_id)
                    {
                        self.heights.push((Some(id.clone()), live.snapshot()));
                    }
                }
            } else {
                self.heights_done = true;
            }
        }
        Ok(
            self.manifest.asset_roots.len() == draft.manifest.asset_roots.len()
                && placements_done
                && self.portals.portals.len() == draft.portals.portals.len()
                && self.boundaries_done
                && active_done
                && water_done
                && self.stored_done
                && self.heights_done,
        )
    }
}

fn append<T: Clone>(target: &mut Vec<T>, source: &[T], count: usize) {
    let start = target.len();
    let end = (start + count).min(source.len());
    target.extend_from_slice(&source[start..end]);
}

fn append_placements(
    target: &mut Vec<ScenePlacementRecord>,
    source: &[ScenePlacementRecord],
) -> Result<bool, String> {
    let mut index = target.len();
    if target
        .last()
        .is_some_and(|last| last.water_surfaces.len() < source[index - 1].water_surfaces.len())
    {
        index -= 1;
    }
    let mut surface_budget = 16;
    for _ in 0..RECORDS_PER_FRAME {
        let Some(record) = source.get(index) else {
            return Ok(true);
        };
        if index == target.len() {
            // 洞口格式仍有32个的真实固定上限，损坏草稿在复制前失败关闭。
            if record
                .terrain_patch
                .as_ref()
                .is_some_and(|patch| patch.cutouts.len() > MAX_TERRAIN_CUTOUTS)
            {
                return Err(format!("摆放 {} 的地形洞口超过正式格式预算", record.id));
            }
            target.push(ScenePlacementRecord {
                mesh_lod: record.mesh_lod,
                terrain_binding: record.terrain_binding.clone(),
                editor: record.editor.clone(),
                terrain_patch: record.terrain_patch.clone(),
                id: record.id.clone(),
                asset_path: record.asset_path.clone(),
                asset_target: record.asset_target,
                building_preset_id: record.building_preset_id.clone(),
                courtyard: record.courtyard.clone(),
                translation: record.translation,
                height_mode: record.height_mode,
                terrain_offset: record.terrain_offset,
                yaw_degrees: record.yaw_degrees,
                scale: record.scale,
                water_surfaces: Vec::new(),
            });
        }
        let output = &mut target[index].water_surfaces;
        let before = output.len();
        append(output, &record.water_surfaces, surface_budget);
        surface_budget -= output.len() - before;
        if output.len() != record.water_surfaces.len() {
            return Ok(false);
        }
        index += 1;
        if surface_budget == 0 {
            break;
        }
    }
    Ok(index == source.len())
}

fn manifest_without_roots(source: &SceneManifestAsset) -> SceneManifestAsset {
    SceneManifestAsset {
        schema_version: source.schema_version,
        id: source.id.clone(),
        display_name: source.display_name.clone(),
        notes: source.notes.clone(),
        runtime_scene: source.runtime_scene.clone(),
        world_min: source.world_min,
        world_max: source.world_max,
        default_spawn: source.default_spawn,
        environment: source.environment.clone(),
        terrain: source.terrain.clone(),
        terrains: source.terrains.clone(),
        puddles: source.puddles.clone(),
        asset_roots: Vec::new(),
        subscenes: source.subscenes.clone(),
        vegetation_paint_groups: source.vegetation_paint_groups.clone(),
    }
}

pub(super) struct CanvasCapture {
    pub folder: String,
    pub manifest: SceneManifestAsset,
    pub surface: Option<SceneSurfaceAsset>,
    pub weights: Option<Arc<Vec<u8>>>,
    pub indices: Option<Arc<Vec<u8>>>,
    pub puddles: Option<Arc<Vec<u8>>>,
    pub vegetation: Option<SceneVegetationAsset>,
    pub densities: BTreeMap<String, Arc<Vec<u8>>>,
    pub water: WaterCapture,
    pub water_path: String,
}

impl CanvasCapture {
    fn new(draft: &EditorSceneDraft) -> Self {
        Self {
            folder: draft.terrain_folder().to_owned(),
            manifest: manifest_without_roots(draft.terrain_manifest()),
            surface: draft.surface.as_ref().map(|surface| SceneSurfaceAsset {
                schema_version: surface.schema_version,
                scene_id: surface.scene_id.clone(),
                weight_source_path: surface.weight_source_path.clone(),
                weight_texture_path: surface.weight_texture_path.clone(),
                index_source_path: surface.index_source_path.clone(),
                index_texture_path: surface.index_texture_path.clone(),
                resolution: surface.resolution,
                layers: Vec::new(),
            }),
            weights: draft.surface_weights.clone(),
            indices: draft.surface_indices.clone(),
            puddles: draft.puddle_mask.clone(),
            vegetation: draft
                .vegetation
                .as_ref()
                .map(|vegetation| SceneVegetationAsset {
                    model_surfaces: vegetation.model_surfaces.clone(),
                    schema_version: vegetation.schema_version,
                    scene_id: vegetation.scene_id.clone(),
                    layers: Vec::new(),
                    authoring: if vegetation.model_surfaces.is_empty() { None } else { vegetation.authoring.clone() },
                }),
            densities: BTreeMap::new(),
            water: WaterCapture::new(draft.water.as_ref()),
            water_path: draft.water_asset_path.clone(),
        }
    }

    fn step(&mut self, draft: &EditorSceneDraft) -> Result<bool, String> {
        let mut complete = true;
        if let (Some(target), Some(source)) = (&mut self.surface, &draft.surface) {
            append(&mut target.layers, &source.layers, RECORDS_PER_FRAME);
            complete &= target.layers.len() == source.layers.len();
        }
        if let (Some(target), Some(source)) = (&mut self.vegetation, &draft.vegetation) {
            let start = target.layers.len();
            append(&mut target.layers, &source.layers, RECORDS_PER_FRAME);
            for layer in &target.layers[start..] {
                if let Some(density) = draft.vegetation_densities.get(&layer.id) {
                    self.densities.insert(layer.id.clone(), density.clone());
                } else {
                    return Err(format!(
                        "植被层 {} 尚未形成完整运行密度，保留上一有效视口",
                        layer.id
                    ));
                }
            }
            complete &= target.layers.len() == source.layers.len();
        }
        complete &= self.water.step(draft.water.as_ref(), &draft.water_masks)?;
        Ok(complete)
    }

    /// 只在后台使用：停放画布本身由 Arc 保有，不在作者帧复制其中所有层元数据。
    pub fn from_stored(canvas: &TerrainCanvas) -> Result<Self, String> {
        let identity = canvas.identity.as_ref().ok_or("停放画布缺少来源身份")?;
        Ok(Self {
            folder: identity.folder.clone(),
            manifest: identity.manifest.clone(),
            surface: canvas.surface.clone(),
            weights: canvas.surface_weights.clone(),
            indices: canvas.surface_indices.clone(),
            puddles: canvas.puddle_mask.clone(),
            vegetation: canvas.vegetation.clone().map(|mut vegetation| {
                if vegetation.model_surfaces.is_empty() { vegetation.authoring = None; }
                vegetation
            }),
            densities: canvas.vegetation_densities.clone(),
            water: WaterCapture {
                document: canvas.water.clone(),
                masks: canvas.water_masks.clone(),
            },
            water_path: canvas.water_asset_path.clone(),
        })
    }
}

pub(super) struct WaterCapture {
    pub document: Option<SceneWaterAsset>,
    pub masks: BTreeMap<String, Arc<Vec<u8>>>,
}
impl WaterCapture {
    fn new(source: Option<&SceneWaterAsset>) -> Self {
        Self {
            document: source.map(|water| SceneWaterAsset {
                schema_version: water.schema_version,
                scene_id: water.scene_id.clone(),
                layers: Vec::new(),
                rivers: Vec::new(),
                waterfalls: Vec::new(),
            }),
            masks: BTreeMap::new(),
        }
    }
    fn step(
        &mut self,
        source: Option<&SceneWaterAsset>,
        masks: &BTreeMap<String, Arc<Vec<u8>>>,
    ) -> Result<bool, String> {
        let (Some(target), Some(source)) = (&mut self.document, source) else {
            return Ok(true);
        };
        let start = target.layers.len();
        append(&mut target.layers, &source.layers, 16);
        // 正式水路没有单条点数上限；连同记录头按点分帧，不能把一条长河当作固定小工作。
        let rivers_done = append_paths(
            &mut target.rivers,
            &source.rivers,
            |river| SceneWaterRiver {
                id: river.id.clone(),
                label: river.label.clone(),
                source_layer_id: river.source_layer_id.clone(),
                target_layer_id: river.target_layer_id.clone(),
                points: Vec::new(),
                base_color_srgba: river.base_color_srgba,
                perceptual_roughness: river.perceptual_roughness,
            },
            |river| &river.points,
            |river| &mut river.points,
        );
        let waterfalls_done = append_paths(
            &mut target.waterfalls,
            &source.waterfalls,
            |waterfall| SceneWaterfall {
                id: waterfall.id.clone(),
                label: waterfall.label.clone(),
                source: waterfall.source.clone(),
                target: waterfall.target.clone(),
                points: Vec::new(),
                foam_strength: waterfall.foam_strength,
                base_color_srgba: waterfall.base_color_srgba,
                perceptual_roughness: waterfall.perceptual_roughness,
            },
            |waterfall| &waterfall.points,
            |waterfall| &mut waterfall.points,
        );
        for layer in &target.layers[start..] {
            let mask = masks
                .get(&layer.id)
                .ok_or_else(|| format!("水层 {} 尚未形成完整遮罩", layer.id))?;
            self.masks.insert(layer.id.clone(), mask.clone());
        }
        Ok(target.layers.len() == source.layers.len() && rivers_done && waterfalls_done)
    }
}

/// 半份路径只存在 Capture 私有槽；完整来源身份不变且所有点到齐，后台编码才取得所有权。
fn append_paths<T>(
    target: &mut Vec<T>,
    source: &[T],
    header: fn(&T) -> T,
    points: fn(&T) -> &Vec<SceneWaterSplinePoint>,
    points_mut: fn(&mut T) -> &mut Vec<SceneWaterSplinePoint>,
) -> bool {
    let mut index = target.len();
    if target
        .last()
        .is_some_and(|last| points(last).len() < points(&source[index - 1]).len())
    {
        index -= 1;
    }
    let mut budget = 256;
    for _ in 0..4 {
        let Some(record) = source.get(index) else {
            return true;
        };
        if index == target.len() {
            target.push(header(record));
        }
        let output = points_mut(&mut target[index]);
        let before = output.len();
        append(output, points(record), budget);
        budget -= output.len() - before;
        if output.len() != points(record).len() {
            return false;
        }
        index += 1;
        if budget == 0 {
            break;
        }
    }
    index == source.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_placement_with_many_mesh_water_bindings_cannot_bypass_the_frame_budget() {
        let mut record: ScenePlacementRecord = serde_json::from_value(serde_json::json!({
            "id": "many-water-meshes", "asset_path": "scenes/example.glb",
            "translation": [0.0, 0.0, 0.0], "yaw_degrees": 0.0, "scale": [1.0, 1.0, 1.0]
        }))
        .unwrap();
        record.water_surfaces = (0..517)
            .map(|index| ScenePlacementWaterSurface {
                mesh_index: index,
                node_path: vec![format!("node-{index}")],
                mesh_name: format!("mesh-{index}"),
                kind: ScenePlacementWaterKind::Horizontal,
                flow_speed: 1.0,
                height_offset: 0.0,
            })
            .collect();
        let source = [record];
        let mut output = Vec::new();
        assert!(!append_placements(&mut output, &source).unwrap());
        assert_eq!(output[0].water_surfaces.len(), 16);
        for _ in 0..31 {
            assert!(!append_placements(&mut output, &source).unwrap());
        }
        assert_eq!(output[0].water_surfaces.len(), 512);
        assert!(append_placements(&mut output, &source).unwrap());
        assert_eq!(output, source);
    }

    #[test]
    fn long_water_paths_are_frozen_in_bounded_chunks_without_publishing_partial_paths() {
        let mut water: SceneWaterAsset = serde_json::from_value(serde_json::json!({
            "schema_version": 2, "scene_id": "MAP-water-budget", "layers": [],
            "rivers": [{"id": "river", "label": "长河流", "points": []}],
            "waterfalls": []
        }))
        .unwrap();
        water.rivers[0].points = (0..1301)
            .map(|index| SceneWaterSplinePoint {
                id: format!("p-{index}"),
                position: [index as f32, 1.0, 0.0],
                width: 2.0,
                depth: 1.0,
                flow_speed: 1.0,
                outgoing: SceneWaterSegmentKind::River,
            })
            .collect();
        let mut capture = WaterCapture::new(Some(&water));
        let masks = BTreeMap::new();
        assert!(!capture.step(Some(&water), &masks).unwrap());
        assert_eq!(
            capture.document.as_ref().unwrap().rivers[0].points.len(),
            256
        );
        for _ in 0..4 {
            assert!(!capture.step(Some(&water), &masks).unwrap());
        }
        assert_eq!(
            capture.document.as_ref().unwrap().rivers[0].points.len(),
            1280
        );
        assert!(capture.step(Some(&water), &masks).unwrap());
        assert_eq!(capture.document.as_ref().unwrap().rivers, water.rivers);
    }
}
