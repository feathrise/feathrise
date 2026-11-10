//! 单体手柄消费正式采用身份：真实系统输入跨候选完成时取消，候选进度仍允许原手势完成。

use super::*;
use bevy::ecs::system::RunSystemOnce;
use feathrise_scene_content::{ScenePlacementTerrainPatch, SceneTerrainKind};
use feathrise_scene::{ScenePlacementInstance, ScenePlacementRuntimePhase, ScenePlacementRuntimeStatus};

fn identity_app(legacy_patch: bool) -> (App, Entity, Entity) {
    let (mut app, root, node) = app();
    app.init_asset::<bevy::world_serialization::WorldAsset>();
    let (map, record, bounds) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        if legacy_patch {
            draft.manifest.terrains = None;
            draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
            draft.placements.placements[0].terrain_patch = Some(ScenePlacementTerrainPatch::default());
            draft.placements.placements[0].water_surfaces.clear();
            draft.placement_water_surface_count = 0;
        }
        (draft.scene_id.clone(), draft.placements.placements[0].clone(), draft.manifest.world_bounds().unwrap())
    };
    app.world_mut().entity_mut(root).insert((record.validated_transform(&map, bounds).unwrap(),
        ScenePlacementInstance::new(&map, &record)));
    app.insert_resource(ScenePlacementRuntimeStatus {
        scene_id: Some(map), phase: ScenePlacementRuntimePhase::Building,
        revision: 17, placement_count: 1, ..default()
    }); // 保存IO已完成，旧正式根仍活动，候选采用尚未推进公开revision。
    (app, root, node)
}

fn input(app: &mut App, node: Entity, cursor: Vec2) {
    app.world_mut().get_mut::<RelativeCursorPosition>(node).unwrap().normalized = Some(cursor);
    app.world_mut().run_system_once(handle_placement_move).unwrap();
    app.world_mut().run_system_once(update_scene_editor_numeric_scrub_mouse_guard).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
}

fn press(app: &mut App, node: Entity) {
    let start = point_on_axis(app, MoveAxis::X, 0.0);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    input(app, node, start);
    assert!(app.world().resource::<PlacementMoveState>().drag.is_some());
}

fn assert_unchanged(app: &App, before: &ScenePlacementRecord, revision: u64) {
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(draft_record(app), before);
    assert_eq!(session.revision, revision);
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn single_runtime_identity_change_cancels_before_preview_or_release_commit() {
    for legacy_patch in [false, true] {
        // 单体没有异步Preparing阶段：分别覆盖起手未预览、拖动中、松手采用前的身份边界。
        for stage in 0..3 {
            for change in 0..4 {
                let (mut app, root, node) = identity_app(legacy_patch);
                let before = draft_record(&app).clone();
                let original = *app.world().get::<Transform>(root).unwrap();
                let revision = app.world().resource::<EditorAuthoringSession>().revision;
                press(&mut app, node);
                let cursor = point_on_axis(&app, MoveAxis::X, 1.0);
                if stage > 0 {
                    input(&mut app, node, cursor);
                    assert!(app.world().resource::<PlacementMoveState>().drag.as_ref().unwrap().preview_applied);
                }
                let membership = app.world().resource::<PlacementInstanceIndex>().membership_revision();
                if change == 3 {
                    app.world_mut().remove_resource::<ScenePlacementRuntimeStatus>();
                } else {
                    let mut status = app.world_mut().resource_mut::<ScenePlacementRuntimeStatus>();
                    match change {
                        0 => status.revision += 1,
                        1 => status.scene_id = None,
                        2 => status.placement_count = 0,
                        _ => unreachable!(),
                    }
                }
                assert_eq!(app.world().resource::<PlacementInstanceIndex>().membership_revision(), membership);
                if stage == 2 {
                    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
                }
                input(&mut app, node, cursor);
                assert!(app.world().resource::<PlacementMoveState>().drag.is_none(),
                    "正式采用身份变化应取消旧单体手势：legacy={legacy_patch}, stage={stage}, change={change}");
                assert_unchanged(&app, &before, revision);
                assert_eq!(app.world().get::<Transform>(root), Some(&original));
                assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
                // 身份恢复或鼠标继续按住不能重放旧按下；只有释放后的新边沿可以重新开始。
                input(&mut app, node, cursor);
                assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
                input(&mut app, node, cursor);
                input(&mut app, node, cursor);
                assert!(!app.world().resource::<PlacementMoveState>().owns_mouse());
                press(&mut app, node);
            }
        }
    }
}

#[test]
fn single_runtime_candidate_progress_keeps_one_drag_and_one_history() {
    for legacy_patch in [false, true] {
        for change in 0..4 {
            let (mut app, root, node) = identity_app(legacy_patch);
            let before = draft_record(&app).clone();
            let revision = app.world().resource::<EditorAuthoringSession>().revision;
            press(&mut app, node);
            {
                let mut status = app.world_mut().resource_mut::<ScenePlacementRuntimeStatus>();
                match change {
                    0 => status.progress_revision += 1,
                    1 => status.phase = ScenePlacementRuntimePhase::Ready,
                    2 => { status.attempt_revision += 1; status.phase = ScenePlacementRuntimePhase::Failed; }
                    3 => status.detail = "仍在生成同一候选".into(),
                    _ => unreachable!(),
                }
            }
            let cursor = point_on_axis(&app, MoveAxis::X, 1.0);
            input(&mut app, node, cursor);
            assert!(app.world().resource::<PlacementMoveState>().drag.is_some());
            assert_unchanged(&app, &before, revision);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            input(&mut app, node, cursor);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
            assert_eq!(session.undo.len(), 1, "legacy={legacy_patch}, change={change}");
            assert!(session.revision > revision);
            assert!((draft_record(&app).translation[0] - before.translation[0] - 1.0).abs() < 0.002);
            assert!((app.world().get::<Transform>(root).unwrap().translation.x - before.translation[0] - 1.0).abs() < 0.002);
        }
    }
}

#[test]
fn single_runtime_adoption_cancels_instead_of_retargeting_new_formal_root() {
    for legacy_patch in [false, true] {
        let (mut app, old, node) = identity_app(legacy_patch);
        app.add_observer(crate::ui::placement_water::register_placement_instance_root)
            .add_observer(crate::ui::placement_water::unregister_placement_instance_root);
        let before = draft_record(&app).clone();
        let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
        let original = *app.world().get::<Transform>(old).unwrap();
        let revision = app.world().resource::<EditorAuthoringSession>().revision;
        app.world_mut().resource_mut::<EditorAuthoringSession>().placement_previews.clear();
        app.world_mut().entity_mut(old).remove::<ScenePlacementInstance>().insert(ScenePlacementInstance::new(&map, &before));
        press(&mut app, node);
        let cursor = point_on_axis(&app, MoveAxis::X, 1.0);
        input(&mut app, node, cursor);
        assert!(app.world().resource::<PlacementMoveState>().drag.as_ref().unwrap().preview_applied);
        let preview = app.world().resource::<EditorAuthoringSession>().placement_previews[&before.id];
        assert_ne!(preview, old, "拖动应取得独立作者根，不能改写旧正式根");
        assert_eq!(app.world().get::<Transform>(old), Some(&original));
        let preview_version = app.world().get::<ScenePlacementInstance>(preview).unwrap().authoring_version();
        let preview_parent = app.world().get::<ChildOf>(preview).map(ChildOf::parent);
        if legacy_patch {
            // 只模拟保存批次精确清理后的实体输入，不把它冒充正式私有共享门的真实采用测试。
            assert_eq!(app.world_mut().resource_mut::<EditorAuthoringSession>().placement_previews.remove(&before.id), Some(preview));
            app.world_mut().despawn(preview);
        }
        app.world_mut().entity_mut(old).remove::<ScenePlacementInstance>().insert(Visibility::Hidden);
        let old_pose = *app.world().get::<Transform>(old).unwrap();
        let formal_parent = app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited)).id();
        let formal = app.world_mut().spawn((original, Visibility::Inherited, ChildOf(formal_parent),
            ScenePlacementInstance::new(&map, &before))).id();
        let formal_version = app.world().get::<ScenePlacementInstance>(formal).unwrap().authoring_version();
        app.world_mut().resource_mut::<ScenePlacementRuntimeStatus>().revision += 1;
        let cursor = point_on_axis(&app, MoveAxis::X, 2.0);
        input(&mut app, node, cursor);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        input(&mut app, node, cursor);
        assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
        assert_unchanged(&app, &before, revision);
        assert!(app.world().resource::<EditorAuthoringSession>().redo.is_empty());
        assert_eq!(app.world().get::<Transform>(formal), Some(&original), "新正式根不能收到旧preview");
        assert_eq!(app.world().get::<ChildOf>(formal).unwrap().parent(), formal_parent);
        let formal_instance = app.world().get::<ScenePlacementInstance>(formal).unwrap();
        assert_eq!(formal_instance.authoring_version(), formal_version);
        assert!(formal_instance.matches_record(&map, &before, &before.water_surfaces));
        assert_eq!(app.world().get::<Transform>(old), Some(&old_pose));
        if legacy_patch {
            assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Inherited));
            assert!(app.world().resource::<EditorAuthoringSession>().placement_previews.is_empty(),
                "已清理作者根时，取消不能补生旧手势模型");
            assert!(app.world().get_entity(preview).is_err());
        } else {
            // 【需求设计】正式采用只取消手势；尚存独立作者根回到当前草稿，继续遮蔽新正式壳。
            assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_previews[&before.id], preview);
            assert_ne!(preview, formal);
            assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden));
            let suppression = app.world().get::<crate::authoring::placement_preview_ownership::components::PlacementPreviewSuppression>(formal).unwrap();
            assert_eq!(suppression.owner, preview);
            assert_eq!(suppression.original_visibility, Visibility::Inherited);
            assert!(app.world().get::<crate::components::EditorPlacedAsset>(preview).is_some());
            assert_eq!(app.world().get::<Transform>(preview), Some(&original));
            assert_eq!(app.world().get::<Visibility>(preview), Some(&Visibility::Inherited));
            assert_eq!(app.world().get::<ChildOf>(preview).map(ChildOf::parent), preview_parent);
            let instance = app.world().get::<ScenePlacementInstance>(preview).unwrap();
            assert!(instance.is_active());
            assert!(instance.matches_record(&map, &before, &before.water_surfaces));
            assert_ne!(instance.authoring_version(), formal_version);
            assert_ne!(instance.authoring_version(), preview_version,
                "取消后根应证明恢复的权威记录，不能沿用拖动候选身份");
        }
    }
}
