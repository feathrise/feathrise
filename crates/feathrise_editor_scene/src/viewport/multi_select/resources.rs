//! 多选集合与未完成的框选身份；完整结果一次发布，旧点击、切换场景和变换不能抢选。

use crate::authoring::EditorAuthoringSession;
use crate::viewport::modes::SelectionFilter;
use crate::viewport::placement_move::MoveCamera;
use bevy::prelude::*;
use feathrise_scene_content::SceneId;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) struct SelectedMember {
    pub id: String,
    pub index: usize,
    pub position: Vec3,
}

pub(super) struct SelectionGesture {
    pub scene_id: SceneId,
    pub author_revision: u64,
    pub structure_revision: u64,
    pub terrain_revision: u64, // 框选冻结源姿态，地形换代不能沿旧投影继续收集。
    pub derived_pose_revision: u64, // 同一作者/地形修订下迟到的贴地结果也会改变投影。
    pub camera: MoveCamera,
    pub ui_size: Vec2,
    pub filter: SelectionFilter,
    pub start: Vec2,
    pub current: Vec2,
    pub additive: bool,
    pub dragging: bool,
}

pub(super) struct SelectionScan {
    pub gesture: SelectionGesture,
    pub cursor: usize,
    pub members: Vec<SelectedMember>,
}

pub(super) struct SelectionRefresh {
    pub revision: u64,
    pub structure: u64,
    pub terrain: u64,
    pub derived_pose: u64,
    pub cursor: usize,
    pub members: Vec<SelectedMember>,
}

#[derive(Resource, Default)]
pub(crate) struct PlacementSelectionSet {
    pub scene_id: Option<SceneId>,
    pub members: Arc<Vec<SelectedMember>>,
    pub revision: u64,
    pub author_revision: u64,
    pub structure_revision: u64,
    pub terrain_revision: u64, // 源局部记录未变时，所属地形的移动仍须局部刷新世界锚点。
    pub derived_pose_revision: u64,
    pub ready: bool,
    pub center: Vec3,
    pub(super) gesture: Option<SelectionGesture>,
    pub(super) scan: Option<SelectionScan>,
    pub(super) refresh: Option<SelectionRefresh>,
    pub(super) owns_mouse: bool,
    pub(crate) click_ready: bool,
    pub(crate) click_additive: bool,
}

impl PlacementSelectionSet {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.owns_mouse
    }
    pub(crate) fn box_busy(&self) -> bool {
        self.gesture
            .as_ref()
            .is_some_and(|gesture| gesture.dragging)
            || self.scan.is_some()
    }
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.members.iter().any(|member| member.id == id)
    }
    pub(crate) fn ready_for(&self, session: &EditorAuthoringSession) -> bool {
        self.ready
            && self.author_revision == session.revision
            && self.structure_revision == session.placement_structure_revision
            && self.terrain_revision == session.terrain_revision
            && self.derived_pose_revision == session.placement_derived_pose_revision
            && session
                .draft
                .as_ref()
                .is_some_and(|draft| self.scene_id.as_ref() == Some(&draft.scene_id))
    }

    pub(crate) fn clear(&mut self) {
        if !self.members.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        self.members = Arc::default();
        self.ready = false;
        self.refresh = None;
    }

    pub(crate) fn publish(
        &mut self,
        members: Vec<SelectedMember>,
        session: &EditorAuthoringSession,
    ) {
        self.scene_id = session.draft.as_ref().map(|draft| draft.scene_id.clone());
        self.center = selection_center(&members);
        self.members = Arc::new(members);
        self.author_revision = session.revision;
        self.structure_revision = session.placement_structure_revision;
        self.terrain_revision = session.terrain_revision;
        self.derived_pose_revision = session.placement_derived_pose_revision;
        self.revision = self.revision.wrapping_add(1);
        self.ready = true;
        self.refresh = None;
    }

    /// Shift只对完整普通实例增减；复制的是有上限的短ID，不复制作者记录、水绑定或场景。
    pub(crate) fn click(
        &mut self,
        member: SelectedMember,
        additive: bool,
        session: &EditorAuthoringSession,
    ) -> Result<(), String> {
        let record = session.draft.as_ref()
            .and_then(|draft| draft.placements.placements.get(member.index))
            .filter(|record| record.id == member.id)
            .ok_or("选中的完整对象已变化")?;
        if crate::authoring::placement_pose::is_automatic_volume(record) {
            return Err("自动三维雕刻层请使用专用作者入口".into());
        } // 单击与Shift追加同样不能把专用拓扑层收编为普通选集；拒绝时原选集保持不变。
        if additive && !self.members.is_empty()
            && session.draft.as_ref().is_some_and(|draft| self.scene_id.as_ref() == Some(&draft.scene_id))
            && !self.ready_for(session)
        {
            return Err("多选对象位置正在更新，请稍后再追加选择".into()); // 先完成原有有界复核，不能给旧世界锚点盖上新源修订。
        }
        let mut members = if additive
            && session
                .draft
                .as_ref()
                .is_some_and(|draft| self.scene_id.as_ref() == Some(&draft.scene_id))
        {
            self.members.as_ref().clone()
        } else {
            Vec::new()
        };
        if let Some(index) = members.iter().position(|old| old.id == member.id) {
            members.remove(index);
        } else {
            if members.len() == super::MAX_SELECTED {
                return Err(format!(
                    "多选最多{}个普通资产，请缩小选择范围",
                    super::MAX_SELECTED
                ));
            }
            members.push(member);
        }
        self.publish(members, session);
        Ok(())
    }
}

fn selection_center(members: &[SelectedMember]) -> Vec3 {
    if members.is_empty() {
        return Vec3::ZERO;
    }
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for member in members {
        min = min.min(member.position);
        max = max.max(member.position);
    }
    (min + max) * 0.5
}
