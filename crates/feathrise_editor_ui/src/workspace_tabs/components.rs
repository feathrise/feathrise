//! 中央文档标签的视觉所有权；各业务正文的普通按钮配色查询必须排除这些独立命中区。

use bevy::prelude::Component;

/// 活动填色与关闭确认由公共标签builder决定，业务动作仍由原有各域Button标记分派。
#[derive(Component)]
pub struct WorkspaceTabButton;
