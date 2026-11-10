//! 设置弹窗的固定表现节点；不建立第二份偏好或作者数据。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct SettingsFrame;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct SettingsHeader;
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SettingsBody;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct SettingsFooter;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct SettingsGroup;
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SettingsThemeText;
/// 关闭和重置是操作，不是可选档位；关闭遮罩不能被按钮样式染成实体面板。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SettingsUtilityButton;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SettingsGeneralOnly;
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SettingsShortcutsOnly;
/// 页签与顶栏入口复用动作，但顶栏在任一设置页打开时都保持选中。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SettingsPageButton;
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SettingsTabs;
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ShortcutSettingsGroup;
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ShortcutSettingsRow(pub(crate) feathrise_editor_core::EditorShortcutAction);
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ShortcutNoticeCard;
/// 原生滚动条绑定唯一快捷键正文，不增加另一份滚动位置或拖动状态机。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ShortcutScrollbarTrack;
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ShortcutScrollbarThumb;
