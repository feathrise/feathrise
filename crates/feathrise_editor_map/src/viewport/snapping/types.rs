//! 普通地形补查的完整输入身份；鼠标移出后仍可完成已确认点，换图/相机/偏好拒收。
use crate::viewport::vegetation_ground::GroundCursorTarget;
use bevy::prelude::*;
use feathrise_map_content::{MapId, TerrainId};
use feathrise_editor_core::EditorTranslationSnap;

/// 同一作者修订也不能把另一地形或另一姿态的后台结果借给当前落点。
#[derive(Clone, PartialEq, Eq)]
pub(super) struct ObjectSnapIdentity {
    pub id: TerrainId,
    pub pose: [u32; 7],
    pub generation: u64,
    pub snapshot: usize,
    pub visibility: usize,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct MeshSnapKey {
    pub map: MapId,
    pub target: GroundCursorTarget,
    pub author: u64,
    pub terrain: u64,
    pub mesh: usize,
    pub index: usize,
    pub object: Option<ObjectSnapIdentity>,
    pub bounds: [u32; 4],
    pub camera: Entity,
    pub view: crate::types::EditorView,
    pub projection: bool,
    pub camera_pose: [u32; 7],
    pub render_size: UVec2,
    pub point: [u32; 3],
    pub snap: EditorTranslationSnap,
}
impl MeshSnapKey {
    pub fn same_scope(&self, other: &Self) -> bool {
        self.map == other.map
            && self.target == other.target
            && self.author == other.author
            && self.terrain == other.terrain
            && self.mesh == other.mesh
            && self.index == other.index
            && self.object == other.object
            && self.bounds == other.bounds
            && self.camera == other.camera
            && self.view == other.view
            && self.projection == other.projection
            && self.camera_pose == other.camera_pose
            && self.render_size == other.render_size
            && self.snap.same_preferences(other.snap)
    }
}
