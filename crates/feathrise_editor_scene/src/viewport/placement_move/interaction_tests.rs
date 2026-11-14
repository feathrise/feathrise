//! 无窗口但运行实际输入、根同步和公共鼠标门；检查边沿、身份与一次撤销。

use super::{
    PlacementMoveState, handle_placement_move,
    math::MoveCamera,
    types::{MoveAxis, MoveHandle},
};
use crate::authoring::buildings::BuildingEditRequest;
use crate::authoring::{
    EditorAuthoringSession, EditorCommand, PlacementTerrainSampler, TerrainExternalChangeState,
};
use crate::authoring::test_support::{refresh_test_clipmap_ground, test_clipmap_draft};
use crate::components::SceneEditorViewport;
use crate::resources::*;
use crate::types::EditorView;
use crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard;
use crate::ui::{
    building_presets::BuildingPlacementControls, courtyard::CourtyardUiState,
    gltf_browser::EditorGltfBrowserState, placement_transform::PlacementTransformInputState,
    placement_water::PlacementInstanceIndex, text_input::EditorTextInputGuard,
    vegetation_preset_editor::VegetationPresetEditorState,
};
use crate::viewport::{
    LayerCandidateState, LayerTarget, camera_math::orbit_angles_from_view, modes::ViewportModeState,
};
use bevy::{asset::AssetPlugin, prelude::*, ui::RelativeCursorPosition};
use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub};
use feathrise_scene_content::{
    ScenePlacementHeightMode, ScenePlacementRecord, ScenePlacementWaterKind, ScenePlacementWaterSurface,
};
use std::sync::Arc;

#[test]
fn embedded_placement_preview_rejected_world_candidate_keeps_last_applied_pose_and_gesture() {
    use crate::standalone::embedded_placement_preview as bridge;
    use feathrise_scene_content::embedded_placement_preview::PlacementPreviewCommand as Command;
    use std::time::{Duration, Instant};

    fn take(app: &mut App) -> Option<Command> {
        let value = bridge::peek(app.world())?;
        let command = serde_json::from_slice(&value.bytes).unwrap();
        assert!(bridge::acknowledge(app.world_mut(), value.sequence));
        Some(command)
    }
    fn wait_x(app: &mut App, x: f32) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            app.update();
            match take(app) {
                Some(Command::Update { transform, .. }) if (transform[12] - x).abs() < 0.002 => return,
                Some(Command::End { .. }) => panic!("失败候选不能提前结束原手势"),
                _ => (),
            }
            assert!(Instant::now() < deadline, "已采用的原手柄姿态未投影");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    let (mut app, root, node) = app();
    bridge::install(&mut app);
    {
        let mut state = app.world_mut().resource_mut::<bridge::EmbeddedPlacementPreview>();
        state.enabled = true;
        state.session = 1;
        state.generation = 1;
    }
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    let start = point_on_axis(&app, MoveAxis::X, 0.0);
    step(&mut app, node, start);
    let accepted = point_on_axis(&app, MoveAxis::X, 0.5);
    step(&mut app, node, accepted);
    wait_x(&mut app, 5.5);

    // 故障输入只污染候选：仍执行真实输入→World公共校验失败分支，不伪造预览成功或IPC结果。
    app.world_mut().resource_mut::<PlacementMoveState>().drag.as_mut().unwrap().preview.scale = [0.0; 3];
    let rejected = point_on_axis(&app, MoveAxis::X, 1.0);
    step(&mut app, node, rejected);
    assert!(app.world().resource::<SceneEditorState>().status.contains("变换预览失败"));
    assert!(!app.world().resource::<PlacementMoveState>().drag.as_ref().unwrap().valid);
    assert!((app.world().get::<Transform>(root).unwrap().translation.x - 5.5).abs() < 0.002);
    for _ in 0..8 {
        app.update();
        if let Some(command) = take(&mut app) {
            assert!(matches!(command, Command::Update { transform, .. }
                if (transform[12] - 5.5).abs() < 0.002 && Mat4::from_cols_array(&transform).determinant() > 0.0));
        }
        assert!(app.world().resource::<bridge::EmbeddedPlacementPreview>().error.is_none());
    }
    let mut moving = app.world_mut().resource_mut::<PlacementMoveState>();
    let drag = moving.drag.as_mut().unwrap();
    drag.preview.scale = drag.before.scale;
    drop(moving);
    let recovered = point_on_axis(&app, MoveAxis::X, 1.5);
    step(&mut app, node, recovered);
    wait_x(&mut app, 6.5);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    step(&mut app, node, recovered);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert_eq!(draft_record(&app).translation, record().translation);
}

#[test]
fn orientation_and_shading_roots_block_the_same_frame_pointer_before_mode_cache_updates() {
    use bevy::ecs::system::RunSystemOnce;
    let (mut app, _, _) = app();
    let pointer_over_ui = |context: super::input::MoveInputContext| context.pointer_over_ui();
    assert!(!app.world_mut().run_system_once(pointer_over_ui).unwrap());
    for orientation in [true, false] {
        let mut root = app.world_mut().spawn(RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        });
        if orientation {
            root.insert(crate::components::SceneEditorOrientationAxisRoot);
        } else {
            root.insert(crate::viewport::shading::ViewShadingRoot);
        }
        let root = root.id();
        // modes缓存尚未刷新时，控件本身也必须阻止按下穿透到对象或元素手柄。
        assert!(!app.world().resource::<ViewportModeState>().pointer_over_controls);
        assert!(app.world_mut().run_system_once(pointer_over_ui).unwrap());
        app.world_mut().despawn(root);
        assert!(!app.world_mut().run_system_once(pointer_over_ui).unwrap());
    }
}

#[path = "multi_select_interaction_tests.rs"]
mod multi_select;

#[path = "terrain_patch_interaction_tests.rs"]
mod terrain_patch;

#[path = "cancel_ownership_tests.rs"]
mod cancel_ownership;

#[path = "runtime_identity_tests.rs"]
mod runtime_identity;

pub(crate) fn record() -> ScenePlacementRecord {
    ScenePlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        id: "move-test".into(),
        asset_path: "gltf/test.glb".into(),
        asset_target: default(),
        building_preset_id: None,
        courtyard: None,
        terrain_patch: None,
        translation: [5.0, 4.0, 5.0],
        height_mode: ScenePlacementHeightMode::FixedHeight,
        terrain_offset: 0.0,
        yaw_degrees: 37.0,
        scale: [1.1, 1.2, 1.3],
        water_surfaces: vec![ScenePlacementWaterSurface {
            mesh_index: 0,
            node_path: vec!["水面".into()],
            mesh_name: "water".into(),
            kind: ScenePlacementWaterKind::Sloped,
            flow_speed: 1.0,
            height_offset: 0.0,
        }],
    }
}

pub(crate) fn replace_ground_heights(app: &mut App, heights: Option<Arc<Vec<u16>>>) {
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let revision = session.terrain_revision;
    let draft = session.draft.as_mut().unwrap();
    draft.heights = heights;
    refresh_test_clipmap_ground(draft, revision);
}

pub(crate) fn app() -> (App, Entity, Entity) {
    let mut draft = test_clipmap_draft(Arc::new(vec![2; 121]), 2);
    let record = record();
    draft.placements.placements.push(record.clone());
    draft.placement_water_surface_count = 1;
    let map = draft.scene_id.clone();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(SceneEditorState {
            open: true,
            selected_scene_id: Some(map.clone()),
            placement_transform_enabled: true,
            view: EditorView::Front,
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            placement_structure_revision: 3,
            terrain_revision: 2,
            ..default()
        })
        .init_resource::<PlacementMoveState>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<LayerCandidateState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<ViewportModeState>()
        .init_resource::<PlacementTransformInputState>()
        .init_resource::<CourtyardUiState>()
        .init_resource::<SceneEditorSettingsState>()
        .init_resource::<SceneBrowserState>()
        .init_resource::<SceneBrowserDragState>()
        .init_resource::<TerrainExternalChangeState>()
        .init_resource::<SceneDeletionTask>()
        .init_resource::<SceneEditorLayoutState>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<ActiveNumericInputScrub>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<BuildingPlacementControls>()
        .init_resource::<PlacementInstanceIndex>()
        .add_message::<BuildingEditRequest>()
        .add_systems(
            Update,
            (
                handle_placement_move,
                update_scene_editor_numeric_scrub_mouse_guard,
            )
                .chain(),
        );
    app.world_mut().resource_mut::<ViewportModeState>().mode =
        crate::viewport::modes::ViewportMode::Move;
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .select_indexed_test_placement(&record.id, 0, 3);
    let (yaw, pitch) = orbit_angles_from_view(EditorView::Front);
    app.insert_resource(SceneEditorViewportState {
        _image: Handle::default(),
        camera: Entity::PLACEHOLDER,
        center: Vec3::from_array(record.translation),
        scene_center: Vec3::new(5.0, 0.0, 5.0),
        scene_half_extents: Vec3::new(5.0, 20.0, 5.0),
        extent: 40.0,
        yaw,
        pitch,
        framed_scene_id: Some(map.clone()),
        render_size: UVec2::new(800, 600),
    });
    let root = app
        .world_mut()
        .spawn((
            Transform::from_translation(Vec3::from_array(record.translation)),
            Visibility::Inherited,
            // 正式作者预览始终携带稳定身份，夹具也必须接受公共同步入口的真实所有权复核。
            feathrise_scene::ScenePlacementInstance::new(&map, &record),
        ))
        .id();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .placement_previews
        .insert(record.id, root);
    let node = app
        .world_mut()
        .spawn((
            SceneEditorViewport,
            ComputedNode::default(),
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
        ))
        .id();
    (app, root, node)
}

pub(crate) fn camera(app: &App) -> MoveCamera {
    MoveCamera::new(
        app.world().resource::<SceneEditorState>(),
        app.world().resource::<SceneEditorViewportState>(),
        1.0,
    )
    .unwrap()
}

fn point_on_axis(app: &App, axis: MoveAxis, delta: f32) -> Vec2 {
    let camera = camera(app);
    let origin = Vec3::from_array(record().translation);
    camera
        .project(origin + axis.direction() * (camera.handle_length(origin).unwrap() * 0.65 + delta))
        .unwrap()
}

pub(crate) fn step(app: &mut App, node: Entity, cursor: Vec2) {
    app.world_mut()
        .entity_mut(node)
        .get_mut::<RelativeCursorPosition>()
        .unwrap()
        .normalized = Some(cursor);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}

#[cfg(test)]
pub(crate) fn batch_undo_resources(app: &mut App) {
    multi_select::undo_resources(app);
}

pub(crate) fn batch_top_camera(app: &mut App) {
    plane_tests::set_camera(app, EditorView::Top, false);
}

/// 进程实时投影回归复用已验证的原多选夹具与输入入口，不构造假的拖动状态。
pub(crate) fn batch_preview_fixture() -> (App, [Entity; 2], Entity, Entity) {
    multi_select::group_app()
}

pub(crate) fn batch_preview_begin(app: &mut App, node: Entity, axis: MoveAxis) {
    multi_select::group_begin(app, node, axis);
}

fn begin(app: &mut App, node: Entity, axis: MoveAxis) {
    let cursor = point_on_axis(app, axis, 0.0);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(app, node, cursor);
    assert!(app.world().resource::<PlacementMoveState>().drag.is_some());
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

fn draft_record(app: &App) -> &ScenePlacementRecord {
    &app.world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements[0]
}

#[test]
fn mesh_lod_restore_gate_rejects_old_press_but_keeps_document_properties_available() {
    fn document_properties_remain_available(
        editor: Res<SceneEditorState>, context: super::input::MoveInputContext,
    ) {
        assert!(context.document_available(&editor));
    }
    for (active, ready) in [(false, true), (true, false)] {
        let (mut app, _, node) = app();
        app.insert_resource(feathrise_scene::MeshLodAuthoringState { active })
            .insert_resource(feathrise_scene::MeshLodRuntimeStatus { authoring_ready: ready, ..default() })
            .add_systems(Update, document_properties_remain_available);
        let cursor = point_on_axis(&app, MoveAxis::X, 0.0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        app.world_mut().resource_mut::<feathrise_scene::MeshLodAuthoringState>().active = true;
        app.world_mut().resource_mut::<feathrise_scene::MeshLodRuntimeStatus>().authoring_ready = true;
        step(&mut app, node, cursor);
        assert!(app.world().resource::<PlacementMoveState>().drag.is_none()); // 恢复后旧按下不得补启动手柄。
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut app, node, cursor);
        begin(&mut app, node, MoveAxis::X); // 完整恢复后的新点击正常使用同一生产输入链。
    }
}

#[test]
fn drag_previews_only_root_and_release_commits_one_complete_undo() {
    let (mut app, root, node) = app();
    let before = draft_record(&app).clone();
    begin(&mut app, node, MoveAxis::X);
    for delta in [0.5, 1.0, 1.37] {
        let cursor = point_on_axis(&app, MoveAxis::X, delta);
        step(&mut app, node, cursor);
        assert_eq!(draft_record(&app), &before);
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
        assert!(!app.world().resource::<SceneEditorState>().dirty);
        assert!(
            (app.world().get::<Transform>(root).unwrap().translation.x - (5.0 + delta)).abs()
                < 0.002
        );
    }
    let cursor = point_on_axis(&app, MoveAxis::X, 1.37);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, cursor);
    let after = draft_record(&app);
    assert!((after.translation[0] - 6.37).abs() < 0.002);
    assert_eq!(after.water_surfaces, before.water_surfaces);
    assert_eq!(after.scale, before.scale);
    assert_eq!(after.yaw_degrees, before.yaw_degrees);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, 8);
    assert_eq!(session.undo.len(), 1);
    assert!(
        matches!(&session.undo[0], EditorCommand::PlacementChanged { before: old, after: new, .. } if old == &before && new == after)
    );
    assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
    step(&mut app, node, cursor);
    assert!(!app.world().resource::<PlacementMoveState>().owns_mouse());
    assert!(
        !app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

#[test]
fn persistent_relative_snap_uses_the_frozen_origin_and_records_one_history() {
    let (mut app, root, node) = app();
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().placements.placements[0].translation[0] += 0.3;
    app.world_mut().get_mut::<Transform>(root).unwrap().translation.x += 0.3;
    app.world_mut().resource_mut::<SceneEditorViewportState>().center.x += 0.3;
    app.insert_resource(feathrise_editor_core::EditorSettings { editor_snapping: feathrise_editor_core::EditorSnapSettings { enabled: true, mode: feathrise_editor_core::EditorSnapMode::Relative, ..default() }, ..default() });
    let before = draft_record(&app).clone();
    let camera = camera(&app);
    let origin = Vec3::from_array(before.translation);
    let grabbed = origin + Vec3::X * (camera.handle_length(origin).unwrap() * 0.65);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut app, node, camera.project(grabbed).unwrap());
    assert!(app.world().resource::<PlacementMoveState>().drag.is_some());
    app.world_mut().resource_mut::<feathrise_editor_core::EditorSettings>().editor_snapping = default(); // 中途修改偏好不能重解释已取得所有权的手势。
    let cursor = camera.project(grabbed + Vec3::X * 0.7).unwrap();
    step(&mut app, node, cursor);
    assert!((app.world().get::<Transform>(root).unwrap().translation.x - origin.x - 1.0).abs() < 0.002);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    step(&mut app, node, cursor);
    assert!((draft_record(&app).translation[0] - before.translation[0] - 1.0).abs() < 0.002);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    assert!(matches!(&app.world().resource::<EditorAuthoringSession>().undo[0], EditorCommand::PlacementChanged { before: old, .. } if old == &before));
}

#[test]
fn escape_and_chord_restore_without_dirty_or_undo_and_wait_for_quiet() {
    for (escape, building) in [(true, false), (false, false), (true, true), (false, true)] {
        let (mut app, root, node) = app();
        if building {
            app.world_mut()
                .resource_mut::<EditorAuthoringSession>()
                .draft
                .as_mut()
                .unwrap()
                .placements
                .placements[0]
                .building_preset_id = Some("test-building".into());
        }
        begin(&mut app, node, MoveAxis::X);
        let cursor = point_on_axis(&app, MoveAxis::X, 2.0);
        step(&mut app, node, cursor);
        assert!((app.world().get::<Transform>(root).unwrap().translation.x - 7.0).abs() < 0.002);
        if escape {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
        } else {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Right);
        }
        step(&mut app, node, cursor);
        assert_eq!(
            app.world().get::<Transform>(root).unwrap().translation,
            Vec3::from_array(record().translation)
        );
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
        assert!(!app.world().resource::<SceneEditorState>().dirty);
        assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
        if !escape {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Right);
            step(&mut app, node, cursor);
        }
        assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
        step(&mut app, node, cursor);
        assert!(!app.world().resource::<PlacementMoveState>().owns_mouse());
    }
}

#[test]
fn no_motion_and_return_to_origin_preserve_revision_history_and_redo() {
    for (moved, building) in [(false, false), (true, false), (false, true), (true, true)] {
        let (mut app, root, node) = app();
        if building {
            app.world_mut()
                .resource_mut::<EditorAuthoringSession>()
                .draft
                .as_mut()
                .unwrap()
                .placements
                .placements[0]
                .building_preset_id = Some("test-building".into());
        }
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .draft
            .as_mut()
            .unwrap()
            .placements
            .placements[0]
            .translation[0] = 5.25;
        app.world_mut()
            .entity_mut(root)
            .get_mut::<Transform>()
            .unwrap()
            .translation
            .x = 5.25;
        let before = draft_record(&app).clone(); // 非网格原点必须在 Ctrl 单击及回起点时保持原样。
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .redo
            .push(EditorCommand::PlacementAdded(record()));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        begin(&mut app, node, MoveAxis::X);
        if moved {
            let cursor = point_on_axis(&app, MoveAxis::X, 1.6);
            step(&mut app, node, cursor);
        }
        let cursor = point_on_axis(&app, MoveAxis::X, 0.0);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        step(&mut app, node, cursor);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, 7);
        assert!(session.undo.is_empty());
        assert_eq!(session.redo.len(), 1);
        assert_eq!(draft_record(&app), &before);
        assert_eq!(
            app.world().get::<Transform>(root).unwrap().translation,
            Vec3::from_array(before.translation)
        );
    }
}

#[test]
fn superseding_edit_restores_current_author_value_instead_of_old_snapshot() {
    let (mut app, root, node) = app();
    begin(&mut app, node, MoveAxis::X);
    let cursor = point_on_axis(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().placements.placements[0].translation[0] = 8.0;
        session.revision += 1;
    }
    step(&mut app, node, cursor);
    assert_eq!(
        app.world().get::<Transform>(root).unwrap().translation.x,
        8.0
    );
    assert_eq!(draft_record(&app).translation[0], 8.0);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn building_drag_previews_real_root_and_rejected_request_restores_unchanged_draft() {
    let (mut app, root, node) = app();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements[0]
        .building_preset_id = Some("test-building".into());
    begin(&mut app, node, MoveAxis::X);
    let cursor = point_on_axis(&app, MoveAxis::X, 1.6);
    step(&mut app, node, cursor);
    assert_eq!(draft_record(&app).translation, record().translation);
    assert_eq!(
        app.world().get::<Transform>(root).unwrap().translation,
        Vec3::new(7.0, 4.0, 5.0)
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, cursor);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    assert_eq!(
        app.world().get::<Transform>(root).unwrap().translation,
        Vec3::new(7.0, 4.0, 5.0)
    ); // 松手帧保持模型预览，等待原校验入口受理。
    let mut cursor = bevy::ecs::message::MessageCursor::<BuildingEditRequest>::default();
    let requests = cursor
        .read(app.world().resource::<Messages<BuildingEditRequest>>())
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 1);
    assert!(
        matches!(requests[0], BuildingEditRequest::Relocate { target, world_xz, terrain_hit: None } if target.author_revision == 7 && world_xz.distance(Vec2::new(7.0, 5.0)) < 0.002)
    );
    let cursor = point_on_axis(&app, MoveAxis::X, 1.6);
    step(&mut app, node, cursor); // 夹具未安装建筑校验槽，相当于请求未受理，必须恢复。
    assert_eq!(
        app.world().get::<Transform>(root).unwrap().translation,
        Vec3::from_array(record().translation)
    );
    assert_eq!(draft_record(&app).translation, record().translation);
    assert!(
        app.world()
            .resource::<PlacementMoveState>()
            .pending_building
            .is_none()
    );
}

#[test]
fn building_preview_waits_without_snapback_and_real_validation_accepts_or_restores() {
    use crate::authoring::buildings::move_test_support::{
        ValidationPollingEnabled, install_existing_edit_validator,
    };
    use feathrise_scene_content::BuildingPresetDescriptor;
    for (handle, delta, accepted, formal_root) in
        [MoveHandle::Axis(MoveAxis::X), MoveHandle::XzPlane]
            .into_iter()
            .flat_map(|handle| {
                [
                    (1.6, true, false),
                    (10.0, false, false),
                    (1.6, true, true),
                    (10.0, false, true),
                ]
                .map(|(delta, accepted, formal_root)| (handle, delta, accepted, formal_root))
            })
    {
        let (mut app, root, node) = app();
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
            record.building_preset_id = Some("test-building".into());
            record.yaw_degrees = 0.0;
            record.scale = [1.0; 3];
        }
        let before = draft_record(&app).clone();
        if formal_root {
            app.add_observer(crate::ui::placement_water::register_placement_instance_root);
            let map = {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                session.placement_previews.clear();
                session.draft.as_ref().unwrap().scene_id.clone()
            };
            app.world_mut()
                .entity_mut(root)
                // 基础夹具已带作者身份，正式根须重新添加组件触发Add观察者登记真实活动索引。
                .remove::<feathrise_scene::ScenePlacementInstance>()
                .insert(feathrise_scene::ScenePlacementInstance::new(&map, &before));
        } // 已保存场景走活动根索引，新放置建筑走作者根；二者都必须实时移动而非额外补一个模型。
        install_existing_edit_validator(
            &mut app,
            BuildingPresetDescriptor::from_asset(
                "test-building".into(),
                "移动测试建筑".into(),
                before.asset_path.clone(),
                before.asset_target,
            ),
        );
        let cursor = if handle == MoveHandle::XzPlane {
            plane_tests::set_camera(&mut app, EditorView::Top, false);
            plane_tests::begin_plane(&mut app, node);
            plane_tests::point_on_plane(&app, Vec2::new(delta, -1.4))
        } else {
            begin(&mut app, node, MoveAxis::X);
            point_on_axis(&app, MoveAxis::X, delta)
        };
        step(&mut app, node, cursor);
        let candidate = app.world().get::<Transform>(root).unwrap().translation;
        assert!(candidate.x > before.translation[0] + 1.0);
        assert_eq!(draft_record(&app), &before);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        step(&mut app, node, cursor);
        for _ in 0..3 {
            step(&mut app, node, cursor);
            assert_eq!(
                app.world().get::<Transform>(root).unwrap().translation,
                candidate
            );
            assert_eq!(draft_record(&app), &before);
            assert!(
                app.world()
                    .resource::<EditorAuthoringSession>()
                    .undo
                    .is_empty()
            );
            assert!(!app.world().resource::<SceneEditorState>().dirty);
        } // 延迟采用真实后台结果，确认等待期间模型没有闪回、作者数据没有提前落位。
        app.world_mut().resource_mut::<ValidationPollingEnabled>().0 = true;
        for _ in 0..64 {
            step(&mut app, node, cursor);
            if app
                .world()
                .resource::<PlacementMoveState>()
                .pending_building
                .is_none()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            app.world()
                .resource::<PlacementMoveState>()
                .pending_building
                .is_none()
        );
        let after = draft_record(&app);
        let session = app.world().resource::<EditorAuthoringSession>();
        if accepted {
            let expected_z = if handle == MoveHandle::XzPlane {
                4.0
            } else {
                5.0
            };
            assert_eq!(after.translation, [7.0, 2.0, expected_z]); // 原五点地基才是最终Y权威。
            assert_eq!(after.water_surfaces, before.water_surfaces);
            assert_eq!(session.revision, 8);
            assert_eq!(session.undo.len(), 1);
            assert!(
                matches!(&session.undo[0], EditorCommand::PlacementChanged { before: old, after: new, .. } if old == &before && new == after)
            );
            assert!(app.world().resource::<SceneEditorState>().dirty);
        } else {
            assert_eq!(after, &before);
            assert_eq!(session.revision, 7);
            assert!(session.undo.is_empty());
            assert!(!app.world().resource::<SceneEditorState>().dirty);
        }
        assert_eq!(
            app.world().get::<Transform>(root).unwrap().translation,
            Vec3::from_array(after.translation)
        );
        if formal_root {
            assert!(session.placement_previews.is_empty());
        }
    }
}

#[path = "plane_interaction_tests.rs"]
mod plane_tests;

#[path = "rotation_scale_interaction_tests.rs"]
mod rotation_scale_tests;

#[test]
fn height_rules_keep_offset_on_horizontal_and_update_it_on_vertical_motion() {
    let (mut app, _, node) = app();
    let sampler = PlacementTerrainSampler::from_sources(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap(),
        None,
        2,
    )
    .unwrap();
    begin(&mut app, node, MoveAxis::X);
    let mut state = app.world_mut().resource_mut::<PlacementMoveState>();
    let drag = state.drag.as_mut().unwrap();
    drag.before.height_mode = ScenePlacementHeightMode::FollowTerrain;
    drag.before.terrain_offset = 2.0;
    drag.preview = drag.before.clone();
    super::transaction::update_preview(drag, Vec3::new(7.0, 4.0, 5.0), Some(&sampler)).unwrap();
    assert_eq!(drag.preview.translation, [7.0, 4.0, 5.0]);
    assert_eq!(drag.preview.terrain_offset, 2.0);
    drag.handle = MoveHandle::Axis(MoveAxis::Y);
    super::transaction::update_preview(drag, Vec3::new(5.0, 7.0, 5.0), Some(&sampler)).unwrap();
    assert_eq!(drag.preview.terrain_offset, 5.0);
    assert_eq!(drag.preview.translation, [5.0, 7.0, 5.0]);
    assert!(super::transaction::update_preview(drag, Vec3::new(5.0, 8.0, 5.0), None).is_err());
    assert_eq!(drag.preview.translation, [5.0, 7.0, 5.0]);
}

#[test]
fn selecting_other_layer_does_not_start_drag_for_old_asset() {
    let (mut app, _, node) = app();
    app.world_mut()
        .resource_mut::<LayerCandidateState>()
        .selected = Some(LayerTarget::Water(
        crate::types::WaterAuthoringFeatureKind::StillWater,
        "pond".into(),
    ));
    // 单独的其他正式选择即使尚无完整候选列表，也不能复用残留资产。
    let cursor = point_on_axis(&app, MoveAxis::X, 0.0);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
}

#[test]
fn control_snaps_only_dragged_axis_and_preserves_fixed_world_height() {
    let (mut app, root, node) = app();
    begin(&mut app, node, MoveAxis::X);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlRight);
    let cursor = point_on_axis(&app, MoveAxis::X, 1.37);
    step(&mut app, node, cursor);
    assert_eq!(
        app.world().get::<Transform>(root).unwrap().translation,
        Vec3::new(6.0, 4.0, 5.0)
    );
    assert_eq!(draft_record(&app).translation, record().translation);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, cursor);
    assert_eq!(draft_record(&app).translation, [6.0, 4.0, 5.0]);
}

#[test]
fn pointer_outside_viewport_keeps_capture_without_old_scene_clamping() {
    let (mut app, root, node) = app();
    begin(&mut app, node, MoveAxis::X);
    app.world_mut()
        .entity_mut(node)
        .get_mut::<RelativeCursorPosition>()
        .unwrap()
        .cursor_over = false;
    step(&mut app, node, Vec2::new(1.1, 0.0));
    assert!(app.world().resource::<PlacementMoveState>().drag.is_some());
    assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
    assert!(app.world().get::<Transform>(root).unwrap().translation.x > 10.0);
    assert_eq!(draft_record(&app).translation, record().translation);
}

#[test]
fn resize_focus_selection_and_terrain_changes_cancel_preview() {
    for case in 0..5 {
        let (mut app, root, node) = app();
        begin(&mut app, node, MoveAxis::X);
        let cursor = point_on_axis(&app, MoveAxis::X, 1.0);
        step(&mut app, node, cursor);
        match case {
            0 => {
                app.world_mut()
                    .resource_mut::<SceneEditorViewportState>()
                    .render_size
                    .x += 100
            }
            1 => {
                app.world_mut().spawn((
                    Window {
                        focused: false,
                        ..default()
                    },
                    bevy::window::PrimaryWindow,
                ));
            }
            2 => {
                app.world_mut()
                    .resource_mut::<EditorGltfBrowserState>()
                    .selected_placement_id = None
            }
            3 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .terrain_revision += 1
            }
            _ => {
                app.world_mut()
                    .entity_mut(node)
                    .get_mut::<ComputedNode>()
                    .unwrap()
                    .size
                    .x += 100.0
            }
        }
        step(&mut app, node, cursor);
        assert!(
            app.world().resource::<PlacementMoveState>().drag.is_none(),
            "取消场景 {case}"
        );
        assert_eq!(
            app.world().get::<Transform>(root).unwrap().translation,
            Vec3::from_array(record().translation)
        );
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }
}

#[test]
fn missing_height_at_release_restores_before_instead_of_last_valid_preview() {
    let (mut app, root, node) = app();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
        record.height_mode = ScenePlacementHeightMode::FollowTerrain;
        record.terrain_offset = 2.0;
    }
    let before = draft_record(&app).clone();
    begin(&mut app, node, MoveAxis::X);
    let cursor = point_on_axis(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    assert!((app.world().get::<Transform>(root).unwrap().translation.x - 6.0).abs() < 0.002);
    replace_ground_heights(&mut app, None);
    let cursor = point_on_axis(&app, MoveAxis::X, 2.0);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, cursor);
    assert_eq!(draft_record(&app), &before);
    assert_eq!(
        app.world().get::<Transform>(root).unwrap().translation,
        Vec3::from_array(before.translation)
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn same_revision_height_arrival_resumes_preview_and_clears_waiting_notice() {
    let (mut app, root, node) = app();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
        record.height_mode = ScenePlacementHeightMode::FollowTerrain;
        record.terrain_offset = 2.0;
    }
    begin(&mut app, node, MoveAxis::X);
    replace_ground_heights(&mut app, None);
    let cursor = point_on_axis(&app, MoveAxis::X, 2.0);
    step(&mut app, node, cursor);
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("松手将取消")
    );
    assert!(
        !app.world()
            .resource::<PlacementMoveState>()
            .drag
            .as_ref()
            .unwrap()
            .valid
    );
    replace_ground_heights(&mut app, Some(Arc::new(vec![2; 121])));
    step(&mut app, node, cursor);
    assert!(
        app.world()
            .resource::<PlacementMoveState>()
            .drag
            .as_ref()
            .unwrap()
            .valid
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("移动预览")
    );
    assert!((app.world().get::<Transform>(root).unwrap().translation.x - 7.0).abs() < 0.002);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, cursor);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    assert!((draft_record(&app).translation[0] - 7.0).abs() < 0.002);
}
