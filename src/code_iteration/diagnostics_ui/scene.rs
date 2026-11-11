//! 诊断浮窗复用共同弱边界、标题、正文、底栏和原生滚动条；无第二个源码编辑器。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::{ROW_HEIGHT, ROW_POOL}};
use bevy::{picking::hover::Hovered, prelude::*, ui::{FocusPolicy, RelativeCursorPosition},
    ui_widgets::{ControlOrientation, Scrollbar, ScrollbarThumb}};
use feathrise_editor_scene::workbench::{WorkbenchFloatingPanel, WorkbenchHeader};
use feathrise_editor_ui::colors as c;

pub(super) fn mount(mut commands: Commands, headers: Query<(Entity, Option<&ChildOf>), Added<WorkbenchHeader>>, assets: Res<AssetServer>) {
    if headers.is_empty() { return; }
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for (header, parent) in &headers {
        let root = parent.map_or(header, ChildOf::parent);
        commands.entity(root).with_children(|root| {
            // 代码与日志可同时打开；重叠处由上层诊断面板独自接收鼠标。
            root.spawn((DiagnosticsOverlay, WorkbenchFloatingPanel, GlobalZIndex(424), FocusPolicy::Block, UiInteraction::None,
                Node { display: Display::None, position_type: PositionType::Absolute, right: px(8), bottom: px(32),
                    width: percent(76.), max_width: px(1200), height: percent(50.), max_height: px(700), ..default() },
            )).with_children(|overlay| {
                overlay.spawn((Node { width: percent(100), height: percent(100),
                    min_width: px(0), min_height: px(0), flex_direction: FlexDirection::Column, overflow: Overflow::clip(),
                    ..feathrise_editor_ui::dialog_panel_node() },
                    BackgroundColor(c::DIALOG_BG), BorderColor::all(c::DIALOG_BORDER), FocusPolicy::Block,
                )).with_children(|panel| {
                    panel.spawn((feathrise_editor_ui::dialog_header_node(), BackgroundColor(c::DIALOG_HEADER_BG),
                        BorderColor::all(c::DIALOG_BORDER),
                    )).with_children(|header| {
                        header.spawn(label(&font, "代码诊断", 13., c::TEXT_PRIMARY));
                        let style = feathrise_editor_ui::dialog_close_button_style();
                        header.spawn((InteractionButton, DiagnosticsClose, Node { margin: UiRect::left(Val::Auto), ..style.node },
                            style.colors, BackgroundColor(style.colors.normal), BorderColor::all(c::DIALOG_BORDER),
                        )).with_child(label(&font, "×", style.font_size, c::TEXT_PRIMARY));
                    });
                    panel.spawn((Node { min_width: px(0), min_height: px(0), flex_grow: 1., flex_direction: FlexDirection::Column,
                        row_gap: px(8), ..feathrise_editor_ui::dialog_body_node() }, BackgroundColor(c::DIALOG_BG),
                    )).with_children(|body| {
                        body.spawn((DiagnosticsStatus, label(&font, "", 12., c::TEXT_PRIMARY),
                            // 状态只占一行；完整编译正文归原工程日志，不能挤占实际诊断列表。
                            Node { min_width: px(0), max_width: percent(100), height: px(20), min_height: px(20), max_height: px(20),
                                flex_shrink: 0., overflow: Overflow::clip(), ..default() }));
                        body.spawn((DiagnosticsEmpty, label(&font, "还没有编译诊断。保存源码后将自动后台编译。", 12., c::TEXT_MUTED),
                            Node { display: Display::None, ..default() }));
                        body.spawn(Node { min_width: px(0), min_height: px(0), flex_grow: 1., column_gap: px(3), ..default() })
                            .with_children(|row| {
                                let target = row.spawn((DiagnosticsPane, RelativeCursorPosition::default(), ScrollPosition::default(),
                                    Node { min_width: px(0), min_height: px(0), flex_grow: 1., flex_basis: px(0),
                                        overflow: Overflow { x: OverflowAxis::Hidden, y: OverflowAxis::Scroll }, ..default() },
                                    BackgroundColor(c::DIALOG_BG), FocusPolicy::Block,
                                )).with_children(|pane| {
                                    pane.spawn((DiagnosticsContent, Node { min_width: percent(100), height: px(0), flex_shrink: 0., ..default() }))
                                        .with_children(|content| {
                                            for slot in 0..ROW_POOL {
                                                content.spawn((InteractionButton, DiagnosticRow { slot, index: None, revision: 0, window_revision: 0 },
                                                    Node { display: Display::None, position_type: PositionType::Absolute,
                                                        left: px(0), top: px(0), width: percent(100), height: px(ROW_HEIGHT),
                                                        padding: UiRect::axes(px(6), px(3)), flex_direction: FlexDirection::Column,
                                                        overflow: Overflow::clip(), ..default() },
                                                    feathrise_editor_ui::dialog_button_style(false).colors,
                                                    BackgroundColor(c::DIALOG_BG),
                                                )).with_children(|row| {
                                                    row.spawn((DiagnosticRowLabel { slot, message: false }, label(&font, "", 11., c::TEXT_MUTED)));
                                                    row.spawn((DiagnosticRowLabel { slot, message: true }, label(&font, "", 12., c::TEXT_PRIMARY)));
                                                });
                                            }
                                        });
                                }).id();
                                row.spawn((Scrollbar::new(target, ControlOrientation::Vertical, 24.), Hovered::default(),
                                    RelativeCursorPosition::default(), FocusPolicy::Block,
                                    Node { width: px(7), height: percent(100), min_width: px(7), min_height: px(7), flex_shrink: 0., ..default() },
                                    BackgroundColor(c::DIALOG_BG),
                                )).with_child((ScrollbarThumb { border_radius: BorderRadius::ZERO, ..default() }, BackgroundColor(c::DIALOG_BORDER)));
                            });
                    });
                    panel.spawn((feathrise_editor_ui::dialog_footer_node(), BackgroundColor(c::DIALOG_HEADER_BG),
                        BorderColor::all(c::DIALOG_BORDER),
                    )).with_child((DiagnosticsFooter, label(&font, "点击诊断位置在中央源码定位；完整输出可在日志中选择复制。", 11., c::TEXT_MUTED),
                        Node { min_width: px(0), overflow: Overflow::clip(), ..default() }));
                });
            });
        });
    }
}

fn label(font: &Handle<Font>, text: &str, size: f32, color: Color) -> impl Bundle {
    (feathrise_editor_ui::localization::editor_label(text), TextFont { font: font.clone().into(), font_size: size.into(), ..default() }, TextColor(color),
        TextLayout::no_wrap(), Pickable::IGNORE)
}
