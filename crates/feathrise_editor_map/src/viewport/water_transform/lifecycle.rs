//! 静水手柄的保存、取消、来源隔离与完整源TRS回归。

use super::*;
use bevy::{input::{mouse::MouseScrollUnit, touch::TouchPhase}, window::PrimaryWindow};
use feathrise_map_content::{TerrainId, TerrainObjectTransform};

#[test]
fn water_move_save_finishes_the_latest_accepted_revision_before_snapshot() {
    let (mut app, node, origin) = app(EditorSnapSettings::default());
    begin(&mut app, node, origin);
    let cursor = pointer(&app, origin, 1.8);
    step(&mut app, node, cursor);
    let accepted = level(&app);
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.world_mut().resource_mut::<MapEditorState>().save_requested = true;
    step(&mut app, node, cursor);
    assert!(app.world().resource::<MapEditorState>().save_requested);
    assert!(app.world().resource::<WaterMoveState>().drag.is_none());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, revision);
    assert!(revision > 7);
    assert_eq!(session.undo.len(), 1);
    assert!(matches!(session.undo[0], EditorCommand::WaterLevelChanged { after, .. } if after == accepted));
    assert_eq!(level(&app), accepted);
    assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
    release(&mut app, node, cursor);
    step(&mut app, node, cursor);
    step(&mut app, node, cursor);
    assert!(!app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
}

#[test]
fn water_move_cancels_owned_height_on_escape_focus_modal_mode_camera_selection_or_close() {
    for invalidation in 0..8 {
        let (mut app, node, origin) = app(EditorSnapSettings::default());
        let window = app.world_mut().spawn((Window::default(), PrimaryWindow)).id();
        begin(&mut app, node, origin);
        let cursor = pointer(&app, origin, 1.8);
        step(&mut app, node, cursor);
        assert_ne!(level(&app), START_LEVEL);
        match invalidation {
            0 => app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape),
            1 => app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
            2 => app.world_mut().resource_mut::<crate::resources::MapEditorSettingsState>().open = true,
            3 => app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Rotate,
            4 => app.world_mut().resource_mut::<MapEditorViewportState>().extent += 1.0,
            5 => confirm_selection(&mut app),
            6 => app.world_mut().resource_mut::<MapEditorState>().close_requested = true,
            _ => app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F12),
        }
        step(&mut app, node, cursor);
        assert!(app.world().resource::<WaterMoveState>().drag.is_none(), "取消场景{invalidation}");
        assert_eq!(level(&app), START_LEVEL, "取消场景{invalidation}");
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(!app.world().resource::<MapEditorState>().dirty);
        if invalidation >= 6 { assert!(app.world().resource::<MapEditorState>().close_requested); }
        assert!(app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
    }
}

#[test]
fn water_move_external_revision_keeps_other_dirty_changes_and_foreign_same_field_value() {
    for replace_level in [false, true] {
        let (mut app, node, origin) = app(EditorSnapSettings::default());
        begin(&mut app, node, origin);
        let cursor = pointer(&app, origin, 1.8);
        step(&mut app, node, cursor);
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.revision += 1;
            let draft = session.draft.as_mut().unwrap();
            draft.placements.placements[0].translation[0] = 99.0;
            if replace_level { draft.water.as_mut().unwrap().layers[0].water_level = 63.0; }
        }
        step(&mut app, node, cursor);
        assert!(app.world().resource::<WaterMoveState>().drag.is_none());
        assert_eq!(level(&app), if replace_level { 63.0 } else { START_LEVEL });
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements[0].translation[0], 99.0);
        assert!(session.undo.is_empty());
        assert!(app.world().resource::<MapEditorState>().dirty);
    }
}

#[test]
fn water_move_blocks_navigation_and_keeps_the_mouse_through_all_chord_release_edges() {
    let (mut app, node, origin) = app(EditorSnapSettings::default());
    begin(&mut app, node, origin);
    let extent = app.world().resource::<MapEditorViewportState>().extent;
    app.world_mut().write_message(MouseWheel { unit: MouseScrollUnit::Line, x: 0.0, y: 3.0,
        window: Entity::PLACEHOLDER, phase: TouchPhase::Moved });
    let cursor = pointer(&app, origin, 1.0);
    step(&mut app, node, cursor);
    assert_eq!(app.world().resource::<MapEditorViewportState>().extent, extent);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<WaterMoveState>().drag.is_none());
    assert_eq!(level(&app), START_LEVEL);
    for button in [MouseButton::Left, MouseButton::Right] {
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(button);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
        assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
    }
    step(&mut app, node, cursor);
    step(&mut app, node, cursor);
    assert!(!app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
    assert!(!app.world().resource::<PlacementMoveState>().owns_mouse());
}

fn active_source(app: &mut App, pose: TerrainObjectTransform, id: &str) -> Vec3 {
    let (map, object) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse(id).unwrap();
        object.transform = pose;
        object.water_path = Some(draft.water_asset_path.clone());
        let source = object.source_manifest(&draft.manifest);
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.terrain_documents.active = Some(crate::authoring::terrain_documents::TerrainDocumentIdentity {
            definition: Arc::new(object.clone()), manifest: source, folder: "maps/test/terrains/source".into(),
        });
        (draft.map_id.clone(), object)
    };
    {
        let mut terrains = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        terrains.map = Some(map.clone());
        terrains.selected = Some(object.id);
        terrains.selected_index = Some(0);
    }
    let origin = pose.as_transform().transform_point(Vec3::new(5.0, START_LEVEL, 5.0));
    app.world_mut().resource_scope(|world, mut selected: Mut<SelectedWaterViewportState>| {
        selected.set_still_water_center_for_tests(world.resource::<EditorAuthoringSession>(), "lake", origin);
    });
    app.world_mut().resource_mut::<MapEditorViewportState>().center = origin;
    origin
}

#[test]
fn water_move_converts_world_y_through_full_source_trs_and_leaves_source_and_mask_unchanged() {
    let (mut app, node, _) = app(EditorSnapSettings::default());
    let pose = TerrainObjectTransform { translation: [7.0, 1.0, 3.0], scale: [1.2, 2.0, 0.8],
        pitch_degrees: 20.0, yaw_degrees: 30.0, roll_degrees: 10.0 };
    let origin = active_source(&mut app, pose, "water-source-a");
    confirm_selection(&mut app);
    let mask = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().water_masks["lake"].clone();
    begin(&mut app, node, origin);
    let cursor = pointer(&app, origin, 2.6);
    step(&mut app, node, cursor);
    let up_y = pose.as_transform().to_matrix().transform_vector3(Vec3::Y).y;
    assert!((level(&app) - (START_LEVEL + 2.6 / up_y)).abs() < 0.002);
    release(&mut app, node, cursor);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.terrain_documents.active.as_ref().unwrap().definition.transform, pose);
    assert_eq!(draft.manifest.terrains.as_ref().unwrap()[0].transform, pose);
    assert!(Arc::ptr_eq(&mask, &draft.water_masks["lake"]));
    assert!(draft.water_dirty_layers.is_empty());
    assert_eq!(session.terrain_revision, 2);
    assert_eq!(session.undo.len(), 1);
}

#[test]
fn water_move_rejects_stale_sibling_canvas_and_never_rolls_back_its_same_named_layer() {
    for during_drag in [false, true] {
        let (mut app, node, origin) = app(EditorSnapSettings::default());
        if during_drag {
            begin(&mut app, node, origin);
            let cursor = pointer(&app, origin, 1.8);
            step(&mut app, node, cursor);
        }
        let origin = active_source(&mut app, TerrainObjectTransform::default(), "water-source-b");
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.water.as_mut().unwrap().layers[0].water_level = 77.0;
            draft.water_masks.insert("lake".into(), Arc::new(vec![255, 0, 0, 0]));
        }
        let cursor = pointer(&app, origin, 0.0);
        if !during_drag { app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left); }
        step(&mut app, node, cursor);
        assert!(app.world().resource::<WaterMoveState>().drag.is_none());
        assert_eq!(level(&app), 77.0);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}

#[test]
fn water_move_hides_unusable_source_y_axis_and_rejects_locked_hidden_or_disabled_canvas() {
    for reject in 0..4 {
        let (mut app, node, _) = app(EditorSnapSettings::default());
        let pose = TerrainObjectTransform { pitch_degrees: if reject == 0 { 90.0 } else { 0.0 }, ..default() };
        let origin = active_source(&mut app, pose, "water-locked-source");
        confirm_selection(&mut app);
        let id = TerrainId::parse("water-locked-source").unwrap();
        match reject {
            1 => { app.world_mut().resource_mut::<TerrainObjectsEditorState>().locked.insert(id); }
            2 => { app.world_mut().resource_mut::<TerrainObjectsEditorState>().hidden.insert(id); }
            3 => {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                Arc::make_mut(&mut session.draft.as_mut().unwrap().terrain_documents.active.as_mut().unwrap().definition).enabled = false;
            }
            _ => {}
        }
        let cursor = pointer(&app, origin, 0.0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<WaterMoveState>().drag.is_none());
        assert_eq!(level(&app), START_LEVEL);
    }
}
