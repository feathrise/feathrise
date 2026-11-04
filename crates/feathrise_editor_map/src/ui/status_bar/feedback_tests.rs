//! 第二批回执的同帧隔离、全局对象、重试与地图结果适配回归；不调用正式文件事务。

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task};

use super::resources::EditorStatusBarState;
use super::systems::observe_status;
use super::types::StatusPhase;
use crate::resources::{
    EditorAssetCatalog, MapBrowserState, MapCreationTask, MapDeletionTask, MapEditorState,
};
use crate::types::{
    CreatedMap, DeletedMap, EditorStatusKind, EditorStatusOperation, EditorStatusTarget,
    NewMapTerrainMode,
};

/// 注册表由正式Startup创建，随后仅显式运行受测适配器，不让磁盘热更覆盖测试快照。
pub(crate) fn asset_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .add_plugins(feathrise_map_content::MapContentPlugin)
        .insert_resource(MapEditorState {
            open: true,
            dirty: true,
            ..default()
        })
        .init_resource::<EditorStatusBarState>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
    app.world_mut().run_schedule(Startup);
    app
}

pub(crate) fn ready_result<Output: Send + 'static>(output: Output) -> Task<Output> {
    let task = IoTaskPool::get().spawn(async move { output });
    for _ in 0..1_000 {
        if task.is_finished() {
            return task;
        }
        std::thread::sleep(std::time::Duration::from_millis(5)); // 仅测试夹具有界等待已知纯结果，不进入生产帧路径。
    }
    assert!(task.is_finished(), "测试任务未在五秒内完成");
    task
}

pub(crate) fn snapshot(app: &mut App) -> (String, String, String) {
    app.world_mut()
        .resource_mut::<EditorStatusBarState>()
        .popup_open = true;
    app.world_mut().run_system_once(observe_status).unwrap();
    let state = app.world().resource::<EditorStatusBarState>();
    (
        state.phase.label().into(),
        state.task.clone(),
        state.detail.clone(),
    )
}

pub(crate) fn acknowledge(app: &mut App) {
    app.init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
        .init_resource::<Clipboard>()
        .add_message::<bevy::input::mouse::MouseWheel>();
    app.world_mut()
        .resource_mut::<EditorStatusBarState>()
        .popup_open = true;
    app.world_mut().spawn((
        super::components::StatusButton(super::types::StatusAction::Dismiss),
        feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed,
    ));
    app.world_mut()
        .run_system_once(super::systems::control_details)
        .unwrap();
}

fn publish(app: &mut App, operation: EditorStatusOperation, id: &str, kind: EditorStatusKind) {
    let mut editor = app.world_mut().resource_mut::<MapEditorState>();
    editor.status = format!("原文/{id}/完整中文诊断\n尾部");
    editor.record_operation_result(operation, &EditorStatusTarget::new(id, "同名对象"), kind);
}

#[test]
fn simultaneous_operation_receipts_keep_all_failures_and_only_matching_success_resolves_one() {
    let mut app = asset_app();
    let operations = [
        EditorStatusOperation::SurfacePresetSave,
        EditorStatusOperation::WaterPresetSave,
        EditorStatusOperation::VegetationPresetSave,
    ];
    for (operation, id) in operations
        .into_iter()
        .zip(["surface-a", "water-a", "vegetation-a"])
    {
        publish(&mut app, operation, id, EditorStatusKind::Error);
    }
    publish(
        &mut app,
        EditorStatusOperation::MapDelete,
        "MAP-unrelated",
        EditorStatusKind::Success,
    );
    let (phase, _, detail) = snapshot(&mut app);
    assert_eq!(phase, "失败");
    for id in ["surface-a", "water-a", "vegetation-a"] {
        assert_eq!(detail.matches(&format!("原文/{id}/")).count(), 1);
    }
    publish(
        &mut app,
        EditorStatusOperation::SurfacePresetSave,
        "surface-b",
        EditorStatusKind::Success,
    );
    assert!(snapshot(&mut app).2.contains("原文/surface-a/"));
    publish(
        &mut app,
        EditorStatusOperation::SurfacePresetSave,
        "surface-a",
        EditorStatusKind::Success,
    );
    snapshot(&mut app);
    let state = app.world().resource::<EditorStatusBarState>();
    assert!(state.notices[EditorStatusOperation::SurfacePresetSave.slot()].is_none());
    assert!(state.notices[EditorStatusOperation::WaterPresetSave.slot()].is_some());
    assert!(state.notices[EditorStatusOperation::VegetationPresetSave.slot()].is_some());
}

#[test]
fn global_operation_errors_survive_map_changes_and_acknowledged_receipts_do_not_replay() {
    let mut app = asset_app();
    publish(
        &mut app,
        EditorStatusOperation::MapCreate,
        "新图",
        EditorStatusKind::Error,
    );
    publish(
        &mut app,
        EditorStatusOperation::WaterPresetSave,
        "lake",
        EditorStatusKind::Warning,
    );
    snapshot(&mut app);
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = Some(feathrise_map_content::MapId::parse("MAP-other").unwrap());
    snapshot(&mut app);
    let state = app.world().resource::<EditorStatusBarState>();
    assert!(state.notices[EditorStatusOperation::MapCreate.slot()].is_some());
    assert!(state.notices[EditorStatusOperation::WaterPresetSave.slot()].is_some());
    {
        let mut state = app.world_mut().resource_mut::<EditorStatusBarState>();
        state.notices = Default::default();
        state.retained_runtime = None;
        state.detail_dirty = true;
    }
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = None;
    snapshot(&mut app);
    assert!(
        app.world()
            .resource::<EditorStatusBarState>()
            .retained
            .is_none()
    );
    publish(
        &mut app,
        EditorStatusOperation::MapCreate,
        "新图",
        EditorStatusKind::Error,
    );
    assert_eq!(snapshot(&mut app).0, "失败");
}

#[test]
fn global_environment_success_is_not_a_map_save_retry() {
    let mut app = asset_app();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .cancel_close_with_error("地图文件失败");
    snapshot(&mut app);
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.status = "全局环境成功".into();
        editor.record_global_status_result(EditorStatusKind::Success);
    }
    assert_eq!(snapshot(&mut app).0, "失败");
    assert!(
        app.world().resource::<EditorStatusBarState>().notices
            [EditorStatusOperation::MapSave.slot()]
        .is_some()
    );
}

#[test]
fn latest_receipt_follows_event_sequence_not_slot_iteration_order() {
    let mut app = asset_app();
    publish(
        &mut app,
        EditorStatusOperation::VegetationPresetSave,
        "tree",
        EditorStatusKind::Success,
    );
    publish(
        &mut app,
        EditorStatusOperation::MapCreate,
        "最后的新图",
        EditorStatusKind::Success,
    );
    snapshot(&mut app);
    let state = app.world().resource::<EditorStatusBarState>();
    assert_eq!(
        state.latest_result.as_ref().unwrap().target.id.as_ref(),
        "最后的新图"
    );
    assert_eq!(state.phase, StatusPhase::Success);
}

#[test]
fn creation_receipt_uses_frozen_name_and_published_files_do_not_mean_map_is_ready() {
    let mut app = asset_app();
    app.init_resource::<EditorAssetCatalog>()
        .init_resource::<MapBrowserState>();
    let id = feathrise_map_content::MapId::parse("MAP-created-feedback").unwrap();
    let target = EditorStatusTarget::new("冻结新图", "冻结新图");
    app.insert_resource(MapCreationTask {
        ticket: 2,
        feedback_target: Some(target.clone()),
        task: Some(ready_result((
            2,
            Ok(CreatedMap {
                map_id: id.clone(),
                display_name: "冻结新图".into(),
                width_meters: 128.0,
                length_meters: 128.0,
                terrain_mode: NewMapTerrainMode::SparseClipmap,
                terrain_mesh_resolution: [129, 129],
                folder_snapshot: None,
            }),
        ))),
        ..default()
    });
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .new_map_name = "后来输入的名字".into();
    app.world_mut()
        .run_system_once(crate::authoring::poll_map_creation)
        .unwrap();
    let editor = app.world().resource::<MapEditorState>();
    let receipt = editor.status_results[EditorStatusOperation::MapCreate.slot()]
        .as_ref()
        .unwrap();
    assert_eq!(receipt.kind, EditorStatusKind::Success);
    assert_eq!(receipt.target.id, target.id);
    assert_eq!(editor.pending_map_transition.as_ref(), Some(&id));
    assert!(editor.dirty); // 原草稿责任不因反馈变化被清除。
    assert_eq!(snapshot(&mut app).0, "处理中");
}

#[test]
fn map_failure_receipts_keep_objects_and_stale_tickets_do_not_publish() {
    let mut app = asset_app();
    app.init_resource::<EditorAssetCatalog>()
        .init_resource::<MapBrowserState>();
    app.insert_resource(MapCreationTask {
        ticket: 3,
        feedback_target: Some(EditorStatusTarget::new("冻结名称", "冻结名称")),
        task: Some(ready_result((3, Err("目录并发冲突".into())))),
        ..default()
    });
    app.world_mut()
        .run_system_once(crate::authoring::poll_map_creation)
        .unwrap();
    let editor = app.world().resource::<MapEditorState>();
    assert!(
        editor.status_results[EditorStatusOperation::MapCreate.slot()]
            .as_ref()
            .unwrap()
            .detail
            .contains("冻结名称")
    );
    assert!(editor.pending_map_transition.is_none());
    let sequence = editor.status_sequence;
    app.insert_resource(MapCreationTask {
        ticket: 5,
        feedback_target: Some(EditorStatusTarget::new("新票据", "新票据")),
        task: Some(ready_result((4, Err("迟到失败".into())))),
        ..default()
    });
    app.world_mut()
        .run_system_once(crate::authoring::poll_map_creation)
        .unwrap();
    assert_eq!(
        app.world().resource::<MapEditorState>().status_sequence,
        sequence
    );

    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active));
    app.insert_resource(MapDeletionTask {
        ticket: 1,
        feedback_target: Some(EditorStatusTarget::new("MAP-delete-a", "原删除对象")),
        task: Some(ready_result((1, Err("引用仍存在".into())))),
        ..default()
    });
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .map_list_selection_id = Some(feathrise_map_content::MapId::parse("MAP-delete-b").unwrap());
    app.world_mut()
        .run_system_once(crate::authoring::poll_map_deletion)
        .unwrap();
    let editor = app.world().resource::<MapEditorState>();
    let receipt = editor.status_results[EditorStatusOperation::MapDelete.slot()]
        .as_ref()
        .unwrap();
    assert_eq!(receipt.kind, EditorStatusKind::Error);
    assert_eq!(receipt.target.id.as_ref(), "MAP-delete-a");
    assert!(receipt.detail.contains("引用仍存在"));
    let target = receipt.target.clone();
    app.insert_resource(MapDeletionTask {
        ticket: 2,
        feedback_target: Some(target),
        task: Some(ready_result((
            2,
            Ok(DeletedMap {
                map_id: feathrise_map_content::MapId::parse("MAP-delete-a").unwrap(),
                display_name: "原删除对象".into(),
                trash_folder: Some("maps/.trash/MAP-delete-a".into()),
                permanently_deleted: false,
                cleanup_warning: None,
                source_folder: "maps/MAP-delete-a".into(),
                folder_snapshot: None,
            }),
        ))),
        ..default()
    });
    app.world_mut()
        .run_system_once(crate::authoring::poll_map_deletion)
        .unwrap();
    let editor = app.world().resource::<MapEditorState>();
    let receipt = editor.status_results[EditorStatusOperation::MapDelete.slot()]
        .as_ref()
        .unwrap();
    assert_eq!(receipt.kind, EditorStatusKind::Success);
    assert!(receipt.detail.contains("可人工恢复"));

    // 文件事务已提交但永久清理失败时，目录仍须刷新，回执必须保留警告而非成功。
    let target = receipt.target.clone();
    app.world_mut().resource_mut::<MapEditorState>().map_list_selection_id =
        Some(feathrise_map_content::MapId::parse("MAP-delete-a").unwrap());
    let previous_revision = app.world().resource::<EditorAssetCatalog>().requested_revision;
    app.insert_resource(MapDeletionTask {
        ticket: 3,
        feedback_target: Some(target),
        task: Some(ready_result((3, Ok(DeletedMap {
            map_id: feathrise_map_content::MapId::parse("MAP-delete-a").unwrap(),
            display_name: "原删除对象".into(),
            trash_folder: None,
            permanently_deleted: false,
            cleanup_warning: Some("永久清理未完成；残余文件位置：测试隔离目录".into()),
            source_folder: "maps/MAP-delete-a".into(),
            folder_snapshot: None,
        })))),
        ..default()
    });
    app.world_mut().run_system_once(crate::authoring::poll_map_deletion).unwrap();
    let editor = app.world().resource::<MapEditorState>();
    let receipt = editor.status_results[EditorStatusOperation::MapDelete.slot()].as_ref().unwrap();
    assert_eq!(receipt.kind, EditorStatusKind::Warning);
    assert!(receipt.detail.contains("残余文件位置"));
    assert!(editor.map_list_selection_id.is_none());
    assert_ne!(app.world().resource::<EditorAssetCatalog>().requested_revision, previous_revision);
}
