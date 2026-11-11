//! 子场景列表、整体数值字段和源场景操作；不提供内部内容覆写入口。

use bevy::prelude::*;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct SceneBranch;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct Summary;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct ListViewport;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct ListContent;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct Row(pub usize);
#[derive(Component, Clone, Copy, Default)]
pub(super) struct RowText(pub usize);
#[derive(Component, Clone, Copy, Default)]
pub(super) struct Field(pub usize); // 位置XYZ与绕Y轴朝向；缩放固定为一。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct Slot(pub usize);
#[derive(Clone, Copy, Default)]
pub(super) enum Action { #[default] Add, Duplicate, Remove, OpenSource }
#[derive(Component, Clone, Copy, Default)]
pub(super) struct ActionButton(pub Action);
