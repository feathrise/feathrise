//! 取消/重试必须经过空世界边界，不能丢弃草稿或采用旧IO结果。

use super::*;
use feathrise_map_content::MapId;

fn fixture(retry: bool) -> (App, MapId) {
    let map = MapId::parse("MAP-recovery").unwrap();
    let mut app = App::new();
    app.insert_resource(State::new(ScenePhase::Active))
        .init_resource::<NextState<ScenePhase>>()
        .init_resource::<MapDocuments>()
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<DraftLoadTask>()
        .insert_resource(MapEditorState {
            open: true, map_transition_in_progress: true,
            selected_map_id: Some(map.clone()), ..default()
        })
        .insert_resource(MapLoadingRecovery { request: Some(MapLoadingRecoveryRequest {
            map_id: map.clone(), retry, returning: false,
        }), ..default() });
    (app, map)
}

#[test]
fn cancel_invalidates_io_and_retains_author_draft_and_other_tabs() {
    let (mut app, map) = fixture(false);
    install_probe(&mut app, false);
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.map_id = map.clone();
    draft.environment_dirty = true;
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft = Some(draft);
    app.world_mut().resource_mut::<MapEditorState>().dirty = true;
    let other = MapId::parse("MAP-other-dirty").unwrap();
    app.world_mut().resource_mut::<MapDocuments>().upsert(other.clone(), "其他草稿".into(), true);
    app.world_mut().resource_mut::<DraftLoadTask>().desired_map_id = Some(map.clone());
    advance_recovery(app.world_mut());
    assert!(app.world().resource::<RecoveryDomainProbe>().rebound);
    assert_eq!(app.world().resource::<DraftLoadTask>().ticket, 1);
    assert!(app.world().resource::<DraftLoadTask>().desired_map_id.is_none());
    assert!(app.world().resource::<MapEditorState>().map_transition_in_progress);
    assert!(app.world().resource::<MapEditorState>().dirty);
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().environment_dirty);
    assert!(app.world().resource::<MapDocuments>().contains(&other));
    assert!(matches!(app.world().resource::<NextState<ScenePhase>>(), NextState::Pending(ScenePhase::Idle)));
    app.insert_resource(State::new(ScenePhase::Idle));
    advance_recovery(app.world_mut());
    assert!(!app.world().resource::<MapEditorState>().map_transition_in_progress);
    assert!(app.world().resource::<MapEditorState>().pending_map_transition.is_none());
    assert!(app.world().resource::<EditorAuthoringSession>().draft.is_some());
}

#[test]
fn retry_waits_for_return_and_enqueues_exact_map_once() {
    let (mut app, map) = fixture(true);
    advance_recovery(app.world_mut());
    advance_recovery(app.world_mut());
    assert!(app.world().resource::<MapEditorState>().pending_map_transition.is_none());
    app.insert_resource(State::new(ScenePhase::Idle));
    advance_recovery(app.world_mut());
    assert_eq!(app.world().resource::<MapEditorState>().pending_map_transition.as_ref(), Some(&map));
    assert!(app.world().resource::<MapLoadingRecovery>().request.is_none());
    advance_recovery(app.world_mut());
    assert_eq!(app.world().resource::<DraftLoadTask>().ticket, 1);
}

#[test]
fn cancel_waits_for_real_author_work_without_dropping_its_task_slot() {
    let (mut app, _) = fixture(false);
    app.add_plugins(MinimalPlugins);
    install_probe(&mut app, true);
    advance_recovery(app.world_mut());
    assert_eq!(app.world().resource::<DraftLoadTask>().ticket, 0);
    assert!(app.world().resource::<RecoveryDomainProbe>().pending);
    assert!(!app.world().resource::<RecoveryDomainProbe>().rebound);
    app.world_mut().resource_mut::<RecoveryDomainProbe>().pending = false;
    advance_recovery(app.world_mut());
    assert_eq!(app.world().resource::<DraftLoadTask>().ticket, 1);
}

#[test]
fn preparation_and_different_target_cannot_be_cancelled_by_stale_request() {
    let (mut app, map) = fixture(false);
    app.world_mut().resource_mut::<MapEditorState>().pending_map_transition = Some(map);
    advance_recovery(app.world_mut());
    assert_eq!(app.world().resource::<DraftLoadTask>().ticket, 0);
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.pending_map_transition = None;
        editor.selected_map_id = Some(MapId::parse("MAP-new-target").unwrap());
    }
    advance_recovery(app.world_mut());
    assert!(app.world().resource::<MapLoadingRecovery>().request.is_none());
    assert_eq!(app.world().resource::<DraftLoadTask>().ticket, 0);
}

// 这里验证普通恢复流程遵守真实装配回调；角色任务细节由游戏自己的适配回归覆盖。
#[derive(Resource)]
struct RecoveryDomainProbe { pending: bool, rebound: bool }
fn install_probe(app: &mut App, pending: bool) {
    use crate::authoring::domain_documents::MapDomainAdapter;
    app.insert_resource(RecoveryDomainProbe { pending, rebound: false }).insert_resource(MapDomainAdapter {
        load: |_, _| Err("恢复夹具不启动文档读取".into()),
        park: |_, _| {}, restore: |_, _, _| {}, close: |_, _| {}, history: |_, _| Ok(()),
        recover: |world| { let mut probe = world.resource_mut::<RecoveryDomainProbe>(); if probe.pending { return false; } probe.rebound = true; true },
    });
}
