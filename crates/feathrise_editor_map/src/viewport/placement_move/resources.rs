//! 一次移动拖动与三键和弦的独占状态，所有者在静默帧才归还鼠标。

use super::types::{MoveDrag, MoveHandle, PendingBuildingMove};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(crate) struct PlacementMoveState {
    pub(super) group: Option<super::group_types::GroupDrag>,
    pub(super) group_task: Option<bevy::tasks::Task<Result<super::group_types::PreparedGroupMove, String>>>,
    pub(super) drag: Option<MoveDrag>,
    pub(super) pending_building: Option<PendingBuildingMove>, // 最多保留本次单槽校验的一个模型预览。
    pub(super) hover: Option<MoveHandle>,
    pub(super) owns_mouse: bool,
}

impl PlacementMoveState {
    /// 常驻游戏只读原手势已经采用的值；不暴露可变事务，也不另行计算鼠标或写历史。
    pub(crate) fn embedded_single(&self) -> Option<(&feathrise_map_content::MapId, usize, u64, u64,
        &feathrise_map_content::MapPlacementRecord, Mat4)> {
        let drag = self.drag.as_ref().or_else(|| self.pending_building.as_ref().map(|pending| &pending.drag))?;
        Some((&drag.map_id, drag.record_index, drag.author_revision, drag.structure_revision, &drag.before, drag.applied_world))
    }

    pub(crate) fn embedded_group(&self) -> Option<(&feathrise_map_content::MapId, u64, u64, u64,
        &std::sync::Arc<Vec<crate::viewport::multi_select::SelectedMember>>, Mat4, bool, bool)> {
        let group = self.group.as_ref()?;
        let rotation = Quat::from_rotation_y(group.rotation_degrees.to_radians());
        let matrix = Mat4::from_scale_rotation_translation(Vec3::splat(group.factor), rotation,
            group.origin + group.delta - rotation * (group.origin * group.factor));
        Some((&group.map, group.author_revision, group.structure_revision, group.selection_revision,
            &group.members, matrix, group.phase == super::group_types::GroupPhase::Finishing, group.committed))
    }

    pub(crate) fn exit_pending(&self) -> bool {
        self.group.is_some() || self.group_task.is_some() || self.drag.is_some() || self.pending_building.is_some()
    }

    pub(crate) fn grid_reference(&self, control: bool) -> Option<(Vec3, Vec3, feathrise_editor_core::EditorTranslationSnap)> {
        if let Some(group) = &self.group { return Some((group.origin, group.origin + group.delta, group.snapping.translation_snap(control, group.origin.to_array()))); }
        let drag = self.drag.as_ref().or_else(|| self.pending_building.as_ref().map(|pending| &pending.drag))?;
        Some((drag.world_origin, drag.preview_world_position(), drag.translation_snap()))
    }
    pub(crate) fn multi_transform_active(&self) -> bool { self.group.is_some() }
    pub(crate) fn group_delta(&self) -> Vec3 { self.group.as_ref().map_or(Vec3::ZERO, |group| group.delta) }
    pub(crate) fn group_position(&self, position: Vec3) -> Vec3 {
        self.group.as_ref().map_or(position, |group| group.transformed_position(position))
    }
    pub(crate) fn owns_mouse(&self) -> bool {
        self.owns_mouse
    }

    /// 静水Y轴沿用移动工具的统一捕获门，最终release与和弦仍由同一静默帧归还。
    pub(crate) fn capture_mouse(&mut self) {
        self.owns_mouse = true;
    }

    pub(super) fn release_when_quiet(&mut self, mouse: &ButtonInput<MouseButton>) {
        if self.drag.is_none() && self.group.is_none()
            && !mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
            && ![MouseButton::Left, MouseButton::Right, MouseButton::Middle]
                .into_iter()
                .any(|key| mouse.just_released(key))
        {
            self.owns_mouse = false;
        } // Esc、主键松开及和弦后续释放都不能让世界补收到旧手势。
    }
}
