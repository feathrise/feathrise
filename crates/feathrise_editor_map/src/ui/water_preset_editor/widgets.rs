//! 水体文档的紧凑控件与弹性表单；保留原名称输入及数值提交协议。

use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter};
use bevy::ui::FocusPolicy;
use feathrise_map_content::WaterPresetDescriptor;

use super::components::WaterButtonKind;
use super::fields::field_value;
use super::types::{
    WaterPresetAction, WaterPresetButton, WaterPresetEditorState, WaterPresetField,
};
use crate::ui::text_input::EditorTextInput;
use crate::ui::theme as colors;

pub(super) fn text_font(face: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: face.clone().into(),
        font_size: ui_metrics::font_size(size).into(),
        ..default()
    }
}

pub(super) fn readonly(parent: &mut ChildSpawnerCommands, face: &Handle<Font>, value: &str) {
    parent.spawn((
        Text::new(value),
        text_font(face, 11.0),
        TextColor(colors::TEXT_MUTED),
        TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            ..default()
        },
    ));
}

pub(super) fn row(parent: &mut ChildSpawnerCommands, body: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(7.0),
            row_gap: Val::Px(3.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(body);
}

pub(super) fn action_button(
    parent: &mut ChildSpawnerCommands,
    face: &Handle<Font>,
    label: &str,
    action: WaterPresetAction,
    kind: WaterButtonKind,
    selected: bool,
) {
    let navigation = kind == WaterButtonKind::Navigation;
    let primary = kind == WaterButtonKind::Primary;
    let emphasized = primary || selected;
    parent
        .spawn((
            Button,
            WaterPresetButton(action),
            kind,
            FocusPolicy::Block,
            Node {
                min_height: Val::Px(if primary { 26.0 } else { 24.0 }),
                max_width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: if navigation {
                    UiRect::bottom(Val::Px(2.0))
                } else {
                    UiRect::all(Val::Px(1.0))
                },
                border_radius: BorderRadius::all(Val::Px(if navigation { 0.0 } else { 3.0 })),
                ..default()
            }, // 顶部页签只用底线；按钮按文字自然测宽，窄窗口整枚换行。
            BackgroundColor(if emphasized {
                colors::SELECT_BG
            } else if navigation {
                Color::NONE
            } else {
                colors::BUTTON_DEV_SMALL_NORMAL
            }),
            BorderColor::all(if emphasized {
                colors::COPPER_BORDER
            } else if navigation {
                Color::NONE
            } else {
                colors::COPPER_BORDER_WEAK
            }),
        ))
        .with_child((
            feathrise_editor_ui::localization::editor_label(label),
            text_font(face, 11.0),
            TextColor(colors::TEXT_PRIMARY),
            TextLayout::no_wrap(),
            FocusPolicy::Pass,
        ));
}

/// 分组仅有标题与一条弱分隔线，不嵌套游戏菜单面板。
pub(super) fn group(
    parent: &mut ChildSpawnerCommands,
    face: &Handle<Font>,
    heading: &str,
    help: &str,
    body: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::top(Val::Px(8.0)),
                border: UiRect::top(Val::Px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(colors::COPPER_BORDER_WEAK),
        ))
        .with_children(|section| {
            section.spawn((
                Text::new(heading),
                text_font(face, 13.0),
                TextColor(colors::TEXT_PRIMARY),
                Node { max_width: Val::Percent(100.0), padding: UiRect::right(Val::Px(22.0)), ..default() },
            ));
            if !help.is_empty() { feathrise_editor_ui::context_help::spawn_help_button(section, face, heading, help); }
            body(section);
        });
}

pub(super) fn field(
    parent: &mut ChildSpawnerCommands,
    face: &Handle<Font>,
    state: &WaterPresetEditorState,
    descriptor: &WaterPresetDescriptor,
    label: &str,
    field: WaterPresetField,
) {
    let active = state.active_field == Some(field);
    let value = if active {
        state.input_buffer.clone()
    } else {
        field_value(descriptor, field)
    };
    parent
        .spawn(Node {
            min_width: Val::Px(0.0),
            max_width: Val::Percent(100.0),
            flex_basis: Val::Px(180.0),
            flex_grow: 1.0,
            flex_shrink: 1.0,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(ui_metrics::GAP),
            ..default()
        })
        .with_children(|column| {
            column.spawn((feathrise_editor_ui::localization::editor_label(label), text_font(face, ui_metrics::FONT_CONTROL), TextColor(colors::TEXT_MUTED),
                TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                Node { width: Val::Px(96.0), height: Val::Px(ui_metrics::label_height(label, 96.0)), min_width: Val::Px(0.0), flex_shrink: 0.0, ..default() }));
            let mut input = column.spawn((
                Button,
                WaterPresetButton(WaterPresetAction::Focus(field)),
                Node {
                    width: Val::Px(0.0),
                    flex_grow: 1.0,
                    flex_basis: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    height: Val::Px(24.0),
                    flex_shrink: 1.0,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(Val::Px(3.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(colors::INPUT_BG),
                BorderColor::all(if active {
                    colors::COPPER_BORDER
                } else {
                    colors::COPPER_BORDER_WEAK
                }),
            ));
            if field.accepts_text() {
                input.insert((
                    EditorTextInput,
                    EditableText {
                        max_characters: Some(96),
                        allow_newlines: false,
                        ..EditableText::new(&value)
                    },
                    EditableTextFilter::new(|character| !character.is_control()),
                    TextLayout::no_wrap(),
                    text_font(face, 11.0),
                    TextColor(colors::TEXT_VALUE),
                )); // 官方光标、选区、IME及名称约束原样保留，稳定帧不替换实体。
            } else {
                input.with_child((
                    Text::new(value),
                    text_font(face, 11.0),
                    TextColor(colors::TEXT_VALUE),
                    TextLayout::no_wrap(),
                    FocusPolicy::Pass,
                )); // 继续走原数值缓冲与Enter/Tab验证，不改变数值算法或键位。
            }
        });
}

/// 作者源保持只读路径与明确采用/清空操作；浏览文件不隐式写草稿。
pub(super) fn texture_row(
    parent: &mut ChildSpawnerCommands,
    face: &Handle<Font>,
    label: &str,
    path: Option<&str>,
    layer: usize,
) {
    readonly(parent, face, label);
    row(parent, |assets| {
        assets.spawn((
            Text::new(path.unwrap_or("未指定 · 中性回退（该层关闭）")),
            text_font(face, 11.0),
            TextColor(colors::TEXT_VALUE),
            TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
            Node {
                min_width: Val::Px(0.0),
                flex_basis: Val::Px(260.0),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                max_width: Val::Percent(100.0),
                ..default()
            }, // 长路径自然换行；窄正文把操作换到下一行，不压住路径。
        ));
        action_button(
            assets,
            face,
            "采用所选图片",
            WaterPresetAction::ApplySelectedAsset(layer),
            WaterButtonKind::Ordinary,
            false,
        );
        action_button(
            assets,
            face,
            "清空",
            WaterPresetAction::ClearTexture(layer),
            WaterButtonKind::Ordinary,
            false,
        );
    });
}
