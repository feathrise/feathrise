//! 管理意图、冻结身份和分帧收集槽；场景及模型数据继续由原作者会话持有。

use super::components::ObjectAction;
use crate::authoring::SceneObjectChange;
use crate::viewport::multi_select::SelectedMember;
use bevy::prelude::*;
use feathrise_scene_content::SceneId;

pub(super) enum ObjectTargets {
    Indices(Vec<usize>),
    Group(String),
}

pub(super) struct PendingManagement {
    pub scene_id: SceneId,
    pub revision: u64,
    pub structure: u64,
    // 只有选组缓存世界锚点；纯元数据管理不因无关地形或迟到贴地结果取消。
    pub terrain_revision: u64,
    pub derived_pose_revision: u64,
    pub targets: ObjectTargets,
    pub action: ObjectAction,
    pub name: String,
    pub group: String,
    pub flag: bool,
    pub flag_set: bool,
    pub cursor: usize,
    pub changes: Vec<SceneObjectChange>,
    pub heap_bytes: usize,
    pub selected: Vec<SelectedMember>,
}

#[derive(Resource, Default)]
pub(crate) struct SceneObjectState {
    pub(super) name: String,
    pub(super) group: String,
    pub(super) bound: Option<(SceneId, Option<String>, Option<String>, u64, u64)>,
    pub(super) pending: Option<PendingManagement>,
    /// 取消或旧redo的拥有型数据在唯一CPU槽析构；不因关闭释放整个组的字符串。
    pub(super) retirement: Option<bevy::tasks::Task<()>>,
}

impl SceneObjectState {
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn accepts_commands(&self) -> bool {
        !self.busy() && self.retirement.is_none()
    }
}
