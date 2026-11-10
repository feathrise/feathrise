//! 工程日志的终端式视图：紧凑工具栏、原生阅读区和单行状态栏；不引入第二套输入或滚动逻辑。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::{LINE_HEIGHT, ProjectLogState}, search::{LogSearch, SEARCH_QUERY_CHARS}, search_paint::LogSearchBrush};
use bevy::{picking::hover::Hovered, prelude::*, text::{EditableText, EditableTextFilter, FontHinting, LineBreak, LineHeight, TextCursorStyle},
    input_focus::{FocusCause, InputFocus}, picking::{events::PointerPress, pointer::PointerButton},
    ui::{FocusPolicy, RelativeCursorPosition}, ui_widgets::{ControlOrientation, Scrollbar, ScrollbarThumb}};
use feathrise_editor_map::workbench::{WorkbenchFloatingPanel, WorkbenchHeader, WorkbenchProjectLogButton};
use feathrise_editor_ui::colors as c;

pub(super) const TERMINAL_BG: Color = Color::srgb(0.118, 0.118, 0.118);
pub(super) const TERMINAL_TEXT: Color = Color::srgb(0.80, 0.80, 0.80);

/// 日志开关只切换共享按钮的选中填充；轮廓和悬停与其他对话框一致。
pub(super) fn tool_button_colors(selected: bool) -> feathrise_editor_ui::ButtonColors {
    feathrise_editor_ui::dialog_button_style(selected).colors
}

pub(super) fn mount(
    mut commands: Commands,
    headers: Query<(Entity, Option<&ChildOf>), Added<WorkbenchHeader>>,
    mut buttons: Query<&mut Node, With<WorkbenchProjectLogButton>>,
    assets: Res<AssetServer>,
    mut fonts: ResMut<Assets<Font>>,
    mut terminal_font: Local<Option<Handle<Font>>>,
) {
    for mut node in &mut buttons {
        if node.display == Display::None { node.display = Display::Flex; }
    }
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    if headers.is_empty() { return; }
    // 显式保留Bevy内嵌的等宽字体，不能依赖用户系统字体，也不能被SDK中文默认字体覆盖。
    // 已注册的中文字体继续承担缺字回退；命中层与绘制层使用完全相同的字形参数。
    let terminal_font = terminal_font.get_or_insert_with(|| fonts.add(
        Font::from_bytes(bevy::text::DEFAULT_FONT_DATA.to_vec()),
    )).clone();
    for (header, parent) in &headers {
        let root = parent.map_or(header, ChildOf::parent);
        commands.entity(root).with_children(|root| {
            root.spawn((
                LogOverlay, WorkbenchFloatingPanel,
                GlobalZIndex(423), // 位于工作区之上，详情、右键菜单与管理模态仍可显示在前。
                FocusPolicy::Block,
                UiInteraction::None,
                Node {
                    display: Display::None, position_type: PositionType::Absolute,
                    // 以工作台根布局，留出底栏和部分视口，整块范围恰好等于日志面板。
                    right: px(8), bottom: px(32), width: percent(86.), max_width: px(1400), height: percent(60.),
                    ..default()
                },
            )).with_children(|overlay| {
                overlay.spawn((
                    LogPanel, FocusPolicy::Block,
                    // 外壳、标题和底栏复用编辑器UI库，终端字形只是正文的一种内容。
                    Node {
                        width: percent(100), height: percent(100), min_width: px(0), min_height: px(0),
                        flex_direction: FlexDirection::Column, overflow: Overflow::clip(),
                        ..feathrise_editor_ui::dialog_panel_node()
                    },
                    BackgroundColor(c::DIALOG_BG), BorderColor::all(c::DIALOG_BORDER),
                )).with_children(|panel| {
                    panel.spawn((
                        LogHeader,
                        feathrise_editor_ui::dialog_toolbar_header_node(),
                        BackgroundColor(c::DIALOG_HEADER_BG), BorderColor::all(c::DIALOG_BORDER),
                    )).with_children(|row| {
                        row.spawn((
                            LogTerminalTab,
                            Node { height: px(28), flex_shrink: 0., align_items: AlignItems::Center, ..default() },
                        )).with_child(caption(&font, "工程日志", feathrise_editor_ui::metrics::FONT_BODY, c::TEXT_PRIMARY));
                        row.spawn(Node {
                            min_width: px(0), max_width: percent(100), flex_grow: 1., flex_basis: px(220),
                            justify_content: JustifyContent::FlexEnd, align_items: AlignItems::Center,
                            flex_wrap: FlexWrap::Wrap, column_gap: px(4), row_gap: px(4), ..default()
                        }).with_children(|actions| {
                            button(actions, &font, LogAction::CopySelection, "复制选中", false);
                            button(actions, &font, LogAction::CopyAll, "复制全部", false);
                            button(actions, &font, LogAction::Follow, "跟随", false);
                            button(actions, &font, LogAction::Wrap, "自动换行", false);
                            button(actions, &font, LogAction::Search, "查找", false);
                            button(actions, &font, LogAction::Clear, "清空", false);
                        });
                        button(row, &font, LogAction::Close, "×", true);
                    });
                    search_bar(panel, &font);
                    panel.spawn(Node {
                        width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.,
                        flex_direction: FlexDirection::Column, ..default()
                    }).with_children(|reading| {
                        let mut target = Entity::PLACEHOLDER;
                        reading.spawn(Node {
                            min_width: px(0), min_height: px(0), flex_grow: 1., ..default()
                        }).with_children(|row| {
                            target = row.spawn((
                                LogScroll, RelativeCursorPosition::default(), ScrollPosition::default(),
                                FocusPolicy::Block,
                                Node {
                                    min_width: px(0), min_height: px(0), flex_grow: 1., flex_basis: px(0),
                                    overflow: Overflow { x: OverflowAxis::Hidden, y: OverflowAxis::Scroll },
                                    // 仅阅读容器保留一份共同正文内边距，避免双层padding。
                                    padding: feathrise_editor_ui::dialog_body_node().padding,
                                    ..default()
                                },
                                BackgroundColor(TERMINAL_BG),
                            )).with_children(|pane| {
                                // 完整高度只由占位节点提供；正文仍只排版当前可见的至多64行。
                                pane.spawn((LogContent, Node {
                                    min_width: percent(100), height: px(LINE_HEIGHT), flex_shrink: 0., ..default()
                                })).with_children(|content| {
                                content.spawn((
                                    LogBody::default(), Name::new("工程日志正文"),
                                    bevy::ui_widgets::TextInput,
                                    EditableText { visible_lines: None, allow_newlines: true, ..default() },
                                    TextFont { font: terminal_font.clone().into(), font_size: 13.0.into(), ..default() },
                                    FontHinting::Enabled,
                                    TextColor(Color::NONE), TextLayout::linebreak(LineBreak::AnyCharacter), LineHeight::Px(LINE_HEIGHT),
                                    TextCursorStyle {
                                        color: TERMINAL_TEXT, selection_color: Color::srgb(0.15, 0.31, 0.47),
                                        unfocused_selection_color: Color::srgb(0.23, 0.24, 0.25),
                                        selection_radius: 0.0, selected_text_color: None,
                                    },
                                    UiInteraction::None, FocusPolicy::Block,
                                    Node { position_type: PositionType::Absolute, left: px(0), top: px(0),
                                        min_width: percent(100), flex_shrink: 0., ..default() },
                                )).observe(focus_text_on_press);
                                // 此层不参与拾取。透明EditableText继续拥有官方字形命中、光标和蓝色选区。
                                content.spawn((
                                    LogDisplay::default(), feathrise_editor_ui::EditorLiteralColor,
                                    Name::new("工程日志终端字形"),
                                    Text::default(),
                                    TextFont { font: terminal_font.clone().into(), font_size: 13.0.into(), ..default() },
                                    FontHinting::Enabled,
                                    TextColor(TERMINAL_TEXT), TextLayout::linebreak(LineBreak::AnyCharacter), LineHeight::Px(LINE_HEIGHT),
                                    Pickable::IGNORE,
                                    Node { position_type: PositionType::Absolute, left: px(0), top: px(0),
                                        min_width: percent(100), flex_shrink: 0., ..default() },
                                )).with_children(LogSearchBrush::spawn);
                                });
                            }).id();
                            scrollbar(row, target, ControlOrientation::Vertical);
                        });
                        scrollbar(reading, target, ControlOrientation::Horizontal);
                    });
                    panel.spawn((
                        LogFooter,
                        feathrise_editor_ui::dialog_footer_node(),
                        BackgroundColor(c::DIALOG_HEADER_BG), BorderColor::all(c::DIALOG_BORDER),
                    )).with_child((
                        LogStatus, caption(&font, "", 11., c::TEXT_MUTED),
                        Node { min_width: px(0), flex_grow: 1., overflow: Overflow::clip_x(), ..default() },
                    ));
                });
            });
        });
    }
}

fn search_bar(panel: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    panel.spawn((
        LogSearchBar,
        Node {
            display: Display::None, flex_wrap: FlexWrap::Wrap, align_items: AlignItems::Center,
            column_gap: px(6), row_gap: px(4), padding: UiRect::axes(px(12), px(4)),
            flex_shrink: 0., min_width: px(0), ..default()
        },
        BackgroundColor(c::DIALOG_HEADER_BG),
    )).with_children(|row| {
        row.spawn(Node { flex_grow: 1., flex_basis: px(160), min_width: px(100), ..default() })
            .with_children(|field| {
                field.spawn((
                    LogSearchInput, feathrise_editor_core::EditorTextInput,
                    // 文本、光标和 IME 组合态由官方单行输入拥有。正文仍是独立的只读选区。
                    feathrise_editor_ui::single_line_input("", SEARCH_QUERY_CHARS),
                    EditableTextFilter::new(|character| !character.is_control()),
                    TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
                    TextLayout::no_wrap(), TextColor(c::TEXT_PRIMARY), FocusPolicy::Block,
                    Node { width: percent(100), min_width: px(0), height: px(28),
                        padding: UiRect::axes(px(7), px(4)), border: UiRect::all(px(1)),
                        overflow: Overflow::clip_x(), ..default() },
                    BackgroundColor(c::INPUT_BG), BorderColor::all(c::COPPER_BORDER_WEAK),
                )).observe(focus_text_on_press);
                field.spawn((
                    LogSearchHint, caption(font, "搜索工程日志", 12., c::TEXT_MUTED),
                    Node { position_type: PositionType::Absolute, left: px(8), top: px(5), ..default() },
                ));
            });
        button(row, font, LogAction::SearchPrevious, "上一处", false);
        button(row, font, LogAction::SearchNext, "下一处", false);
        row.spawn((
            LogSearchCount, caption(font, "", 11., c::TEXT_MUTED),
            Node { min_width: px(0), overflow: Overflow::clip_x(), ..default() },
        ));
    });
}

fn focus_text_on_press(
    press: On<PointerPress>,
    state: Res<ProjectLogState>,
    search: Res<LogSearch>,
    fields: Query<(&ComputedNode, Has<LogSearchInput>), Or<(With<LogBody>, With<LogSearchInput>)>>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary || !state.open || focus.get() == Some(press.entity) {
        return;
    }
    if fields.get(press.entity).is_ok_and(|(node, input)| !node.is_empty() && (!input || search.shown)) {
        // 官方文本控件的按下观察器负责字形命中和选区。浮窗仅为两个文本实体接管同一次按下的焦点。
        focus.set(press.entity, FocusCause::Pressed);
    }
}

fn scrollbar(parent: &mut ChildSpawnerCommands, target: Entity, orientation: ControlOrientation) {
    let vertical = orientation == ControlOrientation::Vertical;
    let mut track = parent.spawn((LogScrollbar, Scrollbar::new(target, orientation, 24.), Hovered::default(),
        RelativeCursorPosition::default(), FocusPolicy::Block,
        Node { display: if vertical { Display::Flex } else { Display::None },
            width: if vertical { px(7) } else { percent(100) },
            height: if vertical { percent(100) } else { px(7) },
            min_width: px(7), min_height: px(7), flex_shrink: 0., ..default() },
        BackgroundColor(TERMINAL_BG),
    ));
    if !vertical { track.insert(LogHorizontalScrollbar); }
    track.with_child((
        // 原生滑块维护真实尺寸和拖动捕获；不另挂Node，也不画高亮闭合边框。
        LogScrollThumb, ScrollbarThumb { border_radius: BorderRadius::ZERO, ..default() },
        feathrise_editor_ui::EditorLiteralColor,
        BackgroundColor(super::scrollbar_feedback::NORMAL),
    ));
}

fn caption(font: &Handle<Font>, text: &str, size: f32, color: Color) -> impl Bundle {
    (feathrise_editor_ui::localization::editor_label(text), TextFont { font: font.clone().into(), font_size: size.into(), ..default() },
        TextColor(color), TextLayout::no_wrap(), Pickable::IGNORE)
}

fn button(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, action: LogAction, text: &str, close: bool) {
    let follow = matches!(action, LogAction::Follow);
    let wrap = matches!(action, LogAction::Wrap);
    let style = if close { feathrise_editor_ui::dialog_close_button_style() }
        else { feathrise_editor_ui::dialog_button_style(follow || wrap) };
    let colors = style.colors;
    let mut node = style.node;
    if close {
        node.position_type = PositionType::Absolute;
        node.right = px(12); node.top = px(4);
    }
    let mut entity = parent.spawn((
        InteractionButton, LogToolButton, action, RelativeCursorPosition::default(), node, colors,
        BackgroundColor(colors.normal), BorderColor::all(c::DIALOG_BORDER),
    ));
    if follow { entity.insert(LogFollowButton); }
    if wrap { entity.insert(LogWrapButton); }
    if close { entity.insert(LogCloseButton); }
    entity.with_children(|button| {
        let mut label = button.spawn((caption(font, text, style.font_size, c::TEXT_PRIMARY), feathrise_editor_ui::text_shadow()));
        if follow { label.insert(LogFollowLabel); }
        if wrap { label.insert(LogWrapLabel); }
    });
}
