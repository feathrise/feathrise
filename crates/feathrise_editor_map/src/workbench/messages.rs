//! 原工作台发给独立宿主的意图；工程代次与稳定地图身份一起冻结。
use super::EditorAction;
use bevy::prelude::*;

/// 选择与进入分开；保存、历史和试玩由宿主执行，UI不跨边界借用业务资源。
#[derive(Message, Clone, Debug)]
pub struct WorkbenchIntent {
    pub context: u64,
    pub action: EditorAction,
    pub selected_map: Option<String>,
    pub selected_index: Option<usize>,
}
