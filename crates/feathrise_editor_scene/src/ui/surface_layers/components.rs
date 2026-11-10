//! 地表作者层列表的固定容器、行与操作按钮身份。

use bevy::prelude::*; // 使用原生UI组件与BSN场景标记。

use super::types::{SurfaceLayerAction, SurfaceLayerTarget}; // 显示目标与删除请求共用稳定身份。

#[derive(Component, Clone, Default)] // BSN只创建一次摘要文本。
pub(super) struct SurfaceLayerListTitle; // 显示当前场景真实地表层数量。

#[derive(Component, Clone, Default)] // BSN只创建一次六槽列表外壳。
pub(super) struct SurfaceLayerList; // 零层时整个容器退出布局。

#[derive(Component)] // 每行绑定实际显示的场景、修订和地表层。
pub(super) struct SurfaceLayerRow {
    pub(super) slot: usize,                        // 固定槽位不随场景层数增加。
    pub(super) target: Option<SurfaceLayerTarget>, // 点击时复验，防止删除错位层。
} // 文本更新不会改变槽位。

#[derive(Component, Clone, Default)] // 查询只访问本列表的短文本。
pub(super) struct SurfaceLayerRowText; // 与其他页面文字保持独立。

#[derive(Component, Clone, Default)] // 操作区随真实列表显隐。
pub(super) struct SurfaceLayerActions; // 空列表不保留按钮空白。

#[derive(Component, Clone, Default)] // BSN构造需要默认模板，真实按钮仍显式提供局部动作。
pub(super) struct SurfaceLayerButton(pub(super) SurfaceLayerAction); // 当前场景操作不会删除全局目录项。
