//! 工程退出选择回归：不伪造脏位清零，取消和失败都回到原作者World。

use super::{cancel_workspace_close, components::CloseAction, resources::*, systems::authorize_exit};
use bevy::prelude::*;
use feathrise_editor_map::standalone::SavedPlayFeedback;
use feathrise_editor_surface_model::SurfacePresetEditorState;

/// 原工程关闭回归显式指定目标，避免默认的编辑器退出目标掩盖两条宿主请求的区别。
fn project_close_state() -> NativeProjectCloseState {
    let mut state = NativeProjectCloseState::default();
    state.set_target(CloseTarget::Project);
    state
}

fn dirty_world() -> World {
    let mut world = World::new();
    world.insert_resource(project_close_state());
    world.init_resource::<Messages<SavedPlayFeedback>>();
    world.insert_resource(crate::native_authoring_validation::dirty_surface_fixture());
    world
}

/// 保留旧退出链夹具入口；按当前真实状态选择可见动作，不能再让干净工程借“放弃”绕过确认。
pub(crate) fn choose_discard(world: &mut World) {
    let state = world.resource::<NativeProjectCloseState>();
    assert_eq!(state.stage, CloseStage::Confirm);
    let action = if state.needs_save_decision() { CloseAction::Discard } else { CloseAction::Close };
    world.resource_mut::<NativeProjectCloseState>().choice = Some(action);
}

/// 跨领域生命周期回归仍由原协调器处理取消，只投递与原确认按钮相同的选择。
pub(crate) fn choose_cancel(world: &mut World) {
    world.resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Cancel);
}

#[test]
fn clean_close_waits_for_explicit_choice_and_cancel_requires_a_fresh_confirmation() {
    let mut world = World::new();
    world.insert_resource(project_close_state());
    world.init_resource::<Messages<SavedPlayFeedback>>();
    let marker = world.spawn_empty().id();
    assert!(!authorize_exit(&mut world, true));
    for _ in 0..120 {
        assert!(!authorize_exit(&mut world, false));
        let state = world.resource::<NativeProjectCloseState>();
        assert_eq!(state.stage, CloseStage::Confirm);
        assert!(state.documents.is_empty());
        assert!(!state.needs_save_decision());
        assert_eq!(state.status, "确定关闭当前工程？");
        assert_eq!(state.intent, None);
        assert!(!state.status.contains("正在结束"));
    }
    world.resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(&mut world, false));
    assert!(!world.resource::<NativeProjectCloseState>().active());
    assert!(world.get_entity(marker).is_ok());
    assert!(!authorize_exit(&mut world, true));
    world.resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Close);
    assert!(authorize_exit(&mut world, false));
}

#[test]
fn explicit_clean_close_never_starts_a_save_or_writes_an_author_document() {
    use feathrise_editor_map::standalone::{ProjectSaveProgress, project_save_progress};
    let mut app = adoption_app(false);
    // 只准备领域初始基线，不给关闭协调器填成功状态；真实保存队列保持Idle。
    app.world_mut().resource_mut::<AdoptionOwner>().dirty = false;
    assert!(!authorize_exit(app.world_mut(), true));
    assert_eq!(project_save_progress(app.world()), ProjectSaveProgress::Idle);
    app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Close);
    assert!(authorize_exit(app.world_mut(), false));
    for _ in 0..4 { app.update(); }
    assert!(authorize_exit(app.world_mut(), false));
    let close = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(close.intent, Some(CloseAction::Close));
    assert_eq!(close.stage, CloseStage::Draining);
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 0);
    assert_eq!(project_save_progress(app.world()), ProjectSaveProgress::Idle,
        "直接关闭不能悄悄转成保存全部或借空保存队列取得成功回执");
}

#[test]
fn editor_exit_uses_its_own_clean_and_dirty_confirmation_and_cancel_receipt() {
    assert_eq!(NativeProjectCloseState::default().target(), CloseTarget::Editor);
    for dirty in [false, true] {
        let mut app = adoption_app(false);
        app.world_mut().resource_mut::<AdoptionOwner>().dirty = dirty;
        app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(CloseTarget::Editor);
        assert!(!authorize_exit(app.world_mut(), true));
        let state = app.world().resource::<NativeProjectCloseState>();
        assert_eq!(state.target(), CloseTarget::Editor);
        assert_eq!(state.stage, CloseStage::Confirm);
        assert_eq!(state.status, if dirty { "有 1 项未保存修改，是否保存后退出？" } else { "确定退出编辑器？" });
        assert_eq!(state.needs_save_decision(), dirty);
        assert_eq!(state.action_visible(CloseAction::Close), !dirty);
        assert_eq!(state.action_visible(CloseAction::Save), dirty);
        assert_eq!(state.action_visible(CloseAction::Discard), dirty);
        assert!(state.action_enabled(CloseAction::Cancel));
        app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Cancel);
        assert!(!authorize_exit(app.world_mut(), false));
        assert!(!app.world().resource::<NativeProjectCloseState>().active());
        assert_eq!(app.world().resource::<NativeProjectCloseState>().target(), CloseTarget::Editor);
        let receipt = app.world_mut().resource_mut::<Messages<SavedPlayFeedback>>().drain().last().unwrap();
        assert_eq!(receipt.0, "已取消退出编辑器，作者草稿和历史保留");
        assert_eq!(app.world().resource::<AdoptionOwner>().dirty, dirty);
        assert_eq!(app.world().resource::<AdoptionOwner>().writes, 0);
    }
}

#[test]
fn workspace_cancel_during_draining_uses_original_cancel_and_keeps_author_documents() {
    use feathrise_editor_map::standalone::{ProjectSaveProgress, project_save_progress};
    for target in [CloseTarget::Project, CloseTarget::Editor] {
        for dirty in [false, true] {
            let mut app = adoption_app(false);
            app.add_message::<AppExit>();
            app.world_mut().resource_mut::<AdoptionOwner>().dirty = dirty;
            app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(target);
            let marker = app.world_mut().spawn_empty().id();
            assert!(!authorize_exit(app.world_mut(), true));
            choose_discard(app.world_mut());
            assert!(authorize_exit(app.world_mut(), false));
            assert_eq!(app.world().resource::<NativeProjectCloseState>().stage, CloseStage::Draining);

            // 父宿主只递交取消意图，原协调器负责撤权；不能在辅助入口伪造空闲或保存成功。
            cancel_workspace_close(app.world_mut());
            let close = app.world().resource::<NativeProjectCloseState>();
            assert_eq!(close.stage, CloseStage::Draining);
            assert_eq!(close.choice, Some(CloseAction::Cancel));
            assert!(!authorize_exit(app.world_mut(), false));
            let close = app.world().resource::<NativeProjectCloseState>();
            assert_eq!(close.stage, CloseStage::Idle);
            assert_eq!(close.target(), target);
            assert_eq!(close.intent, None);
            assert_eq!(close.choice, None);
            assert_eq!(close.host_blocked, None);
            assert!(!close.startup_save_pending);
            assert_eq!(app.world().resource::<AdoptionOwner>().dirty, dirty);
            assert_eq!(app.world().resource::<AdoptionOwner>().writes, 0);
            assert_eq!(project_save_progress(app.world()), ProjectSaveProgress::Idle);
            assert!(app.world().get_entity(marker).is_ok());
            assert!(app.world().resource::<Messages<AppExit>>().is_empty());
            let feedback = app.world_mut().resource_mut::<Messages<SavedPlayFeedback>>().drain().last().unwrap();
            assert_eq!(feedback.0, format!("已取消{}，作者草稿和历史保留", target.operation()));

            assert!(!authorize_exit(app.world_mut(), true));
            assert_eq!(app.world().resource::<NativeProjectCloseState>().stage, CloseStage::Confirm);
            assert!(!authorize_exit(app.world_mut(), false), "再次关闭必须取得新的明确选择");
        }
    }
}

#[test]
fn workspace_cancel_cannot_retract_an_exit_already_queued() {
    for target in [CloseTarget::Project, CloseTarget::Editor] {
        let mut app = adoption_app(false);
        app.add_message::<AppExit>();
        app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(target);
        assert!(!authorize_exit(app.world_mut(), true));
        choose_discard(app.world_mut());
        assert!(authorize_exit(app.world_mut(), false));
        app.world_mut().write_message(AppExit::Success);

        cancel_workspace_close(app.world_mut());
        let close = app.world().resource::<NativeProjectCloseState>();
        assert_eq!(close.stage, CloseStage::Draining);
        assert_eq!(close.intent, Some(CloseAction::Discard));
        assert_eq!(close.choice, None);
        assert_eq!(app.world().resource::<Messages<AppExit>>().len(), 1);
        assert!(app.world().resource::<Messages<SavedPlayFeedback>>().is_empty(),
            "退出已经提交后不能再发取消成功回执");
        assert!(app.world().resource::<AdoptionOwner>().dirty);
        assert_eq!(app.world().resource::<AdoptionOwner>().writes, 0);
    }
}

#[test]
fn project_confirmation_and_draining_authorization_cannot_be_promoted_to_editor_exit() {
    for dirty in [false, true] {
        for already_authorized in [false, true] {
            let mut app = adoption_app(false);
            app.world_mut().resource_mut::<AdoptionOwner>().dirty = dirty;
            assert!(!authorize_exit(app.world_mut(), true));
            assert_eq!(app.world().resource::<NativeProjectCloseState>().target(), CloseTarget::Project);
            let action = if dirty { CloseAction::Discard } else { CloseAction::Close };
            app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(action);
            if already_authorized {
                assert!(authorize_exit(app.world_mut(), false));
                assert_eq!(app.world().resource::<NativeProjectCloseState>().stage, CloseStage::Draining);
            }
            // 同时覆盖尚未采用的旧按钮脉冲和已经授予的Project许可，不能把其提升为退出编辑器。
            app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(CloseTarget::Editor);
            let state = app.world().resource::<NativeProjectCloseState>();
            assert_eq!(state.stage, CloseStage::Inspecting);
            assert_eq!(state.choice, None);
            assert_eq!(state.intent, None);
            assert!(state.focus_cancel);
            assert!(!authorize_exit(app.world_mut(), false));
            let state = app.world().resource::<NativeProjectCloseState>();
            assert_eq!(state.stage, CloseStage::Confirm);
            assert_eq!(state.status, if dirty { "有 1 项未保存修改，是否保存后退出？" } else { "确定退出编辑器？" });
            assert_eq!(state.intent, None);
            assert_eq!(app.world().resource::<AdoptionOwner>().dirty, dirty);
            assert_eq!(app.world().resource::<AdoptionOwner>().writes, 0);

            app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Cancel);
            assert!(!authorize_exit(app.world_mut(), false));
            assert!(!app.world().resource::<NativeProjectCloseState>().active());
            assert!(!authorize_exit(app.world_mut(), true));
            app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(action);
            assert!(authorize_exit(app.world_mut(), false), "新目标必须重新明确确认，随后才可授权");
            let state = app.world().resource::<NativeProjectCloseState>();
            assert_eq!(state.target(), CloseTarget::Editor);
            assert_eq!(state.intent, Some(action));
            assert_eq!(state.stage, CloseStage::Draining);
        }
    }
}

#[test]
fn changing_to_editor_during_real_save_keeps_the_writer_but_requires_a_new_confirmation() {
    use feathrise_editor_map::standalone::{ProjectSaveProgress, project_save_progress};
    let mut app = adoption_app(true);
    assert!(!authorize_exit(app.world_mut(), true));
    app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Save);
    assert!(!authorize_exit(app.world_mut(), false));
    app.update();
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1);
    assert!(app.world().resource::<AdoptionOwner>().pending);
    assert_eq!(app.world().resource::<NativeProjectCloseState>().stage, CloseStage::Saving);
    let original_progress = project_save_progress(app.world());
    assert!(matches!(original_progress, ProjectSaveProgress::Running { .. }));

    app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(CloseTarget::Editor);
    let state = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(state.stage, CloseStage::Saving, "切目标不能drop原写者或伪造其完成");
    assert_eq!(state.intent, None);
    assert_eq!(state.choice, None);
    assert_eq!(project_save_progress(app.world()), original_progress);
    assert!(!authorize_exit(app.world_mut(), false));
    assert!(app.world().resource::<NativeProjectCloseState>().status.contains("退出编辑器"));
    {
        // 仅发布领域采用完成事实，实际队列仍必须经原poll发出唯一成功回执。
        let mut owner = app.world_mut().resource_mut::<AdoptionOwner>();
        owner.pending = false;
        owner.adopted = true;
    }
    for _ in 0..4 { app.update(); }
    assert_eq!(project_save_progress(app.world()), ProjectSaveProgress::Succeeded);
    for _ in 0..4 { assert!(!authorize_exit(app.world_mut(), false)); }
    let state = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(state.stage, CloseStage::Confirm);
    assert_eq!(state.status, "确定退出编辑器？");
    assert_eq!(state.intent, None, "原Project保存成功不能复活被撤销的退出许可");
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1);

    app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(app.world_mut(), false));
    assert!(!authorize_exit(app.world_mut(), true));
    app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Close);
    assert!(authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().target(), CloseTarget::Editor);
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1, "新确认不能重写已成功的原保存");
}

#[test]
fn changing_target_keeps_real_save_failure_and_host_blocker_until_explicit_cancel() {
    use feathrise_editor_map::standalone::{ProjectSaveProgress, project_save_progress, request_save_all};
    let mut app = adoption_app(false);
    request_save_all(app.world_mut()).unwrap();
    for _ in 0..4 { app.update(); }
    assert!(matches!(project_save_progress(app.world()), ProjectSaveProgress::Failed { .. }));
    assert!(!authorize_exit(app.world_mut(), true));
    let failure = app.world().resource::<NativeProjectCloseState>().failed.clone();
    assert!(failure.is_some());
    app.world_mut().resource_mut::<NativeProjectCloseState>().hold("原构建进程尚未退出".into());
    app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(CloseTarget::Editor);
    assert!(!authorize_exit(app.world_mut(), false));
    let state = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(state.stage, CloseStage::Blocked);
    assert_eq!(state.failed, failure);
    assert_eq!(state.host_blocked.as_deref(), Some("原构建进程尚未退出"));
    assert!(state.needs_save_decision());
    assert!(!state.action_visible(CloseAction::Close));
    assert_eq!(state.intent, None);
    app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().stage, CloseStage::Idle);
    assert_eq!(app.world().resource::<NativeProjectCloseState>().target(), CloseTarget::Editor);
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1);
}

#[test]
fn dirty_arriving_before_or_after_clean_close_authorization_revokes_the_old_choice() {
    for already_authorized in [false, true] {
        let mut app = adoption_app(false);
        app.world_mut().resource_mut::<AdoptionOwner>().dirty = false;
        assert!(!authorize_exit(app.world_mut(), true));
        app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Close);
        if already_authorized { assert!(authorize_exit(app.world_mut(), false)); }
        // 覆盖点击后采用前，以及子进程收口期间两种竞态；新增草稿仍来自原领域快照。
        app.world_mut().resource_mut::<AdoptionOwner>().dirty = true;
        assert!(!authorize_exit(app.world_mut(), false));
        let close = app.world().resource::<NativeProjectCloseState>();
        assert_eq!(close.stage, CloseStage::Confirm);
        assert_eq!(close.intent, None);
        assert!(close.needs_save_decision());
        assert_eq!(close.documents.len(), 1);
        assert_eq!(close.status, "有 1 项未保存修改，是否保存后关闭？");
        assert!(!close.action_visible(CloseAction::Close));
        assert!(app.world().resource::<AdoptionOwner>().dirty);
        assert_eq!(app.world().resource::<AdoptionOwner>().writes, 0);
    }
}

#[test]
fn clean_close_waits_for_the_existing_save_queue_receipt_without_starting_another_save() {
    use feathrise_editor_map::standalone::{ProjectSaveProgress, project_save_progress, request_save_all};
    let mut app = adoption_app(false);
    app.world_mut().resource_mut::<AdoptionOwner>().dirty = false;
    assert!(!authorize_exit(app.world_mut(), true));
    app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Close);
    // 这是独立于关闭的原保存请求；即使没有目标，原队列的受理/完成也必须走真实调度。
    request_save_all(app.world_mut()).unwrap();
    assert!(matches!(project_save_progress(app.world()), ProjectSaveProgress::Running { .. }));
    assert!(!authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().stage, CloseStage::Waiting);
    for _ in 0..4 { app.update(); }
    assert!(matches!(project_save_progress(app.world()), ProjectSaveProgress::Succeeded));
    assert!(authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 0);
}

#[test]
fn dirty_and_pending_revoke_waiting_clean_close_even_if_a_later_real_save_becomes_clean() {
    use feathrise_editor_map::standalone::{ProjectSaveProgress, project_save_progress, request_save_all};
    let mut app = adoption_app(true);
    app.world_mut().resource_mut::<AdoptionOwner>().dirty = false;
    assert!(!authorize_exit(app.world_mut(), true));
    app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Close);
    assert!(authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().intent, Some(CloseAction::Close));

    // 宿主收口期间原领域出现新草稿；独立保存先冻结真实目标，随后后台准备同时报告pending。
    app.world_mut().resource_mut::<AdoptionOwner>().dirty = true;
    request_save_all(app.world_mut()).unwrap();
    assert!(matches!(project_save_progress(app.world()), ProjectSaveProgress::Running { .. }));
    {
        // 原适配器同时发布新dirty和pending，不能提前返回而漏撤销旧许可。
        let mut owner = app.world_mut().resource_mut::<AdoptionOwner>();
        owner.pending = true;
    }
    assert!(!authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().intent, None,
        "新dirty与pending同帧出现也必须立即撤销旧Close");
    // 原队列已冻结目标；真实领域request回调完成写入并继续等待采用，不能手清dirty。
    app.update();
    assert!(!app.world().resource::<AdoptionOwner>().dirty);
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1);
    assert!(!authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().intent, None);
    {
        // 只提供原领域后台完成事实，仍等待生产poll及队列完整回执，不写关闭许可或Succeeded。
        let mut owner = app.world_mut().resource_mut::<AdoptionOwner>();
        owner.pending = false;
        owner.adopted = true;
    }
    for _ in 0..4 { app.update(); }
    assert!(matches!(project_save_progress(app.world()), ProjectSaveProgress::Succeeded));
    assert!(!authorize_exit(app.world_mut(), false), "变干净和保存成功都不能恢复已经撤销的Close意图");
    let close = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(close.stage, CloseStage::Confirm);
    assert_eq!(close.intent, None);
    assert!(!close.needs_save_decision());
    assert_eq!(close.status, "确定关闭当前工程？");
}

#[test]
fn save_failure_with_no_dirty_documents_cannot_be_consumed_as_a_clean_close() {
    use feathrise_editor_map::standalone::{ProjectSaveProgress, project_save_progress, request_save_all};
    let mut app = adoption_app(false);
    request_save_all(app.world_mut()).unwrap();
    for _ in 0..4 { app.update(); }
    assert!(matches!(project_save_progress(app.world()), ProjectSaveProgress::Failed { detail }
        if detail.contains("正式采用失败")));
    assert!(!app.world().resource::<AdoptionOwner>().dirty);
    assert!(!authorize_exit(app.world_mut(), true));
    {
        let close = app.world().resource::<NativeProjectCloseState>();
        assert!(close.documents.is_empty() && close.failed.is_some());
        assert!(close.needs_save_decision());
        assert!(!close.action_visible(CloseAction::Close));
        assert!(close.action_visible(CloseAction::Save) && close.action_visible(CloseAction::Discard));
    }
    app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Close);
    assert!(!authorize_exit(app.world_mut(), false), "旧关闭动作不能授权忽略真实保存失败");
    let close = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(close.stage, CloseStage::Confirm);
    assert_eq!(close.intent, None);
    assert!(close.failed.as_deref().is_some_and(|error| error.contains("正式采用失败")));
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1);
}

#[test]
fn ordinary_save_finishing_during_confirmation_cannot_authorize_close() {
    let mut app = adoption_app(false);
    app.world_mut().resource_mut::<AdoptionOwner>().adopted = true;
    assert!(!authorize_exit(app.world_mut(), true));
    // 独立的原保存操作可以清除脏位，但没有选择“保存并关闭”，仍须留在确认层。
    feathrise_editor_map::standalone::request_save_all(app.world_mut()).unwrap();
    for _ in 0..4 { app.update(); }
    assert!(matches!(
        feathrise_editor_map::standalone::project_save_progress(app.world()),
        feathrise_editor_map::standalone::ProjectSaveProgress::Succeeded
    ));
    for _ in 0..4 { assert!(!authorize_exit(app.world_mut(), false)); }
    let state = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(state.stage, CloseStage::Confirm);
    assert!(state.documents.is_empty());
    assert_eq!(state.intent, None);
}

#[test]
fn explicit_save_closes_once_after_receipt_and_cancel_clears_its_authorization() {
    let mut app = adoption_app(false);
    app.world_mut().resource_mut::<AdoptionOwner>().adopted = true;
    assert!(!authorize_exit(app.world_mut(), true));
    app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Save);
    assert!(!authorize_exit(app.world_mut(), false));
    for _ in 0..4 { app.update(); }
    assert!(authorize_exit(app.world_mut(), false));
    assert!(authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1);
    app.world_mut().resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(app.world_mut(), false));
    assert!(!authorize_exit(app.world_mut(), true));
    assert_eq!(app.world().resource::<NativeProjectCloseState>().stage, CloseStage::Confirm);
    assert!(!authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1);
}

#[test]
fn unsaved_close_requires_choice_and_cancel_keeps_original_draft() {
    let mut world = dirty_world();
    assert!(!authorize_exit(&mut world, true));
    assert_eq!(
        world.resource::<NativeProjectCloseState>().stage,
        CloseStage::Confirm
    );
    assert!(
        !world
            .resource::<NativeProjectCloseState>()
            .documents
            .is_empty()
    );
    world.resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(&mut world, false));
    assert!(!world.resource::<NativeProjectCloseState>().active());
    let surface = world.resource::<SurfacePresetEditorState>();
    assert!(surface.tabs[0].draft.dirty());
    assert_eq!(surface.tabs[0].draft.descriptor.label, "未保存地表·修改");
    assert_eq!(surface.tabs[0].draft.baseline.label, "未保存地表");
}

#[test]
fn discard_only_authorizes_exit_without_resetting_dirty_state_or_history() {
    let mut world = dirty_world();
    assert!(!authorize_exit(&mut world, true));
    world.resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Discard);
    assert!(authorize_exit(&mut world, false));
    assert!(
        world.resource::<SurfacePresetEditorState>().tabs[0]
            .draft
            .dirty()
    );
    assert_eq!(
        world.resource::<NativeProjectCloseState>().stage,
        CloseStage::Draining
    );
    // 确认之后仍可在后台进程收口阶段取消，原作者状态从未被销毁。
    world.resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(&mut world, false));
    assert!(
        world.resource::<SurfacePresetEditorState>().tabs[0]
            .draft
            .dirty()
    );
}

#[test]
fn unconfirmed_process_cleanup_cannot_be_overridden_by_discard() {
    let mut world = dirty_world();
    world
        .resource_mut::<NativeProjectCloseState>()
        .hold("子进程尚未确认退出".into());
    world.resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Discard);
    assert!(!authorize_exit(&mut world, false));
    assert_eq!(
        world.resource::<NativeProjectCloseState>().stage,
        CloseStage::Blocked
    );
    world.resource_mut::<NativeProjectCloseState>().choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(&mut world, false));
    assert!(!world.resource::<NativeProjectCloseState>().active());
}

#[test]
fn retry_after_written_but_unadopted_save_must_run_the_original_coordinator_even_when_clean() {
    let mut world = World::new();
    world.insert_resource(project_close_state());
    world.init_resource::<Messages<SavedPlayFeedback>>();
    {
        let mut state = world.resource_mut::<NativeProjectCloseState>();
        state.stage = CloseStage::Confirm;
        state.failed = Some("文件已写入但正式目录尚未采用".into());
        state.choice = Some(CloseAction::Save);
    }
    assert!(
        !authorize_exit(&mut world, false),
        "没有脏位不等于上笔失败已恢复"
    );
    assert_eq!(
        world.resource::<NativeProjectCloseState>().stage,
        CloseStage::Saving
    );
    assert!(matches!(
        feathrise_editor_map::standalone::project_save_progress(&world),
        feathrise_editor_map::standalone::ProjectSaveProgress::Running { .. }
    ));
}

#[test]
fn original_surface_save_failure_keeps_exact_draft_and_cancel_returns_to_same_world() {
    use feathrise_editor_map::standalone::{
        ProjectCloseSaveAdapters, ProjectSavePlugin, ProjectSaveProgress, project_save_progress,
    };
    fn reject(_: &mut World, _: u64) -> Result<(), String> {
        Err("本轮地表保存入口注入失败".into())
    }
    let mut app = App::new();
    app.add_plugins(ProjectSavePlugin)
        .insert_resource(project_close_state())
        .add_message::<SavedPlayFeedback>()
        .insert_resource(ProjectCloseSaveAdapters { surface: reject })
        .insert_resource(crate::native_authoring_validation::dirty_surface_fixture());
    let marker = app.world_mut().spawn_empty().id();
    let before = app.world().resource::<SurfacePresetEditorState>().tabs[0]
        .draft
        .clone();
    assert!(!authorize_exit(app.world_mut(), true));
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .choice = Some(CloseAction::Save);
    assert!(!authorize_exit(app.world_mut(), false));
    app.update(); // 真正原保存调度器消费冻结的Surface目标，不能把手填Failed当作失败回执。
    assert!(
        matches!(project_save_progress(app.world()), ProjectSaveProgress::Failed { detail } if detail.contains("本轮地表保存入口注入失败"))
    );
    assert!(!authorize_exit(app.world_mut(), false));
    assert_eq!(
        app.world().resource::<NativeProjectCloseState>().stage,
        CloseStage::Confirm
    );
    let state = app.world().resource::<SurfacePresetEditorState>();
    assert_eq!(state.tabs[0].draft.descriptor, before.descriptor);
    assert_eq!(state.tabs[0].draft.baseline, before.baseline);
    assert!(state.tabs[0].draft.dirty());
    assert!(app.world().get_entity(marker).is_ok());
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(app.world_mut(), false));
    assert!(!app.world().resource::<NativeProjectCloseState>().active());
    assert_eq!(
        app.world().resource::<SurfacePresetEditorState>().tabs[0]
            .draft
            .descriptor,
        before.descriptor
    );
    assert!(app.world().get_entity(marker).is_ok());
}

/// 领域只模拟自己的写入/采用事实；退出仍走生产保存队列，实际文件事务由地图库回归覆盖。
#[derive(Resource)]
struct AdoptionOwner {
    dirty: bool,
    pending: bool,
    delay_adoption: bool,
    adopted: bool,
    writes: usize,
}

fn adoption_app(delay_adoption: bool) -> App {
    use feathrise_editor_map::standalone::{
        ProjectAuthorAdapter, ProjectAuthorAdapters, ProjectAuthorDocument, ProjectAuthorSnapshot,
        ProjectSavePlugin,
    };
    let mut app = App::new();
    app.add_plugins(ProjectSavePlugin)
        .insert_resource(project_close_state())
        .add_message::<SavedPlayFeedback>()
        .insert_resource(AdoptionOwner {
            dirty: true,
            pending: false,
            delay_adoption,
            adopted: false,
            writes: 0,
        })
        .init_resource::<ProjectAuthorAdapters>();
    app.world_mut()
        .resource_mut::<ProjectAuthorAdapters>()
        .register(ProjectAuthorAdapter {
            domain: "test.adoption",
            order: 0,
            collect: |world| {
                let owner = world.resource::<AdoptionOwner>();
                ProjectAuthorSnapshot {
                    pending: owner.pending.then(|| "正式采用仍在处理".into()),
                    unsaved: owner
                        .dirty
                        .then(|| ProjectAuthorDocument {
                            id: 1,
                            label: "采用回执文档".into(),
                        })
                        .into_iter()
                        .collect(),
                    ..default()
                }
            },
            saved_play_blocker: |_| None,
            request: |world, _| {
                let mut owner = world.resource_mut::<AdoptionOwner>();
                owner.writes += 1;
                owner.dirty = false;
                owner.pending = owner.delay_adoption;
                Ok(())
            },
            poll: |world, _| {
                let owner = world.resource::<AdoptionOwner>();
                if owner.pending {
                    Ok(false)
                } else if owner.adopted {
                    Ok(true)
                } else {
                    Err("文件已写入但正式采用失败".into())
                }
            },
            catalog_hash: |_, _| None,
            rebase_own_write: |_, _, _| (),
        })
        .unwrap();
    app
}

#[test]
fn cancelled_unadopted_save_still_requires_confirmation_on_the_next_close() {
    let mut app = adoption_app(false);
    assert!(!authorize_exit(app.world_mut(), true));
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .choice = Some(CloseAction::Save);
    assert!(!authorize_exit(app.world_mut(), false));
    app.update();
    app.update(); // 由原队列消费请求并接到采用失败，而不是手填Failed。
    assert!(!authorize_exit(app.world_mut(), false));
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(app.world_mut(), false));
    assert!(!authorize_exit(app.world_mut(), true));
    let close = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(close.stage, CloseStage::Confirm);
    assert!(close.documents.is_empty());
    assert!(
        close
            .failed
            .as_deref()
            .is_some_and(|error| error.contains("正式采用失败"))
    );

    // 修复领域采用后，只重验原回执；不能再次调用写入或把空脏队列当成成功。
    app.world_mut().resource_mut::<AdoptionOwner>().adopted = true;
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .choice = Some(CloseAction::Save);
    assert!(!authorize_exit(app.world_mut(), false));
    app.update();
    assert!(authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1);
}

#[test]
fn failure_arriving_after_cancel_and_reclose_cannot_bypass_the_waiting_close() {
    let mut app = adoption_app(true);
    assert!(!authorize_exit(app.world_mut(), true));
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .choice = Some(CloseAction::Save);
    assert!(!authorize_exit(app.world_mut(), false));
    app.update();
    app.update();
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(app.world_mut(), false));
    assert!(!authorize_exit(app.world_mut(), true));
    assert_eq!(
        app.world().resource::<NativeProjectCloseState>().stage,
        CloseStage::Waiting
    );

    app.world_mut().resource_mut::<AdoptionOwner>().pending = false;
    // 模拟本帧Tasks已结束、Last尚未poll：dirty和领域pending均为空，队列仍拥有在途回执。
    assert!(!authorize_exit(app.world_mut(), false));
    assert_eq!(
        app.world().resource::<NativeProjectCloseState>().stage,
        CloseStage::Waiting
    );
    app.update(); // 原在途任务现在才回执失败，当前关闭不是Saving阶段。
    assert!(!authorize_exit(app.world_mut(), false));
    let close = app.world().resource::<NativeProjectCloseState>();
    assert_eq!(close.stage, CloseStage::Confirm);
    assert!(close.documents.is_empty());
    assert!(
        close
            .failed
            .as_deref()
            .is_some_and(|error| error.contains("正式采用失败"))
    );
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1);
}

#[test]
fn discard_after_cancel_still_waits_for_the_original_save_queue_receipt() {
    let mut app = adoption_app(true);
    assert!(!authorize_exit(app.world_mut(), true));
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .choice = Some(CloseAction::Save);
    assert!(!authorize_exit(app.world_mut(), false));
    app.update();
    app.update();
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .choice = Some(CloseAction::Cancel);
    assert!(!authorize_exit(app.world_mut(), false));
    assert!(!authorize_exit(app.world_mut(), true));

    // 放弃未保存内容也不能截断已受理的写盘/采用，先等唯一队列发布原回执。
    app.world_mut().resource_mut::<AdoptionOwner>().pending = false;
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .choice = Some(CloseAction::Discard);
    assert!(!authorize_exit(app.world_mut(), false));
    assert_eq!(
        app.world().resource::<NativeProjectCloseState>().stage,
        CloseStage::Waiting
    );
    app.update();
    assert!(authorize_exit(app.world_mut(), false));
    assert_eq!(app.world().resource::<AdoptionOwner>().writes, 1);
}

#[test]
fn enter_defaults_to_cancel_even_if_a_destructive_button_has_a_stale_pulse() {
    for target in [CloseTarget::Project, CloseTarget::Editor] {
        for key in [KeyCode::Enter, KeyCode::Escape] {
            assert_keyboard_defaults_to_cancel(key, target);
        }
    }
}

fn assert_keyboard_defaults_to_cancel(key: KeyCode, target: CloseTarget) {
    let mut app = App::new();
    app.insert_resource(project_close_state())
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_systems(Update, super::ui::handle);
    let cancel = app
        .world_mut()
        .spawn((
            CloseAction::Cancel,
            Node::default(),
            feathrise_editor_ui::EditorButtonClick::default(),
        ))
        .id();
    app.world_mut().spawn((
        CloseAction::Close,
        Node::default(),
        feathrise_editor_ui::EditorButtonClick::triggered(),
    ));
    app.world_mut().spawn((
        CloseAction::Discard,
        Node { display: Display::None, ..default() },
        bevy::ui::InteractionDisabled,
        feathrise_editor_ui::EditorButtonClick::triggered(),
    ));
    {
        let mut state = app.world_mut().resource_mut::<NativeProjectCloseState>();
        state.set_target(target);
        state.stage = CloseStage::Confirm;
        state.focus_cancel = true;
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    app.update();
    assert_eq!(
        app.world().resource::<NativeProjectCloseState>().choice,
        Some(CloseAction::Cancel)
    );
    assert_eq!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(cancel)
    );
    assert!(
        app.world()
            .resource::<NativeProjectCloseState>()
            .captures_input()
    );
    // 关闭确认消失后的释放帧也不将旧边沿交给原工具。
    app.world_mut()
        .resource_mut::<NativeProjectCloseState>()
        .stage = CloseStage::Idle;
    app.update();
    assert!(
        app.world()
            .resource::<NativeProjectCloseState>()
            .captures_input()
    );
    app.update();
    assert!(
        !app.world()
            .resource::<NativeProjectCloseState>()
            .captures_input()
    );
}
