//! 植被工作台的可选多行备注：标签在上、六行视区、独立滚动；不改变输入事务。

use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::text::LineHeight;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use feathrise_map_content::VegetationPresetDescriptor;

use super::super::fields::field_value;
use super::super::types::{
    PresetEditorAction, PresetEditorButton, PresetEditorFieldButton, PresetEditorFieldText,
    PresetField, PresetNotesScroll,
};
use super::widgets::text_font;

pub(super) const NOTES_LINE_HEIGHT: f32 = 18.0;
const NOTES_VIEW_HEIGHT: f32 = NOTES_LINE_HEIGHT * 6.0 + 2.0;

fn notes_control_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Px(NOTES_VIEW_HEIGHT),
        min_width: Val::Px(0.0),
        flex_shrink: 0.0,
        align_items: AlignItems::Start,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(3.0)),
        padding: UiRect::horizontal(Val::Px(7.0)),
        overflow: Overflow::scroll_y(),
        ..default()
    } // 固定六行和独立滚动仍由正式TextScroll处理，长备注不撑宽或推高输入框。
}

pub(super) fn spawn_notes(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    descriptor: &VegetationPresetDescriptor,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|column| {
            column.spawn(Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                height: Val::Px(NOTES_LINE_HEIGHT),
                padding: UiRect::right(Val::Px(22.0)),
                flex_shrink: 0.0,
                ..default()
            }).with_children(|header| {
                header.spawn((
                    Text::new("备注（可选）"),
                    text_font(font, 11.0),
                    LineHeight::Px(NOTES_LINE_HEIGHT),
                    TextColor(colors::TEXT_MUTED),
                    FocusPolicy::Pass,
                ));
                // 快捷键说明不占备注正文；保留原六行输入、选区与提交事务。
                feathrise_editor_ui::context_help::spawn_help_button(header, font, "备注输入",
                    "Enter 换行 · Ctrl+Enter / Tab 提交 · Ctrl+A 全选 · Ctrl+V 粘贴 · Esc 放弃输入");
            });
            column
                .spawn((
                    Button,
                    PresetEditorButton(PresetEditorAction::Focus(PresetField::Notes)),
                    PresetEditorFieldButton(PresetField::Notes),
                    ScrollPosition::default(),
                    notes_control_node(),
                    BackgroundColor(colors::SLOT_BG),
                    BorderColor::all(colors::COPPER_BORDER_WEAK),
                    FocusPolicy::Block,
                ))
                .with_child((
                    PresetEditorFieldText(PresetField::Notes),
                    PresetNotesScroll,
                    RelativeCursorPosition::default(),
                    super::super::interaction::text_bundle(
                        &field_value(descriptor, PresetField::Notes).unwrap_or_default(),
                        PresetField::Notes,
                    ),
                    text_font(font, 11.0),
                    LineHeight::Px(NOTES_LINE_HEIGHT),
                    TextLayout {
                        linebreak: LineBreak::WordOrCharacter,
                        ..default()
                    },
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    TextColor(colors::TEXT_VALUE),
                    FocusPolicy::Block,
                )); // 保留真实官方实体、光标选区及Some("")清空语义，UI换布局不改原字段提交链。
        });
}

#[cfg(test)]
mod tests;
