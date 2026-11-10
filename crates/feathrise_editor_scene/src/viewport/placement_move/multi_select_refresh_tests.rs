//! 结构历史使多选收缩时恢复完整单体目标，同时保留原有同实例精确网格选择。

use super::*;
use crate::ui::gltf_browser::EditorGltfMeshSelection;

#[test]
fn multi_selection_refresh_after_undo_add_restores_single_properties_and_handle() {
    let (mut app, [first, _], node, _) = group_app();
    undo_resources(&mut app);
    box_systems(&mut app);
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
        .insert_resource(feathrise_scene_content::ActiveDynamicScene { scene_id: Some(map) })
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<crate::authoring::WaterNetworkEditState>()
        .init_resource::<crate::ui::surface_preset_editor::SurfacePresetEditorState>()
        .init_resource::<crate::ui::water_preset_editor::WaterPresetEditorState>()
        .init_resource::<crate::ui::building_presets::BuildingPresetEditorState>()
        .init_resource::<crate::game_authoring::scene_interaction::SceneInteractionState>()
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .add_message::<feathrise_editor_ui::NumericInputSetValue>()
        .add_systems(Update, crate::ui::gltf_browser::refresh_gltf_browser_cache.before(advance_multi_selection))
        .add_systems(Update, crate::ui::placement_transform::sync_placement_transform_panel.after(advance_multi_selection));
    let panel = app.world_mut().spawn((
        crate::ui::placement_transform::SceneEditorPlacementTransformPanel,
        Node { display: Display::None, ..default() },
    )).id(); // 使用正式属性面板系统判断完整实例是否恢复，不能只断言内部选集长度。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let added = session.draft.as_ref().unwrap().placements.placements[1].clone();
        session.undo.push(EditorCommand::PlacementAdded(added));
    } // 夹具中的第二项来自最后一次新增；后续通过真实Ctrl+Z和统一历史移除它。
    let p0 = camera(&app).project(Vec3::from_array(record().translation)).unwrap();
    let p1 = camera(&app).project(Vec3::new(7.0, 6.0, 5.0)).unwrap();
    let min = p0.min(p1) - Vec2::splat(0.02);
    let max = p0.max(p1) + Vec2::splat(0.02);
    step(&mut app, node, min); // 完整采用当前浏览器结构签名。
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut app, node, min);
    step(&mut app, node, max);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    step(&mut app, node, max);
    step(&mut app, node, max); // 静默帧归还框选鼠标，Ctrl+Z才有正式输入资格。
    assert_eq!(app.world().resource::<PlacementSelectionSet>().members.len(), 2);
    assert!(app.world().resource::<EditorGltfBrowserState>().selected_placement_id.is_none());
    assert_eq!(app.world().get::<Node>(panel).unwrap().display, Display::None);
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyZ);
    }
    app.world_mut().run_system_once(crate::ui::actions::handle_editor_shortcuts).unwrap();
    assert!(app.world().resource::<SceneEditorState>().undo_requested);
    crate::authoring::drive_undo_redo_to_completion(&mut app);
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::ControlLeft);
        keys.release(KeyCode::KeyZ);
        keys.clear();
    }
    for _ in 0..4 {
        step(&mut app, node, max); // 原浏览器索引和选集复核各自走正式有界更新。
        let selection = app.world().resource::<PlacementSelectionSet>();
        if selection.ready_for(app.world().resource::<EditorAuthoringSession>()) {
            break;
        }
    }
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert!(selection.ready_for(app.world().resource::<EditorAuthoringSession>()));
    assert_eq!(selection.members.len(), 1);
    assert_eq!(selection.members[0].id, record().id);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().redo.len(), 1);
    assert_eq!(
        app.world().resource::<EditorGltfBrowserState>().selected_placement_id.as_deref(),
        Some(record().id.as_str()),
        "撤销新增后，剩余单项必须重新成为完整资产属性与手柄的正式目标",
    );
    assert_eq!(app.world().get::<Node>(panel).unwrap().display, Display::Flex);
    assert_eq!(
        app.world().resource::<LayerCandidateState>().selected,
        Some(LayerTarget::Placement(record().id)),
    );
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
    app.world_mut().run_system_once(crate::ui::actions::handle_editor_shortcuts).unwrap();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyW);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Move);
    let start = point_on_axis(&app, MoveAxis::X, 0.0);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut app, node, start);
    assert!(app.world().resource::<PlacementMoveState>().drag.is_some(), "剩余单项可立即从真实手柄开始移动");
    let moved = point_on_axis(&app, MoveAxis::X, 1.0);
    step(&mut app, node, moved);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    step(&mut app, node, moved);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements.len(), 1);
    let after = &session.draft.as_ref().unwrap().placements.placements[0];
    assert!((after.translation[0] - record().translation[0] - 1.0).abs() < 0.002);
    assert_eq!(session.undo.len(), 1, "后续移动仍是一条独立历史");
    assert!(session.redo.is_empty());
    assert!((app.world().get::<Transform>(first).unwrap().translation.x - after.translation[0]).abs() < 0.002);
}

#[test]
fn multi_selection_refresh_keeps_same_single_instance_precise_mesh_selection() {
    let (mut app, _, _) = app();
    app.init_resource::<PlacementSelectionSet>();
    let precise = EditorGltfMeshSelection {
        placement_id: record().id,
        node_path: vec!["树根".into(), "叶片".into()],
        mesh_name: "叶片网格".into(),
        mesh_index: Some(1),
        valid: true,
        orphaned_water_binding: false,
    };
    app.world_mut().resource_mut::<EditorGltfBrowserState>().selection = Some(precise.clone());
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        selection.publish(vec![SelectedMember {
            id: record().id,
            index: 0,
            position: Vec3::from_array(record().translation),
        }], world.resource::<EditorAuthoringSession>());
    });
    app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1;
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    assert!(app.world().resource::<PlacementSelectionSet>().ready_for(app.world().resource::<EditorAuthoringSession>()));
    assert_eq!(app.world().resource::<EditorGltfBrowserState>().selected_placement_id.as_deref(), Some(record().id.as_str()));
    assert_eq!(app.world().resource::<EditorGltfBrowserState>().selection, Some(precise), "同ID精确子网格不能被普通作者修订复核清成整实例");
}

/// 同一地形修订下仍未执行完的贴地任务：旧世界Y合法但已过时，必须由真实派生入口纠正。
fn derived_follow_app(count: usize) -> (App, Vec<Entity>, Entity, Entity) {
    assert!(count >= 2);
    let (mut app, existing, node, parent) = group_app();
    app.init_asset::<bevy::world_serialization::WorldAsset>()
        .init_resource::<crate::authoring::TerrainBrushStroke>()
        .init_resource::<crate::authoring::WaterBrushStroke>()
        .init_resource::<crate::authoring::PlacementTerrainFollowState>();
    let (map, mut records) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        (draft.scene_id.clone(), draft.placements.placements.clone())
    };
    for index in records.len()..count {
        let mut next = record();
        next.id = format!("derived-follow-{index}");
        next.translation = [1.0 + (index % 24) as f32 * 0.3, 9.0, 5.0];
        records.push(next);
    }
    for (index, record) in records.iter_mut().enumerate() {
        record.height_mode = ScenePlacementHeightMode::FollowTerrain;
        record.terrain_offset = 1.0 + index as f32 * 0.25;
        record.water_surfaces.clear();
    }
    let mut roots = existing.to_vec();
    for (index, record) in records.iter().enumerate() {
        let transform = Transform::from_translation(record.translation.into())
            .with_rotation(Quat::from_rotation_y(record.yaw_degrees.to_radians()))
            .with_scale(record.scale.into());
        if index < roots.len() {
            app.world_mut().entity_mut(roots[index]).insert((transform, ScenePlacementInstance::new(&map, record)));
        } else {
            roots.push(app.world_mut().spawn((transform, Visibility::Inherited,
                ScenePlacementInstance::new(&map, record), ChildOf(parent))).id());
        }
    }
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.placements.placements = records.clone();
        draft.placement_water_surface_count = 0;
        for (record, root) in records.iter().zip(&roots) {
            session.placement_previews.insert(record.id.clone(), *root);
        }
    }
    {
        let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
        for (index, record) in records.iter().enumerate() {
            browser.select_indexed_test_placement(&record.id, index, 3);
        }
        browser.selected_placement_id = None;
    }
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        selection.publish(records.iter().enumerate().map(|(index, record)| SelectedMember {
            id: record.id.clone(), index, position: record.translation.into(),
        }).collect(), world.resource::<EditorAuthoringSession>());
    });
    (app, roots, node, parent)
}

/// 不伪造修订或直接改草稿Y：真实贴地任务必须更新记录和现存根，同时保持作者历史不变。
fn apply_real_derived_follow(app: &mut App, roots: &[Entity]) -> Vec<ScenePlacementRecord> {
    let before = {
        let session = app.world().resource::<EditorAuthoringSession>();
        (session.revision, session.placement_structure_revision, session.terrain_revision,
            session.placement_derived_pose_revision)
    };
    app.world_mut().run_system_once(crate::authoring::reconcile_following_placements).unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!((session.revision, session.placement_structure_revision, session.terrain_revision),
        (before.0, before.1, before.2),
        "派生采用不能伪造作者编辑、结构或地形修订");
    assert_eq!(session.placement_derived_pose_revision, before.3.wrapping_add(1),
        "同一有界批次的实际姿态采用只发布一次派生代次");
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    let records = session.draft.as_ref().unwrap().placements.placements.clone();
    for (index, (record, root)) in records.iter().zip(roots).enumerate() {
        assert_eq!(record.translation[1], 3.0 + index as f32 * 0.25);
        assert_eq!(record.terrain_offset, 1.0 + index as f32 * 0.25);
        assert_eq!(app.world().get::<Transform>(*root).unwrap().translation.y, record.translation[1]);
    }
    records
}

fn assert_group_rejects_late_derived_follow(stage: usize) {
    let (mut app, roots, node, parent) = derived_follow_app(2);
    let start = group_point(&app, MoveAxis::X, 0.0);
    let cursor = group_point(&app, MoveAxis::X, 1.0);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut app, node, start);
    assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
        .is_some_and(|group| group.phase == GroupPhase::Preparing));
    if stage > 0 {
        step(&mut app, node, start);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
            .is_some_and(|group| group.phase == GroupPhase::Dragging));
    }
    if stage == 2 {
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
            .is_some_and(|group| group.phase == GroupPhase::Waiting));
    }
    // CPU结果可在后台完成，但必须在下一次真实手柄输入采用前先消费派生姿态换代。
    let derived = apply_real_derived_follow(&mut app, &roots);
    step(&mut app, node, if stage == 0 { start } else { cursor });
    assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
        .is_some_and(|group| group.phase == GroupPhase::Finishing),
        "派生贴地迟到后必须取消旧整组手势：stage={stage}");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    finish(&mut app, node, cursor);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, derived,
        "旧整组结果不能覆盖已采用的派生姿态或生成新的离地偏移：stage={stage}");
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    for (root, record) in roots.iter().zip(&derived) {
        assert_eq!(app.world().get::<ChildOf>(*root).map(ChildOf::parent), Some(parent));
        assert_eq!(app.world().get::<Transform>(*root).unwrap().translation.to_array(), record.translation);
    }
}

#[test]
fn derived_follow_cancels_group_preparing_before_old_anchor_is_used() {
    assert_group_rejects_late_derived_follow(0);
}

#[test]
fn derived_follow_cancels_group_dragging_without_overwriting_current_pose() {
    assert_group_rejects_late_derived_follow(1);
}

#[test]
fn derived_follow_cancels_group_waiting_before_worker_result_is_adopted() {
    assert_group_rejects_late_derived_follow(2);
}

#[test]
fn derived_follow_refreshes_idle_selection_in_two_bounded_batches() {
    let (mut app, roots, _, _) = derived_follow_app(17);
    let old_members = app.world().resource::<PlacementSelectionSet>().members.clone();
    let old_center = app.world().resource::<PlacementSelectionSet>().center;
    let derived = apply_real_derived_follow(&mut app, &roots);
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    {
        let selection = app.world().resource::<PlacementSelectionSet>();
        assert!(!selection.ready_for(app.world().resource::<EditorAuthoringSession>()),
            "17项选集不能越过每帧16项复核预算，也不能沿用贴地前的Ready信用");
        assert_eq!(selection.center, old_center, "完整复核前保留原锚点，不发布部分成员中心");
        assert!(Arc::ptr_eq(&selection.members, &old_members));
    }
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert!(selection.ready_for(app.world().resource::<EditorAuthoringSession>()));
    assert_eq!(selection.members.len(), derived.len());
    for (member, record) in selection.members.iter().zip(&derived) {
        assert_eq!(member.id, record.id);
        assert_eq!(member.position.to_array(), record.translation,
            "派生采用后，同一作者修订也必须刷新已选对象的实际世界原点");
    }
    assert_eq!(selection.center.y, 5.0);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn derived_follow_noop_and_completed_frames_do_not_advance_pose_revision() {
    let (mut app, roots, _, _) = derived_follow_app(2);
    apply_real_derived_follow(&mut app, &roots);
    let revision = app.world().resource::<EditorAuthoringSession>().placement_derived_pose_revision;
    for restart in [false, true] {
        if restart {
            app.insert_resource(crate::authoring::PlacementTerrainFollowState::default());
        } // 显式重新走真实采样，区分已完成游标的稳定帧与采样后无变化的批次。
        app.world_mut().run_system_once(crate::authoring::reconcile_following_placements).unwrap();
        assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_derived_pose_revision, revision);
    }
}

#[test]
fn derived_follow_partial_failure_still_revokes_old_pose_credit() {
    let (mut app, _, _, _) = derived_follow_app(2);
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
        .placements.placements[1].terrain_offset = f32::NAN;
    app.world_mut().run_system_once(crate::authoring::reconcile_following_placements).unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.placements.placements[0].translation[1], 3.0,
        "后项失败前，真实入口已经采用首项的派生高度");
    assert_eq!(session.placement_derived_pose_revision, 1);
    assert!(!app.world().resource::<PlacementSelectionSet>().ready_for(session));
    assert!(matches!(app.world().resource::<crate::authoring::PlacementTerrainFollowState>()
        .readiness_for(&draft.scene_id, session.terrain_revision, session.placement_structure_revision, 2),
        crate::authoring::PlacementGroundingReadiness::Failed(_)));
    assert_eq!(session.revision, 7);
    assert_eq!(session.terrain_revision, 2);
    assert_eq!(session.placement_structure_revision, 3);
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn derived_follow_object_worker_publishes_binding_pose_once_and_noop_keeps_revision() {
    use crate::authoring::{PlacementGroundingReadiness, PlacementTerrainFollowState, terrain_ground::TerrainGroundSnapshot};
    use feathrise_scene_content::{SceneTerrainBinding, TerrainId, TerrainObjectTransform};
    let (mut app, roots, _, _) = derived_follow_app(2);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let terrain_revision = session.terrain_revision;
        let draft = session.draft.as_mut().unwrap();
        let id = TerrainId::parse("derived-worker-source").unwrap();
        let pose = TerrainObjectTransform {
            translation: [-1.0, 2.0, 3.0], yaw_degrees: 15.0, scale: [1.0, 1.5, 1.0], ..default()
        };
        let mut objects = draft.manifest.terrain_objects_for_edit().to_vec();
        objects[0].id = id.clone();
        objects[0].transform = pose;
        draft.manifest.terrains = Some(objects.into());
        let mut ground = TerrainGroundSnapshot::test_mesh(draft.scene_id.clone(), id.clone(), pose,
            avian3d::prelude::Collider::cuboid(20.0, 1.0, 20.0));
        Arc::get_mut(&mut ground).unwrap().revision = terrain_revision;
        draft.terrain_ground = Some(ground);
        for record in &mut draft.placements.placements {
            record.height_mode = ScenePlacementHeightMode::FixedHeight;
            record.terrain_offset = 0.0;
            record.terrain_binding = Some(SceneTerrainBinding::capture(id.clone(), default(), TerrainObjectTransform {
                translation: record.translation, yaw_degrees: record.yaw_degrees, scale: record.scale, ..default()
            }).unwrap());
        }
    }
    let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
    for restart in [false, true] {
        if restart { app.insert_resource(PlacementTerrainFollowState::default()); }
        app.world_mut().run_system_once(crate::authoring::reconcile_following_placements).unwrap();
        assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_derived_pose_revision, u64::from(restart),
            "提交后台查询尚未采用时不能预先换代");
        let mut completed = false;
        for _ in 0..500 {
            app.world_mut().run_system_once(crate::authoring::reconcile_following_placements).unwrap();
            let session = app.world().resource::<EditorAuthoringSession>();
            let draft = session.draft.as_ref().unwrap();
            if matches!(app.world().resource::<PlacementTerrainFollowState>()
                .readiness_for(&draft.scene_id, session.terrain_revision, session.placement_structure_revision, 2),
                PlacementGroundingReadiness::Ready) { completed = true; break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(completed, "真实 object_follow 后台结果应完整采用");
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.placement_derived_pose_revision, 1,
            "绑定派生改变平移、旋转和缩放只发布一次；重新查询相同姿态不重复换代");
        for ((record, old), root) in session.draft.as_ref().unwrap().placements.placements.iter().zip(&before).zip(&roots) {
            assert!((record.translation[1] - (2.0 + old.translation[1] * 1.5)).abs() < 0.0001);
            assert!((record.yaw_degrees - old.yaw_degrees - 15.0).abs() < 0.0001);
            assert!(Vec3::from_array(record.scale).abs_diff_eq(Vec3::from_array(old.scale) * Vec3::new(1.0, 1.5, 1.0), 0.0001));
            assert!(app.world().get::<Transform>(*root).unwrap().translation.abs_diff_eq(record.translation.into(), 0.0001));
        }
        assert_eq!(session.revision, 7);
        assert!(session.undo.is_empty());
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
}

fn derived_selection_pointer(app: &mut App, node: Entity, point: Vec2) {
    app.world_mut().get_mut::<bevy::ui::RelativeCursorPosition>(node).unwrap().normalized = Some(point);
    app.world_mut().run_system_once(handle_multi_select_pointer).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
}

#[test]
fn derived_follow_cancels_box_gesture_before_old_projection_can_publish() {
    let (mut app, roots, node, _) = derived_follow_app(2);
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    app.world_mut().resource_mut::<SceneEditorState>().placement_transform_enabled = false;
    let old = app.world().resource::<PlacementSelectionSet>().members.clone();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    derived_selection_pointer(&mut app, node, Vec2::splat(-0.2));
    derived_selection_pointer(&mut app, node, Vec2::splat(0.2));
    assert!(app.world().resource::<PlacementSelectionSet>().box_busy());
    apply_real_derived_follow(&mut app, &roots);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    derived_selection_pointer(&mut app, node, Vec2::splat(0.2));
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert!(!selection.box_busy(), "贴地迟到后松手不能再建立旧投影的框选扫描");
    assert!(Arc::ptr_eq(&old, &selection.members));
    assert!(!selection.ready_for(app.world().resource::<EditorAuthoringSession>()));
}

#[test]
fn derived_follow_discards_incomplete_box_scan_without_publishing_mixed_anchors() {
    let (mut app, _, node, _) = derived_follow_app(129);
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    app.world_mut().resource_mut::<SceneEditorState>().placement_transform_enabled = false;
    let old = app.world().resource::<PlacementSelectionSet>().members.clone();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    derived_selection_pointer(&mut app, node, Vec2::splat(-0.3));
    derived_selection_pointer(&mut app, node, Vec2::splat(0.3));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    derived_selection_pointer(&mut app, node, Vec2::splat(0.3));
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    assert!(app.world().resource::<PlacementSelectionSet>().box_busy(),
        "129条真实候选不能在128条/帧的扫描预算内完成");
    app.world_mut().run_system_once(crate::authoring::reconcile_following_placements).unwrap();
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_derived_pose_revision, 1);
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert!(!selection.box_busy());
    assert!(Arc::ptr_eq(&old, &selection.members), "旧扫描不能把两代世界锚点混合发布为新选集");
    assert!(!selection.ready_for(app.world().resource::<EditorAuthoringSession>()));
}

#[test]
fn derived_follow_restarts_partial_selection_refresh_after_another_adoption() {
    let (mut app, roots, _, _) = derived_follow_app(17);
    apply_real_derived_follow(&mut app, &roots);
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    let old = app.world().resource::<PlacementSelectionSet>().members.clone();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        for record in &mut session.draft.as_mut().unwrap().placements.placements {
            record.translation[1] += 7.0;
        }
    } // 同一来源后续批次再次纠正当前世界Y；不手改任何修订，代次仍由真实派生入口发布。
    app.insert_resource(crate::authoring::PlacementTerrainFollowState::default());
    apply_real_derived_follow(&mut app, &roots);
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    {
        let selection = app.world().resource::<PlacementSelectionSet>();
        assert!(!selection.ready_for(app.world().resource::<EditorAuthoringSession>()),
            "新派生代次必须重启16项/帧复核，不能给旧16项前缀补一个新后缀就发布");
        assert!(Arc::ptr_eq(&old, &selection.members));
    }
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    assert!(app.world().resource::<PlacementSelectionSet>().ready_for(app.world().resource::<EditorAuthoringSession>()));
}
