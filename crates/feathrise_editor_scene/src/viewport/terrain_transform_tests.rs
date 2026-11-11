//! 独立地形真实手柄回归：统一吸附、冻结相位、单击无位移，以及导航/和弦静默释放。
use super::*;
use crate::{
    authoring::WaterNetworkEditState,
    resources::SceneEditorNumericScrubMouseGuard,
    ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
    viewport::placement_move::interaction_tests::{camera, step},
};
use bevy::input::{mouse::{MouseMotion, MouseScrollUnit, MouseWheel}, touch::TouchPhase};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorSnapSettings;
use feathrise_editor_core::EditorSnapMode;
use std::sync::Arc;

#[test]
fn embedded_object_pose_original_terrain_handle_projects_middle_and_escape_authority() {
    use crate::standalone::{embedded_author_preview as bridge, embedded_snapshot::EmbeddedPoseTargets};
    use feathrise_scene_content::embedded_author_preview::{AuthorPreview, decode};
    use std::time::{Duration, Instant};
    fn wait(app: &mut App, active: bool, x: f32) -> u64 {
        let deadline=Instant::now()+Duration::from_secs(4);
        loop {
            app.update();
            let ready=app.world().resource::<bridge::EmbeddedAuthorPreviewState>().ready.clone();
            if let Some(ready)=ready {
                let matches=matches!(decode(&ready.bytes).unwrap(), AuthorPreview::TerrainPose {
                    pose:Some(pose), active:value, .. } if value==active && (pose.translation[0]-x).abs()<0.002);
                app.world_mut().resource_mut::<bridge::EmbeddedAuthorPreviewState>().acknowledged_sequence=ready.sequence;
                if matches { return ready.sequence; }
            }
            assert!(app.world().resource::<bridge::EmbeddedAuthorPreviewState>().error.is_none());
            assert!(Instant::now()<deadline,"原地形手柄姿态没有投影到实时正文");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let (mut app,node,origin)=app(EditorSnapSettings {enabled:false,..default()});
    app.add_plugins(bridge::AuthorPreviewPlugin)
        .configure_sets(Update,crate::SceneEditorSystemSet::Tasks.after(input));
    let (map,id)={let session=app.world().resource::<EditorAuthoringSession>();
        (session.draft.as_ref().unwrap().scene_id.clone(),app.world().resource::<TerrainObjectsEditorState>().selected.clone().unwrap())};
    {
        let mut state=app.world_mut().resource_mut::<bridge::EmbeddedAuthorPreviewState>();
        state.enabled=true; state.session=7; state.generation=1;
    }
    bridge::snapshot_sent(app.world_mut(),7,1,1,map,Arc::new(EmbeddedPoseTargets {
        terrains:[id].into_iter().collect(),..default()}));
    assert!(bridge::snapshot_applied(app.world_mut(),7,1,1));
    begin(&mut app,node,origin,false);
    let cursor=pointer(&app,origin,1.37);
    step(&mut app,node,cursor);
    let active=wait(&mut app,true,origin.x+1.37);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision,7);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    step(&mut app,node,cursor);
    let ended=wait(&mut app,false,origin.x);
    assert!(ended>active);
    let session=app.world().resource::<EditorAuthoringSession>();
    assert!(session.undo.is_empty());
    assert_eq!(session.revision,7);
    assert_eq!(session.draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap()[0].transform.translation,origin.to_array());
    assert!(app.world().resource::<TerrainObjectTransformPreview>().pose.is_none());
}

fn app(snapping: EditorSnapSettings) -> (App, Entity, Vec3) {
    let (mut app, _, node) = crate::viewport::placement_move::interaction_tests::app();
    let origin = Vec3::new(5.3, 4.0, 5.0); // 非整数起点同时区分世界网格和相对步长。
    let (map, id) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("terrain-handle-test").unwrap();
        object.transform.translation = origin.to_array();
        let id = object.id.clone();
        draft.manifest.terrains = Some(Arc::from([object]));
        (draft.scene_id.clone(), id)
    };
    let mut terrain_selection = TerrainObjectsEditorState::default();
    terrain_selection.scene_id = Some(map);
    terrain_selection.selected = Some(id.clone());
    terrain_selection.selected_index = Some(0);
    app.insert_resource(terrain_selection)
        .init_resource::<TransformState>()
        .init_resource::<TerrainObjectTransformPreview>()
        .init_resource::<WaterNetworkEditState>()
        .insert_resource(EditorSettings { editor_snapping: snapping, ..default() })
        .add_message::<MouseMotion>()
        .add_message::<MouseWheel>();
    app.world_mut().resource_mut::<LayerCandidateState>().selected =
        Some(LayerTarget::TerrainObject(id.as_str().to_owned()));
    // 与生产顺序一致：手柄先取得本帧所有权，公共门归类后，相机最后消费并丢弃导航事件。
    app.edit_schedule(Update, |schedule| {
        *schedule = Schedule::new(Update);
        schedule.add_systems((input, update_scene_editor_numeric_scrub_mouse_guard, super::super::control_editor_viewport).chain());
    });
    (app, node, origin)
}

fn pointer(app: &App, origin: Vec3, delta: f32) -> Vec2 {
    let camera = camera(app);
    camera.project(origin + Vec3::X * (camera.handle_length(origin).unwrap() * 0.65 + delta)).unwrap()
}

fn begin(app: &mut App, node: Entity, origin: Vec3, control: bool) {
    if control {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
    }
    let cursor = pointer(app, origin, 0.0);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(app, node, cursor);
    assert!(app.world().resource::<TransformState>().drag.is_some());
    assert!(app.world().resource::<TerrainObjectsEditorState>().transforming);
}

#[test]
fn terrain_handle_hover_uses_original_pick_and_publishes_readonly_semantics() {
    use super::super::placement_move::{MoveAxis, MoveHandle};
    use feathrise_author_tools::AuthorToolPreview;
    for embedded in [false, true] {
        let (mut app, node, origin) = app(EditorSnapSettings::default());
        app.init_resource::<AuthorToolFrame>()
            .init_resource::<crate::standalone::embedded_snapshot::EmbeddedSnapshotState>()
            .add_systems(First, |mut frame: ResMut<AuthorToolFrame>| *frame = default())
            .add_systems(Update, draw.after(input));
        app.world_mut().resource_mut::<crate::standalone::embedded_snapshot::EmbeddedSnapshotState>().enabled = embedded;
        app.world_mut().resource_mut::<SceneEditorState>().view = crate::types::EditorView::Perspective;
        { let mut view = app.world_mut().resource_mut::<SceneEditorViewportState>(); view.yaw = 0.55; view.pitch = 0.65; }
        for handle in [MoveHandle::Axis(MoveAxis::X), MoveHandle::Axis(MoveAxis::Y), MoveHandle::Axis(MoveAxis::Z),
            MoveHandle::XzPlane, MoveHandle::RotationY, MoveHandle::UniformScale] {
            app.world_mut().resource_mut::<ViewportModeState>().mode = handle.mode();
            let camera = camera(&app);
            let length = camera.handle_length(origin).unwrap();
            let point = match handle {
                MoveHandle::Axis(axis) => origin + axis.direction() * length * 0.65,
                MoveHandle::XzPlane => origin + Vec3::new(0.36, 0.0, 0.36) * length,
                MoveHandle::RotationY => rotation_ring(camera, origin).unwrap().world[13],
                MoveHandle::UniformScale => origin,
            };
            step(&mut app, node, camera.project(point).unwrap());
            let state = app.world().resource::<TransformState>();
            assert_eq!(state.hover, Some(handle), "模式{handle:?}必须走原投影与拾取");
            assert!(state.drag.is_none());
            assert!(!app.world().resource::<TerrainObjectsEditorState>().transforming);
            assert!(!app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
            assert!(app.world().resource::<TerrainObjectTransformPreview>().pose.is_none());
            let published = &app.world().resource::<AuthorToolFrame>().preview;
            published.validate().unwrap();
            let transported: AuthorToolPreview = serde_json::from_slice(&serde_json::to_vec(published).unwrap()).unwrap();
            assert_eq!(&transported, published, "悬停使用既有语义合同，两种3D来源传递相同颜色和激活位");
            let AuthorTool::Transform(preview) = &published.items[0] else { panic!("没有原手柄显示") };
            let highlight = Color::srgb(0.92, 0.72, 0.40).to_linear().to_f32_array();
            match (handle, preview) {
                (MoveHandle::Axis(axis), TransformPreview::Move { active_axes, plane_active, color_active, .. }) => {
                    assert_eq!(*active_axes, 1 << axis.index()); assert!(!plane_active); assert_eq!(*color_active, highlight);
                }
                (MoveHandle::XzPlane, TransformPreview::Move { active_axes, plane_active, color_active, .. }) => {
                    assert_eq!(*active_axes, 0); assert!(*plane_active); assert_eq!(*color_active, highlight);
                }
                (_, TransformPreview::Rotate { color, .. } | TransformPreview::Scale { color, .. }) => assert_eq!(*color, highlight),
                _ => panic!("原手柄类型与显示语义不符"),
            }
            // 不按鼠标移出后同一帧清高亮，不能依赖下一次点击或留下作者修改。
            step(&mut app, node, Vec2::new(-0.45, -0.45));
            assert!(app.world().resource::<TransformState>().hover.is_none());
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.revision, 7); assert!(session.undo.is_empty() && session.redo.is_empty());
            assert_eq!(session.draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap()[0].transform.translation, origin.to_array());
        }
    }
}

#[test]
fn terrain_hover_clears_at_input_gates_and_drag_keeps_its_captured_axis() {
    for gate in 0..5 {
        let (mut app, node, origin) = app(EditorSnapSettings::default());
        let cursor = pointer(&app, origin, 0.0);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<TransformState>().hover.is_some());
        match gate {
            0 => app.world_mut().get_mut::<RelativeCursorPosition>(node).unwrap().cursor_over = false,
            1 => app.world_mut().resource_mut::<crate::resources::SceneEditorSettingsState>().open = true,
            2 => { app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right); }
            3 => app.world_mut().resource_mut::<EditorAuthoringSession>().saving = true,
            _ => { let mut selection = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
                let id = selection.selected.clone().unwrap(); selection.locked.insert(id); }
        }
        step(&mut app, node, cursor);
        assert!(app.world().resource::<TransformState>().hover.is_none());
        assert!(app.world().resource::<TransformState>().drag.is_none());
    }
    let (mut app, node, origin) = app(EditorSnapSettings::default());
    app.init_resource::<AuthorToolFrame>().add_systems(Update, draw.after(input));
    begin(&mut app, node, origin, false);
    let outside = pointer(&app, origin, 1.37);
    step(&mut app, node, outside);
    let tools = &app.world().resource::<AuthorToolFrame>().preview.items;
    assert!(matches!(tools.last(), Some(AuthorTool::Transform(TransformPreview::Move {active_axes: 1, ..}))),
        "按住拖出命中区域后，原捕获轴仍显示高亮");
}

#[test]
fn terrain_handle_uses_unified_active_toggle_and_frozen_relative_phase() {
    for (enabled, control, mode, expected_x) in [
        (false, false, EditorSnapMode::WorldGrid, 6.67),
        (false, true, EditorSnapMode::WorldGrid, 7.0),
        (true, false, EditorSnapMode::WorldGrid, 7.0),
        (true, true, EditorSnapMode::WorldGrid, 6.67),
        (true, false, EditorSnapMode::Relative, 6.3),
    ] {
        let (mut app, node, origin) = app(EditorSnapSettings { enabled, mode, ..default() });
        begin(&mut app, node, origin, control);
        app.world_mut().resource_mut::<EditorSettings>().editor_snapping.translation_meters = 9.0;
        let cursor = pointer(&app, origin, 1.37);
        step(&mut app, node, cursor);
        let preview = app.world().resource::<TerrainObjectTransformPreview>().pose.as_ref().unwrap().2;
        assert!((preview.translation[0] - expected_x).abs() < 0.002, "常开={enabled} Ctrl={control} 模式={mode:?}");
        assert_eq!(preview.translation[1], origin.y);
        assert_eq!(preview.translation[2], origin.z);
        let (focus, snap) = app.world().resource::<TransformState>().grid_reference().unwrap();
        assert_eq!(focus.to_array(), preview.translation);
        assert_eq!(snap.step(), 1.0);
        assert_eq!(snap.mode, mode);
        assert_eq!(snap.active(), enabled != control);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 7);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}

#[test]
fn terrain_handle_click_without_drag_never_snaps_the_original_pose_or_starts_history() {
    let (mut app, node, origin) = app(EditorSnapSettings { enabled: true, ..default() });
    begin(&mut app, node, origin, false);
    let cursor = pointer(&app, origin, 0.0);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<TerrainObjectTransformPreview>().pose.is_none());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    step(&mut app, node, cursor);
    assert!(!app.world().resource::<TerrainObjectsEditorState>().preparing());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 7);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

#[test]
fn terrain_handle_return_to_origin_restores_pose_and_preserves_undo_redo() {
    use crate::authoring::EditorCommand;
    let (mut app, node, origin) = app(EditorSnapSettings { enabled: true, ..default() });
    let record = crate::viewport::placement_move::interaction_tests::record();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.undo.push(EditorCommand::PlacementAdded(record.clone()));
        session.redo.push(EditorCommand::PlacementAdded(record.clone()));
    }
    begin(&mut app, node, origin, false);
    let away = pointer(&app, origin, 1.37);
    step(&mut app, node, away);
    assert!(app.world().resource::<TerrainObjectTransformPreview>().pose.is_some());
    // 故意停在起点一像素内，覆盖阈值内必须撤回旧 after 的路径。
    let near_origin = pointer(&app, origin, 0.0) + Vec2::X / camera(&app).size;
    step(&mut app, node, near_origin);
    assert!(app.world().resource::<TerrainObjectTransformPreview>().pose.is_none());
    let drag = app.world().resource::<TransformState>().drag.as_ref().unwrap();
    assert_eq!(drag.before, drag.after);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    step(&mut app, node, near_origin);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, 7);
    assert_eq!(session.undo.len(), 1);
    assert_eq!(session.redo.len(), 1);
    assert!(matches!(&session.undo[0], EditorCommand::PlacementAdded(old) if old == &record));
    assert!(matches!(&session.redo[0], EditorCommand::PlacementAdded(old) if old == &record));
    assert_eq!(session.draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap()[0].transform.translation, origin.to_array());
    assert!(!app.world().resource::<TerrainObjectsEditorState>().preparing());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn terrain_drag_freezes_wheel_and_retains_ownership_until_the_chord_is_quiet() {
    let (mut app, node, origin) = app(EditorSnapSettings::default());
    begin(&mut app, node, origin, false);
    let extent = app.world().resource::<SceneEditorViewportState>().extent;
    app.world_mut().write_message(MouseWheel { unit: MouseScrollUnit::Line, x: 0.0, y: 3.0, window: Entity::PLACEHOLDER, phase: TouchPhase::Moved });
    let cursor = pointer(&app, origin, 1.0);
    step(&mut app, node, cursor);
    assert_eq!(app.world().resource::<SceneEditorViewportState>().extent, extent);
    assert!(app.world().resource::<TransformState>().drag.is_some());
    assert!(app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<TransformState>().drag.is_none());
    assert!(app.world().resource::<TerrainObjectTransformPreview>().pose.is_none());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<TerrainObjectsEditorState>().transforming);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<TerrainObjectsEditorState>().transforming); // 最后release帧也不归还。
    step(&mut app, node, cursor);
    assert!(!app.world().resource::<TerrainObjectsEditorState>().transforming);
    step(&mut app, node, cursor);
    assert!(!app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 7);
}

#[test]
fn terrain_drag_cancels_when_save_or_transform_mode_changes() {
    for saving in [false, true] {
        let (mut app, node, origin) = app(EditorSnapSettings::default());
        begin(&mut app, node, origin, false);
        if saving {
            app.world_mut().resource_mut::<SceneEditorState>().save_requested = true;
        } else {
            app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Rotate;
        }
        let cursor = pointer(&app, origin, 1.0);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<TransformState>().drag.is_none());
        assert!(app.world().resource::<TerrainObjectTransformPreview>().pose.is_none());
        assert!(!app.world().resource::<TerrainObjectsEditorState>().preparing());
        assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 7);
    }
}
