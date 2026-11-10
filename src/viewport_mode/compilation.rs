//! 工程3D启动编译复用原启动按钮提供停止；保留当前模式，明确切换才在真实收口后返回作者3D。

use super::{RequestViewportMode, ViewportMode, ViewportModeState, components::CompilationLaunchDisabled,
    resources::CompilationReturn};
use bevy::{prelude::*, ui::InteractionDisabled};
use feathrise_editor_core::{ProjectLaunchButton, ProjectLaunchInterruption, ProjectLaunchLabel};
use feathrise_editor_ui::EditorButtonClick;

pub(super) fn stop_and_return(world: &mut World) -> bool {
    stop(world, true)
}

fn stop(world: &mut World, return_to_editor: bool) -> bool {
    if world.resource::<ViewportModeState>().selected != ViewportMode::EmbeddedWorld
        || world.get_resource::<crate::native_plugins::management::PluginManagerState>().is_some_and(|plugins|plugins.captures_input())
        || world.get_resource::<crate::workspace_ui::AboutState>().is_some_and(|about|about.captures_input())
        || crate::project_export::ui::captures_input(world) { return false; }
    let revision = world.resource::<ViewportModeState>().revision;
    let pending = world.get_resource::<CompilationReturn>().is_some_and(|intent|intent.selection_revision == Some(revision));
    if !pending && !crate::embedded_viewport::stop_compilation(world) { return false; }
    let mut intent = world.resource_mut::<CompilationReturn>();
    if !pending { *intent = CompilationReturn { selection_revision:Some(revision), return_to_editor }; }
    // 停止过程中仍可明确选择编辑器3D；升级原意图即可，不能对已取消的控制牌再次启动任务。
    intent.return_to_editor |= return_to_editor;
    let returning = intent.return_to_editor;
    world.resource_mut::<ViewportModeState>().status = if returning {
        "正在停止编译；后台任务退出后返回编辑器 3D"
    } else { "正在停止编译；后台任务退出后保留工程 3D" }.into();
    true
}

/// 必须先于原场景按钮分派消费边沿，否则同一按钮会把停止误作预览或关闭作者界面。
pub(super) fn input(world: &mut World) {
    let stopping = world.resource::<CompilationReturn>().selection_revision.is_some();
    let stoppable = world.resource::<ViewportModeState>().selected == ViewportMode::EmbeddedWorld
        && crate::embedded_viewport::can_stop_compilation(world);
    if !stopping && !stoppable { return; }
    let buttons: Vec<_> = world.query_filtered::<(Entity, &EditorButtonClick), With<ProjectLaunchButton>>()
        .iter(world).filter_map(|(entity, click)| click.is_clicked().then_some(entity)).collect();
    let blocked = world.get_resource::<feathrise_editor_core::EditorHostOverlay>().is_some_and(|overlay|overlay.active)
        || world.get_resource::<crate::native_project_close::NativeProjectCloseState>().is_some_and(|close|close.captures_input())
        || world.get_resource::<crate::project_export::ui::ExportOverlayState>().is_some_and(|export|export.captures_input())
        || world.get_resource::<crate::native_plugins::management::PluginManagerState>().is_some_and(|plugins|plugins.captures_input())
        || world.get_resource::<crate::workspace_ui::AboutState>().is_some_and(|about|about.captures_input())
        || world.get_resource::<super::BuildProfileMenuState>().is_some_and(|menu|menu.captures_input());
    let mut clicked = false;
    for button in buttons {
        let allowed = visible(world, button) && world.get::<InteractionDisabled>(button).is_none();
        if let Some(mut click) = world.get_mut::<EditorButtonClick>(button) { click.invalidate(); }
        clicked |= allowed;
    }
    if clicked && !blocked && !stopping { stop(world, false); }
}

/// 只停止时不发布模式请求；明确切回才重放目标，清理失败继续由原门阻止。
pub(super) fn return_when_stopped(world: &mut World) {
    let Some(revision) = world.resource::<CompilationReturn>().selection_revision else { return; };
    let state = world.resource::<ViewportModeState>();
    if state.selected != ViewportMode::EmbeddedWorld || state.revision != revision {
        *world.resource_mut::<CompilationReturn>() = default();
        return;
    }
    if world.get_resource::<crate::embedded_viewport::EmbeddedStatus>().is_some_and(|status| status.active)
        || super::systems::blocked_reason(world, ViewportMode::SavedWorld).is_some() { return; }
    let returning = world.resource::<CompilationReturn>().return_to_editor;
    *world.resource_mut::<CompilationReturn>() = default();
    if returning {
        world.write_message(RequestViewportMode(ViewportMode::SavedWorld));
    } else {
        world.resource_mut::<ViewportModeState>().status = "工程3D编译已停止；点击“预览”可重试".into();
    }
}

/// 复用原64像素按钮；四字停止文案隐藏三角图标，状态变化不移动相邻配置和布局入口。
pub(super) fn present(world: &mut World) {
    let stopping = world.resource::<CompilationReturn>().selection_revision.is_some();
    let stoppable = world.resource::<ViewportModeState>().selected == ViewportMode::EmbeddedWorld
        && crate::embedded_viewport::can_stop_compilation(world);
    let interruption = if stopping { ProjectLaunchInterruption::StoppingCompilation }
        else if stoppable { ProjectLaunchInterruption::CanStopCompilation } else { ProjectLaunchInterruption::None };
    let changed = *world.resource::<ProjectLaunchInterruption>() != interruption;
    world.resource_mut::<ProjectLaunchInterruption>().set_if_neq(interruption);
    let caption = interruption.action_label();
    let label = caption.unwrap_or_else(||world.resource::<feathrise_editor_core::ProjectLaunchSettings>().action_label());
    let label = world.get_resource::<feathrise_editor_ui::localization::EditorLocale>()
        .copied().unwrap_or_default().editor_label(label);
    for mut text in world.query_filtered::<&mut Text, With<ProjectLaunchLabel>>().iter_mut(world) {
        if text.as_str() != label { **text = label.into(); }
    }
    let buttons: Vec<_> = world.query_filtered::<Entity, With<ProjectLaunchButton>>().iter(world).collect();
    for button in buttons {
        // 同一实体的动作已经变了：按住旧“预览”或旧“停止”不能在松开时执行新语义。
        if changed && let Some(mut click) = world.get_mut::<EditorButtonClick>(button) { click.invalidate(); }
        let owned = world.get::<CompilationLaunchDisabled>(button).is_some();
        if stopping && world.get::<InteractionDisabled>(button).is_none() {
            // 原场景表现系统可在Update撤销Close按钮的禁用；每帧布局前重投影，输入仍独立消费。
            world.entity_mut(button).insert((InteractionDisabled, CompilationLaunchDisabled));
        } else if !stopping && owned {
            world.entity_mut(button).remove::<(InteractionDisabled, CompilationLaunchDisabled)>();
        }
        let glyphs: Vec<_> = world.get::<Children>(button).into_iter().flatten().copied()
            .filter(|child| world.get::<Text>(*child).is_some_and(|text|text.as_str() == "▶")).collect();
        for glyph in glyphs {
            if let Some(mut node) = world.get_mut::<Node>(glyph) {
                let display = if caption.is_some() { Display::None } else { Display::Flex };
                if node.display != display { node.display = display; }
            }
        }
    }
}

fn visible(world: &World, mut entity: Entity) -> bool {
    for _ in 0..128 {
        if world.get::<InheritedVisibility>(entity).is_some_and(|visibility|!visibility.get())
            || world.get::<Node>(entity).is_some_and(|node|node.display == Display::None) { return false; }
        let Some(parent) = world.get::<ChildOf>(entity) else { return true; };
        entity = parent.parent();
    }
    false
}

#[cfg(test)]
#[path = "compilation_tests.rs"]
mod tests;
