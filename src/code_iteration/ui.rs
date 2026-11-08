//! 底部原日志旁的代码入口；只占原状态栏，候选版本成功后才提供明确采用按钮。

use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::CodeIterationRuntime, types::CodeIterationPhase as Phase, CodeIterationState, RequestApplyCode};
use bevy::prelude::*;
use feathrise_editor_map::workbench::WorkbenchProjectLogButton;
use feathrise_editor_ui::{colors as c, EditorButtonClick};

pub(super) fn mount(
    mut commands: Commands,
    logs: Query<(Entity, &ChildOf), Added<WorkbenchProjectLogButton>>,
    parents: Query<&Children>,
    assets: Res<AssetServer>,
) {
    if logs.is_empty() { return; }
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for (log, parent) in &logs {
        let controls = commands.spawn((CodeIterationControls,
            Node { height: px(20), flex_shrink: 0., column_gap: px(4), align_items: AlignItems::Center, ..default() }))
            .with_children(|row| {
                for (action, label) in [(CodeIterationAction::Diagnostics, "代码"), (CodeIterationAction::Apply, "应用并重启预览")] {
                    let style = feathrise_editor_ui::dialog_button_style(false);
                    row.spawn((InteractionButton, action, ApplyVersion::default(), style.colors,
                        Node { display: if matches!(action, CodeIterationAction::Apply) { Display::None } else { Display::Flex },
                            height: px(20), min_width: px(42), flex_shrink: 0., align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center, padding: UiRect::horizontal(px(6)),
                            border_radius: BorderRadius::all(px(2)), ..default() },
                        BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL)))
                        .with_children(|button| {
                            button.spawn((CodeIterationLabel(action), Text::new(label), Pickable::IGNORE,
                                TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
                                TextLayout::no_wrap(), TextColor(c::TEXT_PRIMARY)));
                        });
                }
                row.spawn((CodeIterationStatus, Text::new(""), Node { display: Display::None, ..default() },
                    feathrise_editor_ui::PanelStatusText("代码"), feathrise_editor_ui::feedback::GlobalPanelStatus,
                    feathrise_editor_ui::feedback::PanelStatusState::default()));
            }).id();
        let index = parents.get(parent.parent()).ok().and_then(|children| children.iter().position(|child| child == log)).map_or(0, |index| index + 1);
        commands.entity(parent.parent()).insert_children(index, &[controls]);
    }
}

pub(super) fn input(
    mut state: ResMut<CodeIterationState>,
    close: Option<Res<crate::native_project_close::NativeProjectCloseState>>,
    plugins: Option<Res<crate::native_plugins::management::PluginManagerState>>,
    buttons: Query<(&CodeIterationAction, &ApplyVersion, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut apply: MessageWriter<RequestApplyCode>,
) {
    // 插件模态及其关闭释放帧都消费残留按钮回执，不等待稍后聚合的EditorHostOverlay再补救。
    if close.is_some_and(|close| close.captures_input())
        || plugins.is_some_and(|plugins| plugins.captures_input()) { return; }
    for (action, version, click) in &buttons {
        if !click.is_clicked() { continue; }
        match action {
            CodeIterationAction::Diagnostics => state.diagnostics_open = !state.diagnostics_open,
            CodeIterationAction::Apply => if let Some(generation) = version.0 { apply.write(RequestApplyCode { generation }); },
        }
    }
}

pub(super) fn present(
    state: Res<CodeIterationState>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    runtime: NonSend<CodeIterationRuntime>,
    mode: Res<crate::viewport_mode::ViewportModeState>,
    mut buttons: Query<(&CodeIterationAction, &mut ApplyVersion, &mut Node, Option<&mut EditorButtonClick>)>,
    mut labels: Query<(&CodeIterationLabel, &mut Text, &mut TextColor)>,
) {
    let candidate = state.pending.as_ref().filter(|version| runtime.backend.as_ref().is_some_and(|backend| backend.is_current(&version.identity)))
        .map(|version| version.identity.generation).filter(|_| state.applying.is_none()
            && runtime.lifecycle == super::resources::BackendLifecycle::Running);
    for (action, mut bound, mut node, click) in &mut buttons {
        if !matches!(action, CodeIterationAction::Apply) { continue; }
        if bound.0 != candidate {
            if let Some(mut click) = click { click.invalidate(); }
            bound.0 = candidate;
        }
        let display = if candidate.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (label, mut text, mut color) in &mut labels {
        let value = match label.0 {
            CodeIterationAction::Diagnostics if !state.diagnostics.is_empty() =>
                locale.format("code.diagnostics.count", &[("count", state.diagnostics.len().to_string())]),
            CodeIterationAction::Diagnostics => locale.editor_label("代码").into(),
            CodeIterationAction::Apply => if state.pending.as_ref().is_some_and(|version| matches!(version.artifact, super::PreparedArtifact::Patch(_))) {
                locale.text("code.apply.hot").into()
            } else if mode.selected == crate::viewport_mode::ViewportMode::SavedWorld {
                locale.text("code.apply.restart").into()
            } else { locale.text("code.apply.restart_preview").into() },
        };
        if text.0 != value { text.0 = value; }
        color.set_if_neq(TextColor(if matches!(label.0, CodeIterationAction::Diagnostics) && state.phase == Phase::Failed { c::TEXT_ACCENT } else { c::TEXT_PRIMARY }));
    }
}

pub(super) fn status(
    state: Res<CodeIterationState>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    mut labels: Query<(&mut feathrise_editor_ui::feedback::PanelStatusState, &mut Text), With<CodeIterationStatus>>,
) {
    use feathrise_editor_ui::feedback::PanelStatusPhase;
    let phase = match state.phase {
        Phase::Waiting | Phase::Building | Phase::Applying | Phase::Stopping => PanelStatusPhase::Working,
        Phase::Failed => PanelStatusPhase::Failed,
        _ => PanelStatusPhase::Info,
    };
    let summary = match state.phase {
        Phase::Watching => "保存代码后自动编译", Phase::Waiting => "等待代码编译", Phase::Building => "代码编译中",
        Phase::Failed => "代码构建或应用失败", Phase::Ready => "新代码可应用", Phase::Applying => "正在应用新代码",
        Phase::Applied => "新代码版本已应用", Phase::Stopping => "代码构建收口中",
    };
    let locale_changed = locale.as_ref().is_some_and(|locale| locale.is_changed());
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (mut label, mut text) in &mut labels {
        if label.phase != phase || label.summary != summary {
            label.phase = phase;
            label.summary = summary;
        }
        if state.is_changed() || locale_changed || text.is_added() {
            // 构建工具名来自编辑器自己的选择。构建失败的原始诊断仍作为完整参数呈现。
            let detail = locale.editor_notice_with("code.status.watching_config", &state.status, |name, value| {
                if name == "backend" { locale.editor_label(value).into() } else { value.into() }
            }).map_or_else(|| locale.editor_notice(&state.status), std::borrow::Cow::Owned);
            if text.as_str() != detail.as_ref() { text.0 = detail.into_owned(); }
        }
    }
}
