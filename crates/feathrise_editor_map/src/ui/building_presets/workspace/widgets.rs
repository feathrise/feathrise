//! 建筑工作台的弹性按钮、分组、只读来源和真实官方单行输入；保留原字段身份与解析。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::super::{components::BuildingButtonKind, fields::field_value, types::*};
use crate::ui::{text_input::EditorTextInput, theme as colors};
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, LineHeight};
use bevy::ui::FocusPolicy;
use feathrise_map_content::BuildingPresetDescriptor;

const CONTROL_HEIGHT: f32 = ui_metrics::CONTROL_HEIGHT;
const FIELD_LINE_HEIGHT: f32 = 14.0;

pub(super) fn text_font(font: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: font.clone().into(),
        font_size: ui_metrics::font_size(size).into(),
        ..default()
    }
}

pub(super) fn row(parent: &mut ChildSpawnerCommands, body: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            column_gap: Val::Px(7.0),
            row_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(body);
}

/// 自然宽度和整枚换行适配窄窗；导航仅用底线，不叠加菜单卡片。
pub(super) fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    action: BuildingPresetAction,
    kind: BuildingButtonKind,
    active: bool,
) {
    let navigation = kind == BuildingButtonKind::Navigation;
    let emphasized = active || kind == BuildingButtonKind::Primary;
    parent
        .spawn((
            InteractionButton,
            BuildingPresetButton(action),
            kind,
            Node {
                min_height: Val::Px(30.0),
                min_width: Val::Px(0.0),
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
            },
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
            FocusPolicy::Block,
        ))
        .with_child((
            feathrise_editor_ui::localization::editor_label(label),
            text_font(font, 11.0),
            TextColor(colors::TEXT_VALUE),
            TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
            FocusPolicy::Pass,
        ));
}

/// 身份与长路径完整按字符换行，不能以尾部裁切掩盖Scene/primitive来源。
pub(super) fn readonly(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    value: &str,
) {
    parent.spawn((
        Text::new(format!("{label}：{value}")),
        text_font(font, 11.0),
        TextColor(colors::TEXT_MUTED),
        TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            ..default()
        },
        FocusPolicy::Pass,
    ));
}

pub(super) fn group(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    title: &str,
    help: &str,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            padding: UiRect::top(Val::Px(6.0)),
            ..default()
        })
        .with_children(|heading| {
            heading.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(1.0),
                    ..default()
                },
                BackgroundColor(colors::COPPER_BORDER_WEAK),
            ));
            heading.spawn((
                feathrise_editor_ui::localization::editor_label(title),
                text_font(font, 13.0),
                TextColor(colors::TEXT_PRIMARY),
                Node { max_width: Val::Percent(100.0), padding: UiRect::right(Val::Px(22.0)), ..default() },
            ));
            if !help.is_empty() {
                feathrise_editor_ui::context_help::spawn_help_button(heading, font, title, help);
            }
        });
}

fn control_node(text: bool) -> Node {
    Node {
        width: Val::Px(0.0),
        flex_grow: 1.0,
        flex_basis: Val::Px(0.0),
        min_width: Val::Px(0.0),
        height: Val::Px(CONTROL_HEIGHT),
        flex_shrink: 0.0,
        align_items: AlignItems::Center,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(3.0)),
        padding: UiRect::axes(
            Val::Px(7.0),
            Val::Px(if text {
                (CONTROL_HEIGHT - 2.0 - FIELD_LINE_HEIGHT) * 0.5
            } else {
                0.0
            }),
        ), // 原Button与EditableText同实体：对称内边距居中官方行框，同时保留整个32px点击区。
        overflow: Overflow::clip(),
        ..default()
    }
}

/// 与其余预设库共用标签/值同行布局；名称和连接技术字段仍独占整行。
pub(super) fn field(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &BuildingPresetEditorState,
    descriptor: &BuildingPresetDescriptor,
    label: &str,
    field: BuildingPresetField,
) {
    let active = state.active_field == Some(field);
    let value = if active {
        state.input_buffer.clone()
    } else {
        field_value(descriptor, field)
    };
    parent
        .spawn(Node {
            width: if field.accepts_text() {
                Val::Percent(100.0)
            } else {
                Val::Auto
            },
            min_width: Val::Px(0.0),
            max_width: Val::Percent(100.0),
            flex_basis: if field.accepts_text() {
                Val::Auto
            } else {
                Val::Px(200.0)
            },
            flex_grow: 1.0,
            flex_shrink: 1.0,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(ui_metrics::GAP),
            ..default()
        })
        .with_children(|column| {
            column.spawn((feathrise_editor_ui::localization::editor_label(label), text_font(font, ui_metrics::FONT_CONTROL),
                TextColor(colors::TEXT_MUTED), TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                Node { width: Val::Px(96.0), height: Val::Px(ui_metrics::label_height(label, 96.0)), min_width: Val::Px(0.0), flex_shrink: 0.0, ..default() }));
            let mut control = column.spawn((
                InteractionButton,
                BuildingPresetButton(BuildingPresetAction::Focus(field)),
                BuildingButtonKind::Field,
                control_node(field.accepts_text()),
                BackgroundColor(colors::INPUT_BG),
                BorderColor::all(if active {
                    colors::COPPER_BORDER
                } else {
                    colors::COPPER_BORDER_WEAK
                }),
                FocusPolicy::Block,
            ));
            if field.accepts_text() {
                let filter = if field == BuildingPresetField::Label {
                    EditableTextFilter::new(|character| !character.is_control())
                } else {
                    EditableTextFilter::new(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                    })
                };
                control.insert((
                    EditorTextInput,
                    EditableText {
                        max_characters: Some(128),
                        allow_newlines: false,
                        ..EditableText::new(&value)
                    },
                    filter,
                    TextLayout::no_wrap(),
                    text_font(font, 11.0),
                    LineHeight::Px(FIELD_LINE_HEIGHT),
                    TextColor(colors::TEXT_VALUE),
                )); // 维持原按钮/文本实体身份及中文、ASCII、光标、选区、横滚和IME协议。
            } else {
                control.with_child((
                    Text::new(format!("{value}{}", if active { "|" } else { "" })),
                    text_font(font, 11.0),
                    LineHeight::Px(FIELD_LINE_HEIGHT),
                    TextColor(colors::TEXT_VALUE),
                    TextLayout::no_wrap(),
                    FocusPolicy::Pass,
                )); // 数值仍显示原有界缓冲和光标，由原字段解析器提交。
            }
        });
}

pub(super) fn fields(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &BuildingPresetEditorState,
    descriptor: &BuildingPresetDescriptor,
    entries: &[(&str, BuildingPresetField)],
) {
    for (label, entry) in entries {
        field(parent, font, state, descriptor, label, *entry);
    }
}
