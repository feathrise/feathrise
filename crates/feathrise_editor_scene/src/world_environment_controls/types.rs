//! 环境模拟参数编辑的稳定对象身份和单次拖动快照。

use bevy::prelude::Resource;
use feathrise_scene_content::{EnvironmentSimulation, SceneId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SimulationTarget {
    Scene(SceneId),
    Profile(String),
}

pub(super) struct SimulationDrag {
    pub(super) target: SimulationTarget,
    pub(super) version: (u64, u64),
    pub(super) before: Option<EnvironmentSimulation>,
    pub(super) latest: EnvironmentSimulation,
    pub(super) dirty_before: bool,
}

#[derive(Resource, Default)]
pub(crate) struct SimulationInputState {
    pub(super) drag: Option<SimulationDrag>,
    pub(super) blocked: bool,
}

impl SimulationInputState {
    /// 保存入口等待原拖动松手提交，避免冻结未进入历史的中间值。
    pub(crate) fn busy(&self) -> bool { self.drag.is_some() }
}
