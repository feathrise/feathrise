//! 地图快捷键设置：组合键录入、冲突提示、解绑与默认恢复。
//!
//! 【功能设计】复用设置窗口的唯一输入所有权；只修改偏好，原作者会话继续后台工作。
//! 设置内四组十二行独立滚动；帮助统一由问号浮层管理，不维护第二套开关状态。

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{ControlOrientation, Scrollbar, ScrollbarDragState};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorKeyBinding;
use feathrise_editor_core::EditorShortcutConflict;
use feathrise_editor_core::EditorKeyBindings;

use super::editor_settings::components::{
    ShortcutNoticeCard, ShortcutScrollbarThumb,
    ShortcutScrollbarTrack,
};
use super::theme as colors;
use crate::components::{
    MapEditorShortcutBindingText, MapEditorShortcutNoticeText,
    MapEditorShortcutSettingsBody,
};
use crate::resources::{MapEditorSettingsState, MapEditorState};
use crate::types::{EditorSettingsAction, EditorShortcutNoticeKind};


pub(super) fn apply_shortcut_button(
    action: EditorSettingsAction,
    state: &mut ResMut<MapEditorSettingsState>,
    settings: &mut ResMut<EditorSettings>,
) {
    match action {
        EditorSettingsAction::RebindShortcut(action) => {
            state.listening = Some(action);
            state.shortcut_notice = format!("请为“{}”按新组合键；Esc取消", action.label());
            state.shortcut_notice_kind = EditorShortcutNoticeKind::Listening;
        }
        EditorSettingsAction::UnbindShortcut(action) => {
            if settings.editor_keybindings.get(action).is_some() {
                settings
                    .editor_keybindings
                    .assign(action, None)
                    .expect("解绑不可能冲突");
            }
            state.listening = None;
            state.shortcut_notice = format!("已清除“{}”的快捷键", action.label());
            state.shortcut_notice_kind = EditorShortcutNoticeKind::Success;
        }
        EditorSettingsAction::ResetShortcuts => {
            let defaults = EditorKeyBindings::default();
            if settings.editor_keybindings != defaults {
                settings.editor_keybindings = defaults;
            }
            state.listening = None;
            state.shortcut_notice = "已恢复默认快捷键".into();
            state.shortcut_notice_kind = EditorShortcutNoticeKind::Success;
        }
        _ => {}
    }
}

pub(super) fn capture_shortcut(
    keys: &ButtonInput<KeyCode>,
    state: &mut ResMut<MapEditorSettingsState>,
    settings: &mut ResMut<EditorSettings>,
) {
    let Some(action) = state.listening else {
        return;
    };
    if keys.just_pressed(KeyCode::Escape) {
        state.listening = None;
        state.shortcut_notice = "已取消快捷键录入".into();
        state.shortcut_notice_kind = EditorShortcutNoticeKind::Idle;
        return;
    }
    let mut pressed = keys
        .get_just_pressed()
        .copied()
        .filter(|&key| !EditorKeyBinding::is_modifier(key));
    let Some(key) = pressed.next() else {
        return;
    };
    if pressed.next().is_some() {
        state.shortcut_notice = "一次只按一个主键，可同时按Ctrl、Shift或Alt".into();
        state.shortcut_notice_kind = EditorShortcutNoticeKind::Warning;
        return;
    }
    if keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]) {
        state.shortcut_notice = "系统组合键不能绑定到编辑器".into();
        state.shortcut_notice_kind = EditorShortcutNoticeKind::Warning;
        return;
    }
    let binding = EditorKeyBinding::from_input(key, keys);
    let mut updated = settings.editor_keybindings.clone(); // 固定十二项；验证失败和重复选择都不触发保存。
    match updated.assign(action, Some(binding)) {
        Ok(()) => {
            if updated != settings.editor_keybindings {
                settings.editor_keybindings = updated;
            }
            state.listening = None;
            state.shortcut_notice = format!("“{}”已设置为{}", action.label(), binding.display());
            state.shortcut_notice_kind = EditorShortcutNoticeKind::Success;
        }
        Err(EditorShortcutConflict::InUse(other)) => {
            state.shortcut_notice_kind = EditorShortcutNoticeKind::Warning;
            state.shortcut_notice = format!(
                "{}已用于“{}”，请换键或先清除原绑定",
                binding.display(),
                other.label()
            );
        }
        Err(EditorShortcutConflict::ReservedKey) => {
            state.shortcut_notice = "该按键属于固定入口，请选择其他按键".into();
            state.shortcut_notice_kind = EditorShortcutNoticeKind::Warning;
        }
    }
}

pub(crate) fn sync_shortcut_settings_panel(
    mut commands: Commands,
    state: Res<MapEditorSettingsState>,
    settings: Res<EditorSettings>,
    bodies: Query<Entity, With<MapEditorShortcutSettingsBody>>,
    tracks: Query<Entity, (With<ShortcutScrollbarTrack>, Without<Scrollbar>)>,
    mut thumbs: Query<&mut ScrollbarDragState, With<ShortcutScrollbarThumb>>,
    mut labels: Query<
        (
            &MapEditorShortcutBindingText,
            &mut Text,
            Option<&mut TextColor>,
        ),
        Without<MapEditorShortcutNoticeText>,
    >,
    mut notices: Query<
        (&mut Text, Option<&mut TextColor>),
        (
            With<MapEditorShortcutNoticeText>,
            Without<MapEditorShortcutBindingText>,
        ),
    >,
    mut cards: Query<
        (&mut BackgroundColor, &mut BorderColor, Option<&mut Node>),
        With<ShortcutNoticeCard>,
    >,
) {
    // BSN先生成固定实体，再一次性绑定原生滚动条；已有正文和输入所有权保持唯一。
    if let Ok(body) = bodies.single() {
        for track in &tracks {
            commands
                .entity(track)
                .insert(Scrollbar::new(body, ControlOrientation::Vertical, 20.0));
        }
    }
    if !state.open || !state.shortcuts {
        for mut thumb in &mut thumbs {
            if thumb.dragging {
                thumb.dragging = false;
            }
        }
    }
    if !state.is_changed() && !settings.is_changed() {
        return;
    }
    for (action, mut text, color) in &mut labels {
        let label = if state.listening == Some(action.0) {
            "按新组合键…".into()
        } else {
            settings
                .editor_keybindings
                .get(action.0)
                .map_or_else(|| "未绑定".into(), EditorKeyBinding::display)
        };
        if text.as_str() != label {
            **text = label;
        }
        if let Some(mut color) = color {
            let wanted = if state.listening == Some(action.0)
                || settings.editor_keybindings.get(action.0).is_some()
            {
                colors::TEXT_ACCENT
            } else {
                colors::TEXT_MUTED
            };
            color.set_if_neq(TextColor(wanted));
        }
    }
    for (mut text, color) in &mut notices {
        let label = if state.shortcut_notice.is_empty() {
            "点击键位录入组合键；Esc取消。修改自动保存。"
        } else {
            state.shortcut_notice.as_str()
        };
        if text.as_str() != label {
            **text = label.into();
        }
        if let Some(mut color) = color {
            color.set_if_neq(TextColor(match state.shortcut_notice_kind {
                EditorShortcutNoticeKind::Idle => colors::TEXT_MUTED,
                EditorShortcutNoticeKind::Listening => colors::TEXT_ACCENT,
                EditorShortcutNoticeKind::Success => colors::TEXT_PRIMARY,
                EditorShortcutNoticeKind::Warning => colors::DANGER_TEXT,
            }));
        }
    }
    for (mut background, mut border, node) in &mut cards {
        if let Some(mut node) = node {
            let display = if state.open && state.shortcuts && !state.shortcut_notice.is_empty() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
        let (fill, edge) = match state.shortcut_notice_kind {
            EditorShortcutNoticeKind::Warning => (colors::DANGER_BG, colors::DANGER_BORDER),
            EditorShortcutNoticeKind::Listening => (colors::SELECT_BG, colors::COPPER_BORDER),
            _ => (colors::PANEL_BG_SECTION, colors::COPPER_BORDER_WEAK),
        };
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(edge));
    }
}

pub(crate) fn scroll_shortcut_settings(
    mut wheels: MessageReader<MouseWheel>,
    editor: Res<MapEditorState>,
    state: Res<MapEditorSettingsState>,
    mut was_open: Local<bool>,
    tracks: Query<&RelativeCursorPosition, With<ShortcutScrollbarTrack>>,
    mut bodies: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<MapEditorShortcutSettingsBody>,
    >,
) {
    // 每页消费自己的滚轮消息；切页/关闭归顶，像素按真实UI与DPI换算而非仅UiScale。
    let (lines, pixels) = wheels.read().filter(|event| event.y.is_finite()).fold(
        (0.0, 0.0),
        |(lines, pixels), event| match event.unit {
            MouseScrollUnit::Line => (lines + event.y, pixels),
            MouseScrollUnit::Pixel => (lines, pixels + event.y),
        },
    );
    let visible = editor.open && state.open && state.shortcuts;
    let reopened = visible && !*was_open;
    *was_open = visible;
    let over_track = tracks.iter().any(RelativeCursorPosition::cursor_over);
    for (cursor, node, mut position) in &mut bodies {
        if !visible || reopened {
            if position.x != 0.0 || position.y != 0.0 {
                *position = ScrollPosition::default();
            }
            continue;
        }
        if node.size().min_element() <= 0.0 {
            continue;
        }
        let maximum =
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        let amount = if cursor.cursor_over() || over_track {
            lines * 24.0 + pixels * node.inverse_scale_factor()
        } else {
            0.0
        };
        let wanted = (position.y - amount).clamp(0.0, maximum);
        if position.y != wanted {
            position.y = wanted;
        }
    }
}

#[cfg(test)]
#[path = "shortcut_settings_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "shortcut_settings_layout_tests.rs"]
mod layout_tests;
