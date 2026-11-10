//! 对象管理固定字段和按钮身份；不进入全局EditorAction枚举。

use bevy::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ObjectField {
    #[default]
    Search,
    Name,
    Group,
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ObjectInput(pub(super) ObjectField);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ObjectAction {
    Focus(ObjectField),
    #[default]
    Rename,
    AssignGroup,
    Ungroup,
    SelectGroup,
    ToggleHidden,
    ToggleLocked,
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ObjectButton(pub(super) ObjectAction);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ObjectSummary;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct SceneAssetSearch;
