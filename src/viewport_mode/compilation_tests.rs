//! 停止保留模式、明确返回意图与原按钮输入隔离回归；真实Cargo停止由专项窗口验收另证。

use super::*;
use crate::{embedded_viewport::EmbeddedStatus, viewport_mode::{ViewportModeAvailability, ViewportModeChanged}};
use feathrise_editor_map::standalone::SavedPlayFeedback;

fn app() -> App {
    let mut app = App::new();
    app.insert_resource(ViewportModeState { selected:ViewportMode::EmbeddedWorld, revision:1, status:String::new() })
        .insert_resource(CompilationReturn { selection_revision:Some(1), return_to_editor:false })
        .init_resource::<ViewportModeAvailability>()
        .init_resource::<EmbeddedStatus>()
        .insert_resource(feathrise_editor_core::ProjectLaunchSettings { preview:true, ..default() })
        .init_resource::<ProjectLaunchInterruption>()
        .add_message::<RequestViewportMode>()
        .add_message::<ViewportModeChanged>()
        .add_message::<SavedPlayFeedback>()
        .add_systems(Update, (input, return_when_stopped, super::super::systems::apply).chain());
    app
}

#[test]
fn stop_return_waits_for_real_active_flag_and_existing_switch_gate() {
    let mut app = app();
    app.world_mut().resource_mut::<CompilationReturn>().return_to_editor = true;
    app.world_mut().resource_mut::<EmbeddedStatus>().active = true;
    app.update();
    assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::EmbeddedWorld);
    app.world_mut().resource_mut::<EmbeddedStatus>().active = false;
    app.world_mut().resource_mut::<ViewportModeAvailability>().blocked_reason = Some("清理未确认".into());
    app.update();
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 1);
    app.world_mut().resource_mut::<ViewportModeAvailability>().blocked_reason = None;
    app.update();
    assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::SavedWorld);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 2);
    assert!(app.world().resource::<CompilationReturn>().selection_revision.is_none());
}

#[test]
fn stop_without_mode_request_keeps_project_3d_after_real_drain() {
    let mut app = app();
    app.world_mut().resource_mut::<EmbeddedStatus>().active = true;
    app.update();
    assert_eq!(app.world().resource::<CompilationReturn>().selection_revision, Some(1));
    app.world_mut().resource_mut::<EmbeddedStatus>().active = false;
    app.world_mut().resource_mut::<ViewportModeAvailability>().blocked_reason = Some("清理未确认".into());
    app.update();
    assert_eq!(app.world().resource::<CompilationReturn>().selection_revision, Some(1));
    app.world_mut().resource_mut::<ViewportModeAvailability>().blocked_reason = None;
    app.update();
    assert!(app.world().resource::<CompilationReturn>().selection_revision.is_none());
    assert!(app.world().resource::<Messages<RequestViewportMode>>().is_empty());
    assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::EmbeddedWorld);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 1);
    assert!(app.world().resource::<ViewportModeState>().status.contains("点击“预览”可重试"));
}

#[test]
fn explicit_editor_selection_while_stopping_upgrades_only_the_return_intent() {
    let mut app = app();
    app.world_mut().resource_mut::<EmbeddedStatus>().active = true;
    app.world_mut().write_message(RequestViewportMode(ViewportMode::SavedWorld));
    app.update();
    assert!(app.world().resource::<CompilationReturn>().return_to_editor);
    assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::EmbeddedWorld);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 1);
    app.world_mut().resource_mut::<EmbeddedStatus>().active = false;
    app.update();
    assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::SavedWorld);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 2);
}

#[test]
fn stopping_consumes_original_launch_click_even_when_another_style_system_reenables_it() {
    let mut app = app();
    app.world_mut().resource_mut::<EmbeddedStatus>().active = true;
    let button = app.world_mut().spawn((ProjectLaunchButton, EditorButtonClick::triggered(), Node::default())).id();
    app.update();
    assert!(!app.world().get::<EditorButtonClick>(button).unwrap().is_clicked());
    assert!(app.world().resource::<Messages<RequestViewportMode>>().is_empty());
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 1);
}

#[test]
fn pending_stop_never_changes_a_later_selection_revision() {
    let mut app = app();
    app.world_mut().resource_mut::<CompilationReturn>().return_to_editor = true;
    app.world_mut().resource_mut::<ViewportModeState>().revision = 3;
    app.update();
    assert!(app.world().resource::<CompilationReturn>().selection_revision.is_none());
    assert!(!app.world().resource::<CompilationReturn>().return_to_editor);
    assert_eq!(app.world().resource::<ViewportModeState>().selected, ViewportMode::EmbeddedWorld);
    assert_eq!(app.world().resource::<ViewportModeState>().revision, 3);
}

#[test]
fn compact_stop_projection_preserves_original_button_geometry_and_restores_its_icon() {
    let mut app = app();
    let glyph = app.world_mut().spawn((Text::new("▶"), Node::default())).id();
    let label = app.world_mut().spawn((ProjectLaunchLabel, Text::new("预览"))).id();
    let button = app.world_mut().spawn((ProjectLaunchButton, EditorButtonClick::triggered(), Node { width:px(64), height:px(26), ..default() }))
        .add_children(&[glyph,label]).id();
    present(app.world_mut());
    assert_eq!(app.world().get::<Text>(label).unwrap().as_str(), "正在停止");
    assert_eq!(app.world().get::<Node>(glyph).unwrap().display, Display::None);
    assert_eq!(app.world().get::<Node>(button).unwrap().width, px(64));
    assert!(app.world().get::<InteractionDisabled>(button).is_some());
    assert!(!app.world().get::<EditorButtonClick>(button).unwrap().is_clicked(), "动作语义改变须撤销旧按下代次");
    app.world_mut().entity_mut(button).remove::<InteractionDisabled>();
    present(app.world_mut());
    assert!(app.world().get::<InteractionDisabled>(button).is_some(), "原Close样式撤销禁用后仍须重新投影停止状态");
    app.world_mut().resource_mut::<CompilationReturn>().selection_revision = None;
    present(app.world_mut());
    assert_eq!(app.world().get::<Node>(glyph).unwrap().display, Display::Flex);
    assert_eq!(app.world().get::<Text>(label).unwrap().as_str(), "预览", "停止后保留工程3D，结束投影的同一帧应恢复预览文案与图标");
    assert!(app.world().get::<InteractionDisabled>(button).is_none());
    assert_eq!(*app.world().resource::<ProjectLaunchInterruption>(), ProjectLaunchInterruption::None);
}
