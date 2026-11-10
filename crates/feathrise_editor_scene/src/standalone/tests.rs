//! 独立试玩边界回归：原关闭行为不被默认改变、未保存草稿不写盘、显式保存失败不误启动。

use super::{resources::StandalonePlayRequest, systems::publish_saved_play, *};
use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState};
use bevy::prelude::*;
use bevy::ecs::system::RunSystemOnce;

fn press_play(mut request: StandalonePlayRequest, mut editor: ResMut<SceneEditorState>) {
    if !request.request(&mut editor) {
        editor.request_close();
    }
}

#[test]
fn integrated_mode_keeps_original_close_request() {
    let mut app = App::new();
    app.insert_resource(SceneEditorState {
        open: true,
        ..default()
    })
    .add_systems(Update, press_play);
    app.update();
    assert!(app.world().resource::<SceneEditorState>().close_requested);
}

#[test]
fn project_preview_disables_f12_but_keeps_the_real_launch_button_transaction() {
    fn shortcut(mut request: StandalonePlayRequest, mut editor: ResMut<SceneEditorState>) {
        if request.legacy_shortcut_allowed() { request.request(&mut editor); }
    }
    for preview in [false, true] {
        let mut app = App::new();
        app.insert_resource(SceneEditorState { open: true, dirty: true, ..default() })
            .init_resource::<StandaloneEditorMode>()
            .insert_resource(feathrise_editor_core::ProjectLaunchSettings { preview, ..default() })
            .add_systems(Update, shortcut);
        app.update();
        assert_eq!(app.world().resource::<StandaloneEditorMode>().pending, !preview);
        if preview {
            // 按钮继续调用同一请求方法；禁止的只有旧快捷键入口。
            app.world_mut().run_system_once(press_play).unwrap();
            assert!(app.world().resource::<StandaloneEditorMode>().pending);
        }
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.open && editor.dirty && !editor.save_requested);
    }
}

#[test]
fn independent_request_keeps_unsaved_author_state_and_does_not_save() {
    let mut app = App::new();
    app.insert_resource(SceneEditorState {
        open: true,
        dirty: true,
        ..default()
    })
    .init_resource::<EditorAuthoringSession>()
    .init_resource::<StandaloneEditorMode>()
    .add_message::<RunSavedProject>()
    .add_systems(Update, (press_play, publish_saved_play).chain());
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open && editor.dirty);
    assert!(!editor.save_requested && !editor.close_requested);
    let mut cursor = bevy::ecs::message::MessageCursor::<RunSavedProject>::default();
    let messages: Vec<_> = cursor
        .read(app.world().resource::<Messages<RunSavedProject>>())
        .collect();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].has_unsaved_changes);
    assert!(!messages[0].stop_only);
}

#[test]
fn failed_explicit_save_cancels_pending_play_and_preserves_editor() {
    let mut app = App::new();
    app.insert_resource(SceneEditorState {
        open: true,
        dirty: true,
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        saving: true,
        ..default()
    })
    .insert_resource(StandaloneEditorMode {
        pending: true,
        ..default()
    })
    .add_message::<RunSavedProject>()
    .add_systems(Update, publish_saved_play);
    app.update();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = false;
    app.update();
    assert!(!app.world().resource::<StandaloneEditorMode>().pending);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("保存尚未成功")
    );
    assert!(app.world().resource::<SceneEditorState>().open);
    assert!(
        app.world()
            .resource::<Messages<RunSavedProject>>()
            .is_empty()
    );
}

#[test]
fn successful_explicit_save_releases_one_waiting_play() {
    let mut app = App::new();
    app.insert_resource(SceneEditorState {
        open: true,
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        saving: true,
        successful_save_sequence: 10,
        ..default()
    })
    .insert_resource(StandaloneEditorMode {
        pending: true,
        ..default()
    })
    .add_message::<RunSavedProject>()
    .add_systems(Update, publish_saved_play);
    app.update();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.saving = false;
        session.successful_save_sequence = 11;
    }
    app.update();
    assert_eq!(app.world().resource::<Messages<RunSavedProject>>().len(), 1);
    app.update();
    assert!(!app.world().resource::<StandaloneEditorMode>().pending);
}

#[test]
fn running_play_receives_second_f12_cancel_even_while_author_save_is_pending() {
    let mut app = App::new();
    app.insert_resource(SceneEditorState {
        open: true,
        dirty: true,
        save_requested: true,
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        saving: true,
        ..default()
    })
    .insert_resource(StandaloneEditorMode {
        pending: true,
        external_play_active: true,
        ..default()
    })
    .add_message::<RunSavedProject>()
    .add_systems(Update, publish_saved_play);
    app.update();
    assert_eq!(app.world().resource::<Messages<RunSavedProject>>().len(), 1);
    let mut cursor = bevy::ecs::message::MessageCursor::<RunSavedProject>::default();
    let request = cursor
        .read(app.world().resource::<Messages<RunSavedProject>>())
        .next()
        .unwrap();
    assert!(
        request.stop_only,
        "运行中的F12必须保持停止意图，不依赖宿主稍后仍有工作槽"
    );
    assert!(!app.world().resource::<StandaloneEditorMode>().pending);
    assert!(app.world().resource::<EditorAuthoringSession>().saving);
    assert!(app.world().resource::<SceneEditorState>().open);
}
#[test]
fn play_request_freezes_the_selected_build_profile_without_saving_author_drafts() {
    use feathrise_editor_core::{ProjectBuildProfile, ProjectLaunchSettings};
    let mut app = App::new();
    app.insert_resource(SceneEditorState { open: true, dirty: true, ..default() })
        .init_resource::<EditorAuthoringSession>()
        .insert_resource(StandaloneEditorMode { pending: true, ..default() })
        .insert_resource(ProjectLaunchSettings { profile: ProjectBuildProfile::Release, ..default() })
        .add_message::<RunSavedProject>()
        .add_systems(Update, publish_saved_play);
    app.update();
    app.world_mut().resource_mut::<ProjectLaunchSettings>().profile = ProjectBuildProfile::Debug;
    let mut cursor = bevy::ecs::message::MessageCursor::<RunSavedProject>::default();
    let request = cursor.read(app.world().resource::<Messages<RunSavedProject>>()).next().unwrap();
    assert_eq!(request.profile, ProjectBuildProfile::Release);
    assert!(request.has_unsaved_changes);
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open && editor.dirty && !editor.save_requested);
}
