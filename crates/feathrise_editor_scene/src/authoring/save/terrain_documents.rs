//! 对象画布保存投影只包含自己的作者域；集合更新在CPU池完成，所有文件复用唯一回滚事务。
use crate::authoring::{
    EditorSceneDraft,
    terrain_documents::{TerrainCanvas, TerrainDocumentStore},
};
use feathrise_scene_content::*;
use std::sync::Arc;

impl TerrainCanvas {
    pub(crate) fn snapshot(draft: &EditorSceneDraft) -> Self {
        Self {
            vegetation_only: false,
            identity: draft.terrain_documents.active.clone(),
            structural_dirty: draft.terrain_documents.active_dirty,
            terrain_mesh: draft.terrain_mesh.clone(),
            terrain_editing_index: draft.terrain_editing_index.clone(),
            terrain_source_sha256: draft.terrain_source_sha256.clone(),
            terrain_overwrite_authorized_sha256: draft.terrain_overwrite_authorized_sha256.clone(),
            heights: draft.heights.clone(),
            surface: draft.surface.clone(),
            surface_weights: draft.surface_weights.clone(),
            surface_indices: draft.surface_indices.clone(),
            puddle_mask: draft.puddle_mask.clone(),
            vegetation: draft.vegetation.clone(),
            vegetation_densities: draft.vegetation_densities.clone(),
            vegetation_nonzero_layers: draft.vegetation_nonzero_layers.clone(),
            vegetation_density_stats: draft.vegetation_density_stats.clone(),
            vegetation_author_densities: draft.vegetation_author_densities.clone(),
            vegetation_conflicts: draft.vegetation_conflicts.clone(),
            water: draft.water.clone(),
            water_masks: draft.water_masks.clone(),
            water_asset_path: draft.water_asset_path.clone(),
            terrain_dirty: draft.terrain_dirty.clone(),
            surface_dirty: draft.surface_dirty.clone(),
            puddles_dirty: draft.puddles_dirty.clone(),
            vegetation_dirty_layers: draft.vegetation_dirty_layers.clone(),
            vegetation_author_dirty_layers: draft.vegetation_author_dirty_layers.clone(),
            water_dirty_layers: draft.water_dirty_layers.clone(),
            water_catalog_registration_required: draft.water_catalog_registration_required.clone(),
            water_registration_source_hash: draft.water_registration_source_hash.clone(),
            vegetation_registration_source_hash: draft.vegetation_registration_source_hash.clone(),
        }
    }
    fn projection(&self) -> Result<EditorSceneDraft, String> {
        let identity = self.identity.as_ref().ok_or("对象画布没有完整来源身份")?;
        let map = identity.manifest.id.clone();
        Ok(EditorSceneDraft {
            scene_id: map.clone(),
            folder: identity.folder.clone(),
            placement_glb_hash: None,
            manifest: identity.manifest.clone(),
            terrain_ground: None,
            terrain_documents: TerrainDocumentStore {
                detached: true,
                active: Some(identity.clone()),
                ..Default::default()
            },
            placements: ScenePlacementsAsset {
                schema_version: SCENE_SCHEMA_VERSION,
                scene_id: map.clone(),
                placements: Vec::new(),
            },
            placement_water_surface_count: 0,
            portals: ScenePortalsAsset {
                schema_version: SCENE_SCHEMA_VERSION,
                scene_id: map.clone(),
                portals: Vec::new(),
            },
            domain: None, preserved_asset_paths: Default::default(),
            boundaries: None,
            boundary_catalog_registration_required: false,
            boundaries_dirty: false,
            environment_dirty: false,
            terrain_mesh: self.terrain_mesh.clone(),
            terrain_editing_index: self.terrain_editing_index.clone(),
            terrain_source_sha256: self.terrain_source_sha256.clone(),
            terrain_overwrite_authorized_sha256: self.terrain_overwrite_authorized_sha256.clone(),
            heights: self.heights.clone(),
            surface: self.surface.clone(),
            surface_weights: self.surface_weights.clone(),
            surface_indices: self.surface_indices.clone(),
            puddle_mask: self.puddle_mask.clone(),
            vegetation: self.vegetation.clone(),
            vegetation_densities: self.vegetation_densities.clone(),
            vegetation_nonzero_layers: self.vegetation_nonzero_layers.clone(),
            vegetation_density_stats: self.vegetation_density_stats.clone(),
            vegetation_author_densities: self.vegetation_author_densities.clone(),
            vegetation_conflicts: self.vegetation_conflicts.clone(),
            water: self.water.clone(),
            water_masks: self.water_masks.clone(),
            water_asset_path: self.water_asset_path.clone(),
            terrain_dirty: self.terrain_dirty.clone(),
            surface_dirty: self.surface_dirty.clone(),
            puddles_dirty: self.puddles_dirty.clone(),
            vegetation_dirty_layers: self.vegetation_dirty_layers.clone(),
            vegetation_author_dirty_layers: self.vegetation_author_dirty_layers.clone(),
            water_dirty_layers: self.water_dirty_layers.clone(),
            water_catalog_registration_required: self.water_catalog_registration_required.clone(),
            water_registration_source_hash: self.water_registration_source_hash.clone(),
            vegetation_registration_source_hash: self.vegetation_registration_source_hash.clone(),
        })
    }
}
pub(super) async fn prepare(
    draft: &EditorSceneDraft,
) -> Result<
    (
        Option<SceneManifestAsset>,
        Vec<EditorSceneDraft>,
        Vec<TerrainId>,
    ),
    String,
> {
    let Some(objects) = &draft.manifest.terrains else {
        return Ok((None, Vec::new(), Vec::new()));
    };
    let objects = objects.clone();
    let active = TerrainCanvas::snapshot(draft);
    let mut sources: Vec<_> = draft
        .terrain_documents
        .values
        .iter()
        .filter(|(id, _)| !draft.terrain_documents.saved.contains(*id))
        .map(|(_, canvas)| canvas.clone())
        .collect();
    if active.identity.is_some() {
        sources.push(Arc::new(active));
    }
    let owner = draft.manifest.clone();
    bevy::tasks::AsyncComputeTaskPool::get()
        .spawn(async move {
            let mut records = objects.to_vec();
            let mut owner = owner;
            let mut projections = Vec::new();
            let mut saved = Vec::new();
            for canvas in sources {
                let Some(identity) = &canvas.identity else {
                    continue;
                };
                let Some(object) = records
                    .iter_mut()
                    .find(|object| object.id == identity.definition.id)
                else {
                    // 删除只改变容器引用。仍保存脏作者文件，保证本会话撤销删除能恢复完整涂绘。
                    if canvas.dirty() {
                        saved.push(identity.definition.id.clone());
                        projections.push(canvas.projection()?);
                    }
                    continue;
                };
                object.puddles = identity.manifest.puddles.clone();
                if canvas.dirty() {
                    if canvas.surface.is_some() {
                        object.surface_path =
                            Some(format!("{}/{}", identity.folder, SCENE_SURFACE_FILE_NAME));
                    }
                    if canvas.vegetation.is_some() {
                        object.vegetation_path =
                            Some(format!("{}/{}", identity.folder, SCENE_VEGETATION_FILE_NAME));
                    }
                    if canvas.water.is_some() {
                        object.water_path = Some(canvas.water_asset_path.clone());
                    }
                    saved.push(object.id.clone());
                    projections.push(canvas.projection()?);
                }
            }
            owner.terrains = Some(records.into());
            validate_terrain_objects(&owner)?;
            Ok((Some(owner), projections, saved))
        })
        .await
}
