//! 工程导出面板复用编辑器公共标题、正文和底栏配方；输入框始终只有一个原生EditableText。

use super::super::{components::*, resources::ExportState};
use bevy::{input_focus::tab_navigation::TabIndex, prelude::*, text::{EditableTextFilter, TextCursorStyle}, ui::{FocusPolicy, RelativeCursorPosition}};
use feathrise_editor_map::workbench::WorkbenchHeader;
use feathrise_editor_ui::colors as c;
use feathrise_editor_ui::localization::LocalizedText;

pub(in crate::project_export) fn mount(
    mut commands: Commands,
    headers: Query<Entity, Added<WorkbenchHeader>>,
    assets: Res<AssetServer>,
    state: Res<ExportState>,
) {
    if headers.is_empty() { return; }
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for header in &headers {
        commands.entity(header).with_children(|header| {
            header.spawn((ExportOverlay, GlobalZIndex(4360), FocusPolicy::Block, Interaction::None,
                Node { display: Display::None, position_type: PositionType::Absolute, left: px(0), top: px(0),
                    width: Val::Vw(100.), height: Val::Vh(100.), align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center, ..default() }, BackgroundColor(c::DIALOG_BACKDROP),
            )).with_children(|overlay| {
                overlay.spawn((Node { width: Val::Vw(94.), max_width: px(640), max_height: Val::Vh(94.),
                    min_width: px(0), min_height: px(0), overflow: Overflow::clip(), flex_direction: FlexDirection::Column,
                    ..feathrise_editor_ui::dialog_panel_node() }, BackgroundColor(c::DIALOG_BG),
                    BorderColor::all(c::DIALOG_BORDER), FocusPolicy::Block,
                )).with_children(|panel| {
                    panel.spawn((feathrise_editor_ui::dialog_header_node(), BackgroundColor(c::DIALOG_HEADER_BG),
                        BorderColor::all(c::DIALOG_BORDER),
                    )).with_children(|header| {
                        header.spawn(label(&font, "导出工程", 13., c::TEXT_PRIMARY));
                        let style = feathrise_editor_ui::dialog_close_button_style();
                        header.spawn((Button, ExportAction::Close, Node { margin: UiRect::left(Val::Auto), ..style.node },
                            style.colors, BackgroundColor(style.colors.normal), BorderColor::all(c::DIALOG_BORDER),
                        )).with_child(label(&font, "×", style.font_size, c::TEXT_PRIMARY));
                    });
                    panel.spawn((ExportBody, RelativeCursorPosition::default(), ScrollPosition::default(),
                        Node { row_gap: px(9), min_width: px(0), min_height: px(0), flex_shrink: 1.,
                        max_height: Val::Vh(64.), overflow: Overflow::scroll_y(),
                        flex_direction: FlexDirection::Column, ..feathrise_editor_ui::dialog_body_node() },
                        BackgroundColor(c::DIALOG_BG),
                    )).with_children(|body| {
                        body.spawn((label(&font, "导出目录", 12., c::TEXT_PRIMARY),
                            Node { flex_shrink: 0., ..default() }));
                        body.spawn((ExportDestination, feathrise_editor_core::EditorTextInput,
                            feathrise_editor_ui::single_line_input(&state.destination, 2048),
                            EditableTextFilter::new(|character| !character.is_control()),
                            Node { width: percent(100), min_width: px(0), height: px(30), flex_shrink: 0.,
                                padding: UiRect::axes(px(7), px(5)), border: UiRect::all(px(1)),
                                overflow: Overflow::clip_x(), ..default() },
                            TextFont { font: font.clone().into(), font_size: 13.0.into(), ..default() },
                            TextColor(c::TEXT_PRIMARY), TextLayout::no_wrap(),
                            TextCursorStyle { color: c::TEXT_PRIMARY, selection_color: c::SELECT_BG_HOVER,
                                unfocused_selection_color: c::SELECT_BG, selected_text_color: Some(c::TEXT_PRIMARY) },
                            BackgroundColor(c::INPUT_BG), BorderColor::all(c::COPPER_BORDER_WEAK), TabIndex(0),
                        ));
                        body.spawn(Node { column_gap: px(8), row_gap: px(5), flex_wrap: FlexWrap::Wrap,
                            flex_shrink: 0., ..default() })
                            .with_children(|row| {
                                button(row, &font, ExportAction::Browse, "选择目录…", false);
                            });
                        // 配置独占完整正文宽度，避免初次空文本在可换行按钮行里被压成零宽。
                        body.spawn((ExportProfile, wrapped_label(&font, "构建配置", 12., c::TEXT_MUTED),
                            Node { width: percent(100), min_width: px(0), flex_shrink: 0., ..default() }));
                        body.spawn((wrapped_label(&font,
                            "默认导出到工程文件夹的 output。导出使用已保存的场景、资产和源码；未保存草稿请先通过原保存入口保存。",
                            12., c::TEXT_MUTED), LocalizedText::new("project.export.path_help"),
                            Node { flex_shrink: 0., min_width: px(0), ..default() }));
                        // 状态投影本身有界，正文滚动承担长错误；不能再次裁掉错误尾部。
                        body.spawn((ExportStatus, wrapped_label(&font, "准备导出。", 12., c::TEXT_PRIMARY),
                            Node { min_width: px(0), flex_shrink: 0., ..default() }));
                    });
                    panel.spawn((Node { column_gap: px(8), row_gap: px(5), flex_wrap: FlexWrap::Wrap,
                        ..feathrise_editor_ui::dialog_footer_node() }, BackgroundColor(c::DIALOG_HEADER_BG),
                        BorderColor::all(c::DIALOG_BORDER),
                    )).with_children(|footer| {
                        button(footer, &font, ExportAction::OpenDirectory, "打开输出目录", false);
                        button(footer, &font, ExportAction::Start, "开始导出", true);
                        button(footer, &font, ExportAction::Cancel, "关闭 (Esc)", false);
                    });
                });
            });
        });
    }
}

fn label(font: &Handle<Font>, text: &str, size: f32, color: Color) -> impl Bundle {
    (feathrise_editor_ui::localization::editor_label(text), TextFont { font: font.clone().into(), font_size: size.into(), ..default() },
        TextColor(color), TextLayout::no_wrap(), Pickable::IGNORE)
}

fn wrapped_label(font: &Handle<Font>, text: &str, size: f32, color: Color) -> impl Bundle {
    (Text::new(text), TextFont { font: font.clone().into(), font_size: size.into(), ..default() },
        TextColor(color), TextLayout { linebreak: bevy::text::LineBreak::WordOrCharacter, ..default() }, Pickable::IGNORE)
}

fn button(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, action: ExportAction, text: &str, primary: bool) {
    let style = feathrise_editor_ui::dialog_button_style(primary);
    parent.spawn((Button, action, style.node, style.colors, BackgroundColor(style.colors.normal),
        BorderColor::all(c::DIALOG_BORDER),
    )).with_child((ExportActionLabel(action), Text::new(text),
        TextFont { font: font.clone().into(), font_size: style.font_size.into(), ..default() },
        TextColor(c::TEXT_PRIMARY), TextLayout::no_wrap(), Pickable::IGNORE));
}
