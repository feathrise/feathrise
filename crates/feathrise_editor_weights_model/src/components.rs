//! 权重布局生成与场景交互共用同一组 ECS 组件。

use crate::types::Tool;
use bevy::prelude::Component;

/// 权重页、资产入口和视口按钮进入原场景权重动作系统。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Discard,
    OpenSaved,
    Rest,
    Compare,
    Heat,
    Skeleton,
    Visible,
    Mirror,
    Mask,
    Edit,
    Mode(Tool),
    Mesh(i8),
    Lock,
    Clear,
    Apply,
    Assign,
    Remove,
}

#[derive(Component, Clone, Copy)]
pub enum Label {
    Notice,
    EntryNotice,
    Target,
    Geometry,
    Joint,
    Selection,
    Legend,
    Button(Action),
}

#[derive(Component, Clone, Default)]
pub struct Entry;

#[derive(Component, Clone, Copy, Default)]
pub struct EditControls;
