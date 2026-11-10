//! 正式关闭驱动的保存阶段回归：具体预检原因可见，重复帧不增长，完成后继续原关闭门。
use super::*;

fn closing_app(save_requested: bool, saving: bool, status: &str) -> App {
    let mut app = App::new();
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
        .insert_resource(feathrise_scene_content::ActiveDynamicScene { scene_id: Some(SceneId::parse("MAP-close").unwrap()) })
        .insert_resource(SceneEditorState {
            open: true,
            close_requested: true,
            save_requested,
            status: status.into(),
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            saving,
            ..default()
        })
        .init_resource::<TerrainBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<TerrainLiveApplyState>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<WaterFileHandoff>()
        .init_resource::<BoundaryEditState>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<SceneCreationTask>()
        .init_resource::<SceneDeletionTask>()
        .add_systems(Update, finish_scene_editor_close);
    app
}

#[test]
fn queued_save_close_preserves_current_preflight_reason_and_keeps_repeated_frames_stable() {
    let reason = "建筑保存预检正在等待当前地形来源重新采用";
    let mut app = closing_app(true, false, reason);
    let expected = format!("{SAVE_CLOSE_WAITING_PREFIX}{reason}{SAVE_CLOSE_SUFFIX}");
    for _ in 0..8 {
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.open && editor.close_requested && editor.save_requested);
        assert!(!app.world().resource::<EditorAuthoringSession>().saving);
        assert_eq!(editor.status, expected);
        assert_eq!(editor.status.matches(SAVE_CLOSE_SUFFIX).count(), 1);
    }
    // 下一帧保存Tasks推进到另一前置条件，关闭驱动必须展示新原因而非保留旧包装。
    let next_reason = "当前GLB地形修订仍在后台生成高度、法线和Trimesh，完成后自动继续保存";
    app.world_mut().resource_mut::<SceneEditorState>().status = next_reason.into();
    app.update();
    assert_eq!(
        app.world().resource::<SceneEditorState>().status,
        format!("{SAVE_CLOSE_WAITING_PREFIX}{next_reason}{SAVE_CLOSE_SUFFIX}")
    );
}

#[test]
fn inflight_save_close_has_a_distinct_stage_and_continues_only_after_completion() {
    let reason = "正在后台原子保存稀疏高度块和版本化清单";
    let mut app = closing_app(false, true, reason);
    for _ in 0..8 {
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.open && editor.close_requested);
        assert!(!editor.save_requested);
        assert!(app.world().resource::<EditorAuthoringSession>().saving);
        assert_eq!(
            editor.status,
            format!("{SAVE_CLOSE_ACTIVE_PREFIX}{reason}{SAVE_CLOSE_SUFFIX}")
        );
    }
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = false;
    app.world_mut().resource_mut::<SceneEditorState>().status = "场景保存成功".into();
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(!editor.open && !editor.close_requested && !editor.save_requested);
    assert_eq!(editor.status, "场景保存成功");
}

#[test]
fn save_close_fallback_replaces_generic_notice_and_cancel_keeps_save_intent() {
    for saving in [false, true] {
        let mut app = closing_app(!saving, saving, "");
        // request_close在输入阶段只能知道关闭意图；首次回显必须解释真实保存阶段。
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .request_close();
        app.update();
        let first = app.world().resource::<SceneEditorState>().status.clone();
        assert!(first.starts_with(if saving {
            SAVE_CLOSE_ACTIVE_PREFIX
        } else {
            SAVE_CLOSE_WAITING_PREFIX
        }));
        assert!(!first.contains("正在确认未保存地形画面"));
        for _ in 0..8 {
            app.update();
            assert_eq!(app.world().resource::<SceneEditorState>().status, first);
        }
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .toggle_close_request();
        app.update();
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.open && !editor.close_requested);
        assert_eq!(editor.save_requested, !saving);
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().saving,
            saving
        );
    }
}
