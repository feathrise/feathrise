//! 已保存试玩的小型进程内消息；不传作者草稿、撤销历史、实体或GPU句柄。

use bevy::prelude::*;

/// 宿主按本编辑实例绑定的工程启动普通游戏；场景ID只选择已经保存的正式场景。
#[derive(Message, Clone, Debug)]
pub struct RunSavedProject {
    /// 原按钮和F12在发布时冻结；后台构建和随后运行不得读取变化后的顶栏选择。
    pub profile: feathrise_editor_core::ProjectBuildProfile,
    pub scene_id: Option<String>,
    pub has_unsaved_changes: bool,
    /// 发布时已经在试玩：即使宿主同帧回收已结束的工作槽，也只能停止，不能转成新启动。
    pub stop_only: bool,
}

/// 后台构建、退出或失败回显；由共享工作台沿用原状态栏展示。
#[derive(Message, Clone, Debug)]
pub struct SavedPlayFeedback(pub String);

/// 编译诊断只能定位本作者实例的原中央文档；路径相对工程根，不读取或覆盖未保存草稿。
#[derive(Message, Clone, Debug)]
pub struct OpenProjectSourceLocation {
    pub context: u64,
    pub project_root: std::path::PathBuf,
    pub path: std::path::PathBuf,
    /// 从 1 开始的已保存源码行列；现有草稿有修改时仍定位草稿，并明确显示版本提示。
    pub line: u32,
    pub column: u32,
}
