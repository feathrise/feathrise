//! 导出浮窗的输入释放与有界状态投影回归，不以普通编译代替实窗验收。

use super::{resources::ExportOverlayState, systems::visible_status};
use super::super::{components::*, messages::{CancelExport, RequestExport}, resources::ExportState};
use bevy::{ecs::message::MessageCursor, input_focus::InputFocus, prelude::*, text::EditableText};

fn input_app(working: bool) -> App {
    let mut app = App::new();
    app.insert_resource(ExportState { open: true, destination: "D:/中文工程/output".into(),
        status: "准备导出".into(), failed: None, receipt: None, revision: 1, working })
        .init_resource::<InputFocus>().init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<feathrise_editor_core::EditorTextInputGuard>()
        .init_resource::<feathrise_editor_core::EditorHostOverlay>()
        .add_message::<RequestExport>().add_message::<CancelExport>()
        .add_systems(Update, super::systems::handle);
    super::install(&mut app);
    app.world_mut().spawn((ExportDestination, EditableText::new("尚未同步的原值")));
    app
}

#[test]
fn export_button_uses_current_native_input_and_keeps_author_shortcuts_blocked() {
    let mut app = input_app(false);
    app.update();
    let field = app.world_mut().query_filtered::<Entity, With<ExportDestination>>().single(app.world()).unwrap();
    assert_eq!(app.world().get::<EditableText>(field).unwrap().value(), "D:/中文工程/output");
    app.world_mut().get_mut::<EditableText>(field).unwrap().editor.set_text("D:/中文工程/发布 包");
    app.world_mut().spawn((ExportAction::Start, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F12);
    app.update();
    let mut cursor = MessageCursor::<RequestExport>::default();
    let requests: Vec<_> = cursor.read(app.world().resource::<Messages<RequestExport>>()).collect();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].destination, std::path::PathBuf::from("D:/中文工程/发布 包"));
    assert!(app.world().resource::<feathrise_editor_core::EditorTextInputGuard>().block_shortcuts);
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::F12));
    assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
}

#[test]
fn cancel_waits_for_the_real_background_busy_gate_before_closing() {
    let mut app = input_app(true);
    let cancel = app.world_mut().spawn((ExportAction::Cancel, feathrise_editor_ui::EditorButtonClick::triggered())).id();
    app.update();
    let mut cursor = MessageCursor::<CancelExport>::default();
    assert_eq!(cursor.read(app.world().resource::<Messages<CancelExport>>()).count(), 1);
    assert!(app.world().resource::<ExportState>().open);
    assert!(app.world().resource::<ExportOverlayState>().close_when_idle);
    app.world_mut().get_mut::<feathrise_editor_ui::EditorButtonClick>(cancel).unwrap().invalidate();
    app.update();
    assert!(app.world().resource::<ExportState>().open);
    // 只模拟后台槽的真实完成回执；不把这项单测当成Cargo取消或实窗验收。
    app.world_mut().resource_mut::<ExportState>().working = false;
    app.update();
    assert!(!app.world().resource::<ExportState>().open);
    assert!(super::captures_input(app.world()));
    app.update();
    assert!(!super::captures_input(app.world()));
}

#[test]
fn busy_export_does_not_accept_a_second_start_or_an_edited_destination() {
    let mut app = input_app(true);
    app.world_mut().spawn((ExportAction::Start, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    let field = app.world_mut().query_filtered::<Entity, With<ExportDestination>>().single(app.world()).unwrap();
    app.world_mut().get_mut::<EditableText>(field).unwrap().editor.set_text("D:/另一个输出");
    app.update();
    let mut cursor = MessageCursor::<RequestExport>::default();
    assert_eq!(cursor.read(app.world().resource::<Messages<RequestExport>>()).count(), 0);
    assert_eq!(app.world().resource::<ExportState>().destination, "D:/中文工程/output");
}

#[test]
fn export_overlay_keeps_one_release_frame_and_picker_ownership() {
    let mut state = ExportOverlayState::default();
    state.capture(true, false);
    assert!(state.captured);
    state.capture(false, false);
    assert!(state.captured, "关闭帧不能把取消输入穿透给场景");
    state.capture(false, false);
    assert!(!state.captured);
    state.capture(false, true);
    assert!(state.captured, "系统目录任务仍在途时保留所有权");
    state.capture(false, false);
    assert!(!state.captured);
}

#[test]
fn export_error_projection_preserves_utf8_and_full_short_error() {
    let error = "输出目录不可写：D:/中文工程/output";
    assert_eq!(visible_status(error), error);
    let long = "中文路径/".repeat(600);
    let visible = visible_status(&long);
    assert!(visible.len() < 2150);
    assert!(visible.ends_with("…（完整输出见日志）"));
    assert!(long.starts_with(visible.split('…').next().unwrap()));
}
