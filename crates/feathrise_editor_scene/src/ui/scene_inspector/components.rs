//! 检查器只读概览和反馈的固定组件，不创建第二份选择或编辑状态。

use super::types::{SceneInspectorField, SelectedToolPropertiesKind};
use bevy::prelude::*;

/// 新挂载概览时强制一次回显，后续稳定帧只观察原资源的变化。
#[derive(Component, Clone, Copy, Default)]
pub struct SceneInspectorOverview;

/// 旧草稿、失效结构下标和无图时，一次收起两个原属性域，概览仍显示原因。
#[derive(Component, Clone, Copy, Default)]
pub struct SceneInspectorProperties;
/// 已有网格专用来源分组时收起通用元数据入口，操作反馈统一留在全局底栏。
#[derive(Component, Clone, Copy, Default)]
pub struct SceneInspectorMetadata;

/// 身份、来源与操作反馈分开排版，避免把数值和长路径塞进同一摘要。
#[derive(Component, Clone, Copy, Default)]
pub struct SceneInspectorText(pub(super) SceneInspectorField);

/// 固定的属性域只负责显隐，正式选择和作者事务仍归原功能所有。
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub enum InspectorSection {
    #[default]
    Terrain,
    Water,
    Vegetation,
    Domain,
    Subscene,
    ObjectManagement,
    ObjectName,
}

#[derive(Component, Clone, Copy, Default)]
pub struct InspectorTree;
#[derive(Component, Clone, Copy, Default)]
pub struct InspectorBody;

/// 原宿主在首次出现时冻结；切页仅改变父节点，输入焦点、缓冲和后台事务继续使用原实体。
#[derive(Component, Clone, Default)]
pub struct SelectedToolProperties {
    pub(super) kind: SelectedToolPropertiesKind,
    pub(super) home: Option<Entity>,
    pub(super) in_inspector: bool,
}

impl SelectedToolProperties {
    pub fn new(kind: SelectedToolPropertiesKind) -> Self {
        Self { kind, home: None, in_inspector: false }
    }

    pub fn shown_in_inspector(&self, kind: SelectedToolPropertiesKind) -> bool {
        self.in_inspector && self.kind == kind
    }
}

#[derive(Component, Clone, Default)]
pub struct SelectedToolPropertiesHost;
