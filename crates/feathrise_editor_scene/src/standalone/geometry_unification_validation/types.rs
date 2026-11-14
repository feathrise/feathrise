//! 几何统一实窗验收的阶段和只读证据；不定义替代作者格式。

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    Idle, Create, WaitCreate, TinyReadback, ReduceLods, WaitLods,
    RejectBudget, WaitRejectBudget, RemoveHuge, WaitRemoveHuge,
    Focus, WaitFocus, CreateLayer, WaitLayer, Before, Hover, Paint, Release,
    WaitLayerPreset, SaveLayerPreset, WaitLayerPresetSave, CloseLayerPreset, WaitLayerPresetClose,
    After, Undo, WaitUndo, Undone, Redo, WaitRedo, Redone, Settle, Capture,
    SaveHeight, WaitSaveHeight, ReadSavedHeight, Complete, Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Creation { #[default] Tiny, Small, Huge, Voxel }
impl Creation {
    /// 局部验收对象必须离开示例地面的[-32,32]²范围；同高度重叠不能指定“选中者”穿透真实拾取。
    pub(super) fn origin(self) -> bevy::prelude::Vec3 {
        use bevy::prelude::Vec3;
        match self {
            Self::Tiny => Vec3::new(-48., 0., -48.),
            Self::Small => Vec3::new(-48., 0., 48.),
            Self::Huge => Vec3::new(0., -2000., 0.),
            Self::Voxel => Vec3::new(48., 0., -48.),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Domain { #[default] Height, HeightVegetation, Voxel, VoxelUnderside }

/// 体素取证只保留有界源身份和O(1)计数；完整字段扫描继续由正式后台worker负责。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct VolumeFieldIdentity {
    pub entity: bevy::prelude::Entity,
    pub snapshot: usize,
    pub geometry: ([u8; 32], [u8; 32]),
    pub cell_size: f32,
    pub primary_samples: usize,
    pub additional_cells: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct VolumeWindowIdentity {
    pub entity: bevy::prelude::Entity,
    pub source_revision: u64,
    pub source: bevy::prelude::Handle<bevy::prelude::Image>,
    pub density: feathrise_environment::vegetation::blade::GpuBladeDensityMap,
    pub source_to_world: [f32; 16],
    pub lod: u32,
    pub candidates: u32,
    pub origin: bevy::prelude::Vec3,
    pub focus: bevy::prelude::Vec3,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct VolumeCaptureIdentity {
    pub domain: Domain,
    pub scene: String,
    pub terrain: String,
    pub layer: String,
    pub instance: bevy::prelude::Entity,
    pub author_revision: u64,
    pub density_revision: u64,
    pub generation: u64,
    pub root: Option<bevy::prelude::Entity>,
    pub adopted_fields: Vec<(bevy::prelude::Entity, usize)>,
    pub fields: Vec<VolumeFieldIdentity>,
    pub ground: Option<[u64; 7]>,
    pub source: Option<bevy::prelude::Handle<bevy::prelude::Image>>,
    pub source_count: u32,
    pub positive_source_count: u32,
    pub expected_windows: usize,
    pub camera: bevy::prelude::Entity,
    pub camera_pose: [f32; 16],
    pub projection: [f32; 16],
    pub windows: Vec<VolumeWindowIdentity>,
    pub cpu_ready: bool,
}

/// 同一张作者二维页与后台正式R8输出的完整摘要，落盘和新进程重开继续独立回读。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeometryDensityProof {
    pub author_layer: String,
    pub output_layer: String,
    pub author_source_path: String,
    pub output_source_path: String,
    pub author_resolution: [u32; 2],
    pub output_resolution: [u32; 2],
    pub author_sha256: String,
    pub output_sha256: String,
    pub author_nonzero: usize,
    pub output_nonzero: usize,
}

/// 摘要来自实际作者源；大数组读取和计数只在唯一后台任务执行。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GeometrySourceProof {
    pub terrain_id: String,
    pub kind: String,
    pub source_path: String,
    pub sha256: String,
    pub world_extent: [f32; 2],
    pub cells: [u64; 2],
    pub display_lod_count: usize,
    pub nonzero_samples: usize,
    pub surface_samples: usize,
    #[serde(default)]
    pub cell_size_meters: f32,
    #[serde(default)]
    pub source_height_meters: f32,
    #[serde(default)]
    pub positive_samples: usize,
    #[serde(default)]
    pub negative_samples: usize,
    #[serde(default)]
    pub opposite_surfaces_same_cell: usize,
    #[serde(default)]
    pub positive_sha256: String,
    #[serde(default)]
    pub negative_sha256: String,
    #[serde(default)]
    pub positive_layers: Vec<String>,
    #[serde(default)]
    pub negative_layers: Vec<String>,
    #[serde(default)]
    pub density: Option<GeometryDensityProof>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeometryStrokeProof {
    pub before: GeometrySourceProof,
    pub after: GeometrySourceProof,
    pub undo_matches_before: bool,
    pub redo_matches_after: bool,
    pub collision_ready: bool,
    pub surface_hit: [f32; 3],
    pub screenshot: String,
}

/// 只证明原创建、LOD、画笔、历史与源回读；保存回执和 F12 由宿主随后继续核对。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GeometryUnificationProof {
    pub complete: bool,
    pub error: Option<String>,
    pub scene_id: String,
    pub input: String,
    pub tiny: Option<GeometrySourceProof>,
    pub small: Option<GeometryStrokeProof>,
    #[serde(default)]
    pub height_vegetation: Option<GeometryStrokeProof>,
    #[serde(default)]
    pub height_vegetation_gpu: Option<serde_json::Value>,
    #[serde(default)]
    pub hover_queries: Vec<serde_json::Value>,
    #[serde(default)]
    pub height_save_sequence: Option<u64>,
    #[serde(default)]
    pub height_density_saved_matches: bool,
    pub voxel: Option<GeometryStrokeProof>,
    #[serde(default)]
    pub voxel_gpu: Option<serde_json::Value>,
    #[serde(default)]
    pub voxel_underside: Option<GeometryStrokeProof>,
    #[serde(default)]
    pub voxel_underside_gpu: Option<serde_json::Value>,
    pub vegetation_output_layer: Option<String>,
    #[serde(default)]
    pub underside_vegetation_output_layer: Option<String>,
    pub overbudget_error: Option<String>,
    pub overbudget_source_unchanged: bool,
    pub parent_save_required: bool,
}

impl GeometryUnificationProof {
    pub fn verify_height_vegetation(&self) -> Result<(), String> {
        let sculpt = self.small.as_ref().ok_or("缺少小Clipmap雕刻证据")?;
        let vegetation = self.height_vegetation.as_ref().ok_or("缺少小Clipmap真实种植证据")?;
        let before = vegetation.before.density.as_ref().ok_or("缺少空白二维作者密度")?;
        let after = vegetation.after.density.as_ref().ok_or("缺少正式二维密度输出")?;
        if sculpt.after.terrain_id != vegetation.after.terrain_id
            || sculpt.after.source_path != vegetation.after.source_path
            || before.author_layer != after.author_layer || before.output_layer != after.output_layer
            || before.author_nonzero != 0 || before.output_nonzero != 0
            || after.author_nonzero == 0 || after.output_nonzero == 0
            || after.author_sha256 == before.author_sha256 || after.output_sha256 == before.output_sha256
            || !vegetation.undo_matches_before || !vegetation.redo_matches_after || !vegetation.collision_ready {
            return Err("小Clipmap未证明原笔刷产生二维作者密度、正式R8和完整历史".into());
        }
        Ok(())
    }

    /// 双侧证据必须来自原笔刷、持久根与真实提取面；不能用薄板参数或总样本数冒充隔层正确。
    pub fn verify_volume_sides(&self) -> Result<(), String> {
        let top = self.voxel.as_ref().ok_or("缺少体素上表面原笔刷证据")?;
        let bottom = self.voxel_underside.as_ref().ok_or("缺少体素底面原笔刷证据")?;
        let top_layer = self.vegetation_output_layer.as_ref().ok_or("缺少上表面图层身份")?;
        let bottom_layer = self.underside_vegetation_output_layer.as_ref().ok_or("缺少底面图层身份")?;
        let after = &bottom.after;
        // 两个相反方向的原射线须在同一XZ处命中相隔真实厚度的两面。
        // 仅源参数写0.05m、或一个双面零厚平面的相反法线，都不能证明提取保留了薄板。
        let hit_thickness = top.surface_hit[1] - bottom.surface_hit[1];
        if top.surface_hit.into_iter().chain(bottom.surface_hit).any(|value| !value.is_finite())
            || (top.surface_hit[0] - bottom.surface_hit[0]).abs() > 0.001
            || (top.surface_hit[2] - bottom.surface_hit[2]).abs() > 0.001
            || !hit_thickness.is_finite() || hit_thickness <= 0.
            || (hit_thickness - after.source_height_meters).abs() > after.source_height_meters * 0.1 {
            return Err("体素薄板的正反真实射线命中没有保留同XZ的源厚度（容差10%）".into());
        }
        if top_layer == bottom_layer || top.after.terrain_id != after.terrain_id
            || top.after != bottom.before
            || !(after.source_height_meters > 0. && after.source_height_meters < after.cell_size_meters * 0.5)
            || top.after.positive_samples == 0 || top.after.negative_samples != 0
            || after.negative_samples == 0 || after.opposite_surfaces_same_cell == 0
            || after.positive_samples != top.after.positive_samples || after.positive_sha256 != top.after.positive_sha256
            || top.after.positive_layers != [top_layer.clone()] || after.positive_layers != [top_layer.clone()]
            || after.negative_layers != [bottom_layer.clone()]
            || !top.undo_matches_before || !top.redo_matches_after || !top.collision_ready
            || !bottom.undo_matches_before || !bottom.redo_matches_after || !bottom.collision_ready {
            return Err("体素薄板未证明上侧保持、下侧独立、同格相反表面及完整历史".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_side_proof_requires_two_real_aligned_hits_at_the_source_thickness() {
        let top_source = GeometrySourceProof { terrain_id: "thin-voxel".into(), cell_size_meters: 0.25,
            source_height_meters: 0.05, positive_samples: 1, positive_sha256: "top-samples".into(),
            positive_layers: vec!["top".into()], ..Default::default() };
        let mut bottom_source = top_source.clone();
        bottom_source.negative_samples = 1; bottom_source.opposite_surfaces_same_cell = 1;
        bottom_source.negative_layers = vec!["bottom".into()];
        let top = GeometryStrokeProof { before: GeometrySourceProof::default(), after: top_source.clone(),
            undo_matches_before: true, redo_matches_after: true, collision_ready: true,
            surface_hit: [24., 0., -24.], screenshot: "top.png".into() };
        let bottom = GeometryStrokeProof { before: top_source, after: bottom_source,
            surface_hit: [24., -0.05, -24.], ..top.clone() };
        let mut proof = GeometryUnificationProof { voxel: Some(top), voxel_underside: Some(bottom),
            vegetation_output_layer: Some("top".into()), underside_vegetation_output_layer: Some("bottom".into()),
            ..Default::default() };
        proof.verify_volume_sides().unwrap();
        proof.voxel_underside.as_mut().unwrap().surface_hit[1] = 0.;
        assert!(proof.verify_volume_sides().is_err(), "同一零厚双面不能冒充保留薄板");
        proof.voxel_underside.as_mut().unwrap().surface_hit = [24.02, -0.05, -24.];
        assert!(proof.verify_volume_sides().is_err(), "两个不对齐位置的碰巧高度差不能冒充同处厚度");
    }
}
