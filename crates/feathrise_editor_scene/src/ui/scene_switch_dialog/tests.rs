//! 通过正式模态输入与保存续接系统验证三个选择，不执行场景文件写入。

use feathrise_editor_ui::UiInteraction;
use bevy::ui::FocusPolicy;
use feathrise_scene_content::SceneId;
use std::sync::Arc;

use super::*;
use crate::components::{SceneSwitchDialog, SceneSwitchDialogText};

fn request_app() -> (App, SceneId) {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.environment_dirty = true;
    let source = draft.scene_id.clone();
    let target = SceneId::parse("MAP-switch-target").unwrap();
    let mut app = App::new();
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(source.clone()),
        scene_list_selection_id: Some(target.clone()),
        dirty: true,
        scene_switch_confirmation: Some(SceneSwitchConfirmation {
            source_scene_id: Some(source),
            source_revision: 7,
            global_revision: 0,
            target_scene_id: target.clone(),
            target_display_name: "目标场景".to_owned(),
            diagnostic: None,
            stage: SceneSwitchStage::Choosing,
        }),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        revision: 7,
        ..default()
    })
    .init_resource::<GlobalEnvironmentAuthoringState>()
    .init_resource::<SceneBrowserDragState>()
    .init_resource::<ButtonInput<KeyCode>>()
    .add_systems(
        Update,
        (
            handle_scene_switch_input,
            resume_scene_switch_after_save,
            sync_scene_switch_dialog,
        )
            .chain(),
    );
    (app, target)
}

fn press(app: &mut App, action: EditorAction) -> Entity {
    app.world_mut()
        .spawn((SceneEditorActionButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
        .id()
}

fn close_request_app() -> (App, SceneId) {
    let (mut app, _) = request_app();
    let source = app
        .world()
        .resource::<SceneEditorState>()
        .selected_scene_id
        .clone()
        .unwrap();
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        let request = editor.scene_switch_confirmation.as_mut().unwrap();
        request.target_scene_id = source.clone();
        request.target_display_name = "待关闭场景".into();
    }
    let mut documents = crate::ui::scene_documents::SceneDocuments::default();
    documents.upsert(source.clone(), "待关闭场景".into(), true);
    documents.close_requested = Some(source.clone());
    documents.closing = Some(source.clone());
    app.insert_resource(documents);
    (app, source)
}

#[test]
fn cancelling_scene_tab_close_keeps_the_document_and_clears_only_close_intent() {
    let (mut app, source) = close_request_app();
    press(&mut app, EditorAction::CancelSceneSwitch);
    app.update();
    let docs = app
        .world()
        .resource::<crate::ui::scene_documents::SceneDocuments>();
    assert!(docs.contains(&source) && docs.close_requested.is_none() && docs.closing.is_none());
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.dirty && editor.scene_switch_confirmation.is_none());
    assert_eq!(editor.selected_scene_id.as_ref(), Some(&source));
}

#[test]
fn discarding_scene_tab_close_preserves_shared_global_environment() {
    let (mut app, source) = close_request_app();
    app.world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>()
        .dirty = true;
    press(&mut app, EditorAction::DiscardAndSwitchScene);
    app.update();
    let docs = app
        .world()
        .resource::<crate::ui::scene_documents::SceneDocuments>();
    assert!(docs.close_ready && docs.close_requested.as_ref() == Some(&source));
    assert!(
        app.world()
            .resource::<GlobalEnvironmentAuthoringState>()
            .dirty
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
}

#[test]
fn scene_tab_close_save_requires_a_success_receipt_and_does_not_queue_same_scene_reload() {
    let (mut app, _) = close_request_app();
    press(&mut app, EditorAction::SaveAndSwitchScene);
    app.update();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .save_requested = false;
    app.world_mut().resource_mut::<SceneEditorState>().dirty = false;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .successful_save_sequence += 1;
    app.update();
    assert!(
        app.world()
            .resource::<crate::ui::scene_documents::SceneDocuments>()
            .close_ready
    );
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.scene_switch_confirmation.is_none() && editor.pending_scene_transition.is_none());
}

#[test]
fn failed_scene_tab_close_save_keeps_the_unsaved_tab_and_allows_retry() {
    let (mut app, source) = close_request_app();
    press(&mut app, EditorAction::SaveAndSwitchScene);
    app.update();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .save_requested = false;
    app.world_mut().resource_mut::<SceneEditorState>().status = "测试保存失败".into();
    app.update();
    let docs = app
        .world()
        .resource::<crate::ui::scene_documents::SceneDocuments>();
    assert!(docs.contains(&source) && !docs.close_ready && docs.closing.as_ref() == Some(&source));
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.dirty && editor.status.contains("测试保存失败"));
    assert_eq!(
        editor.scene_switch_confirmation.as_ref().unwrap().stage,
        SceneSwitchStage::Choosing
    );
}

#[test]
fn save_waits_for_a_success_receipt_and_continues_the_frozen_target() {
    let (mut app, target) = request_app();
    // 后续目录高亮不允许把已经打开的弹框重定向到另一个场景。
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .scene_list_selection_id = Some(SceneId::parse("MAP-other").unwrap());
    press(&mut app, EditorAction::SaveAndSwitchScene);
    app.update();
    assert!(app.world().resource::<SceneEditorState>().save_requested);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .save_requested = false;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    app.update();
    assert!(matches!(
        app.world()
            .resource::<SceneEditorState>()
            .scene_switch_confirmation
            .as_ref()
            .unwrap()
            .stage,
        SceneSwitchStage::Saving { .. }
    ));
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.saving = false;
        session.successful_save_sequence += 1;
    }
    app.world_mut().resource_mut::<SceneEditorState>().dirty = false;
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert_eq!(editor.pending_scene_transition.as_ref(), Some(&target));
    assert!(editor.scene_switch_confirmation.is_none());
}

#[test]
fn rejected_save_keeps_the_source_draft_and_exposes_the_failure() {
    let (mut app, _) = request_app();
    press(&mut app, EditorAction::SaveAndSwitchScene);
    app.update();
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.save_requested = false;
        editor.status = "保存未开始：场景预检失败".to_owned();
    }
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.dirty && editor.pending_scene_transition.is_none());
    assert_eq!(
        editor.scene_switch_confirmation.as_ref().unwrap().stage,
        SceneSwitchStage::Choosing
    );
    assert!(editor.status.contains("场景预检失败"));
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .environment_dirty
    );
    // 后台预览状态可以更新底栏，但不能抹去这次切换场景保存失败的原因。
    let text = app
        .world_mut()
        .spawn((SceneSwitchDialogText, Text::default()))
        .id();
    app.world_mut().resource_mut::<SceneEditorState>().status = "另一后台预览已完成".to_owned();
    app.update();
    assert!(
        app.world()
            .get::<Text>(text)
            .unwrap()
            .0
            .contains("场景预检失败")
    );
}

#[test]
fn clearing_dirty_without_a_save_receipt_cannot_continue_switching() {
    let (mut app, _) = request_app();
    press(&mut app, EditorAction::SaveAndSwitchScene);
    app.update();
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.save_requested = false;
        editor.dirty = false;
    }
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition
            .is_none()
    );
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .scene_switch_confirmation
            .as_ref()
            .unwrap()
            .stage,
        SceneSwitchStage::Choosing
    );
}

#[test]
fn discard_button_only_registers_authorization_and_retains_all_data_until_adoption() {
    let (mut app, target) = request_app();
    press(&mut app, EditorAction::DiscardAndSwitchScene);
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert_eq!(editor.pending_scene_transition.as_ref(), Some(&target));
    assert!(editor.dirty && !editor.save_requested);
    assert_eq!(
        editor.scene_switch_confirmation.as_ref().unwrap().stage,
        SceneSwitchStage::DiscardApproved
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .environment_dirty
    );
}

#[test]
fn revision_change_requires_another_explicit_choice() {
    let (mut app, _) = request_app();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .revision += 1;
    press(&mut app, EditorAction::DiscardAndSwitchScene);
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.pending_scene_transition.is_none() && editor.dirty);
    assert_eq!(
        editor.scene_switch_confirmation.as_ref().unwrap().stage,
        SceneSwitchStage::Choosing
    );
    assert_eq!(
        editor
            .scene_switch_confirmation
            .as_ref()
            .unwrap()
            .source_revision,
        8
    );
    assert!(editor.status.contains("重新选择"));
}

#[test]
fn cancel_during_save_does_not_interrupt_the_running_file_transaction() {
    let (mut app, _) = request_app();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .scene_switch_confirmation
        .as_mut()
        .unwrap()
        .stage = SceneSwitchStage::Saving {
        success_sequence_before: 0,
    };
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    press(&mut app, EditorAction::CancelSceneSwitch);
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.scene_switch_confirmation.is_none() && editor.pending_scene_transition.is_none());
    assert!(editor.dirty);
    assert!(app.world().resource::<EditorAuthoringSession>().saving);
}

#[test]
fn cancel_releases_hidden_overlay_and_retains_the_click_and_keyboard_gate() {
    let (mut app, _) = request_app();
    let root = app
        .world_mut()
        .spawn((
            SceneSwitchDialog,
            Node::default(),
            feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,
            FocusPolicy::Block,
        ))
        .id();
    let cancel = press(&mut app, EditorAction::CancelSceneSwitch);
    app.world_mut().entity_mut(root).add_child(cancel);
    app.world_mut()
        .spawn((SceneSwitchDialogText, Text::default()));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyS);
    app.update();
    assert_eq!(
        app.world().get::<Node>(root).unwrap().display,
        Display::None
    );
    assert_eq!(
        *app.world().get::<FocusPolicy>(root).unwrap(),
        FocusPolicy::Pass
    );
    assert_eq!(
        *app.world().get::<UiInteraction>(cancel).unwrap(),
        UiInteraction::None
    );
    assert!(app.world().resource::<SceneBrowserDragState>().blocked);
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::KeyS)
    );
    assert!(app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn confirmation_releases_underlying_official_text_focus_even_on_stable_frames() {
    use bevy::input_focus::{FocusCause, InputFocus};
    let (mut app, _) = request_app();
    app.init_resource::<InputFocus>();
    let field = app.world_mut().spawn_empty().id();
    for _ in 0..2 {
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Navigated);
        app.update();
        assert!(app.world().resource::<InputFocus>().get().is_none());
    }
}

#[test]
fn confirmation_and_cancel_release_block_the_shared_workspace_mouse_guard() {
    use crate::resources::SceneEditorNumericScrubMouseGuard;
    use crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard;
    let (mut app, _) = request_app();
    app.init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .add_systems(
            Update,
            update_scene_editor_numeric_scrub_mouse_guard.after(handle_scene_switch_input),
        );
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
    press(&mut app, EditorAction::CancelSceneSwitch);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .scene_switch_confirmation
            .is_none()
    );
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<SceneBrowserDragState>()
        .blocked = false;
    app.update();
    assert!(
        !app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

#[test]
fn escape_cancels_queued_discard_without_discarding_the_source() {
    let (mut app, target) = request_app();
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.scene_switch_confirmation.as_mut().unwrap().stage = SceneSwitchStage::DiscardApproved;
        editor.pending_scene_transition = Some(target);
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(
        editor.pending_scene_transition.is_none()
            && editor.scene_switch_confirmation.is_none()
            && editor.dirty
    );
}

#[test]
fn success_for_a_new_revision_does_not_consume_an_older_switch_request() {
    let (mut app, _) = request_app();
    press(&mut app, EditorAction::SaveAndSwitchScene);
    app.update();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.revision += 1;
        session.successful_save_sequence += 1;
    }
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.save_requested = false;
        editor.dirty = false;
    }
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.pending_scene_transition.is_none() && editor.scene_switch_confirmation.is_none());
    assert!(editor.status.contains("作者修订已变化"));
}
