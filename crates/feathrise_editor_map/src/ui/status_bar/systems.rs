//! 底栏回执保留、详情鼠标所有权与差异回显；不执行或重试作者任务。

use feathrise_editor_ui::EditorButtonClick;
use std::sync::Arc;

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use super::components::*;
use super::helpers::{copy_detail, phase_color, runtime_detail, summary, tool_context};
use super::inputs::StatusInputs;
use super::resources::EditorStatusBarState;
use super::types::{RuntimePhase, StatusAction, StatusPhase, StatusTextRole};
use crate::components::MapEditorRoot;
use crate::resources::MapEditorNumericScrubMouseGuard;
use crate::types::{EditorStatusKind, EditorStatusOperation};
use crate::ui::theme as colors;

/// 在领域表现完成之后隐藏重复反馈；保留原唯一文字实体，业务阶段与错误内容不被重写。
pub(super) fn hide_panel_status_texts(mut texts: Query<&mut Node, With<PanelStatusText>>) {
    for mut node in &mut texts {
        if node.display != Display::None { node.display = Display::None; }
    }
}

pub(super) fn observe_status(
    inputs: StatusInputs,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    mut state: ResMut<EditorStatusBarState>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    panels: Query<(Entity, Ref<PanelStatusText>, Ref<Text>, Has<feathrise_editor_ui::feedback::GlobalPanelStatus>)>,
    external: Query<&feathrise_editor_ui::feedback::PanelStatusState, With<PanelStatusText>>,
) {
    let editor = &inputs.editor;
    if state.map_id != editor.selected_map_id {
        state.map_id = editor.selected_map_id.clone();
        state.notices[EditorStatusOperation::MapSave.slot()] = None;
        state.retained_runtime = None;
        if state
            .latest_result
            .as_ref()
            .is_some_and(|result| result.operation == EditorStatusOperation::MapSave)
        {
            state.latest_result = None;
        }
        state.runtime_errors = Default::default();
        state.runtime_details = Default::default();
        state.detail_dirty = true;
        state.scroll_revision = state.scroll_revision.wrapping_add(1);
        state.popup_open = false;
        state.copy_result.clear(); // 旧地图诊断和阅读位置不能带到另一张地图。
        // 地图作者反馈失效，独立文件和预设仍使用原身份；不能让隐藏的错误因切图丢失。
        state.panel_status.retain(|entity, _| panels.get(*entity).is_ok_and(|(_, _, _, global)| global));
    }
    if (inputs.suppressed() || windows.single().is_ok_and(|window| !window.focused))
        && state.popup_open
    {
        state.popup_open = false;
    }
    if !editor.open {
        return;
    }
    let before_count = state.panel_status.len();
    state.panel_status.retain(|entity, _| panels.contains(*entity));
    if before_count != state.panel_status.len() { state.detail_dirty = true; }
    for (entity, source, text, _) in &panels {
        if !source.is_added() && !text.is_changed() { continue; }
        let value = text.as_str().trim();
        if value.is_empty() {
            if state.panel_status.remove(&entity).is_some() { state.detail_dirty = true; }
        } else if state.panel_status.get(&entity).is_none_or(|(label, old)| *label != source.0 || old.as_ref() != value) {
            state.panel_status.insert(entity, (source.0, Arc::from(value)));
            state.detail_dirty = true;
        }
    } // 只收集固定UI源的真实变化，不逐帧复制长文案，也不猜测文字中的“成功/失败”字样。
    let entries = inputs.entries();
    for (index, entry) in entries.iter().enumerate() {
        let current = entry.phase.map(|phase| (phase, entry.detail));
        if state.runtime_details[index]
            .as_ref()
            .map(|(phase, detail)| (*phase, detail.as_ref()))
            != current
        {
            state.runtime_details[index] =
                current.map(|(phase, detail)| (phase, Arc::from(detail)));
            state.detail_dirty = true;
        }
        let error = (entry.phase == Some(RuntimePhase::Failed)).then_some(entry.detail);
        if state.runtime_errors[index].as_deref() != error {
            state.runtime_errors[index] = error.map(Arc::from);
            if let Some(error) = error {
                state.retained_runtime = Some((
                    EditorStatusKind::Error,
                    Arc::from(format!("{}失败：{}", entry.label, error)),
                ));
                state.scroll_revision = state.scroll_revision.wrapping_add(1);
            }
        }
    }
    for result in editor.status_results.iter().flatten() {
        let slot = result.operation.slot();
        if result.sequence == state.seen_results[slot] {
            continue;
        }
        state.seen_results[slot] = result.sequence; // 旧地图回执也消费，标签返回不能重放已确认的诊断。
        if result.operation != EditorStatusOperation::MapSave
            || result.map_id == editor.selected_map_id
        {
            if state
                .latest_result
                .as_ref()
                .is_none_or(|latest| result.sequence.wrapping_sub(latest.sequence) < u64::MAX / 2)
            {
                state.latest_result = Some(result.clone());
            }
            state.detail_dirty = true;
            if result.kind != EditorStatusKind::Success {
                state.notices[slot] = Some(result.clone());
                state.scroll_revision = state.scroll_revision.wrapping_add(1);
            } else if state.notices[slot]
                .as_ref()
                .is_some_and(|notice| notice.target.id == result.target.id)
            {
                // 只解除同操作、同对象的诊断；另一预设、其他操作及运行时失败继续保留。
                state.notices[slot] = None;
                state.scroll_revision = state.scroll_revision.wrapping_add(1);
            }
        }
    }
    state.retained = state.retained_runtime.clone().or_else(|| {
        state
            .notices
            .iter()
            .flatten()
            .max_by_key(|notice| (notice.kind == EditorStatusKind::Error, notice.sequence))
            .map(|notice| (notice.kind, notice.detail.clone()))
    });
    let locale_changed = locale.as_ref().is_some_and(|locale| locale.is_changed());
    let locale = locale.as_deref().copied().unwrap_or_default();
    let editor_task = inputs.task(&entries);
    let mut task = locale.editor_notice(&editor_task).into_owned();
    use feathrise_editor_ui::feedback::PanelStatusPhase;
    let external_failure = external.iter().find(|status| !status.summary.is_empty() && status.phase == PanelStatusPhase::Failed);
    let working = !task.is_empty() || external.iter().any(|status| !status.summary.is_empty() && status.phase == PanelStatusPhase::Working);
    // 宿主阶段是持续只读投影，不重写 editor.status，也不把“已连接”的信息项误算为在途任务。
    for status in external.iter().filter(|status| !status.summary.is_empty()) {
        // 每个任务在拼接之前翻译。多项并行时完整字符串无法匹配单项文案键。
        if !task.is_empty() {
            task.push_str(if locale == feathrise_editor_ui::localization::EditorLocale::English { " · " } else { "；" });
        }
        task.push_str(&locale.editor_notice(status.summary));
    }
    let context = tool_context(editor, locale);
    let preset_failures = inputs.preset_failures();
    let phase = if inputs.loading_failed() {
        StatusPhase::Error // 正式失败仍需重试或取消，清除阅读提示不能把加载失败显示为正常处理中。
    } else if let Some((kind, _)) = &state.retained {
        match kind {
            EditorStatusKind::Error => StatusPhase::Error,
            _ => StatusPhase::Warning,
        }
    } else if entries
        .iter()
        .any(|entry| entry.phase == Some(RuntimePhase::Failed))
        || preset_failures.iter().any(Option::is_some)
        || external_failure.is_some()
    {
        StatusPhase::Error // 清除提示只是确认阅读，不会把仍失败的正式状态冒充就绪。
    } else if working {
        StatusPhase::Working
    } else if state.latest_result.as_ref().is_some_and(|result| {
        result.kind == EditorStatusKind::Success && result.status_text.as_ref() == editor.status
    }) {
        StatusPhase::Success
    } else {
        StatusPhase::Ready
    };
    let message = state
        .retained
        .as_ref()
        .map_or(editor.status.as_str(), |(_, detail)| detail.as_ref());
    let short = if state.retained.is_none()
        && let Some(entry) = entries
            .iter()
            .find(|entry| entry.phase == Some(RuntimePhase::Failed))
    {
        format!("{}仍未采用：{}", entry.label, summary(entry.detail))
    } else if state.retained.is_none()
        && let Some((_, _, detail)) = preset_failures.iter().flatten().next()
    {
        summary(detail) // 确认阅读不能把仍失败的目录采用显示为正常等待。
    } else if state.retained.is_none()
        && let Some(failure) = external_failure
    {
        summary(failure.summary)
    } else {
        summary(message)
    };
    if state.phase != phase {
        state.phase = phase;
    }
    if state.summary != short {
        state.summary = short;
    }
    if state.task != task {
        state.task = task;
        state.detail_dirty = true;
    }
    if state.context != context {
        state.context = context;
        state.detail_dirty = true;
    }
    if state.observed_status != editor.status {
        state.observed_status.clone_from(&editor.status);
        state.detail_dirty = true;
    }
    if state.observed_dirty != editor.dirty {
        state.observed_dirty = editor.dirty;
        state.detail_dirty = true;
    }

    // 完整诊断只在显式展开后组装，关闭的底栏不逐帧复制长路径和运行时明细。
    if state.popup_open && (state.detail_dirty || locale_changed || state.detail.is_empty()) {
        let mut detail = String::new();
        let current_message = locale.editor_notice(&editor.status);
        // 切图时先呈现当前正式阶段及完整失败原因，窄窗不必翻过旧地图的工具信息。
        if editor.map_transition_in_progress
            && let Some((label, value)) = state.panel_status.values().find(|(label, _)| *label == "地图加载")
        {
            detail.push_str(locale.editor_label(label));
            detail.push('\n');
            detail.push_str(value);
            detail.push_str("\n\n");
        }
        if let Some((_, retained)) = &state.retained_runtime {
            detail.push_str(locale.text("status.detail.retained_runtime"));
            detail.push_str(retained);
            detail.push_str("\n\n");
        }
        for notice in state.notices.iter().flatten() {
            detail.push_str(locale.text("status.detail.retained_operation"));
            detail.push_str(&notice.detail);
            detail.push_str("\n\n");
        }
        if let Some(result) = &state.latest_result
            && !state
                .notices
                .iter()
                .flatten()
                .any(|notice| notice.sequence == result.sequence)
        {
            detail.push_str(locale.text("status.detail.receipt"));
            detail.push_str(&result.detail);
            detail.push_str("\n\n");
        }
        let current_is_receipt = state
            .retained
            .as_ref()
            .is_some_and(|(_, retained)| retained.as_ref() == editor.status)
            || state
                .notices
                .iter()
                .flatten()
                .any(|notice| notice.status_text.as_ref() == editor.status)
            || state
                .latest_result
                .as_ref()
                .is_some_and(|result| result.status_text.as_ref() == editor.status);
        if !current_is_receipt {
            detail.push_str(locale.text("status.detail.current"));
            detail.push_str(&current_message);
        }
        let task = if state.task.is_empty() {
            locale.text("status.detail.no_task").into()
        } else {
            state.task.clone()
        };
        detail.push_str(&locale.format("status.detail.task", &[("task", task)]));
        detail.push_str(locale.text(if editor.dirty {
            "status.detail.drafts_dirty"
        } else {
            "status.detail.drafts_clean"
        }));
        detail.push_str(&locale.format("status.detail.view", &[("view", state.context.clone())]));
        detail.push_str(&runtime_detail(&entries, locale));
        if !state.panel_status.is_empty() {
            detail.push_str(locale.text("status.detail.panels"));
            let mut panels: Vec<_> = state.panel_status.values().collect();
            panels.sort_by_key(|(label, _)| *label);
            for (label, value) in panels {
                if editor.map_transition_in_progress && *label == "地图加载" { continue; }
                let value = locale.editor_notice(value);
                if value == current_message { continue; }
                // 只匹配编辑器已知回执模板，模板中的路径和外部诊断参数保留原文。
                detail.push_str("\n\n"); detail.push_str(locale.editor_label(label)); detail.push('\n');
                detail.push_str(&value);
            }
        }
        for (operation, target, error) in preset_failures.into_iter().flatten() {
            if state.notices[operation.slot()]
                .as_ref()
                .is_some_and(|notice| notice.target.id == target.id)
                || state.latest_result.as_ref().is_some_and(|result| {
                    result.operation == operation
                        && result.target.id == target.id
                        && result.kind == EditorStatusKind::Error
                })
            {
                continue;
            }
            detail.push_str(&locale.format("status.detail.catalog_failure", &[
                ("operation", locale.editor_label(operation.label()).into()),
                ("target", target.label.to_string()),
                ("id", target.id.to_string()),
                ("error", error.into()),
            ]));
        }
        if state.detail != detail {
            state.detail = detail;
            state.copy_result.clear();
        }
        state.detail_dirty = false;
    }
}

/// 在所有Update作者输入之前取得浮层下方的鼠标门，避免刚移入正文的首帧点透。
pub(super) fn track_pointer(
    inputs: StatusInputs,
    mouse: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<EditorStatusBarState>,
    mut guard: ResMut<MapEditorNumericScrubMouseGuard>,
    popups: Query<
        (&RelativeCursorPosition, &ComputedNode, &InheritedVisibility),
        With<StatusPopup>,
    >,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    panels: crate::workbench::WorkbenchFloatingPanels,
) {
    if state.captured_this_frame {
        state.captured_this_frame = false;
    }
    let hovered = state.popup_open
        && !inputs.suppressed()
        && !windows.single().is_ok_and(|window| !window.focused)
        // 详情仍可留在原位，只有实际处于最上层的面板接收重叠处滚轮。
        && panels.hovered().is_none_or(|entity| popups.contains(entity))
        && popups.iter().any(|(cursor, node, visible)| {
            visible.get() && node.size().min_element() > 0.0 && cursor.cursor_over()
        });
    if state.popup_hovered != hovered {
        state.popup_hovered = hovered;
    }
    if hovered {
        guard.advance(
            true,
            mouse.get_pressed().next().is_some(),
            mouse.get_just_released().next().is_some(),
        );
    }
}

pub(super) fn control_details(
    inputs: StatusInputs,
    mouse: Res<ButtonInput<MouseButton>>,
    active: Option<Res<feathrise_editor_ui::ActiveNumericInputScrub>>,
    mut state: ResMut<EditorStatusBarState>,
    mut guard: ResMut<MapEditorNumericScrubMouseGuard>,
    mut clipboard: ResMut<Clipboard>,
    buttons: Query<(&StatusButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut wheels: MessageReader<MouseWheel>,
    mut scrolls: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<StatusDetailScroll>,
    >,
    mut previous: Local<(bool, u64)>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let delta: f32 = wheels
        .read()
        .map(|event| {
            -event.y
                * match event.unit {
                    MouseScrollUnit::Line => 24.0,
                    MouseScrollUnit::Pixel => 1.0,
                }
        })
        .sum(); // 所有关闭/模态帧也消费边沿，不在恢复可用后重放旧点击或滚轮。
    let unavailable = inputs.suppressed()
        || windows.single().is_ok_and(|window| !window.focused)
        || active
            .as_ref()
            .is_some_and(|active| active.entity.is_some())
        || (guard.blocked() && !state.owns_mouse());
    for (button, interaction) in &buttons {
        if unavailable || !interaction.is_clicked() {
            continue;
        }
        if button.0 != StatusAction::Toggle && !state.popup_open {
            continue;
        }
        state.captured_this_frame = true;
        guard.advance(
            true,
            mouse.get_pressed().next().is_some(),
            mouse.get_just_released().next().is_some(),
        );
        match button.0 {
            StatusAction::Toggle => {
                state.popup_open = !state.popup_open;
                state.detail_dirty = true;
                state.copy_result.clear();
            }
            StatusAction::Close => state.popup_open = false,
            StatusAction::Dismiss => {
                state.retained = None;
                state.notices = Default::default();
                state.retained_runtime = None;
                state.detail_dirty = true;
                state.popup_open = false;
            }
            StatusAction::Copy => {
                let (failed, result) =
                    copy_detail(&state.detail, |detail| clipboard.set_text(detail));
                state.copy_failed = failed;
                state.copy_result = result; // 只执行本次明确复制，不读取或隐式重试系统剪贴板。
            }
        }
    }
    for (cursor, node, mut scroll) in &mut scrolls {
        let reset = !state.popup_open || *previous != (state.popup_open, state.scroll_revision);
        let maximum =
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        let next = if reset {
            0.0
        } else if !unavailable && cursor.cursor_over() && node.size().min_element() > 0.0 {
            (scroll.y + delta).clamp(0.0, maximum)
        } else {
            scroll.y.min(maximum)
        };
        if scroll.y != next {
            scroll.y = next;
        }
    }
    *previous = (state.popup_open, state.scroll_revision);
}

pub(super) fn sync_presentation(
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    inputs: StatusInputs,
    state: Res<EditorStatusBarState>,
    roots: Query<&ComputedNode, With<MapEditorRoot>>,
    mut popups: Query<(&mut Node, &mut GlobalZIndex), With<StatusPopup>>,
    mut footers: Query<&mut GlobalZIndex, (With<StatusFooter>, Without<StatusPopup>)>,
    mut contents: Query<&mut Node, (With<StatusFooterContent>, Without<StatusPopup>, Without<StatusText>)>,
    mut texts: Query<(&StatusText, &mut Text, &mut TextColor, &mut Node), Without<StatusPopup>>,
) {
    let visible = state.popup_open && !inputs.suppressed();
    let loading = inputs.editor.open && inputs.editor.map_transition_in_progress;
    // 只把诊断和原恢复按钮提到加载输入门上方，工作台与世界控件仍不可操作。
    for mut layer in &mut footers {
        let wanted = if loading { 1901 } else { 421 }; // 恢复后仍须高于工作台根的420。
        if layer.0 != wanted { layer.0 = wanted; }
    }
    for mut node in &mut contents {
        let wanted = if loading { Display::None } else { Display::Flex };
        if node.display != wanted { node.display = wanted; }
    }
    for (mut popup, mut layer) in &mut popups {
        let wanted = if loading { 1902 } else { 425 };
        if layer.0 != wanted { layer.0 = wanted; }
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if popup.display != display {
            popup.display = display;
        }
    }
    let width = roots
        .single()
        .map_or(0.0, |node| node.size().x * node.inverse_scale_factor());
    let full_context = width >= 700.0;
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (role, mut text, mut color, mut node) in &mut texts {
        let (value, tint, show) = match role.0 {
            StatusTextRole::Phase => (state.phase.label(), phase_color(state.phase), true),
            StatusTextRole::Summary => (state.summary.as_str(), colors::TEXT_PRIMARY, true),
            StatusTextRole::Task => (
                state.task.as_str(),
                colors::TEXT_ACCENT,
                !state.task.is_empty() && width >= 360.0,
            ),
            StatusTextRole::Context => (state.context.as_str(), colors::TEXT_MUTED, full_context),
            StatusTextRole::Detail => (
                if visible { state.detail.as_str() } else { "" },
                colors::TEXT_PRIMARY,
                true,
            ),
            StatusTextRole::Title => ("状态详情", colors::TEXT_PRIMARY, true),
            StatusTextRole::CopyResult => (
                state.copy_result.as_str(),
                if state.copy_failed {
                    colors::DANGER_TEXT
                } else {
                    colors::TEXT_GREEN
                },
                !state.copy_result.is_empty(),
            ),
        };
        let value = if matches!(role.0, StatusTextRole::Detail | StatusTextRole::Context | StatusTextRole::Task) {
            std::borrow::Cow::Borrowed(value) // 已按当前语言组装；保持正文中的原始错误载荷。
        } else {
            locale.editor_notice(value)
        };
        if text.as_str() != value.as_ref() {
            **text = value.into_owned();
        }
        if color.0 != tint {
            color.0 = tint;
        }
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// 小逻辑视口沿用UI库的共同紧凑档，保持原锚点/60%高度上限及唯一可滚动正文。
pub(super) fn sync_popup_layout(
    roots: Query<&ComputedNode, With<MapEditorRoot>>,
    mut sections: Query<(&mut Node, Has<StatusPopupHeader>, Has<StatusPopupFooter>),
        (Or<(With<StatusPopupHeader>, With<StatusPopupFooter>, With<StatusDetailScroll>)>, Without<StatusButton>)>,
    mut buttons: Query<(&StatusButton, &mut Node),
        (Without<StatusPopupHeader>, Without<StatusPopupFooter>, Without<StatusDetailScroll>)>,
) {
    let Ok(root) = roots.single() else { return; };
    let size = root.size() * root.inverse_scale_factor();
    let available = Vec2::new((size.x - 16.).max(0.), (size.y * 0.6).min(280.));
    let compact = feathrise_editor_ui::dialog_uses_compact_layout(available);
    for (mut node, header, footer) in &mut sections {
        let next = if header {
            if compact { feathrise_editor_ui::dialog_compact_header_node() }
            else { feathrise_editor_ui::dialog_header_node() }
        } else if footer {
            if compact { feathrise_editor_ui::dialog_compact_footer_node() }
            else { feathrise_editor_ui::dialog_footer_node() }
        } else { super::scene::detail_scroll_node(compact) };
        if *node != next { *node = next; }
    }
    for (action, mut node) in &mut buttons {
        // 24px底栏的入口不属于弹窗外壳，不因展开或缩放改变原命中范围。
        if action.0 == StatusAction::Toggle { continue; }
        let style = match (compact, action.0 == StatusAction::Close) {
            (true, true) => feathrise_editor_ui::dialog_compact_close_button_style(),
            (true, false) => feathrise_editor_ui::dialog_compact_button_style(false),
            (false, true) => feathrise_editor_ui::dialog_close_button_style(),
            (false, false) => feathrise_editor_ui::dialog_button_style(false),
        };
        if *node != style.node { *node = style.node; }
    }
}
