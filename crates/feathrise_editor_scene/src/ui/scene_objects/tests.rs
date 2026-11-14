//! 场景对象管理的分帧、版本拒收和保护回归。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::*, systems::*, visibility};
use crate::authoring::{EditorAuthoringSession, EditorCommand, test_heightfield_draft};
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState};
use crate::ui::gltf_browser::EditorGltfBrowserState;
use crate::viewport::content_selection::LayerCandidateState;
use crate::viewport::multi_select::PlacementSelectionSet;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use std::sync::Arc;

fn session(count: usize, group: &str) -> EditorAuthoringSession {
    let mut draft = test_heightfield_draft(Arc::new(vec![2; 121]));
    for index in 0..count {
        let mut record = crate::viewport::placement_move::interaction_tests::record();
        record.id = format!("object-{index}");
        Arc::make_mut(&mut record.editor).group = group.into();
        draft.placements.placements.push(record);
    }
    EditorAuthoringSession {
        draft: Some(draft),
        ..Default::default()
    }
}

fn pending(
    session: &EditorAuthoringSession,
    action: ObjectAction,
    targets: ObjectTargets,
) -> PendingManagement {
    PendingManagement {
        scene_id: session.draft.as_ref().unwrap().scene_id.clone(),
        revision: session.revision,
        structure: session.placement_structure_revision,
        terrain_revision: session.terrain_revision,
        derived_pose_revision: session.placement_derived_pose_revision,
        targets,
        action,
        name: "村口路灯".into(),
        group: "村口".into(),
        flag: true,
        flag_set: false,
        cursor: 0,
        changes: Vec::new(),
        heap_bytes: 0,
        selected: Vec::new(),
    }
}

fn app(count: usize, group: &str) -> App {
    let session = session(count, group);
    let map = session.draft.as_ref().unwrap().scene_id.clone();
    let mut browser = EditorGltfBrowserState::default();
    for (index, record) in session
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .iter()
        .enumerate()
    {
        browser.select_indexed_test_placement(
            &record.id,
            index,
            session.placement_structure_revision,
        );
    }
    browser.selected_placement_id = (count != 0).then(|| "object-0".into());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(session)
        .insert_resource(browser)
        .insert_resource(SceneEditorState {
            open: true,
            selected_scene_id: Some(map),
            ..Default::default()
        })
        .init_resource::<InputFocus>()
        .init_resource::<PlacementSelectionSet>()
        .init_resource::<LayerCandidateState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::viewport::placement_move::PlacementMoveState>()
        .init_resource::<crate::authoring::placement_batch::PlacementBatchState>()
        .init_resource::<ButtonInput<KeyCode>>();
    super::install(&mut app);
    app
}

fn press(app: &mut App, action: ObjectAction) {
    let button = app
        .world_mut()
        .spawn((feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, ObjectButton(action)))
        .id();
    app.update();
    app.world_mut().entity_mut(button).insert(UiInteraction::None);
}

fn finish(app: &mut App) {
    for _ in 0..100 {
        app.update();
        if !app.world().resource::<SceneObjectState>().busy() {
            return;
        }
    }
    panic!("有界管理应完成");
}

#[test]
fn group_capture_is_bounded_and_adopts_one_complete_history() {
    let mut session = session(300, "旧组");
    let mut editor = SceneEditorState::default();
    let original = session.draft.as_ref().unwrap().placements.clone();
    let mut request = pending(
        &session,
        ObjectAction::AssignGroup,
        ObjectTargets::Group("旧组".into()),
    );
    assert!(!advance_management(&mut request, &session).unwrap());
    assert_eq!(request.cursor, 128);
    assert!(adopt_management(&mut request, &mut session, &mut editor).is_err());
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements,
        original.placements
    );
    assert!(!advance_management(&mut request, &session).unwrap());
    assert!(advance_management(&mut request, &session).unwrap());
    assert_eq!(
        adopt_management(&mut request, &mut session, &mut editor)
            .unwrap()
            .0,
        300
    );
    assert_eq!(session.undo.len(), 1);
    assert_eq!(session.scene_objects_revision, 1);
    assert_eq!(session.placement_structure_revision, 0);
    let EditorCommand::SceneObjectsChanged(change) = &session.undo[0] else {
        panic!("管理差分历史")
    };
    let mut restored = session.draft.as_ref().unwrap().placements.clone();
    change.apply(&mut restored, true).unwrap();
    assert_eq!(restored.placements, original.placements);
    change.apply(&mut restored, false).unwrap();
    assert!(
        restored
            .placements
            .iter()
            .all(|record| record.editor.group == "村口")
    );
}

#[test]
fn stale_capture_and_corrupt_history_never_apply_a_prefix() {
    let mut session = session(130, "旧组");
    let mut request = pending(
        &session,
        ObjectAction::ToggleLocked,
        ObjectTargets::Group("旧组".into()),
    );
    assert!(!advance_management(&mut request, &session).unwrap());
    session.revision += 1;
    assert!(advance_management(&mut request, &session).is_err());
    assert!(
        session
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .iter()
            .all(|record| !record.editor.locked)
    );
    let mut request = pending(
        &session,
        ObjectAction::AssignGroup,
        ObjectTargets::Indices(vec![0, 1]),
    );
    assert!(advance_management(&mut request, &session).unwrap());
    let mut editor = SceneEditorState::default();
    adopt_management(&mut request, &mut session, &mut editor).unwrap();
    let EditorCommand::SceneObjectsChanged(change) = &session.undo[0] else {
        panic!()
    };
    let mut placements = session.draft.as_ref().unwrap().placements.clone();
    placements.placements[1].id = "另一对象".into();
    assert!(change.apply(&mut placements, true).is_err());
    assert_eq!(placements.placements[0].editor.group, "村口");
}

#[test]
fn name_changes_preserve_complete_records_and_empty_names_restore_default() {
    let mut session = session(1, "村口");
    let before = session.draft.as_ref().unwrap().placements.placements[0].clone();
    let mut editor = SceneEditorState::default();
    let mut request = pending(
        &session,
        ObjectAction::Rename,
        ObjectTargets::Indices(vec![0]),
    );
    advance_management(&mut request, &session).unwrap();
    adopt_management(&mut request, &mut session, &mut editor).unwrap();
    let mut after = session.draft.as_ref().unwrap().placements.placements[0].clone();
    assert_eq!(after.editor.name, "村口路灯");
    let json = serde_json::to_string(&after).unwrap();
    assert_eq!(
        serde_json::from_str::<feathrise_scene_content::ScenePlacementRecord>(&json).unwrap(),
        after
    );
    after.editor = before.editor.clone();
    assert_eq!(after, before);
    let mut request = pending(
        &session,
        ObjectAction::Rename,
        ObjectTargets::Indices(vec![0]),
    );
    request.name.clear();
    advance_management(&mut request, &session).unwrap();
    adopt_management(&mut request, &mut session, &mut editor).unwrap();
    assert!(
        session.draft.as_ref().unwrap().placements.placements[0]
            .editor
            .name
            .is_empty()
    );
}

#[test]
fn group_toggle_can_restore_all_members_and_selection_skips_protected_objects() {
    let mut session = session(3, "村口");
    let mut editor = SceneEditorState::default();
    for expected in [true, false] {
        let mut request = pending(
            &session,
            ObjectAction::ToggleHidden,
            ObjectTargets::Group("村口".into()),
        );
        advance_management(&mut request, &session).unwrap();
        adopt_management(&mut request, &mut session, &mut editor).unwrap();
        assert!(
            session
                .draft
                .as_ref()
                .unwrap()
                .placements
                .placements
                .iter()
                .all(|r| r.editor.hidden == expected)
        );
    }
    Arc::make_mut(&mut session.draft.as_mut().unwrap().placements.placements[0].editor).locked =
        true;
    Arc::make_mut(&mut session.draft.as_mut().unwrap().placements.placements[1].editor).hidden =
        true;
    let mut request = pending(
        &session,
        ObjectAction::SelectGroup,
        ObjectTargets::Group("村口".into()),
    );
    assert!(advance_management(&mut request, &session).unwrap());
    assert_eq!(request.selected.len(), 1);
    assert_eq!(request.selected[0].id, "object-2");
}

#[test]
fn static_patch_selection_group_uses_world_origin_and_skips_missing_source() {
    use feathrise_scene_content::{ScenePlacementTerrainPatch, SceneTerrainDescriptor, SceneTerrainKind, SceneTerrainObject, TerrainId, TerrainObjectTransform};
    let mut session = session(3, "三维地形组");
    let source = SceneTerrainObject {
        clipmap_stitches: default(), mesh_lod: None,
        id: TerrainId::parse("group-selection-source").unwrap(), display_name: "目录选择夹具".into(), enabled: true,
        transform: TerrainObjectTransform { translation: [41.0, 6.0, -17.0], yaw_degrees: -29.0,
            pitch_degrees: 11.0, roll_degrees: 7.0, scale: [2.0, 3.0, 4.0] },
        world_min: [0.0; 2], world_max: [64.0; 2],
        terrain: SceneTerrainDescriptor { kind: SceneTerrainKind::SparseClipmap, ..SceneTerrainDescriptor::empty() },
        voxel: None, surface_path: None, vegetation_path: None, water_path: None, puddles: None,
    };
    let world = {
        let draft = session.draft.as_mut().unwrap();
        let record = &mut draft.placements.placements[0];
        record.water_surfaces.clear();
        record.terrain_patch = Some(ScenePlacementTerrainPatch { terrain_id: Some(source.id.clone()), ..default() });
        let world = source.transform.as_transform().transform_point(Vec3::from_array(record.translation));
        let invalid = &mut draft.placements.placements[1];
        invalid.water_surfaces.clear();
        invalid.terrain_patch = Some(ScenePlacementTerrainPatch { terrain_id: Some(TerrainId::parse("missing-source").unwrap()), ..default() });
        draft.manifest.terrains = Some(vec![source].into());
        world
    };
    let mut request = pending(&session, ObjectAction::SelectGroup, ObjectTargets::Group("三维地形组".into()));
    assert!(advance_management(&mut request, &session).unwrap());
    assert_eq!(request.selected.len(), 2);
    assert_eq!(request.selected[0].id, "object-0");
    assert!(request.selected[0].position.abs_diff_eq(world, 0.0001));
    assert_eq!(request.selected[1].id, "object-2");
    assert_eq!(request.selected[1].position, Vec3::new(5.0, 4.0, 5.0));
    assert!(session.undo.is_empty());
}

#[test]
fn f12_and_close_button_cancel_unadopted_management_without_dirty_data() {
    for via_key in [true, false] {
        let mut app = app(300, "村口");
        let request = pending(
            app.world().resource::<EditorAuthoringSession>(),
            ObjectAction::ToggleHidden,
            ObjectTargets::Group("村口".into()),
        );
        app.world_mut().resource_mut::<SceneObjectState>().pending = Some(request);
        app.update();
        assert!(app.world().resource::<SceneObjectState>().busy());
        if via_key {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::F12);
        } else {
            app.world_mut().spawn((
                feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,
                crate::components::SceneEditorActionButton(crate::types::EditorAction::Close),
            ));
        }
        app.update();
        assert!(app.world().resource::<SceneEditorState>().close_requested);
        assert!(!app.world().resource::<SceneObjectState>().busy());
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.undo.is_empty());
        assert!(
            session
                .draft
                .as_ref()
                .unwrap()
                .placements
                .placements
                .iter()
                .all(|r| !r.editor.hidden)
        );
    }
}

#[test]
fn hidden_roots_restore_on_close_and_preserve_external_deletion() {
    let mut app = app(2, "村口");
    let records = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .clone();
    let map = records.scene_id.clone();
    let roots: Vec<_> = records
        .placements
        .iter()
        .map(|r| {
            app.world_mut()
                .spawn((
                    feathrise_scene::ScenePlacementInstance::new(&map, r),
                    Visibility::Inherited,
                ))
                .id()
        })
        .collect();
    press(&mut app, ObjectAction::ToggleHidden);
    finish(&mut app);
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(roots[0]).unwrap(),
        Visibility::Hidden
    );
    app.world_mut()
        .entity_mut(roots[0])
        .insert(Visibility::Hidden); // 正式删除/退役仍拥有隐藏意图。
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .request_close();
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(roots[0]).unwrap(),
        Visibility::Hidden
    );
    assert_eq!(
        *app.world().get::<Visibility>(roots[1]).unwrap(),
        Visibility::Inherited
    );
    assert!(
        app.world()
            .resource::<visibility::ObjectVisibilityState>()
            .restored()
    );
}

#[test]
fn locking_keeps_tree_management_available_but_blocks_transform_targets() {
    let mut app = app(1, "村口");
    press(&mut app, ObjectAction::ToggleLocked);
    finish(&mut app);
    assert!(
        crate::ui::placement_transform::selected_target(
            app.world().resource::<EditorGltfBrowserState>(),
            app.world().resource::<EditorAuthoringSession>()
        )
        .is_none()
    );
    assert_eq!(
        app.world()
            .resource::<EditorGltfBrowserState>()
            .selected_placement_id
            .as_deref(),
        Some("object-0")
    );
    press(&mut app, ObjectAction::ToggleLocked);
    finish(&mut app);
    assert!(
        crate::ui::placement_transform::selected_target(
            app.world().resource::<EditorGltfBrowserState>(),
            app.world().resource::<EditorAuthoringSession>()
        )
        .is_some()
    );
}

#[test]
fn closing_restores_at_most_64_roots_each_frame_and_reopening_reapplies_flags() {
    let mut app = app(130, "村口");
    let (map, records) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        for record in &mut session.draft.as_mut().unwrap().placements.placements {
            Arc::make_mut(&mut record.editor).hidden = true;
        }
        let draft = session.draft.as_ref().unwrap();
        (draft.scene_id.clone(), draft.placements.placements.clone())
    };
    let roots: Vec<_> = records
        .iter()
        .map(|r| {
            app.world_mut()
                .spawn((
                    feathrise_scene::ScenePlacementInstance::new(&map, r),
                    Visibility::Inherited,
                ))
                .id()
        })
        .collect();
    for _ in 0..4 {
        app.update();
    }
    assert!(
        roots
            .iter()
            .all(|&root| *app.world().get::<Visibility>(root).unwrap() == Visibility::Hidden)
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .request_close();
    app.update();
    assert_eq!(
        roots
            .iter()
            .filter(|&&root| *app.world().get::<Visibility>(root).unwrap() == Visibility::Inherited)
            .count(),
        64
    );
    assert!(
        !app.world()
            .resource::<visibility::ObjectVisibilityState>()
            .restored()
    );
    app.update();
    app.update();
    assert!(
        app.world()
            .resource::<visibility::ObjectVisibilityState>()
            .restored()
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .close_requested = false;
    for _ in 0..4 {
        app.update();
    }
    assert!(
        roots
            .iter()
            .all(|&root| *app.world().get::<Visibility>(root).unwrap() == Visibility::Hidden)
    );
}

#[test]
fn metadata_is_editor_only_and_default_is_selectable() {
    let metadata = feathrise_scene_content::ScenePlacementEditorMetadata::default();
    assert!(metadata.selectable());
    assert!(metadata.is_empty());
}

#[test]
fn scene_context_menu_metadata_uses_only_the_frozen_object_and_one_reversible_history() {
    use crate::ui::scene_context_menu::{SceneContextMenuState, SceneMenuAction, SceneObjectKey, SceneObjectRequest, SceneObjectTarget};
    for action in [SceneMenuAction::Rename, SceneMenuAction::Visibility, SceneMenuAction::Lock] {
        let mut app = app(3, "原分组");
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        let original = draft.placements.clone();
        let mut menu = SceneContextMenuState::default();
        menu.ready = Some(SceneObjectRequest { target: SceneObjectTarget { scene_id: draft.scene_id.clone(),
            revision: session.revision, key: SceneObjectKey::Placement { index: 2, id: "object-2".into(), mesh: None },
            name: String::new(), hidden: false, locked: false, copyable: true }, action, name: "村口 铜灯".into() });
        app.insert_resource(menu);
        app.update();
        app.world_mut().resource_mut::<SceneContextMenuState>().ready = None;
        finish(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        let records = &session.draft.as_ref().unwrap().placements.placements;
        assert_eq!(&records[..2], &original.placements[..2]);
        match action {
            SceneMenuAction::Rename => assert_eq!(records[2].editor.name, "村口 铜灯"),
            SceneMenuAction::Visibility => assert!(records[2].editor.hidden),
            SceneMenuAction::Lock => assert!(records[2].editor.locked),
            _ => unreachable!(),
        }
        assert_eq!(session.undo.len(), 1);
        let EditorCommand::SceneObjectsChanged(change) = &session.undo[0] else { panic!("应复用对象元数据历史"); };
        let mut restored = session.draft.as_ref().unwrap().placements.clone();
        change.apply(&mut restored, true).unwrap(); assert_eq!(restored.placements, original.placements);
        change.apply(&mut restored, false).unwrap(); assert_eq!(restored.placements, *records);
    }
}

/// 真实贴地入口及活动根共享给选组拒收与元数据成功两侧，避免用手改Y代替派生采用。
fn following_group_app(count: usize, group: &str) -> (App, Vec<Entity>) {
    use feathrise_scene_content::ScenePlacementHeightMode;

    let mut app = app(count, group);
    app.add_plugins(bevy::asset::AssetPlugin::default())
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .init_resource::<crate::authoring::TerrainBrushStroke>()
        .init_resource::<crate::authoring::WaterBrushStroke>()
        .init_resource::<crate::authoring::PlacementTerrainFollowState>()
        .add_observer(crate::ui::placement_water::register_placement_instance_root)
        .add_observer(crate::ui::placement_water::unregister_placement_instance_root);
    let (map, records) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let terrain = crate::authoring::test_support::test_clipmap_draft(Arc::new(vec![2;121]), session.terrain_revision);
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.terrain = terrain.manifest.terrain;
        draft.manifest.terrains = terrain.manifest.terrains;
        draft.terrain_ground = terrain.terrain_ground; // 贴地专用夹具使用已采用Clipmap真实三角面，不借GLB内存字段冒充来源。
        for (index, record) in session.draft.as_mut().unwrap().placements.placements.iter_mut().enumerate() {
            record.height_mode = ScenePlacementHeightMode::FollowTerrain;
            record.terrain_offset = 1.0 + index as f32 * 0.001;
            record.water_surfaces.clear();
        }
        let draft = session.draft.as_ref().unwrap();
        (draft.scene_id.clone(), draft.placements.placements.clone())
    };
    let roots: Vec<_> = records.iter().map(|record| app.world_mut().spawn((
        Transform::from_translation(record.translation.into()), Visibility::Inherited,
        feathrise_scene::ScenePlacementInstance::new(&map, record),
    )).id()).collect();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        for (record, root) in records.iter().zip(&roots) {
            session.placement_previews.insert(record.id.clone(), *root);
        }
    }
    (app, roots)
}

/// 只推进正式后台贴地的一批采用，保留130项测试中128+2的真实边界，不执行对象管理帧。
fn follow_one_batch(app: &mut App) {
    use bevy::ecs::system::RunSystemOnce;
    let expected = app.world().resource::<EditorAuthoringSession>().placement_derived_pose_revision.wrapping_add(1);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        app.world_mut().run_system_once(crate::authoring::reconcile_following_placements).unwrap();
        let revision = app.world().resource::<EditorAuthoringSession>().placement_derived_pose_revision;
        if revision == expected { break; }
        assert!(std::time::Instant::now() < deadline, "真实贴地worker未按批次采用");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn select_group_rejects_cached_world_anchors_after_real_derived_follow_between_capture_frames() {
    use crate::viewport::multi_select::SelectedMember;

    // 130项迫使真实SelectGroup跨越128项预算；旧选集保持一项，便于证明失败没有发布新组。
    let (mut app, roots) = following_group_app(130, "迟到贴地组");
    let (records, revisions, derived_revision) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        (session.draft.as_ref().unwrap().placements.placements.clone(),
            (session.revision, session.placement_structure_revision, session.terrain_revision),
            session.placement_derived_pose_revision)
    };
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        let record = &records[129];
        selection.publish(vec![SelectedMember { id: record.id.clone(), index: 129,
            position: record.translation.into() }], world.resource::<EditorAuthoringSession>());
    });
    let (old_members, old_center, old_selection_revision) = {
        let selection = app.world().resource::<PlacementSelectionSet>();
        (selection.members.clone(), selection.center, selection.revision)
    };
    {
        let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
        browser.selected_group = Some("迟到贴地组".into());
        browser.selected_placement_id = None;
    }
    press(&mut app, ObjectAction::SelectGroup);
    app.update();
    {
        let state = app.world().resource::<SceneObjectState>();
        let pending = state.pending.as_ref().expect("真实目录按钮应进入分帧SelectGroup");
        assert_eq!(pending.cursor, 128);
        assert_eq!(pending.selected.len(), 128);
        assert!(pending.selected.iter().all(|member| member.position.y == 4.0));
    }

    // 不手改Y或修订；同一高度地形的迟到派生结果由正式后台分两次采用，各次仍不超过128项。
    follow_one_batch(&mut app);
    {
        let session = app.world().resource::<EditorAuthoringSession>();
        let current = &session.draft.as_ref().unwrap().placements.placements;
        assert_eq!(current.iter().filter(|record| record.translation[1] != 4.0).count(), 128);
        assert_eq!(session.placement_derived_pose_revision, derived_revision.wrapping_add(1));
        assert_eq!((session.revision, session.placement_structure_revision, session.terrain_revision), revisions);
    }
    follow_one_batch(&mut app);
    let derived = {
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.placement_derived_pose_revision, derived_revision.wrapping_add(2));
        assert_eq!((session.revision, session.placement_structure_revision, session.terrain_revision), revisions);
        let current = session.draft.as_ref().unwrap().placements.placements.clone();
        for (record, root) in current.iter().zip(&roots) {
            assert_eq!(record.translation[1], 2.0 + record.terrain_offset);
            assert_eq!(app.world().get::<Transform>(*root).unwrap().translation.y, record.translation[1]);
        }
        current
    };
    app.update();
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert!(Arc::ptr_eq(&selection.members, &old_members),
        "不能把首批128项旧Y与末批2项新Y发布成盖有新派生代次的完整组选择");
    assert_eq!(selection.center, old_center);
    assert_eq!(selection.revision, old_selection_revision);
    assert!(!app.world().resource::<SceneObjectState>().busy(), "派生换代应取消旧捕获");
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, derived);
    assert_eq!((session.revision, session.placement_structure_revision, session.terrain_revision), revisions);
    assert!(session.undo.is_empty());
    assert!(session.redo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn select_group_rejects_independent_terrain_revision_between_production_capture_frames() {
    use crate::viewport::multi_select::SelectedMember;

    let mut app = app(130, "地形换代组");
    let original = app.world().resource::<EditorAuthoringSession>()
        .draft.as_ref().unwrap().placements.clone();
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        let record = &original.placements[129];
        selection.publish(vec![SelectedMember { id: record.id.clone(), index: 129,
            position: record.translation.into() }], world.resource::<EditorAuthoringSession>());
    });
    let (old_members, old_center, old_revision, old_ticket) = {
        let selection = app.world().resource::<PlacementSelectionSet>();
        (selection.members.clone(), selection.center, selection.revision,
            (selection.scene_id.clone(), selection.author_revision, selection.structure_revision,
                selection.terrain_revision, selection.derived_pose_revision, selection.ready))
    };
    {
        let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
        browser.selected_group = Some("地形换代组".into());
        browser.selected_placement_id = None;
    }
    press(&mut app, ObjectAction::SelectGroup);
    app.update();
    {
        let pending = app.world().resource::<SceneObjectState>().pending.as_ref().unwrap();
        assert_eq!(pending.cursor, 128);
        assert_eq!(pending.selected.len(), 128);
    }
    // 独立推进源地形身份，刻意保持作者、结构与迟到贴地身份不变，隔离地形判代门。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.terrain_revision = session.terrain_revision.wrapping_add(1);
        assert_eq!((session.revision, session.placement_structure_revision,
            session.placement_derived_pose_revision), (old_ticket.1, old_ticket.2, old_ticket.4));
    }
    app.update();
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert!(Arc::ptr_eq(&selection.members, &old_members));
    assert_eq!(selection.center, old_center);
    assert_eq!(selection.revision, old_revision);
    assert_eq!((selection.scene_id.clone(), selection.author_revision, selection.structure_revision,
        selection.terrain_revision, selection.derived_pose_revision, selection.ready), old_ticket);
    assert!(!app.world().resource::<SceneObjectState>().busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, original.placements);
    assert!(session.undo.is_empty());
    assert!(session.redo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn group_metadata_completes_one_reversible_history_after_real_derived_follow_between_capture_frames() {
    let (mut app, roots) = following_group_app(130, "贴地元数据组");
    let (author_revision, structure_revision, terrain_revision, derived_revision) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        (session.revision, session.placement_structure_revision, session.terrain_revision,
            session.placement_derived_pose_revision)
    };
    {
        let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
        browser.selected_group = Some("贴地元数据组".into());
        browser.selected_placement_id = None;
    }
    press(&mut app, ObjectAction::ToggleLocked);
    app.update();
    {
        let pending = app.world().resource::<SceneObjectState>().pending.as_ref().unwrap();
        assert_eq!(pending.cursor, 128);
        assert_eq!(pending.changes.len(), 128);
    }
    // 同一批真实派生采用会使选组过期，但锁定只捕获元数据Arc，应继续提交且保留新姿态。
    follow_one_batch(&mut app);
    follow_one_batch(&mut app);
    let derived = {
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.placement_derived_pose_revision, derived_revision.wrapping_add(2));
        assert_eq!((session.revision, session.placement_structure_revision, session.terrain_revision),
            (author_revision, structure_revision, terrain_revision));
        let records = session.draft.as_ref().unwrap().placements.clone();
        for (record, root) in records.placements.iter().zip(&roots) {
            assert_eq!(record.translation[1], 2.0 + record.terrain_offset);
            assert_eq!(app.world().get::<Transform>(*root).unwrap().translation.y, record.translation[1]);
            assert!(!record.editor.locked, "尚未完整收集时不能应用元数据前缀");
        }
        records
    };
    finish(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let records = &session.draft.as_ref().unwrap().placements.placements;
    assert!(records.iter().all(|record| record.editor.locked));
    for (record, before) in records.iter().zip(&derived.placements) {
        let mut without_metadata_change = record.clone();
        without_metadata_change.editor = before.editor.clone();
        assert_eq!(&without_metadata_change, before, "元数据事务必须保留完整派生后记录");
    }
    assert_eq!(session.undo.len(), 1);
    assert!(session.redo.is_empty());
    assert_eq!(session.revision, author_revision.wrapping_add(1));
    assert_eq!(session.placement_structure_revision, structure_revision);
    assert_eq!(session.terrain_revision, terrain_revision);
    assert_eq!(session.placement_derived_pose_revision, derived_revision.wrapping_add(2));
    assert!(app.world().resource::<SceneEditorState>().dirty);
    let EditorCommand::SceneObjectsChanged(change) = &session.undo[0] else {
        panic!("跨帧元数据管理应只生成一条完整历史");
    };
    assert_eq!(change.changes.len(), 130);
    let mut restored = session.draft.as_ref().unwrap().placements.clone();
    change.apply(&mut restored, true).unwrap();
    assert_eq!(restored.placements, derived.placements);
    change.apply(&mut restored, false).unwrap();
    assert_eq!(restored.placements, *records);
}
