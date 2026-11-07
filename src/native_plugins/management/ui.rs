//! 工程插件使用共同弹窗外壳、原生滚动条和固定行池；呈现不执行文件IO或装卸动态库。

use super::{
    components::*,
    resources::PluginManagerState,
    types::{PackageEntry, PendingKind, PluginScope},
};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    picking::hover::Hovered,
    prelude::*,
    ui::{FocusPolicy, InteractionDisabled, RelativeCursorPosition},
    ui_widgets::{ControlOrientation, Scrollbar, ScrollbarDragState, ScrollbarThumb},
    window::PrimaryWindow,
};
use feathrise_editor_map::workbench::WorkbenchHeader;
use feathrise_editor_ui::{EditorButtonClick, colors as c, localization::EditorLocale};

pub(super) const ROW_HEIGHT: f32 = 64.;
const ROW_POOL: usize = 24;
#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;

pub(super) fn mount(
    mut commands: Commands,
    headers: Query<Entity, Added<WorkbenchHeader>>,
    assets: Res<AssetServer>,
) {
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for header in &headers {
        commands.entity(header).with_children(|root| {
            root.spawn((ManagerOverlay, ManagerLayout::Overlay, GlobalZIndex(4370), FocusPolicy::Block,
                Interaction::None, Node { display: Display::None, position_type: PositionType::Absolute,
                    left: px(0), top: px(0), width: Val::Vw(100.), height: Val::Vh(100.),
                    align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() },
                BackgroundColor(c::DIALOG_BACKDROP),
            )).with_children(|overlay| {
                overlay.spawn((ManagerPanel, Node { width: Val::Vw(92.), max_width: px(840),
                    height: Val::Vh(84.), max_height: px(620), ..feathrise_editor_ui::dialog_panel_node() },
                    BackgroundColor(c::DIALOG_BG), BorderColor::all(c::DIALOG_BORDER), FocusPolicy::Block,
                )).with_children(|panel| {
                    panel.spawn((ManagerHeader, feathrise_editor_ui::dialog_header_node(),
                        BackgroundColor(c::DIALOG_HEADER_BG), BorderColor::all(c::DIALOG_BORDER),
                    )).with_children(|header| {
                        header.spawn(label(&font, "插件管理", 13., c::TEXT_PRIMARY, false));
                        header.spawn((ManagerText::Count, label(&font, "", 11., c::TEXT_MUTED, false)));
                        let style = feathrise_editor_ui::dialog_close_button_style();
                        header.spawn((Button, ManagerAction::Close,
                            Node { margin: UiRect::left(Val::Auto), ..style.node }, style.colors,
                            BackgroundColor(style.colors.normal), BorderColor::all(c::DIALOG_BORDER),
                        )).with_child(label(&font, "×", style.font_size, c::TEXT_PRIMARY, false));
                    });
                    panel.spawn((ManagerBody, Node { flex_grow: 1., ..feathrise_editor_ui::dialog_body_node() },
                        BackgroundColor(c::DIALOG_BG),
                    )).with_children(|body| {
                        body.spawn(Node { column_gap: px(6), row_gap: px(4), flex_wrap: FlexWrap::Wrap,
                            align_items: AlignItems::Center, flex_shrink: 0., ..default() },
                        ).with_children(|toolbar| {
                            for (scope, title) in [(PluginScope::Global, "全局"), (PluginScope::Project, "当前工程")] {
                                button(toolbar, &font, ManagerAction::SetScope(scope), ManagerControl::Scope(scope), title, false);
                            }
                            button(toolbar, &font, ManagerAction::Install, ManagerControl::Install, "安装插件文件…", true);
                            button(toolbar, &font, ManagerAction::Refresh, ManagerControl::Refresh, "刷新", false);
                        });
                        body.spawn((ManagerText::ScopeInfo, label(&font, "", 11., c::TEXT_MUTED, true)));
                        button(body, &font, ManagerAction::OpenDirectory, ManagerControl::OpenDirectory, "打开插件目录", false);
                        body.spawn((ManagerLayout::Columns, Node { min_width: px(0), min_height: px(0),
                            flex_grow: 1., column_gap: px(12), row_gap: px(8), ..default() },
                        )).with_children(|columns| {
                            columns.spawn((ManagerLayout::ListRegion, Node { min_width: px(0), min_height: px(0),
                                flex_basis: percent(43), flex_grow: 1., column_gap: px(3), ..default() },
                            )).with_children(|region| {
                                let pane = region.spawn((ManagerPane, ScrollPosition::default(), RelativeCursorPosition::default(),
                                    Node { min_width: px(0), min_height: px(0), flex_grow: 1., flex_basis: px(0),
                                        flex_direction: FlexDirection::Column,
                                        overflow: Overflow { x: OverflowAxis::Hidden, y: OverflowAxis::Scroll }, ..default() },
                                    FocusPolicy::Block,
                                )).with_children(|pane| {
                                    pane.spawn((ManagerLayout::Empty, label(&font, "尚未安装作者插件。选择当前平台的单个插件文件。", 12., c::TEXT_MUTED, true),
                                        Node { padding: UiRect::all(px(8)), ..default() }));
                                    pane.spawn((ManagerContent, ManagerLayout::Content, Node { min_width: percent(100),
                                        height: px(0), flex_shrink: 0., ..default() },
                                    )).with_children(|content| {
                                        // 列表总高度保留完整目录，实体数只由可见范围预算决定。
                                        for slot in 0..ROW_POOL {
                                            content.spawn((Button, ManagerRow { slot, index: None, id: None, revision: u64::MAX },
                                                ManagerBinding::default(), ManagerAction::Select(String::new()),
                                                Node { display: Display::None, position_type: PositionType::Absolute,
                                                    left: px(0), top: px(0), width: percent(100), height: px(ROW_HEIGHT),
                                                    padding: UiRect::axes(px(8), px(5)), flex_direction: FlexDirection::Column,
                                                    row_gap: px(2), overflow: Overflow::clip(), border: UiRect::bottom(px(1)), ..default() },
                                                feathrise_editor_ui::dialog_button_style(false).colors,
                                                BackgroundColor(c::DIALOG_BG), BorderColor::all(c::DIALOG_BORDER),
                                            )).with_children(|row| {
                                                for line in 0..3 {
                                                    row.spawn((ManagerText::Row { slot, line }, label(&font, "", if line == 0 { 13. } else { 11. },
                                                        if line == 0 { c::TEXT_PRIMARY } else { c::TEXT_MUTED }, false)));
                                                }
                                            });
                                        }
                                    });
                                }).id();
                                scrollbar(region, pane);
                            });
                            columns.spawn(Node { min_width: px(0), min_height: px(0), flex_basis: percent(57),
                                flex_grow: 1., column_gap: px(3), ..default() },
                            ).with_children(|region| {
                                let pane = region.spawn((ManagerDetailsPane, ScrollPosition::default(), RelativeCursorPosition::default(),
                                    FocusPolicy::Block, Node { min_width: px(0), min_height: px(0), flex_grow: 1., flex_basis: px(0),
                                        flex_direction: FlexDirection::Column,
                                        overflow: Overflow { x: OverflowAxis::Hidden, y: OverflowAxis::Scroll }, ..default() },
                                )).with_children(|pane| {
                                    pane.spawn(Node { min_width: percent(100), flex_direction: FlexDirection::Column,
                                        row_gap: px(8), padding: UiRect::right(px(4)), flex_shrink: 0., ..default() },
                                    ).with_children(|detail| {
                                        for kind in [DetailKind::Name, DetailKind::Identity, DetailKind::Source, DetailKind::State, DetailKind::Path,
                                            DetailKind::Error, DetailKind::Pending, DetailKind::Runtime] {
                                            detail.spawn((ManagerText::Detail(kind), label(&font, "", if matches!(kind, DetailKind::Name) { 13. } else { 12. },
                                                if matches!(kind, DetailKind::Name) { c::TEXT_PRIMARY } else { c::TEXT_MUTED }, true)));
                                        }
                                        detail.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() })
                                            .with_children(|actions| {
                                                button(actions, &font, ManagerAction::SetEnabled { id: String::new(), enabled: false },
                                                    ManagerControl::Toggle, "停用", false);
                                                button(actions, &font, ManagerAction::Update(String::new()), ManagerControl::Update, "更新…", false);
                                                button(actions, &font, ManagerAction::RequestRemove(String::new()), ManagerControl::Remove, "移除…", false);
                                                button(actions, &font, ManagerAction::CancelPending(String::new()), ManagerControl::CancelPending, "撤销待生效变更", false);
                                            });
                                        detail.spawn((ManagerLayout::Confirmation, Node { display: Display::None, flex_direction: FlexDirection::Column,
                                            row_gap: px(8), padding: UiRect::all(px(8)), border: UiRect::all(px(1)),
                                            border_radius: BorderRadius::all(px(3)), ..default() },
                                            BackgroundColor(c::DIALOG_HEADER_BG), BorderColor::all(c::DIALOG_BORDER),
                                        )).with_children(|confirm| {
                                            confirm.spawn((ManagerText::Confirmation, label(&font, "", 12., c::TEXT_PRIMARY, true)));
                                            confirm.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() })
                                                .with_children(|actions| {
                                                    button(actions, &font, ManagerAction::ConfirmRemove, ManagerControl::Confirm, "确认移除", true);
                                                    button(actions, &font, ManagerAction::CancelRemove, ManagerControl::CancelConfirm, "取消", false);
                                                });
                                        });
                                        detail.spawn(label(&font, "这里只管理编辑器作者插件。工程运行能力仍需接入对应依赖；启停和移除保留作者数据。", 11., c::TEXT_MUTED, true));
                                    });
                                }).id();
                                scrollbar(region, pane);
                            });
                        });
                    });
                    panel.spawn((ManagerFooter, feathrise_editor_ui::dialog_footer_node(),
                        BackgroundColor(c::DIALOG_HEADER_BG), BorderColor::all(c::DIALOG_BORDER),
                    )).with_child((ManagerText::Status, label(&font, "", 11., c::TEXT_MUTED, true),
                        Node { flex_grow: 1., min_width: px(0), max_height: px(44), overflow: Overflow::clip(), ..default() }));
                });
            });
        });
    }
}

fn label(font: &Handle<Font>, text: &str, size: f32, color: Color, wrap: bool) -> impl Bundle {
    (
        feathrise_editor_ui::localization::editor_label(text),
        TextFont {
            font: font.clone().into(),
            font_size: size.into(),
            ..default()
        },
        TextColor(color),
        TextLayout {
            linebreak: if wrap {
                bevy::text::LineBreak::WordOrCharacter
            } else {
                bevy::text::LineBreak::NoWrap
            },
            ..default()
        },
        Pickable::IGNORE,
    )
}

fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    action: ManagerAction,
    marker: ManagerControl,
    text: &str,
    primary: bool,
) -> Entity {
    let mut style = feathrise_editor_ui::dialog_button_style(primary);
    let toggle = matches!(marker, ManagerControl::Toggle);
    let directory = matches!(marker, ManagerControl::OpenDirectory);
    if directory {
        // 路径仍使用共享按钮外壳；长中文路径在窄窗口自然换行，不挤掉操作区。
        style.node.height = Val::Auto;
        style.node.min_height = px(28);
        style.node.min_width = px(0);
        style.node.justify_content = JustifyContent::Start;
        style.font_size = 11.;
    }
    parent
        .spawn((
            Button,
            action,
            marker,
            ManagerBinding(u64::MAX),
            style.node,
            style.colors,
            BackgroundColor(style.colors.normal),
            BorderColor::all(c::DIALOG_BORDER),
        ))
        .with_children(|button| {
            let mut text =
                button.spawn(label(font, text, style.font_size, style.text_color, false));
            if toggle {
                text.insert(ManagerText::Toggle);
            }
            if directory {
                text.insert((ManagerText::Directory, TextLayout {
                    linebreak: bevy::text::LineBreak::WordOrCharacter, ..default()
                }));
            }
        })
        .id()
}

fn scrollbar(parent: &mut ChildSpawnerCommands, target: Entity) {
    parent
        .spawn((
            ManagerScrollTrack,
            Scrollbar::new(target, ControlOrientation::Vertical, 24.),
            Hovered::default(),
            RelativeCursorPosition::default(),
            FocusPolicy::Block,
            Node {
                width: px(7),
                height: percent(100),
                min_width: px(7),
                min_height: px(7),
                flex_shrink: 0.,
                ..default()
            },
            BackgroundColor(c::DIALOG_BG),
        ))
        .with_child((
            ManagerScrollThumb,
            ScrollbarThumb {
                border_radius: BorderRadius::ZERO,
                ..default()
            },
            BackgroundColor(Color::srgb(0.27, 0.27, 0.27)),
        ));
}

pub(super) fn scroll(
    state: Res<PluginManagerState>,
    mut wheels: MessageReader<MouseWheel>,
    mut panes: Query<
        (
            &RelativeCursorPosition,
            &ComputedNode,
            &mut ScrollPosition,
            Has<ManagerPane>,
        ),
        Or<(With<ManagerPane>, With<ManagerDetailsPane>)>,
    >,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    for wheel in wheels.read() {
        if !state.open || !wheel.y.is_finite() || windows.iter().any(|window| !window.focused) {
            continue;
        }
        for (cursor, node, mut scroll, list) in &mut panes {
            if !cursor.cursor_over() || node.is_empty() {
                continue;
            }
            let height = node.content_box().height() * node.inverse_scale_factor();
            let content = if list {
                state.rows.len() as f32 * ROW_HEIGHT
            } else {
                node.content_size().y * node.inverse_scale_factor()
            };
            let maximum = (content - height).max(0.);
            let scale = if wheel.unit == MouseScrollUnit::Line {
                36.
            } else {
                node.inverse_scale_factor()
            };
            scroll.y = (scroll.y - wheel.y * scale).clamp(0., maximum);
        }
    }
}

pub(super) fn scrollbar_feedback(
    tracks: Query<&Hovered, With<ManagerScrollTrack>>,
    mut thumbs: Query<
        (&ChildOf, &ScrollbarDragState, &mut BackgroundColor),
        With<ManagerScrollThumb>,
    >,
) {
    for (parent, drag, mut color) in &mut thumbs {
        let hovered = tracks.get(parent.parent()).is_ok_and(Hovered::get);
        let value = if drag.dragging {
            0.55
        } else if hovered {
            0.42
        } else {
            0.27
        };
        let next = Color::srgb(value, value, value);
        if color.0 != next {
            color.0 = next;
        }
    }
}

pub(super) fn present(
    mut commands: Commands,
    state: Res<PluginManagerState>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut layouts: Query<(&ManagerLayout, &mut Node), Without<ManagerRow>>,
    mut panes: Query<(&ComputedNode, &mut ScrollPosition), With<ManagerPane>>,
    mut rows: Query<
        (
            &mut ManagerRow,
            &mut ManagerAction,
            &mut ManagerBinding,
            &mut Node,
            &mut EditorButtonClick,
            &mut Interaction,
            &mut feathrise_editor_ui::ButtonColors,
            &mut BackgroundColor,
            Option<&RelativeCursorPosition>,
        ),
        Without<ManagerLayout>,
    >,
    mut texts: Query<(&ManagerText, &mut Text, &mut TextColor), With<ManagerText>>,
    mut button_labels: Query<&mut TextColor, Without<ManagerText>>,
    mut actions: Query<
        (
            Entity,
            &ManagerControl,
            &mut ManagerAction,
            &mut ManagerBinding,
            &mut EditorButtonClick,
            &mut Interaction,
            Has<InteractionDisabled>,
            &mut feathrise_editor_ui::ButtonColors,
            &mut BackgroundColor,
            Option<&RelativeCursorPosition>,
            Option<&Children>,
            Option<&mut BorderColor>,
        ),
        Without<ManagerRow>,
    >,
) {
    let narrow = windows
        .iter()
        .next()
        .is_some_and(|window| window.width() < 650.);
    let selected = state
        .selected
        .as_ref()
        .and_then(|id| state.rows.iter().find(|entry| &entry.id == id));
    let mut first = 0;
    for (node, mut scroll) in &mut panes {
        let height = node.content_box().height() * node.inverse_scale_factor();
        scroll.y = scroll
            .y
            .clamp(0., (state.rows.len() as f32 * ROW_HEIGHT - height).max(0.));
        first = (scroll.y / ROW_HEIGHT).floor() as usize;
    }
    for (kind, mut node) in &mut layouts {
        match kind {
            ManagerLayout::Overlay => {
                node.display = if state.open {
                    Display::Flex
                } else {
                    Display::None
                }
            }
            ManagerLayout::Columns => {
                node.flex_direction = if narrow {
                    FlexDirection::Column
                } else {
                    FlexDirection::Row
                }
            }
            ManagerLayout::ListRegion => {
                node.flex_basis = if narrow { px(160) } else { percent(43) };
                node.flex_grow = if narrow { 0. } else { 1. };
                node.flex_shrink = if narrow { 0. } else { 1. };
            }
            ManagerLayout::Empty => {
                node.display = if state.rows.is_empty() {
                    Display::Flex
                } else {
                    Display::None
                }
            }
            ManagerLayout::Content => node.height = px(state.rows.len() as f32 * ROW_HEIGHT),
            ManagerLayout::Confirmation => {
                node.display = if state.confirm_remove.is_some() {
                    Display::Flex
                } else {
                    Display::None
                }
            }
        }
    }
    if !state.open {
        return;
    }
    for (
        mut row,
        mut action,
        mut binding,
        mut node,
        mut click,
        mut interaction,
        mut colors,
        mut background,
        cursor,
    ) in &mut rows
    {
        let index = first.saturating_add(row.slot);
        let entry = state.rows.get(index);
        let next = entry.map(|entry| entry.id.clone());
        if row.id != next || row.revision != state.revision || row.index != entry.map(|_| index) {
            row.id = next.clone();
            row.index = entry.map(|_| index);
            row.revision = state.revision;
            binding.0 = state.revision;
            *action = ManagerAction::Select(next.unwrap_or_default());
            click.invalidate();
            *interaction = Interaction::None;
        }
        node.display = if entry.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        node.top = px(index as f32 * ROW_HEIGHT);
        let active =
            entry.is_some_and(|entry| state.selected.as_deref() == Some(entry.id.as_str()));
        let palette = if active {
            feathrise_editor_ui::dialog_button_style(true).colors
        } else {
            feathrise_editor_ui::ButtonColors::new(
                c::DIALOG_BG,
                c::BUTTON_DEV_SMALL_HOVERED,
                c::BUTTON_DEV_SMALL_PRESSED,
            )
        };
        if colors.normal != palette.normal {
            *colors = palette;
        }
        background.0 = button_background(&palette, *interaction, cursor, false);
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (kind, mut text, mut color) in &mut texts {
        let next = match *kind {
            ManagerText::Count => {
                let count = state.rows.len();
                let key = if count == 1 { "plugin.count.one" } else { "plugin.count" };
                locale.format(key, &[("count", count.to_string())])
            }
            ManagerText::ScopeInfo => match state.scope {
                PluginScope::Global if !state.has_project() => "全局插件供当前用户的所有工程使用；安装和设置完成后即生效。".into(),
                PluginScope::Global => "全局插件供当前用户的所有工程使用；当前工程在重开后采用变更。".into(),
                PluginScope::Project => "工程同名插件优先；可单独停用继承的全局插件。安装、更新与移除在重开工程后生效。".into(),
            },
            ManagerText::Directory => super::directory::path(&state.roots, state.scope)
                .map(|path| format!("打开目录  {}", crate::project::display_path(&path)))
                .unwrap_or_else(|error| format!("插件目录不可用：{error}")),
            ManagerText::Status => state.initialization_error.as_ref().map_or_else(
                || {
                    if state.status.is_empty() && !state.has_project() {
                        "全局安装和设置完成后即生效；已运行的工程保留原会话。".into()
                    } else if state.status.is_empty() {
                        "变更在重开工程后生效；当前已加载工具继续可用。".into()
                    } else {
                        state.status.clone()
                    }
                },
                |error| format!("{error}；{}", state.status),
            ),
            ManagerText::Row { slot, line } => {
                state
                    .rows
                    .get(first + slot)
                    .map_or_else(String::new, |entry| match line {
                        0 => format!(
                            "{}  {}",
                            entry.tool,
                            entry.version.as_deref().unwrap_or_else(|| locale.text("plugin.version.unknown"))
                        ),
                        1 => format!("{} · {}", entry.id, locale.editor_label(source_label(entry))),
                        _ => state_line(entry, &state, locale),
                    })
            }
            ManagerText::Detail(kind) => detail_text(selected, kind, &state, locale),
            ManagerText::Toggle => {
                if selected.is_some_and(|entry| entry.enabled) {
                    "停用".into()
                } else {
                    "启用".into()
                }
            }
            ManagerText::Confirmation => {
                let fallback = selected.is_some_and(|entry| entry.scope == PluginScope::Project && entry.global_available);
                let impact = if selected.is_some_and(|entry| entry.legacy) {
                    "归档移除旧格式插件，原文件保留在工程管理备份目录。"
                } else if state.scope == PluginScope::Global && !state.has_project() {
                    "移除全局插件；已运行的工程保留原会话。"
                } else if state.scope == PluginScope::Global {
                    "影响当前用户所有继承此插件的工程，各工程重开后生效。"
                } else if fallback {
                    "重开工程后将采用同名全局版本，其启停和可用状态由全局设置决定。"
                } else { "重开工程后移除本工程的作者插件。" };
                locale.format("plugin.remove.confirm", &[
                    ("id", state.confirm_remove.clone().unwrap_or_default()),
                    ("impact", locale.editor_label(impact).to_owned()),
                ])
            },
        };
        // 插件名称、ID和版本属于插件方；内置管理状态使用编辑器词表。
        let next = if matches!(*kind, ManagerText::Row { line: 0 | 1, .. })
            || selected.is_some() && matches!(*kind, ManagerText::Detail(DetailKind::Name)) {
            std::borrow::Cow::Borrowed(next.as_str())
        } else {
            locale.editor_notice(&next)
        };
        if text.as_str() != next.as_ref() {
            **text = next.into_owned();
        }
        let failed = match *kind {
            ManagerText::Row { slot, line: 2 } => state
                .rows
                .get(first + slot)
                .is_some_and(|entry| entry.error.is_some()),
            ManagerText::Detail(DetailKind::Error) => {
                selected.is_some_and(|entry| entry.error.is_some())
            }
            _ => false,
        };
        let next_color = if failed {
            Color::srgb(0.92, 0.45, 0.40)
        } else if matches!(
            kind,
            ManagerText::Row { line: 0, .. }
                | ManagerText::Detail(DetailKind::Name)
                | ManagerText::Confirmation
        ) {
            c::TEXT_PRIMARY
        } else {
            c::TEXT_MUTED
        };
        if color.0 != next_color {
            color.0 = next_color;
        }
    }
    for (entity, control, mut action, mut binding, mut click, mut interaction, disabled, mut colors, mut background, cursor, children, border) in
        &mut actions
    {
        let id = selected.map(|entry| entry.id.clone()).unwrap_or_default();
        let has_pending = selected.is_some_and(|entry| entry.pending.is_some());
        let confirming = state.confirm_remove.is_some();
        let (next, enabled) = match control {
            // 校验失败的包不能启用；当前已启用的旧会话仍允许安排停用，更新和移除继续可用。
            ManagerControl::Toggle => (
                ManagerAction::SetEnabled {
                    id,
                    enabled: !selected.is_some_and(|entry| entry.enabled),
                },
                selected.is_some_and(|entry| entry.enabled || entry.error.is_none())
                    && selected.is_some_and(|entry| !entry.legacy)
                    && !has_pending
                    && !confirming,
            ),
            ManagerControl::Update => (
                ManagerAction::Update(id),
                selected.is_some_and(|entry| !entry.inherited && !entry.legacy) && !has_pending && !confirming,
            ),
            ManagerControl::Remove => (
                ManagerAction::RequestRemove(id),
                selected.is_some_and(|entry| !entry.inherited
                    && (!entry.legacy || entry.scope == PluginScope::Project && state.has_project()))
                    && !has_pending && !confirming,
            ),
            ManagerControl::CancelPending => {
                (ManagerAction::CancelPending(id), has_pending && !confirming)
            }
            ManagerControl::Confirm => (ManagerAction::ConfirmRemove, confirming),
            ManagerControl::CancelConfirm => (ManagerAction::CancelRemove, confirming),
            ManagerControl::Install => (ManagerAction::Install, !confirming),
            ManagerControl::Refresh => (ManagerAction::Refresh, !confirming),
            ManagerControl::OpenDirectory => (
                ManagerAction::OpenDirectory,
                !confirming && super::directory::path(&state.roots, state.scope).is_ok(),
            ),
            ManagerControl::Scope(scope) => (
                ManagerAction::SetScope(*scope),
                !confirming && (*scope == PluginScope::Global || state.has_project()),
            ),
        };
        if *action != next || binding.0 != state.revision {
            *action = next;
            binding.0 = state.revision;
            click.invalidate();
            *interaction = Interaction::None;
        }
        let next_disabled = state.busy || !enabled;
        // 状态来自真实禁用条件与官方Interaction；不把灰显按钮画成可点击，也不保留拖出后的按压色。
        let primary = matches!(control, ManagerControl::Install | ManagerControl::Confirm)
            || matches!(control, ManagerControl::Scope(scope) if *scope == state.scope);
        let palette = if next_disabled {
            feathrise_editor_ui::ButtonColors::new(c::DIALOG_BG, c::DIALOG_BG, c::DIALOG_BG)
        } else {
            feathrise_editor_ui::dialog_button_style(primary).colors
        };
        if colors.normal != palette.normal || colors.hovered != palette.hovered || colors.pressed != palette.pressed {
            *colors = palette;
        }
        background.0 = button_background(&palette, *interaction, cursor, next_disabled);
        if let Some(mut border) = border {
            *border = BorderColor::all(if next_disabled { c::DIALOG_BORDER.with_alpha(0.35) } else { c::DIALOG_BORDER });
        }
        let label_color = if next_disabled { c::TEXT_MUTED.with_alpha(0.45) } else { c::TEXT_PRIMARY };
        for child in children.into_iter().flat_map(|children| children.iter()) {
            if let Ok((_, _, mut color)) = texts.get_mut(child) {
                color.0 = label_color;
            } else if let Ok(mut color) = button_labels.get_mut(child) {
                color.0 = label_color;
            }
        }
        if disabled != next_disabled {
            if next_disabled {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
    }
}

fn button_background(colors: &feathrise_editor_ui::ButtonColors, interaction: Interaction,
    cursor: Option<&RelativeCursorPosition>, disabled: bool) -> Color {
    if disabled {
        return c::DIALOG_BG;
    }
    match interaction {
        Interaction::Pressed if cursor.is_none_or(RelativeCursorPosition::cursor_over) => colors.pressed,
        Interaction::Hovered => colors.hovered,
        _ => colors.normal,
    }
}

fn pending_label(kind: &PendingKind) -> &'static str {
    match kind {
        PendingKind::Install => "待安装",
        PendingKind::Update => "待更新",
        PendingKind::Enable => "待启用",
        PendingKind::Disable => "待停用",
        PendingKind::Remove => "待移除",
    }
}

fn state_line(entry: &PackageEntry, state: &PluginManagerState, locale: EditorLocale) -> String {
    // 安全拒载会令catalog.enabled=false，但不代表作者主动停用，不能据此伪造停用状态。
    let fallback = if entry
        .pending
        .as_ref()
        .is_some_and(|change| change.kind == PendingKind::Install)
    {
        "尚未安装"
    } else if entry.error.is_some() {
        "不可加载"
    } else if entry.enabled {
        if state.has_project() { "未加载" } else { "已生效" }
    } else {
        "已停用"
    };
    let actual = locale.editor_notice(state
        .load_states
        .get(&entry.id)
        .map(String::as_str)
        .unwrap_or(fallback));
    if entry.overridden_by_project {
        return locale.format("plugin.state.project_override", &[("state", actual.into_owned())]);
    }
    if let Some(change) = &entry.pending {
        let condition = if state.has_project() { "重开工程后生效" } else { "尚未完成" };
        locale.format("plugin.state.pending", &[
            ("state", actual.into_owned()),
            ("pending", locale.editor_label(pending_label(&change.kind)).to_owned()),
            ("condition", locale.editor_label(condition).to_owned()),
        ])
    } else if entry.error.is_some() {
        locale.format("plugin.state.check_failed", &[("state", actual.into_owned())])
    } else {
        actual.into_owned()
    }
}

fn source_label(entry: &PackageEntry) -> &'static str {
    if entry.inherited { "继承全局" }
    else if entry.scope == PluginScope::Project { "工程" }
    else { "全局" }
}

fn detail_text(
    entry: Option<&PackageEntry>,
    kind: DetailKind,
    state: &PluginManagerState,
    locale: EditorLocale,
) -> String {
    let Some(entry) = entry else {
        return if matches!(kind, DetailKind::Name) {
            "选择插件查看详情".into()
        } else {
            String::new()
        };
    };
    match kind {
        DetailKind::Name => format!(
            "{}  {}",
            entry.tool,
            entry.version.as_deref().unwrap_or_else(|| locale.text("plugin.version.unknown"))
        ),
        DetailKind::Identity => format!("标识  {}", entry.id),
        DetailKind::Source => {
            let key = if entry.inherited {
                "plugin.source.inherited"
            } else if entry.overridden_by_project {
                "plugin.source.overridden"
            } else if entry.scope == PluginScope::Global {
                "plugin.source.global"
            } else { "plugin.source.project" };
            locale.format(key, &[("scope", locale.editor_label(source_label(entry)).to_owned())])
        },
        DetailKind::Path => format!(
            "安装路径  {}",
            crate::project::display_path(&entry.path)
        ),
        DetailKind::State => locale.format("plugin.detail.state", &[("state", state_line(entry, state, locale))]),
        DetailKind::Error => entry
            .error
            .as_ref()
            .map_or_else(String::new, |error| format!("错误原因  {error}")),
        DetailKind::Pending => entry.pending.as_ref().map_or_else(String::new, |change| {
            let condition = if state.has_project() { "重开工程后生效" } else { "尚未完成，请查看操作错误或刷新" };
            locale.format("plugin.detail.pending", &[
                ("pending", locale.editor_label(pending_label(&change.kind)).to_owned()),
                ("version", change
                    .version
                    .as_ref()
                    .map_or_else(String::new, |version| format!(" {version}"))),
                ("condition", locale.editor_label(condition).to_owned()),
            ])
        }),
        DetailKind::Runtime => {
            let separator = if locale == EditorLocale::English { ", " } else { "、" };
            if !state.has_project() {
                return if entry.runtime_dependencies.is_empty() {
                    locale.text("plugin.runtime.check_on_open").into()
                } else {
                    locale.format("plugin.runtime.declared_closed", &[("packages", entry.runtime_dependencies.join(separator))])
                };
            }
            if !entry.runtime_missing.is_empty() {
                locale.format("plugin.runtime.missing", &[("packages", entry.runtime_missing.join(separator))])
            } else if !entry.runtime_dependencies.is_empty() {
                locale.format("plugin.runtime.declared", &[("packages", entry.runtime_dependencies.join(separator))])
            } else {
                locale.text("plugin.runtime.none").into()
            }
        }
    }
}
