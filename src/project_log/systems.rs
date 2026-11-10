//! 日志邮箱按固定预算非阻塞采用；浮窗输入和回显不拥有任何进程控制牌。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::*};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel}, input_focus::InputFocus, prelude::*,
    ui::RelativeCursorPosition, window::PrimaryWindow,
    text::{EditableText, FontCx, LayoutCx, LineBreak, TextLayoutInfo},
};
use feathrise_editor_scene::workbench::{WorkbenchFloatingPanels, WorkbenchHostPageState, WorkbenchProjectLogButton};
use feathrise_editor_ui::EditorButtonClick;
use std::sync::atomic::Ordering;

pub(super) fn collect(
    channel: Res<ProjectLogChannel>, inbox: NonSend<ProjectLogInbox>, mut state: ResMut<ProjectLogState>,
) {
    let generation = channel.generation.load(Ordering::Acquire);
    if state.generation != generation {
        state.generation = generation;
        state.clear();
    }
    // 即使新任务的首条分隔消息尚未到达，也要按完整阅读身份撤回旧任务尾行和索引。
    state.refresh_tasks();
    for event in inbox.0.try_iter().take(FRAME_LINES) { state.adopt(event); }
    let dropped = channel.dropped.load(Ordering::Relaxed);
    if state.dropped != dropped { state.dropped = dropped; state.dirty = true; }
}

pub(super) fn control(
    mut state: ResMut<ProjectLogState>,
    channel: Option<Res<ProjectLogChannel>>,
    toggles: Query<&EditorButtonClick, With<WorkbenchProjectLogButton>>,
    buttons: Query<(&LogAction, &EditorButtonClick)>,
    input: (ResMut<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>),
    close: Option<Res<crate::native_project_close::NativeProjectCloseState>>,
    export: Option<Res<crate::project_export::ui::ExportOverlayState>>,
    blockers: (
        Option<Res<crate::native_plugins::management::PluginManagerState>>,
        Option<Res<crate::workspace_ui::AboutState>>,
        Option<Res<WorkbenchHostPageState>>,
        Option<Res<feathrise_editor_core::EditorHostOverlay>>,
    ),
    mut focus: ResMut<InputFocus>,
    mut wheels: MessageReader<MouseWheel>,
    mut panes: Query<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<LogScroll>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    panels: WorkbenchFloatingPanels,
    overlays: Query<(), With<LogOverlay>>,
    bodies: Query<(), With<LogBody>>,
    mut thumbs: Query<&mut bevy::ui_widgets::ScrollbarDragState, With<LogScrollThumb>>,
) {
    let (mut keys, mouse) = input;
    let previous = (state.open, state.follow, state.wrap, state.scroll_back, state.visible_rows);
    let close_active = close.is_some_and(|close| close.captures_input())
        || export.is_some_and(|export| export.captures_input())
        || blockers.0.is_some_and(|plugins| plugins.captures_input())
        || blockers.1.is_some_and(|about| about.captures_input())
        || blockers.2.is_some_and(|page| page.settings_open);
    let focused = !windows.single().is_ok_and(|window| !window.focused);
    let hovered = panels.hovered().is_some_and(|entity| overlays.contains(entity))
        && !blockers.3.is_some_and(|overlay| overlay.active);
    let body_focused = focus.get().is_some_and(|entity| bodies.contains(entity));
    if close_active {
        state.open = false; // 原工程关闭确认优先，日志不能挡住保存/放弃/取消。
    } else {
        if toggles.iter().any(EditorButtonClick::is_clicked) {
            state.open = !state.open;
        }
        if state.open {
            for (action, click) in &buttons {
                if !click.is_clicked() { continue; }
                match action {
                    LogAction::Close => state.open = false,
                    LogAction::Clear => {
                        state.clear();
                        if let Some(channel) = &channel {
                            // 已排队的旧行也属于清空之前；不必在输入帧排空整个邮箱。
                            state.after_sequence = channel.sequence.load(Ordering::Acquire);
                        }
                    }
                    LogAction::Follow => {
                        state.follow = !state.follow;
                        if state.follow { state.scroll_back = 0; }
                    }
                    LogAction::Wrap => {
                        super::wrapping::toggle(&mut state);
                        // 隐藏横向滚动条时撤回旧拖动捕获；重开换行不能遗留横向偏移。
                        if state.wrap { for mut thumb in &mut thumbs { thumb.dragging = false; } }
                    }
                    LogAction::CopySelection => state.copy_selection = true,
                    LogAction::CopyAll => state.copy_all = true,
                }
            }
            if focused && (body_focused || (hovered && focus.get().is_none()))
                && keys.clear_just_pressed(KeyCode::Escape) {
                state.open = false;
            }
        }
    }
    if !state.open || !focused {
        for mut thumb in &mut thumbs { thumb.dragging = false; }
    }
    // 正文只在实际获焦时持有快捷键。浮窗外的新点击及时归还键盘给工作台。
    if body_focused && (!state.open || !focused || (!hovered && mouse.get_just_pressed().next().is_some())) {
        focus.clear();
    }
    state.keyboard_owned = state.open && focused && focus.get().is_some_and(|entity| bodies.contains(entity));
    // 滚动条直接修改原ScrollPosition；将真实位移采用到虚拟窗口，而不是另造一套拖动命中。
    if state.open {
        for (_, node, scroll) in &panes {
            if node.is_empty() { continue; }
            let height = (node.content_box().height() * node.inverse_scale_factor()).max(LINE_HEIGHT);
            state.visible_rows = ((height / LINE_HEIGHT).ceil() as usize + 1).clamp(1, VIEW_LINES);
            if (scroll.y - state.last_scroll_y).abs() > 0.5 {
                let first = (scroll.y.max(0.) / LINE_HEIGHT).floor() as usize;
                state.scroll_back = state.display_row_count().saturating_sub(first + state.visible_rows);
                let max = (state.display_row_count() as f32 * LINE_HEIGHT - height).max(0.);
                state.follow = scroll.y >= max - 0.5;
            }
        }
    }
    for wheel in wheels.read() {
        if !state.open || !hovered { continue; }
        for (cursor, node, mut scroll) in &mut panes {
            if !cursor.cursor_over() || node.is_empty() { continue; }
            let multiplier = if wheel.unit == MouseScrollUnit::Line { 3. } else { 1. / LINE_HEIGHT };
            let rows = (wheel.y * multiplier).round() as isize;
            if rows > 0 { state.scroll_back = state.scroll_back.saturating_add(rows as usize); state.follow = false; }
            if rows < 0 { state.scroll_back = state.scroll_back.saturating_sub(rows.unsigned_abs()); state.follow = state.scroll_back == 0; }
            scroll.x = if state.wrap { 0. } else { (scroll.x - wheel.x * multiplier * LINE_HEIGHT).max(0.) };
        }
    }
    if state.keyboard_owned || (state.open && hovered && focus.get().is_none()) {
        if keys.clear_just_pressed(KeyCode::PageUp) { state.scroll_back = state.scroll_back.saturating_add(state.visible_rows); state.follow = false; }
        if keys.clear_just_pressed(KeyCode::PageDown) { state.scroll_back = state.scroll_back.saturating_sub(state.visible_rows); state.follow = state.scroll_back == 0; }
        if keys.clear_just_pressed(KeyCode::Home) { state.scroll_back = state.display_row_count(); state.follow = false; }
        if keys.clear_just_pressed(KeyCode::End) { state.scroll_back = 0; state.follow = true; }
    }
    if state.keyboard_owned && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight))
        && keys.just_pressed(KeyCode::KeyC) {
        state.copy_selection = true;
    }
    state.clamp_scroll();
    if state.open {
        for (_, node, mut scroll) in &mut panes {
            let height = (node.content_box().height() * node.inverse_scale_factor()).max(LINE_HEIGHT);
            let max = (state.display_row_count() as f32 * LINE_HEIGHT - height).max(0.);
            scroll.y = if state.scroll_back == 0 { max } else { (state.first_row() as f32 * LINE_HEIGHT).min(max) };
            if state.wrap { scroll.x = 0.; }
            state.last_scroll_y = scroll.y;
        }
    }
    if previous != (state.open, state.follow, state.wrap, state.scroll_back, state.visible_rows) { state.dirty = true; }
}

pub(super) fn present(
    mut commands: Commands,
    (mut state, locale): (ResMut<ProjectLogState>, Option<Res<feathrise_editor_ui::localization::EditorLocale>>),
    mut overlays: Query<&mut Node, With<LogOverlay>>,
    mut bodies: Query<(&mut LogBody, &mut EditableText, &mut Node, &mut TextLayout), (Without<LogOverlay>, Without<LogContent>, Without<LogDisplay>)>,
    mut contents: Query<&mut Node, (With<LogContent>, Without<LogOverlay>, Without<LogBody>, Without<LogDisplay>)>,
    mut displays: Query<(Entity, &mut LogDisplay, &mut Node, &TextFont, Option<&Children>, &mut TextLayout), (Without<LogOverlay>, Without<LogBody>, Without<LogContent>)>,
    mut spans: Query<(&mut TextSpan, &mut TextColor, &mut TextFont), (With<LogRun>, Without<LogDisplay>)>,
    mut statuses: Query<&mut Text, (With<LogStatus>, Without<LogBody>, Without<LogFollowLabel>, Without<LogWrapLabel>)>,
    mut follows: Query<&mut Text, (With<LogFollowLabel>, Without<LogBody>, Without<LogStatus>, Without<LogWrapLabel>)>,
    mut follow_buttons: Query<(&UiInteraction, &mut feathrise_editor_ui::ButtonColors, &mut BackgroundColor), (With<LogFollowButton>, Without<LogWrapButton>)>,
    mut fonts: ResMut<FontCx>,
    mut layouts: ResMut<LayoutCx>,
    mut wraps: Query<&mut Text, (With<LogWrapLabel>, Without<LogStatus>, Without<LogFollowLabel>, Without<LogBody>)>,
    mut wrap_buttons: Query<(&UiInteraction, &mut feathrise_editor_ui::ButtonColors, &mut BackgroundColor), (With<LogWrapButton>, Without<LogFollowButton>)>,
    mut horizontal_bars: Query<&mut Node, (With<LogHorizontalScrollbar>, Without<LogOverlay>, Without<LogBody>, Without<LogContent>, Without<LogDisplay>, Without<LogScroll>)>,
    mut panes: Query<&mut Node, (With<LogScroll>, Without<LogOverlay>, Without<LogBody>, Without<LogContent>, Without<LogDisplay>)>,
) {
    if !state.dirty && !locale.as_ref().is_some_and(|locale| locale.is_changed()) { return; }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for mut node in &mut overlays {
        node.display = if state.open { Display::Flex } else { Display::None };
    }
    if state.open {
        // 宽度重排分帧期间，旧行边界不能切新UTF-8尾行。保持上一页实际字形，
        // 直到同一正文修订的完整索引发布后，再原子替换命中层与富色层。
        if !state.wrap || state.wrap_layout.revision == Some(state.content_revision) {
        let (origin, body, styles) = state.visible_styled_text();
        let width = if state.wrap { state.wrap_layout.width.max(1.) } else { state.content_width };
        let row_count = if state.wrap && !state.wrap_layout.rows.is_empty() {
            state.visible_rows.min(VIEW_LINES).min(state.display_row_count().saturating_sub(state.first_row()))
        } else { body.lines().count().max(1) };
        let linebreak = if state.wrap { LineBreak::AnyCharacter } else { LineBreak::NoWrap };
        let selection = if state.select_all {
            Some((origin, origin + body.len() as u64))
        } else { state.selection.and_then(|selection| Some((state.offset_of(selection.anchor)?, state.offset_of(selection.focus)?))) };
        for (mut marker, mut text, mut node, mut layout) in &mut bodies {
            if text.value().to_string() != body { text.editor.set_text(&body); }
            marker.origin = origin;
            node.top = px(state.first_row() as f32 * LINE_HEIGHT);
            node.height = px(row_count as f32 * LINE_HEIGHT);
            node.width = px(width);
            if layout.linebreak != linebreak { layout.linebreak = linebreak; }
            let (anchor, focus) = selection.map_or((0, 0), |(anchor, focus)| (
                super::reading::local_position(&body, origin, anchor),
                super::reading::local_position(&body, origin, focus),
            ));
            text.editor.driver(&mut fonts, &mut layouts.0).select_byte_range(anchor, focus);
        }
        for mut node in &mut contents {
            node.height = px(state.display_row_count().max(2) as f32 * LINE_HEIGHT);
            node.width = px(width);
        }
        for (entity, mut marker, mut node, font, children, mut layout) in &mut displays {
            node.top = px(state.first_row() as f32 * LINE_HEIGHT);
            node.height = px(row_count as f32 * LINE_HEIGHT);
            node.width = px(width);
            if layout.linebreak != linebreak { layout.linebreak = linebreak; }
            super::display::sync(&mut commands, entity, &mut marker, children, &mut spans, font, &body, &styles);
        }
        }
        let mut status = locale.format("log.lines", &[("count", state.row_count().to_string())]);
        if state.trimmed > 0 {
            status.push_str(&locale.format("log.trimmed", &[("count", state.trimmed.to_string())]));
        }
        if state.dropped > 0 {
            status.push_str(&locale.format("log.dropped", &[("count", state.dropped.to_string())]));
        }
        if !state.notice.is_empty() {
            status.push_str("  ");
            status.push_str(&locale.editor_notice(&state.notice));
        }
        for mut text in &mut statuses { if text.0 != status { text.0.clone_from(&status); } }
        for mut text in &mut follows { text.0 = locale.text(if state.follow { "log.follow.on" } else { "log.follow.off" }).into(); }
        for (interaction, mut colors, mut background) in &mut follow_buttons {
            *colors = super::scene::tool_button_colors(state.follow);
            background.0 = match interaction { UiInteraction::Hovered => colors.hovered,
                UiInteraction::Pressed => colors.pressed, UiInteraction::None => colors.normal };
        }
        for mut text in &mut wraps { text.0 = locale.text(if state.wrap { "log.wrap.on" } else { "log.wrap.off" }).into(); }
        for (interaction, mut colors, mut background) in &mut wrap_buttons {
            *colors = super::scene::tool_button_colors(state.wrap);
            background.0 = match interaction { UiInteraction::Hovered => colors.hovered,
                UiInteraction::Pressed => colors.pressed, UiInteraction::None => colors.normal };
        }
        for mut node in &mut horizontal_bars { node.display = if state.wrap { Display::None } else { Display::Flex }; }
        for mut node in &mut panes { node.overflow.x = if state.wrap { OverflowAxis::Hidden } else { OverflowAxis::Scroll }; }
    }
    state.dirty = false;
}

/// 新正文的真实宽度在布局后才存在，最后一条长日志也必须触发横向滚动范围更新。
pub(super) fn measure_width(
    mut state: ResMut<ProjectLogState>,
    bodies: Query<&TextLayoutInfo, With<LogBody>>,
) {
    if !state.open || state.wrap { return; }
    for layout in &bodies {
        if layout.size.x > state.content_width + 0.5 {
            state.content_width = layout.size.x.ceil();
            state.dirty = true;
        }
    }
}
