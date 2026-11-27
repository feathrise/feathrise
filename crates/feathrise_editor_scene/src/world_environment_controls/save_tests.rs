//! 环境滑块尚未松手时的真实异步保存回归；独立进程隔离作者资产根。

use std::{path::{Path, PathBuf}, time::{Duration, Instant}};

use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce};
use feathrise_scene_content::{
    ActiveDynamicScene, EnvironmentSimulation, SCENE_MANIFEST_FILE_NAME, SceneEnvironmentSource,
    SceneManifestAsset, ScenePlacementsAsset, ScenePortalsAsset, SceneSurfaceAsset,
    SceneVegetationAsset, SceneWaterAsset, WORLD_ENVIRONMENT_ASSET_PATH, WorldEnvironmentAsset,
    WorldEnvironmentLibrary,
};

use super::*;
use crate::{
    authoring::{
        DraftSaveTask, PlacementTerrainFollowState, PlacementWaterEvidenceRegistry,
        PortalPreviewHandoff, SparseTerrainAuthoringState, TerrainBrushStroke, TerrainExternalChangeState,
        VegetationCompileState, WaterBrushStroke,
        WaterFileHandoff, WaterPreviewState, poll_draft_save, poll_global_environment_save,
        reconcile_following_placements, start_draft_save, start_global_environment_save,
        start_sparse_terrain_save,
    },
    resources::SceneDeletionTask,
    ui::{environment::EnvironmentPage, placement_water::PlacementInstanceIndex, sidebars::EditorRightSidebarTab},
};

const CHILD_ROOT: &str = "FEATHRISE_ENVIRONMENT_SAVE_REGRESSION_ROOT";

fn isolated(test_name: &str, run: impl FnOnce(&Path)) {
    // libtest用例名不含crate前缀；否则--exact会零用例退出并造成假通过。
    let test_name = test_name.strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::")).unwrap();
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = PathBuf::from(root);
        assert!(root.join(".environment-save-test-owner").is_file());
        // 路径覆盖只在本次独占测试进程生效，不能影响并行回归或用户工程。
        feathrise_editor_core::set_runtime_test_asset_root(Some(root.clone()));
        run(&root);
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "feathrise-environment-save-{}", bevy::asset::uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join(".environment-save-test-owner"), test_name).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(CHILD_ROOT, &root).output().unwrap();
    assert!(output.status.success(), "环境保存子进程失败：\n{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).lines().any(|line| line == "running 1 test"),
        "独立进程必须实际执行当前用例");
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    std::fs::remove_dir_all(root).unwrap();
}

#[derive(Resource, Default)]
struct SaveAfterControls(bool);

fn request_after_controls(mut request: ResMut<SaveAfterControls>, mut editor: ResMut<SceneEditorState>) {
    // 和正式快捷键一样，保存请求晚于本帧滑块输入到达，不能靠控件入口提前读取请求掩盖问题。
    if std::mem::take(&mut request.0) { editor.save_requested = true; }
}

fn time_cursor(hour: f32) -> Vec2 {
    // 原滑块的归一化坐标以轨道中心为零，保留两端半个手柄的内边距。
    Vec2::new((hour_thumb_left(hour) + super::super::helpers::WORLD_SLIDER_THUMB_WIDTH * 0.5)
        / WORLD_SLIDER_TRACK_WIDTH - 0.5, 0.0)
}

fn editor(root: &Path, local: bool) -> (App, Entity, PathBuf) {
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    draft.manifest.environment.source = SceneEnvironmentSource::Local;
    draft.manifest.environment.simulation = Some(EnvironmentSimulation { hour: 6.0, ..default() });
    let manifest_path = root.join(&draft.folder).join(SCENE_MANIFEST_FILE_NAME);
    std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    std::fs::write(&manifest_path, serde_json::to_vec(&draft.manifest).unwrap()).unwrap();
    let mut global = GlobalEnvironmentAuthoringState::default();
    global.draft.library = WorldEnvironmentLibrary::default().with_legacy_default(&global.draft.celestial_lighting);
    global.saved = global.draft.clone();
    global.maintain_environment_selection();
    let global_path = root.join(WORLD_ENVIRONMENT_ASSET_PATH);
    std::fs::create_dir_all(global_path.parent().unwrap()).unwrap();
    std::fs::write(&global_path, serde_json::to_vec(&global.draft).unwrap()).unwrap();
    let scene_id = draft.scene_id.clone();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin { file_path: root.to_string_lossy().into_owned(), ..default() }))
        .init_asset::<SceneManifestAsset>()
        .init_asset::<ScenePlacementsAsset>()
        .init_asset::<ScenePortalsAsset>()
        .init_asset::<SceneSurfaceAsset>()
        .init_asset::<SceneVegetationAsset>()
        .init_asset::<SceneWaterAsset>()
        .add_message::<bevy::asset::AssetEvent<bevy::world_serialization::WorldAsset>>()
        .add_message::<feathrise_scene::DynamicWaterRebuildRequest>()
        .add_message::<feathrise_scene::ScenePlacementRebuildRequest>()
        .insert_resource(global)
        .insert_resource(SceneEditorState { open: true, selected_scene_id: Some(scene_id), ..default() })
        .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
        .init_resource::<SceneEditorSidebarState>()
        .init_resource::<EnvironmentPageState>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<WorldClock>()
        .init_resource::<WeatherDebugOverride>()
        .init_resource::<EditorSettings>()
        .init_resource::<SimulationInputState>()
        .init_resource::<SaveAfterControls>()
        .init_resource::<GlobalEnvironmentSaveTask>()
        .init_resource::<DraftSaveTask>()
        .init_resource::<SceneDeletionTask>()
        .init_resource::<VegetationCompileState>()
        .init_resource::<ActiveDynamicScene>()
        .init_resource::<feathrise_scene::DynamicWaterStatus>()
        .init_resource::<feathrise_scene::ScenePlacementRuntimeStatus>()
        .init_resource::<feathrise_scene::ScenePortalRuntimeStatus>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<TerrainBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<PlacementInstanceIndex>()
        .init_resource::<PlacementTerrainFollowState>()
        .init_resource::<SparseTerrainAuthoringState>()
        .init_resource::<PlacementWaterEvidenceRegistry>()
        .init_resource::<WaterFileHandoff>()
        .init_resource::<PortalPreviewHandoff>()
        .init_resource::<TerrainExternalChangeState>()
        .add_systems(Update, (
            handle_world_environment_controls, request_after_controls,
            // 即使没有摆放，也由原生产系统建立当前地形/结构来源的保存信用。
            reconcile_following_placements, start_global_environment_save,
            start_sparse_terrain_save, start_draft_save,
        ).chain());
    app.world_mut().resource_mut::<SceneEditorSidebarState>().right = EditorRightSidebarTab::Environment;
    app.world_mut().resource_mut::<EnvironmentPageState>().page =
        if local { EnvironmentPage::Scene } else { EnvironmentPage::Library };
    let track = app.world_mut().spawn((WorldTimeSlider, UiInteraction::Pressed,
        RelativeCursorPosition { cursor_over: true, normalized: Some(time_cursor(18.0)) })).id();
    (app, track, if local { manifest_path } else { global_path })
}

fn hour(app: &App, local: bool) -> f32 {
    if local {
        app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap()
            .manifest.environment.simulation.unwrap().hour
    } else {
        app.world().resource::<GlobalEnvironmentAuthoringState>().selected_environment().unwrap()
            .appearance.simulation.hour
    }
}

fn save_while_held(app: &mut App, track: Entity, local: bool) {
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app.update();
    assert_eq!(hour(app, local), 18.0);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    app.world_mut().resource_mut::<SaveAfterControls>().0 = true;
    for cursor in [time_cursor(12.0), time_cursor(18.0)] {
        app.world_mut().get_mut::<RelativeCursorPosition>(track).unwrap().normalized = Some(cursor);
        app.update();
        assert!(!app.world().resource::<EditorAuthoringSession>().saving,
            "按住时的保存请求必须等待原拖动事务松手，不能提前启动IO");
        assert!(app.world().resource::<SceneEditorState>().save_requested);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
    assert_eq!(hour(app, local), 18.0, "等待保存时仍可继续调整同一笔拖动");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.world().resource::<EditorAuthoringSession>().saving {
        app.update();
        assert!(Instant::now() < deadline, "原保存入口没有启动：{}", app.world().resource::<SceneEditorState>().status);
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1,
        "保存快照前必须正式提交一次拖动历史");
    // 刻意把生产完成poll延后一帧，保证下一帧控件确实观察到saving，而非碰巧同帧完成绕过缺陷。
    app.update();
    assert_eq!(hour(app, local), 18.0, "保存中不得把已冻结的拖动值回滚到起点");
    while app.world().resource::<EditorAuthoringSession>().saving {
        app.world_mut().run_system_once(poll_global_environment_save).unwrap();
        app.world_mut().run_system_once(poll_draft_save).unwrap();
        assert!(Instant::now() < deadline, "原保存完成回执超时");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn saved_drag_stays_closed(app: &mut App, local: bool) {
    let undo_count = app.world().resource::<EditorAuthoringSession>().undo.len();
    app.update();
    assert_eq!(hour(app, local), 18.0, "完成保存后不能因旧按压状态重开拖动");
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), undo_count);
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn scene_simulation_save_during_drag_keeps_memory_and_disk_equal() {
    isolated(concat!(module_path!(), "::scene_simulation_save_during_drag_keeps_memory_and_disk_equal"), |root| {
        let (mut app, track, path) = editor(root, true);
        save_while_held(&mut app, track, true);
        let saved: SceneManifestAsset = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved.environment.simulation.unwrap().hour, hour(&app, true));
        assert_eq!(app.world().resource::<EditorAuthoringSession>().successful_save_sequence, 1);
        assert!(!app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().environment_dirty);
        saved_drag_stays_closed(&mut app, true);
    });
}

#[test]
fn profile_simulation_save_during_drag_keeps_memory_and_disk_equal() {
    isolated(concat!(module_path!(), "::profile_simulation_save_during_drag_keeps_memory_and_disk_equal"), |root| {
        let (mut app, track, path) = editor(root, false);
        save_while_held(&mut app, track, false);
        let saved: WorldEnvironmentAsset = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let id = app.world().resource::<GlobalEnvironmentAuthoringState>().selected_environment_id.clone().unwrap();
        assert_eq!(saved.library.profiles[&id].appearance.simulation.hour, hour(&app, false));
        assert_eq!(app.world().resource::<EditorAuthoringSession>().successful_save_sequence, 1);
        assert!(!app.world().resource::<GlobalEnvironmentAuthoringState>().dirty);
        saved_drag_stays_closed(&mut app, false);
    });
}

#[test]
fn failed_profile_simulation_save_during_drag_preserves_the_committed_edit() {
    isolated(concat!(module_path!(), "::failed_profile_simulation_save_during_drag_preserves_the_committed_edit"), |root| {
        let (mut app, track, path) = editor(root, false);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap(); // 自有目录阻挡真实文件发布，让原事务返回IO失败而非伪造结果。
        save_while_held(&mut app, track, false);
        assert_eq!(hour(&app, false), 18.0);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().successful_save_sequence, 0);
        assert!(app.world().resource::<GlobalEnvironmentAuthoringState>().dirty);
        assert!(app.world().resource::<SceneEditorState>().dirty);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
        assert!(path.is_dir());
    });
}

#[test]
fn sparse_save_waits_for_the_environment_drag_without_freezing_the_session() {
    isolated(concat!(module_path!(), "::sparse_save_waits_for_the_environment_drag_without_freezing_the_session"), |root| {
        let (mut app, _, path) = editor(root, true);
        let before = std::fs::read(&path).unwrap();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        // 尚无稀疏作者源的夹具只检查保存入口的互斥边界，不能越过滑块先占用saving。
        app.world_mut().resource_mut::<EditorAuthoringSession>().sparse_terrain_dirty = true;
        app.world_mut().resource_mut::<SaveAfterControls>().0 = true;
        app.update();
        assert!(app.world().resource::<SimulationInputState>().busy());
        assert!(app.world().resource::<SceneEditorState>().save_requested);
        assert!(!app.world().resource::<EditorAuthoringSession>().saving);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().successful_save_sequence, 0);
        assert!(app.world().resource::<DraftSaveTask>().task.is_none());
        assert_eq!(std::fs::read(path).unwrap(), before);
    });
}

#[test]
fn cancelled_or_unchanged_environment_drag_does_not_save_intermediate_values() {
    isolated(concat!(module_path!(), "::cancelled_or_unchanged_environment_drag_does_not_save_intermediate_values"), |root| {
        for local in [true, false] {
            for escape in [true, false] {
                let (mut app, track, path) = editor(root, local);
                let before_hour = hour(&app, local);
                let before_bytes = std::fs::read(&path).unwrap();
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
                app.update();
                app.world_mut().resource_mut::<SaveAfterControls>().0 = true;
                app.update();
                assert!(app.world().resource::<SceneEditorState>().save_requested);
                assert!(!app.world().resource::<EditorAuthoringSession>().saving);
                if escape {
                    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
                } else {
                    app.world_mut().get_mut::<RelativeCursorPosition>(track).unwrap().normalized = Some(time_cursor(before_hour));
                }
                app.update();
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
                // Esc取消后继续按住不能重开旧Pressed；拖回原值仍需等松手才解除保存等待。
                assert_eq!(app.world().resource::<SimulationInputState>().busy(), !escape);
                app.update();
                assert_eq!(hour(&app, local), before_hour);
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
                app.update();
                let session = app.world().resource::<EditorAuthoringSession>();
                assert!(!session.saving && session.undo.is_empty());
                assert_eq!(session.successful_save_sequence, 0);
                assert!(!app.world().resource::<SceneEditorState>().dirty);
                assert!(!app.world().resource::<SceneEditorState>().save_requested);
                assert!(!app.world().resource::<GlobalEnvironmentAuthoringState>().dirty);
                assert!(!session.draft.as_ref().unwrap().environment_dirty);
                assert!(app.world().resource::<DraftSaveTask>().task.is_none());
                assert!(app.world().resource::<GlobalEnvironmentSaveTask>().task.is_none());
                assert_eq!(std::fs::read(path).unwrap(), before_bytes);
            }
        }
    });
}
