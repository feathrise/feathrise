//! 地形作者源与画布的完整槽位；移动/浅共享数组，不在切换对象的帧复制整图像素。
use super::super::{
    resources::EditorSceneDraft,
    vegetation_compile::{CompiledDensityImage, CompiledDensityStats},
};
use feathrise_scene_content::*;
use feathrise_terrain::{TerrainEditingIndex, TerrainMeshInput};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone)]
pub(crate) struct TerrainDocumentIdentity {
    pub definition: Arc<SceneTerrainObject>,
    pub manifest: SceneManifestAsset,
    pub folder: String,
}
#[derive(Clone, Default)]
pub(crate) struct TerrainDocumentStore {
    pub active: Option<TerrainDocumentIdentity>,
    /// 根静水独立于网格；切换私有画布时仅停放水文档及其像素、路径和保存责任。
    pub root_water: Option<RootWaterCanvas>,
    pub root_water_revision: u64,
    pub values: BTreeMap<TerrainId, Arc<TerrainCanvas>>,
    pub scoped_through: usize,
    /// 仅保存投影使用；阻止对象画布保存制造另一份场景/角色/摆放文档。
    pub detached: bool,
    pub active_dirty: bool,
    pub saved: BTreeSet<TerrainId>,
    /// 停放槽无需复制大画布；撤销首次迁移时仍须采用保存后的精确GLB基线。
    pub saved_digests: BTreeMap<TerrainId, [u8; 32]>,
    /// 停放时增量登记脏责任，UI/关闭门不再逐帧扫描全部缓存画布。
    pub dirty_stored: BTreeSet<TerrainId>,
    pub loading: bool,
    pub paint_groups_dirty: bool, // 场景容器的绘制关联独立于活动网格脏位。
}
#[derive(Clone, Default)]
pub(crate) struct RootWaterCanvas {
    pub revision: u64,
    /// 仅改水位或删除最后一层时没有R8脏层，仍须保留根JSON的保存责任。
    pub structural_dirty: bool,
    pub water: Option<SceneWaterAsset>,
    pub water_masks: BTreeMap<String, Arc<Vec<u8>>>,
    pub water_asset_path: String,
    pub water_dirty_layers: BTreeSet<String>,
    pub water_catalog_registration_required: bool,
    pub water_registration_source_hash: Option<String>,
}
#[derive(Clone, Default)]
pub(crate) struct TerrainCanvas {
    /// 跨网格冷目标只载入植被；首次成为活动画布时补齐其余域，不能重新读取已画植被。
    pub vegetation_only: bool,
    pub structural_dirty: bool,
    pub identity: Option<TerrainDocumentIdentity>,
    pub terrain_mesh: Option<Arc<TerrainMeshInput>>,
    pub terrain_editing_index: Option<Arc<TerrainEditingIndex>>,
    pub terrain_source_sha256: Option<[u8; 32]>,
    pub terrain_overwrite_authorized_sha256: Option<[u8; 32]>,
    pub heights: Option<Arc<Vec<u16>>>,
    pub surface: Option<SceneSurfaceAsset>,
    pub surface_weights: Option<Arc<Vec<u8>>>,
    pub surface_indices: Option<Arc<Vec<u8>>>,
    pub puddle_mask: Option<Arc<Vec<u8>>>,
    pub vegetation: Option<SceneVegetationAsset>,
    pub vegetation_densities: BTreeMap<String, Arc<Vec<u8>>>,
    pub vegetation_nonzero_layers: BTreeSet<String>,
    pub vegetation_density_stats: BTreeMap<String, CompiledDensityStats>,
    pub vegetation_author_densities: BTreeMap<String, Arc<Vec<u8>>>,
    pub vegetation_conflicts: BTreeMap<String, CompiledDensityImage>,
    pub water: Option<SceneWaterAsset>,
    pub water_masks: BTreeMap<String, Arc<Vec<u8>>>,
    pub water_asset_path: String,
    pub terrain_dirty: bool,
    pub surface_dirty: bool,
    pub puddles_dirty: bool,
    pub vegetation_dirty_layers: BTreeSet<String>,
    pub vegetation_author_dirty_layers: BTreeSet<String>,
    pub water_dirty_layers: BTreeSet<String>,
    pub water_catalog_registration_required: bool,
    pub water_registration_source_hash: Option<String>,
    pub vegetation_registration_source_hash: Option<String>,
}
impl TerrainCanvas {
    pub fn validate_paths(&self, root: &std::path::Path, folder: &str) -> Result<(), String> {
        let check = |path: &str| {
            super::super::files::safe_scene_private_asset_path(root, folder, path).map(|_| ())
        };
        if let Some(surface) = &self.surface {
            for path in [
                &surface.weight_source_path,
                &surface.weight_texture_path,
                &surface.index_source_path,
                &surface.index_texture_path,
            ] {
                check(path)?;
            }
        }
        if let Some(vegetation) = &self.vegetation {
            for layer in &vegetation.layers {
                check(&layer.density_source_path)?;
                check(&layer.density_texture_path)?;
            }
            if let Some(authoring) = &vegetation.authoring {
                for layer in &authoring.layers {
                    check(&layer.density_source_path)?;
                }
            }
        }
        if let Some(water) = &self.water {
            check(&self.water_asset_path)?;
            for layer in &water.layers {
                check(&layer.mask_source_path)?;
                check(&layer.mask_texture_path)?;
            }
        }
        if let Some(puddles) = self
            .identity
            .as_ref()
            .and_then(|identity| identity.manifest.puddles.as_ref())
        {
            check(&puddles.source_path)?;
            check(&puddles.texture_path)?;
        }
        Ok(())
    }
    pub fn swap(&mut self, draft: &mut EditorSceneDraft) {
        // 字段整体交换保持源/派生/脏层同代，不允许把A对象的描述与B对象的像素配对。
        macro_rules! swap { ($($field:ident),* $(,)?) => { $(std::mem::swap(&mut self.$field, &mut draft.$field);)* }; }
        swap!(
            terrain_mesh,
            terrain_editing_index,
            terrain_source_sha256,
            terrain_overwrite_authorized_sha256,
            heights,
            surface,
            surface_weights,
            surface_indices,
            puddle_mask,
            vegetation,
            vegetation_densities,
            vegetation_nonzero_layers,
            vegetation_density_stats,
            vegetation_author_densities,
            vegetation_conflicts,
            water,
            water_masks,
            water_asset_path,
            terrain_dirty,
            surface_dirty,
            puddles_dirty,
            vegetation_dirty_layers,
            vegetation_author_dirty_layers,
            water_dirty_layers,
            water_catalog_registration_required,
            water_registration_source_hash,
            vegetation_registration_source_hash
        );
        std::mem::swap(&mut self.identity, &mut draft.terrain_documents.active);
        std::mem::swap(
            &mut self.structural_dirty,
            &mut draft.terrain_documents.active_dirty,
        );
    }
    pub fn dirty(&self) -> bool {
        self.structural_dirty
            || self.terrain_dirty
            || self.surface_dirty
            || self.puddles_dirty
            || !self.vegetation_dirty_layers.is_empty()
            || !self.vegetation_author_dirty_layers.is_empty()
            || !self.water_dirty_layers.is_empty()
            || self.water_catalog_registration_required
            || self.water_registration_source_hash.is_some()
            || self.vegetation_registration_source_hash.is_some()
    }
    pub fn clear_dirty(&mut self) {
        self.structural_dirty = false;
        self.terrain_dirty = false;
        self.surface_dirty = false;
        self.puddles_dirty = false;
        self.vegetation_dirty_layers.clear();
        self.vegetation_author_dirty_layers.clear();
        self.water_dirty_layers.clear();
        self.water_catalog_registration_required = false;
        self.water_registration_source_hash = None;
        self.vegetation_registration_source_hash = None;
    }
}
impl EditorSceneDraft {
    pub fn stored_terrain_changes(&self) -> bool {
        !self.terrain_documents.dirty_stored.is_empty() || self.terrain_documents.active_dirty || self.terrain_documents.paint_groups_dirty
            || self.terrain_documents.root_water.as_ref().is_some_and(RootWaterCanvas::dirty)
    }
    pub fn terrain_manifest(&self) -> &SceneManifestAsset {
        self.terrain_documents
            .active
            .as_ref()
            .map_or(&self.manifest, |active| &active.manifest)
    }
    pub fn terrain_manifest_mut(&mut self) -> &mut SceneManifestAsset {
        self.terrain_documents
            .active
            .as_mut()
            .map_or(&mut self.manifest, |active| &mut active.manifest)
    }
    pub fn terrain_folder(&self) -> &str {
        self.terrain_documents
            .active
            .as_ref()
            .map_or(&self.folder, |active| &active.folder)
    }
    pub fn terrain_point(&self, world: bevy::prelude::Vec3) -> bevy::prelude::Vec3 {
        self.terrain_documents
            .active
            .as_ref()
            .map_or(world, |active| {
                active
                    .definition
                    .transform
                    .as_transform()
                    .to_matrix()
                    .inverse()
                    .transform_point3(world)
            })
    }
    pub fn uses_legacy_clipmap_cursor(&self) -> bool {
        self.manifest.terrains.is_none()
            && self.terrain_manifest().terrain.kind == SceneTerrainKind::SparseClipmap
    }
}
