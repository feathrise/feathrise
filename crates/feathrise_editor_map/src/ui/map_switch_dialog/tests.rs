//! 通过正式模态输入与保存续接系统验证三个选择，不执行地图文件写入。

use bevy::ui::FocusPolicy;
use feathrise_map_content::MapId;
use std::sync::Arc;

use super::*;
use crate::components::{MapSwitchDialog, MapSwitchDialogText};

fn request_app() -> (App, MapId) {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.environment_dirty = true;
    let source = draft.map_id.clone();
    let target = MapId::parse("MAP-switch-target").unwrap();
    let mut app = App::new();
    app.insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(source.clone()),
        map_list_selection_id: Some(target.clone()),
        dirty: true,
        map_switch_confirmation: Some(MapSwitchConfirmation {
            source_map_id: Some(source),
            source_revision: 7,
            global_revision: 0,
            target_map_id: target.clone(),
            target_display_name: "目标地图".to_owned(),
            diagnostic: None,
            stage: MapSwitchStage::Choosing,
        }),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        revision: 7,
        ..default()
    })
    .init_resource::<GlobalEnvironmentAuthoringState>()
    .init_resource::<MapBrowserDragState>()
    .init_resource::<ButtonInput<KeyCode>>()
    .add_systems(
        Update,
        (
            handle_map_switch_input,
            resume_map_switch_after_save,
            sync_map_switch_dialog,
        )
            .chain(),
    );
    (app, target)
}

fn press(app: &mut App, action: EditorAction) -> Entity {
    app.world_mut()
        .spawn((MapEditorActionButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed))
        .id()
}

fn close_request_app() -> (App, MapId) {
    let (mut app, _) = request_app();
    let source = app
        .world()
        .resource::<MapEditorState>()
        .selected_map_id
        .clone()
        .unwrap();
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        let request = editor.map_switch_confirmation.as_mut().unwrap();
        request.target_map_id = source.clone();
        request.target_display_name = "待关闭地图".into();
    }
    let mut documents = crate::ui::map_documents::MapDocuments::default();
    documents.upsert(source.clone(), "待关闭地图".into(), true);
    documents.close_requested = Some(source.clone());
    documents.closing = Some(source.clone());
    app.insert_resource(documents);
    (app, source)
}

#[test]
fn cancelling_map_tab_close_keeps_the_document_and_clears_only_close_intent() {
    let (mut app, source) = close_request_app();
    press(&mut app, EditorAction::CancelMapSwitch);
    app.update();
    let docs = app
        .world()
        .resource::<crate::ui::map_documents::MapDocuments>();
    assert!(docs.contains(&source) && docs.close_requested.is_none() && docs.closing.is_none());
    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.dirty && editor.map_switch_confirmation.is_none());
    assert_eq!(editor.selected_map_id.as_ref(), Some(&source));
}

#[test]
fn discarding_map_tab_close_preserves_shared_global_environment() {
    let (mut app, source) = close_request_app();
    app.world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>()
        .dirty = true;
    press(&mut app, EditorAction::DiscardAndSwitchMap);
    app.update();
    let docs = app
        .world()
        .resource::<crate::ui::map_documents::MapDocuments>();
    assert!(docs.close_ready && docs.close_requested.as_ref() == Some(&source));
    assert!(
        app.world()
            .resource::<GlobalEnvironmentAuthoringState>()
            .dirty
    );
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .pending_map_transition
            .is_none()
    );
}

#[test]
fn map_tab_close_save_requires_a_success_receipt_and_does_not_queue_same_map_reload() {
    let (mut app, _) = close_request_app();
    press(&mut app, EditorAction::SaveAndSwitchMap);
    app.update();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .save_requested = false;
    app.world_mut().resource_mut::<MapEditorState>().dirty = false;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .successful_save_sequence += 1;
    app.update();
    assert!(
        app.world()
            .resource::<crate::ui::map_documents::MapDocuments>()
            .close_ready
    );
    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.map_switch_confirmation.is_none() && editor.pending_map_transition.is_none());
}

#[test]
fn failed_map_tab_close_save_keeps_the_unsaved_tab_and_allows_retry() {
    let (mut app, source) = close_request_app();
    press(&mut app, EditorAction::SaveAndSwitchMap);
    app.update();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .save_requested = false;
    app.world_mut().resource_mut::<MapEditorState>().status = "测试保存失败".into();
    app.update();
    let docs = app
        .world()
        .resource::<crate::ui::map_documents::MapDocuments>();
    assert!(docs.contains(&source) && !docs.close_ready && docs.closing.as_ref() == Some(&source));
    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.dirty && editor.status.contains("测试保存失败"));
    assert_eq!(
        editor.map_switch_confirmation.as_ref().unwrap().stage,
        MapSwitchStage::Choosing
    );
}

#[test]
fn save_waits_for_a_success_receipt_and_continues_the_frozen_target() {
    let (mut app, target) = request_app();
    // 后续目录高亮不允许把已经打开的弹框重定向到另一张地图。
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .map_list_selection_id = Some(MapId::parse("MAP-other").unwrap());
    press(&mut app, EditorAction::SaveAndSwitchMap);
    app.update();
    assert!(app.world().resource::<MapEditorState>().save_requested);
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .pending_map_transition
            .is_none()
    );
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .save_requested = false;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    app.update();
    assert!(matches!(
        app.world()
            .resource::<MapEditorState>()
            .map_switch_confirmation
            .as_ref()
            .unwrap()
            .stage,
        MapSwitchStage::Saving { .. }
    ));
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.saving = false;
        session.successful_save_sequence += 1;
    }
    app.world_mut().resource_mut::<MapEditorState>().dirty = false;
    app.update();
    let editor = app.world().resource::<MapEditorState>();
    assert_eq!(editor.pending_map_transition.as_ref(), Some(&target));
    assert!(editor.map_switch_confirmation.is_none());
}

#[test]
fn rejected_save_keeps_the_source_draft_and_exposes_the_failure() {
    let (mut app, _) = request_app();
    press(&mut app, EditorAction::SaveAndSwitchMap);
    app.update();
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.save_requested = false;
        editor.status = "保存未开始：地图预检失败".to_owned();
    }
    app.update();
    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.dirty && editor.pending_map_transition.is_none());
    assert_eq!(
        editor.map_switch_confirmation.as_ref().unwrap().stage,
        MapSwitchStage::Choosing
    );
    assert!(editor.status.contains("地图预检失败"));
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .environment_dirty
    );
    // 后台预览状态可以更新底栏，但不能抹去这次切图保存失败的原因。
    let text = app
        .world_mut()
        .spawn((MapSwitchDialogText, Text::default()))
        .id();
    app.world_mut().resource_mut::<MapEditorState>().status = "另一后台预览已完成".to_owned();
    app.update();
    assert!(
        app.world()
            .get::<Text>(text)
            .unwrap()
            .0
            .contains("地图预检失败")
    );
}

#[test]
fn clearing_dirty_without_a_save_receipt_cannot_continue_switching() {
    let (mut app, _) = request_app();
    press(&mut app, EditorAction::SaveAndSwitchMap);
    app.update();
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.save_requested = false;
        editor.dirty = false;
    }
    app.update();
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .pending_map_transition
            .is_none()
    );
    assert_eq!(
        app.world()
            .resource::<MapEditorState>()
            .map_switch_confirmation
            .as_ref()
            .unwrap()
            .stage,
        MapSwitchStage::Choosing
    );
}

#[test]
fn discard_button_only_registers_authorization_and_retains_all_data_until_adoption() {
    let (mut app, target) = request_app();
    press(&mut app, EditorAction::DiscardAndSwitchMap);
    app.update();
    let editor = app.world().resource::<MapEditorState>();
    assert_eq!(editor.pending_map_transition.as_ref(), Some(&target));
    assert!(editor.dirty && !editor.save_requested);
    assert_eq!(
        editor.map_switch_confirmation.as_ref().unwrap().stage,
        MapSwitchStage::DiscardApproved
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
    press(&mut app, EditorAction::DiscardAndSwitchMap);
    app.update();
    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.pending_map_transition.is_none() && editor.dirty);
    assert_eq!(
        editor.map_switch_confirmation.as_ref().unwrap().stage,
        MapSwitchStage::Choosing
    );
    assert_eq!(
        editor
            .map_switch_confirmation
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
        .resource_mut::<MapEditorState>()
        .map_switch_confirmation
        .as_mut()
        .unwrap()
        .stage = MapSwitchStage::Saving {
        success_sequence_before: 0,
    };
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    press(&mut app, EditorAction::CancelMapSwitch);
    app.update();
    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.map_switch_confirmation.is_none() && editor.pending_map_transition.is_none());
    assert!(editor.dirty);
    assert!(app.world().resource::<EditorAuthoringSession>().saving);
}

#[test]
fn cancel_releases_hidden_overlay_and_retains_the_click_and_keyboard_gate() {
    let (mut app, _) = request_app();
    let root = app
        .world_mut()
        .spawn((
            MapSwitchDialog,
            Node::default(),
            feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed,
            FocusPolicy::Block,
        ))
        .id();
    let cancel = press(&mut app, EditorAction::CancelMapSwitch);
    app.world_mut().entity_mut(root).add_child(cancel);
    app.world_mut()
        .spawn((MapSwitchDialogText, Text::default()));
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
        *app.world().get::<Interaction>(cancel).unwrap(),
        Interaction::None
    );
    assert!(app.world().resource::<MapBrowserDragState>().blocked);
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::KeyS)
    );
    assert!(app.world().resource::<MapEditorState>().dirty);
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
    use crate::resources::MapEditorNumericScrubMouseGuard;
    use crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard;
    let (mut app, _) = request_app();
    app.init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .add_systems(
            Update,
            update_map_editor_numeric_scrub_mouse_guard.after(handle_map_switch_input),
        );
    app.update();
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    press(&mut app, EditorAction::CancelMapSwitch);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .map_switch_confirmation
            .is_none()
    );
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<MapBrowserDragState>()
        .blocked = false;
    app.update();
    assert!(
        !app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

#[test]
fn escape_cancels_queued_discard_without_discarding_the_source() {
    let (mut app, target) = request_app();
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.map_switch_confirmation.as_mut().unwrap().stage = MapSwitchStage::DiscardApproved;
        editor.pending_map_transition = Some(target);
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    let editor = app.world().resource::<MapEditorState>();
    assert!(
        editor.pending_map_transition.is_none()
            && editor.map_switch_confirmation.is_none()
            && editor.dirty
    );
}

#[test]
fn success_for_a_new_revision_does_not_consume_an_older_switch_request() {
    let (mut app, _) = request_app();
    press(&mut app, EditorAction::SaveAndSwitchMap);
    app.update();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.revision += 1;
        session.successful_save_sequence += 1;
    }
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.save_requested = false;
        editor.dirty = false;
    }
    app.update();
    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.pending_map_transition.is_none() && editor.map_switch_confirmation.is_none());
    assert!(editor.status.contains("作者修订已变化"));
}
