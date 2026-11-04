//! 关于的输入租约回归：驱动生产输入系统，验证表单提交隔离、关闭释放帧及高优先级退出。

use super::{about, components::*, resources::{AboutState, Presentation}, *};
use bevy::{input_focus::InputFocus, prelude::*, window::Ime};
use feathrise_editor_map::workbench::WorkbenchHostPageState;
use feathrise_editor_ui::EditorButtonClick;

fn fixture() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<AboutState>()
        .insert_resource(Presentation { page: Page::New, name: "关于隔离夹具".into(), parent: "D:/不创建此工程".into(), ..default() })
        .init_resource::<WorkspaceModel>().init_resource::<WorkbenchHostPageState>()
        .init_resource::<InputFocus>().init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<Ime>().add_message::<WorkspaceCommand>()
        .add_systems(Update, (about::input, systems::input).chain());
    let about = app.world_mut().spawn((Action::ShowAbout, EditorButtonClick::triggered(), InheritedVisibility::VISIBLE)).id();
    let create = app.world_mut().spawn((Action::Create, EditorButtonClick::default(), InheritedVisibility::VISIBLE)).id();
    app.update();
    app.world_mut().entity_mut(about).insert(EditorButtonClick::default());
    assert!(app.world().resource::<AboutState>().open);
    (app, about, create)
}

#[test]
fn about_blocks_create_and_shortcuts_until_the_close_release_frame_finishes() {
    let (mut app, _, create) = fixture();
    for key in [KeyCode::Enter, KeyCode::F12] { app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key); }
    app.world_mut().entity_mut(create).insert(EditorButtonClick::triggered());
    app.update();
    assert!(app.world().resource::<Messages<WorkspaceCommand>>().is_empty());
    assert!(app.world().resource::<WorkbenchHostPageState>().menu_open);
    assert!(app.world().resource::<AboutState>().open);
    for key in [KeyCode::Enter, KeyCode::F12] { assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(key)); }

    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<AboutState>().open);
    assert!(app.world().resource::<AboutState>().captures_input());
    assert!(app.world().resource::<Messages<WorkspaceCommand>>().is_empty());
    assert_eq!(app.world().resource::<Presentation>().name, "关于隔离夹具");
    app.world_mut().entity_mut(create).insert(EditorButtonClick::default());
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    app.update();
    assert!(!app.world().resource::<AboutState>().captures_input());
    assert!(!app.world().resource::<WorkbenchHostPageState>().menu_open);
    app.world_mut().entity_mut(create).insert(EditorButtonClick::triggered());
    app.update();
    assert!(!app.world().resource::<Messages<WorkspaceCommand>>().is_empty(), "释放后新的明确操作应恢复");
}

#[test]
fn project_exit_confirmation_dismisses_about_without_losing_the_form() {
    let (mut app, _, _) = fixture();
    let mut close = crate::native_project_close::NativeProjectCloseState::default();
    close.hold("关于退出优先级夹具".into());
    app.insert_resource(close);
    app.update();
    assert!(!app.world().resource::<AboutState>().open);
    assert_eq!(app.world().resource::<Presentation>().page, Page::New);
    assert_eq!(app.world().resource::<Presentation>().name, "关于隔离夹具");
    assert!(app.world().resource::<Messages<WorkspaceCommand>>().is_empty());
}

#[test]
fn export_capture_drops_about_click_until_a_new_explicit_action() {
    let mut app = App::new();
    app.init_resource::<AboutState>().init_resource::<Presentation>()
        .init_resource::<InputFocus>().init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_systems(Update, about::input);
    let mut export = crate::project_export::ui::ExportOverlayState::default();
    export.captured = true; // 导出打开及其释放帧共用同一租约，不允许旧关于点击穿透。
    app.insert_resource(export);
    let button = app.world_mut().spawn((Action::ShowAbout, EditorButtonClick::triggered(), InheritedVisibility::VISIBLE)).id();
    app.update();
    assert!(!app.world().resource::<AboutState>().open);
    app.world_mut().resource_mut::<crate::project_export::ui::ExportOverlayState>().captured = false;
    app.update();
    assert!(!app.world().resource::<AboutState>().open, "被拒绝的旧按钮边沿不能在导出释放后重放");
    app.world_mut().entity_mut(button).insert(EditorButtonClick::triggered());
    app.update();
    assert!(app.world().resource::<AboutState>().open, "释放后的新明确点击仍能打开关于");
}
