//! 植被工作台的弹性字段、顶部导航和轻量分组；输入仍由原官方文本会话管理。

use feathrise_editor_ui::metrics as ui_metrics;
use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use feathrise_map_content::VegetationPresetDescriptor;

use super::super::components::{PresetButtonKind, PresetEditorNavigation, PresetPathValue};
use super::super::fields::field_value;
use super::super::types::{
    BladeLodField, MeshPartField, PresetEditorAction, PresetEditorButton, PresetEditorFieldButton,
    PresetEditorFieldText, PresetEditorSection, PresetField,
};

const FORM_CONTROL_HEIGHT: f32 = 24.0;

pub(super) fn text_font(font: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: font.clone().into(),
        font_size: ui_metrics::font_size(size).into(),
        ..default()
    }
}

/// 窄窗把整枚控件换到下一行，不用固定按钮宽度裁切中文。
pub(super) fn spawn_row(
    parent: &mut ChildSpawnerCommands,
    body: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            column_gap: Val::Px(7.0),
            row_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(body);
}

pub(super) fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    action: PresetEditorAction,
    kind: PresetButtonKind,
    active: bool,
) {
    let navigation = kind == PresetButtonKind::Navigation;
    let emphasized = kind == PresetButtonKind::Primary || active;
    parent
        .spawn((
            Button,
            PresetEditorButton(action),
            kind,
            Node {
                min_height: Val::Px(24.0),
                max_width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
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
            TextColor(colors::TEXT_PRIMARY),
            TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
            FocusPolicy::Pass,
        ));
}

/// 旧调用点的宽度提示不再决定按钮尺寸，结构动作与选择按钮都使用真实字体测宽。
pub(super) fn spawn_action_button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    action: PresetEditorAction,
    _width: f32,
    active: bool,
) {
    spawn_button(
        parent,
        font,
        label,
        action,
        PresetButtonKind::Ordinary,
        active,
    );
}

pub(super) fn spawn_readonly(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    value: &str,
) {
    parent.spawn((
        Text::new(if value.is_empty() {
            label.to_owned()
        } else {
            format!("{label}：{value}")
        }),
        text_font(font, 11.0),
        TextColor(colors::TEXT_MUTED),
        TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            flex_shrink: 0.0,
            ..default()
        },
        FocusPolicy::Pass,
    ));
}

fn field_column_node(full_width: bool) -> Node {
    Node {
        width: if full_width {
            Val::Percent(100.0)
        } else {
            Val::Auto
        },
        min_width: Val::Px(0.0),
        max_width: Val::Percent(100.0),
        flex_basis: if full_width {
            Val::Auto
        } else {
            Val::Px(200.0)
        },
        flex_grow: 1.0,
        flex_shrink: 1.0,
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(2.0),
        ..default()
    }
}

fn field_control_node() -> Node {
    Node {
        height: Val::Px(FORM_CONTROL_HEIGHT),
        flex_basis: Val::Px(220.0),
        flex_grow: 1.0,
        min_width: Val::Px(0.0),
        max_width: Val::Percent(100.0),
        align_items: AlignItems::Center,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(3.0)),
        padding: UiRect::horizontal(Val::Px(7.0)),
        overflow: Overflow::clip_x(),
        ..default()
    }
}

pub(super) fn is_asset_path(field: PresetField) -> bool {
    matches!(
        field,
        PresetField::BladeBaseColorPath
            | PresetField::BladeLod {
                field: BladeLodField::MeshPath,
                ..
            }
            | PresetField::MeshPart {
                field: MeshPartField::MeshPath | MeshPartField::MaterialPath,
                ..
            }
    )
}

/// 普通字段采用标签/值同行；路径镜像单独换行，资产动作保留原明确采用入口。
pub(super) fn spawn_field(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    descriptor: &VegetationPresetDescriptor,
    label: &str,
    field: PresetField,
) {
    let value = field_value(descriptor, field).unwrap_or_else(|_| "—".to_owned());
    parent
        .spawn(field_column_node(field.accepts_text()))
        .with_children(|column| {
            column.spawn(Node { width: Val::Percent(100.0), min_width: Val::Px(0.0),
                align_items: AlignItems::Center, column_gap: Val::Px(ui_metrics::GAP), ..default() })
            .with_children(|line| {
                line.spawn((feathrise_editor_ui::localization::editor_label(label), text_font(font, ui_metrics::FONT_CONTROL), TextColor(colors::TEXT_MUTED),
                    TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                    Node { width: Val::Px(96.0), height: Val::Px(ui_metrics::label_height(label, 96.0)), min_width: Val::Px(0.0), flex_shrink: 0.0, ..default() }));
                line.spawn(Node { min_width: Val::Px(0.0), flex_grow: 1.0, flex_basis: Val::Px(0.0),
                    flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(ui_metrics::GAP), row_gap: Val::Px(ui_metrics::GAP), ..default() }).with_children(|row| {
                row.spawn((
                    Button,
                    PresetEditorButton(PresetEditorAction::Focus(field)),
                    PresetEditorFieldButton(field),
                    field_control_node(),
                    BackgroundColor(colors::INPUT_BG),
                    BorderColor::all(colors::COPPER_BORDER_WEAK),
                    FocusPolicy::Block,
                ))
                .with_children(|control| {
                    if field.accepts_text() {
                        control.spawn((
                            PresetEditorFieldText(field),
                            super::super::interaction::text_bundle(&value, field),
                            TextLayout::no_wrap(),
                            Node {
                                width: Val::Percent(100.0),
                                ..default()
                            }, // 官方单行测量提供真实行高，父值框居中整行；撑满框高会让字和光标贴顶。
                            text_font(font, 11.0),
                            TextColor(colors::TEXT_VALUE),
                            FocusPolicy::Block,
                        )); // 原官方实体继续负责中文、选区、光标和横滚，不另造隐藏输入代理。
                    } else {
                        control.spawn((
                            PresetEditorFieldText(field),
                            Text::new(&value),
                            text_font(font, 11.0),
                            TextColor(colors::TEXT_VALUE),
                            TextLayout::no_wrap(),
                            FocusPolicy::Pass,
                        )); // 保留原数值缓冲、解析和提交键位。
                    }
                });
                super::super::asset_reference::spawn_field_actions(row, font, field);
                });
            });
            if is_asset_path(field) {
                column.spawn((
                    PresetPathValue(field),
                    Text::new(if value.is_empty() {
                        "未指定"
                    } else {
                        &value
                    }),
                    text_font(font, 10.0),
                    TextColor(colors::TEXT_MUTED),
                    TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                    Node {
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        ..default()
                    },
                    FocusPolicy::Pass,
                )); // 完整路径回显可自然换行，输入本体仍是原单行协议。
            }
        });
}

/// 轻量分组不叠加游戏菜单卡片；整行标题让相关参数只在本组内排多列。
pub(super) fn spawn_group_title(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    title: &str,
    detail: &str,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                border: UiRect::top(Val::Px(1.0)),
                padding: UiRect::top(Val::Px(8.0)),
                ..default()
            },
            BorderColor::all(colors::COPPER_BORDER_WEAK),
            FocusPolicy::Pass,
        ))
        .with_children(|group| {
            group.spawn((
                feathrise_editor_ui::localization::editor_label(title),
                text_font(font, 13.0),
                TextColor(colors::TEXT_PRIMARY),
                TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                Node {
                    width: Val::Percent(100.0),
                    min_width: Val::Px(0.0),
                    padding: UiRect::right(Val::Px(22.0)), // 同组帮助固定在标题右侧。
                    ..default()
                },
            ));
            if !detail.is_empty() {
                feathrise_editor_ui::context_help::spawn_help_button(group, font, title, detail);
            }
        });
}

pub(super) fn spawn_section_nav(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    sections: &[(PresetEditorSection, &str)],
    active: PresetEditorSection,
) {
    parent
        .spawn((
            PresetEditorNavigation,
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                row_gap: Val::Px(2.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(colors::COPPER_BORDER_WEAK),
        ))
        .with_children(|row| {
            for (section, label) in sections {
                spawn_button(
                    row,
                    font,
                    label,
                    PresetEditorAction::SelectSection(*section),
                    PresetButtonKind::Navigation,
                    *section == active,
                );
            }
        });
}

pub(super) fn spawn_selector_row(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    labels: impl IntoIterator<Item = (String, PresetEditorAction, bool)>,
) {
    spawn_row(parent, |row| {
        for (label, action, active) in labels {
            spawn_action_button(row, font, &label, action, 0.0, active);
        }
    });
}

pub(super) fn spawn_action_row(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    actions: &[(&str, PresetEditorAction, f32)],
) {
    spawn_row(parent, |row| {
        for (label, action, width) in actions {
            spawn_action_button(row, font, label, *action, *width, false);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_wrap_without_fixed_label_columns_or_clipped_identity() {
        let numeric = field_column_node(false);
        assert_eq!(numeric.flex_basis, Val::Px(200.0));
        assert_eq!(numeric.max_width, Val::Percent(100.0));
        assert_eq!(numeric.flex_direction, FlexDirection::Column);
        let path = field_column_node(true);
        assert_eq!(path.width, Val::Percent(100.0));
        assert_eq!(field_control_node().height, Val::Px(24.0));
    }

    #[test]
    fn path_mirrors_cover_only_asset_fields_not_names_or_notes() {
        assert!(is_asset_path(PresetField::BladeBaseColorPath));
        assert!(is_asset_path(PresetField::MeshPart {
            lod: 7,
            part: 12,
            field: MeshPartField::MaterialPath
        }));
        assert!(!is_asset_path(PresetField::Label));
        assert!(!is_asset_path(PresetField::Notes));
        assert!(!is_asset_path(PresetField::Seed));
    }
}
