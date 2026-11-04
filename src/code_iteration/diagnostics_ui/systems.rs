//! 可见诊断行按结果与滚动代次换绑；旧按下/释放不得导航到新文件，静止帧不重排整份诊断。

use super::{components::*, resources::*};
use super::super::resources::CodeIterationState;
use super::super::types::CodeIterationPhase;
use crate::compiler_diagnostics::{CompilerDiagnostic, DiagnosticLevel};
use bevy::{input::mouse::{MouseScrollUnit, MouseWheel}, input_focus::InputFocus, prelude::*,
    ui::{InteractionDisabled, RelativeCursorPosition}, window::PrimaryWindow};
use feathrise_editor_map::{standalone::OpenProjectSourceLocation, workbench::WorkbenchModel};
use feathrise_editor_ui::{EditorButtonClick, colors as c};

pub(super) fn current(row: &DiagnosticRow, state: &CodeIterationState, view: &DiagnosticsOverlayState, scroll: Vec2) -> bool {
    row.revision == state.diagnostic_revision && row.window_revision == view.window_revision
        && view.scroll == scroll && row.index.is_some_and(|index| index < state.diagnostics.len())
}

pub(super) fn control(
    mut state: ResMut<CodeIterationState>, mut view: ResMut<DiagnosticsOverlayState>,
    close: Option<Res<crate::native_project_close::NativeProjectCloseState>>,
    plugins: Option<Res<crate::native_plugins::management::PluginManagerState>>,
    logs: Option<Res<crate::project_log::ProjectLogState>>, model: Option<Res<WorkbenchModel>>,
    export: Option<Res<crate::project_export::ui::ExportOverlayState>>,
    keys: Res<ButtonInput<KeyCode>>, windows: Query<&Window, With<PrimaryWindow>>,
    mut focus: ResMut<InputFocus>, mut wheels: MessageReader<MouseWheel>,
    closes: Query<&EditorButtonClick, With<DiagnosticsClose>>,
    rows: Query<(&DiagnosticRow, &EditorButtonClick)>,
    mut panes: Query<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<DiagnosticsPane>>,
    mut open: MessageWriter<OpenProjectSourceLocation>,
) {
    let was_open = view.was_open;
    if close.is_some_and(|close| close.captures_input()) || logs.is_some_and(|logs| logs.captures_input())
        || export.is_some_and(|export| export.captures_input())
        || plugins.is_some_and(|plugins| plugins.captures_input()) {
        state.diagnostics_open = false;
    }
    if state.diagnostics_open && !was_open { focus.clear(); }
    let focused = windows.iter().all(|window| window.focused);
    for wheel in wheels.read() {
        if !state.diagnostics_open || !focused { continue; }
        for (cursor, node, mut scroll) in &mut panes {
            if cursor.cursor_over() && !node.is_empty() {
                let unit = if wheel.unit == MouseScrollUnit::Line { ROW_HEIGHT * 3. } else { 1. };
                let height = node.content_box().height() * node.inverse_scale_factor();
                let maximum = (state.diagnostics.len() as f32 * ROW_HEIGHT - height).max(0.);
                scroll.y = (scroll.y - wheel.y * unit).clamp(0., maximum);
            }
        }
    }
    if state.diagnostics_open {
        if keys.just_pressed(KeyCode::Escape) || closes.iter().any(EditorButtonClick::is_clicked) {
            state.diagnostics_open = false;
        } else {
            let scroll = panes.single().ok().map(|(_, _, scroll)| scroll.0).unwrap_or_default();
            for (row, click) in &rows {
                if !click.is_clicked() || !current(row, &state, &view, scroll) { continue; }
                let Some(location) = state.diagnostics[row.index.unwrap()].location.as_ref() else { continue; };
                open.write(OpenProjectSourceLocation {
                    context: model.as_ref().map_or(state.source_context, |model| model.context),
                    project_root: state.project_root.clone(), path: location.path.clone(), line: location.line, column: location.column,
                });
                state.diagnostics_open = false;
                break;
            }
        }
    }
    // 保留原宿主的整窗模态与释放帧合同；真正的文档定位在门释放后继续完成。
    view.captured = was_open || state.diagnostics_open;
    view.was_open = state.diagnostics_open;
}

pub(super) fn present(
    mut commands: Commands, state: Res<CodeIterationState>, mut view: ResMut<DiagnosticsOverlayState>,
    mut overlays: Query<&mut Node, With<DiagnosticsOverlay>>,
    mut panes: Query<(&ComputedNode, &mut ScrollPosition), With<DiagnosticsPane>>,
    mut contents: Query<&mut Node, (With<DiagnosticsContent>, Without<DiagnosticsOverlay>, Without<DiagnosticRow>, Without<DiagnosticsEmpty>)>,
    mut rows: Query<(Entity, &mut DiagnosticRow, &mut Node, &mut EditorButtonClick, &mut Interaction, Has<InteractionDisabled>),
        (Without<DiagnosticsOverlay>, Without<DiagnosticsContent>, Without<DiagnosticsEmpty>)>,
    mut labels: Query<(&DiagnosticRowLabel, &mut Text, &mut TextColor), (Without<DiagnosticsStatus>, Without<DiagnosticsFooter>)>,
    mut statuses: Query<&mut Text, (With<DiagnosticsStatus>, Without<DiagnosticRowLabel>, Without<DiagnosticsFooter>)>,
    mut footers: Query<&mut Text, (With<DiagnosticsFooter>, Without<DiagnosticRowLabel>, Without<DiagnosticsStatus>)>,
    mut empties: Query<&mut Node, (With<DiagnosticsEmpty>, Without<DiagnosticsOverlay>, Without<DiagnosticsContent>, Without<DiagnosticRow>)>,
) {
    for mut overlay in &mut overlays {
        let display = if state.diagnostics_open { Display::Flex } else { Display::None };
        if overlay.display != display { overlay.display = display; }
    }
    if !state.diagnostics_open { return; }
    let reset = view.revision != Some(state.diagnostic_revision);
    let mut scroll = Vec2::ZERO;
    for (node, mut position) in &mut panes {
        let height = node.content_box().height() * node.inverse_scale_factor();
        let maximum = (state.diagnostics.len() as f32 * ROW_HEIGHT - height).max(0.);
        position.y = if reset { 0. } else { position.y.clamp(0., maximum) };
        scroll = position.0;
    }
    let first = (scroll.y.max(0.) / ROW_HEIGHT).floor() as usize;
    if reset || view.first != first || view.scroll != scroll {
        view.revision = Some(state.diagnostic_revision);
        view.first = first;
        view.scroll = scroll;
        view.window_revision = view.window_revision.wrapping_add(1);
    }
    for mut node in &mut contents {
        let height = px(state.diagnostics.len() as f32 * ROW_HEIGHT);
        if node.height != height { node.height = height; }
    }
    for (entity, mut binding, mut node, mut click, mut interaction, disabled) in &mut rows {
        let index = first.saturating_add(binding.slot);
        let next = (index < state.diagnostics.len()).then_some(index);
        if binding.index == next && binding.revision == state.diagnostic_revision && binding.window_revision == view.window_revision { continue; }
        binding.index = next;
        binding.revision = state.diagnostic_revision;
        binding.window_revision = view.window_revision;
        click.invalidate();
        *interaction = Interaction::None;
        node.display = if next.is_some() { Display::Flex } else { Display::None };
        node.top = px(index as f32 * ROW_HEIGHT);
        let can_open = next.is_some_and(|index| state.diagnostics[index].location.is_some());
        if can_open == disabled {
            if can_open { commands.entity(entity).remove::<InteractionDisabled>(); }
            else { commands.entity(entity).insert(InteractionDisabled); }
        }
        for (label, mut text, mut color) in &mut labels {
            if label.slot != binding.slot { continue; }
            let value = next.map_or_else(String::new, |index| row_text(&state.diagnostics[index], label.message));
            if text.as_str() != value { **text = value; }
            color.0 = if label.message { c::TEXT_PRIMARY } else {
                match next.map(|index| state.diagnostics[index].level) {
                    Some(DiagnosticLevel::Error) => Color::srgb(0.92, 0.45, 0.40),
                    Some(DiagnosticLevel::Warning) => Color::srgb(0.87, 0.72, 0.39),
                    _ => c::TEXT_MUTED,
                }
            };
        }
    }
    // state.status 保留原构建失败上下文，可能含多行 stderr 和 ANSI；状态行只投影阶段，
    // 不把终端正文交给普通 Text 排版，也不另写一套终端转义解析器。
    let status = match state.phase {
        CodeIterationPhase::Watching => "保存代码后自动编译",
        CodeIterationPhase::Waiting => "等待代码编译",
        CodeIterationPhase::Building => "正在后台编译代码",
        CodeIterationPhase::Failed => "代码构建或应用失败 · 完整输出见日志",
        CodeIterationPhase::Ready => "编译完成，可应用并重启预览",
        CodeIterationPhase::Applying => "正在应用新代码并重启预览",
        CodeIterationPhase::Applied => "新代码版本已应用",
        CodeIterationPhase::Stopping => "正在收口代码构建任务",
    };
    for mut text in &mut statuses { if text.as_str() != status { **text = status.into(); } }
    let footer = format!("{} 条诊断 · 点击位置在中央源码定位；完整输出可在日志中选择复制。", state.diagnostics.len());
    for mut text in &mut footers { if text.as_str() != footer { **text = footer.clone(); } }
    for mut node in &mut empties {
        let display = if state.diagnostics.is_empty() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
}

pub(super) fn row_text(diagnostic: &CompilerDiagnostic, message: bool) -> String {
    if message {
        // 诊断保留完整有界文本供数据验证；固定行只准备首段和有限字形，完整上下文仍由原终端呈现。
        let first = diagnostic.message.lines().next().unwrap_or_default();
        let mut end = first.len().min(1024);
        while !first.is_char_boundary(end) { end -= 1; }
        return if end == first.len() { first.to_owned() } else { format!("{}…", &first[..end]) };
    }
    let code = diagnostic.code.as_deref().map_or(String::new(), |code| format!(" {code}"));
    let position = diagnostic.location.as_ref().map_or_else(|| "无当前工程源码位置".into(),
        |location| format!("{}:{}:{}", location.path.display(), location.line, location.column));
    format!("{}{code} · {position}", diagnostic.level.label())
}
