//! XZ平面实际输入回归：复用已有单资产夹具，验证双轴预览、高度、历史与取消。

use super::super::math::plane_corners;
use super::*;

pub(super) fn set_camera(app: &mut App, view: EditorView, orthographic: bool) {
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.view = view;
        editor.orthographic = orthographic;
    }
    let (yaw, pitch) = orbit_angles_from_view(view);
    let mut viewport = app.world_mut().resource_mut::<SceneEditorViewportState>();
    viewport.yaw = yaw;
    viewport.pitch = pitch;
}

pub(super) fn point_on_plane(app: &App, delta: Vec2) -> Vec2 {
    let camera = camera(app);
    let before = app
        .world()
        .resource::<PlacementMoveState>()
        .drag
        .as_ref()
        .map_or_else(|| draft_record(app), |drag| &drag.before);
    let origin = Vec3::from_array(before.translation);
    let corners = plane_corners(origin, camera.handle_length(origin).unwrap());
    camera
        .project(corners[0].lerp(corners[2], 0.5) + Vec3::new(delta.x, 0.0, delta.y))
        .unwrap()
}

pub(super) fn begin_plane(app: &mut App, node: Entity) {
    let cursor = point_on_plane(app, Vec2::ZERO);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(app, node, cursor);
    let state = app.world().resource::<PlacementMoveState>();
    assert_eq!(
        state.drag.as_ref().map(|drag| drag.handle),
        Some(MoveHandle::XzPlane)
    );
    assert!(state.owns_mouse());
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

#[test]
fn plane_drag_previews_both_axes_and_releases_one_complete_history_entry() {
    for view in [EditorView::Perspective, EditorView::Top] {
        for orthographic in [false, true] {
            for follows_terrain in [false, true] {
                let (mut app, root, node) = app();
                set_camera(&mut app, view, orthographic);
                if follows_terrain {
                    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                    let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
                    record.height_mode = ScenePlacementHeightMode::FollowTerrain;
                    record.terrain_offset = 2.0;
                }
                let before = draft_record(&app).clone();
                begin_plane(&mut app, node);
                let delta = Vec2::new(1.37, -1.62);
                let cursor = point_on_plane(&app, delta);
                step(&mut app, node, cursor);
                let expected =
                    Vec3::from_array(before.translation) + Vec3::new(delta.x, 0.0, delta.y);
                assert!(
                    app.world()
                        .get::<Transform>(root)
                        .unwrap()
                        .translation
                        .distance(expected)
                        < 0.002
                );
                assert_eq!(draft_record(&app), &before);
                assert!(
                    app.world()
                        .resource::<EditorAuthoringSession>()
                        .undo
                        .is_empty()
                );
                assert!(!app.world().resource::<SceneEditorState>().dirty);
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .release(MouseButton::Left);
                step(&mut app, node, cursor);
                let after = draft_record(&app);
                assert!(Vec3::from_array(after.translation).distance(expected) < 0.002);
                assert_eq!(after.terrain_offset, before.terrain_offset);
                assert_eq!(after.water_surfaces, before.water_surfaces);
                assert_eq!(after.scale, before.scale);
                assert_eq!(after.yaw_degrees, before.yaw_degrees);
                let session = app.world().resource::<EditorAuthoringSession>();
                assert_eq!(session.revision, 8);
                assert_eq!(session.undo.len(), 1);
                assert!(
                    matches!(&session.undo[0], EditorCommand::PlacementChanged { before: old, after: new, .. } if old == &before && new == after)
                );
                assert!(app.world().resource::<SceneEditorState>().dirty);
            }
        }
    }
}

#[test]
fn plane_control_and_building_grid_snap_both_coordinates() {
    for building in [false, true] {
        let (mut app, root, node) = app();
        set_camera(&mut app, EditorView::Top, true);
        if building {
            app.world_mut()
                .resource_mut::<EditorAuthoringSession>()
                .draft
                .as_mut()
                .unwrap()
                .placements
                .placements[0]
                .building_preset_id = Some("test-building".into());
            app.world_mut()
                .resource_mut::<BuildingPlacementControls>()
                .grid_step_meters = 2.0;
        }
        begin_plane(&mut app, node);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlRight);
        let cursor = point_on_plane(&app, Vec2::new(1.37, -1.62));
        step(&mut app, node, cursor);
        let expected = Vec3::new(6.0, 4.0, if building { 4.0 } else { 3.0 });
        assert_eq!(
            app.world().get::<Transform>(root).unwrap().translation,
            expected
        );
        assert_eq!(draft_record(&app).translation, record().translation);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        step(&mut app, node, cursor);
        if building {
            let mut reader = bevy::ecs::message::MessageCursor::<BuildingEditRequest>::default();
            let requests = reader
                .read(app.world().resource::<Messages<BuildingEditRequest>>())
                .collect::<Vec<_>>();
            assert_eq!(requests.len(), 1);
            assert!(
                matches!(requests[0], BuildingEditRequest::Relocate { world_xz, .. } if *world_xz == expected.xz())
            );
            assert!(
                app.world()
                    .resource::<EditorAuthoringSession>()
                    .undo
                    .is_empty()
            );
        } else {
            assert_eq!(draft_record(&app).translation, expected.to_array());
            assert_eq!(
                app.world().resource::<EditorAuthoringSession>().undo.len(),
                1
            );
        }
    }
}

#[test]
fn plane_motion_samples_new_xz_on_a_slope_and_preserves_terrain_offset() {
    let (mut app, root, node) = app();
    set_camera(&mut app, EditorView::Perspective, false);
    let heights = Arc::new((0..121).map(|i| (2 * (i % 11) + i / 11) as u16).collect());
    let mut draft = test_heightfield_draft(heights);
    let mut before = record();
    before.height_mode = ScenePlacementHeightMode::FollowTerrain;
    before.terrain_offset = 2.0;
    before.translation[1] = 17.0;
    draft.placements.placements.push(before.clone());
    draft.placement_water_surface_count = 1;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft = Some(draft);
    app.world_mut()
        .get_mut::<Transform>(root)
        .unwrap()
        .translation = Vec3::from_array(before.translation);
    begin_plane(&mut app, node);
    for delta in [Vec2::new(1.2, -1.6), Vec2::new(1.7, -1.1)] {
        let cursor = point_on_plane(&app, delta);
        step(&mut app, node, cursor);
        let expected = Vec3::new(5.0 + delta.x, 17.0 + 2.0 * delta.x + delta.y, 5.0 + delta.y);
        assert!(
            app.world()
                .get::<Transform>(root)
                .unwrap()
                .translation
                .distance(expected)
                < 0.005
        );
        assert_eq!(draft_record(&app), &before);
    }
    let cursor = point_on_plane(&app, Vec2::new(1.7, -1.1));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, cursor);
    assert!((draft_record(&app).translation[1] - 19.3).abs() < 0.005);
    assert_eq!(draft_record(&app).terrain_offset, 2.0);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn plane_cancellation_restores_current_authority_for_assets_and_buildings() {
    for building in [false, true] {
        for cause in 0..4 {
            let (mut app, root, node) = app();
            set_camera(&mut app, EditorView::Top, false);
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
            begin_plane(&mut app, node);
            let cursor = point_on_plane(&app, Vec2::new(2.0, -2.0));
            step(&mut app, node, cursor);
            assert!(
                app.world()
                    .get::<Transform>(root)
                    .unwrap()
                    .translation
                    .distance(Vec3::new(7.0, 4.0, 3.0))
                    < 0.002
            );
            match cause {
                0 => app
                    .world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Escape),
                1 => app
                    .world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .press(MouseButton::Right),
                2 => {
                    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                    session.draft.as_mut().unwrap().placements.placements[0].translation[2] = 8.0;
                    session.revision += 1;
                }
                _ => {
                    app.world_mut()
                        .resource_mut::<SceneEditorViewportState>()
                        .render_size
                        .x += 100
                }
            }
            step(&mut app, node, cursor);
            assert_eq!(
                app.world().get::<Transform>(root).unwrap().translation,
                Vec3::from_array(draft_record(&app).translation)
            );
            assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
            assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
            assert!(
                app.world()
                    .resource::<EditorAuthoringSession>()
                    .undo
                    .is_empty()
            );
            assert!(!app.world().resource::<SceneEditorState>().dirty);
        }
    }
}

#[test]
fn plane_click_and_return_to_start_keep_off_grid_position_and_redo() {
    for building in [false, true] {
        for moved in [false, true] {
            let (mut app, root, node) = app();
            set_camera(&mut app, EditorView::Top, false);
            let origin = Vec3::new(5.25, 4.0, 5.25);
            {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
                record.translation = origin.to_array();
                if building {
                    record.building_preset_id = Some("test-building".into());
                }
                session
                    .redo
                    .push(EditorCommand::PlacementAdded(super::record()));
            }
            app.world_mut()
                .get_mut::<Transform>(root)
                .unwrap()
                .translation = origin;
            let before = draft_record(&app).clone();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ControlLeft);
            begin_plane(&mut app, node);
            if moved {
                let cursor = point_on_plane(&app, Vec2::new(2.2, -1.6));
                step(&mut app, node, cursor);
            }
            let cursor = point_on_plane(&app, Vec2::ZERO);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left);
            step(&mut app, node, cursor);
            assert_eq!(draft_record(&app), &before);
            assert_eq!(
                app.world().get::<Transform>(root).unwrap().translation,
                origin
            );
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.revision, 7);
            assert!(session.undo.is_empty());
            assert_eq!(session.redo.len(), 1);
        }
    }
}

#[test]
fn plane_missing_height_cancels_or_resumes_at_both_new_coordinates() {
    for resumes in [false, true] {
        let (mut app, root, node) = app();
        set_camera(&mut app, EditorView::Top, false);
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
            record.height_mode = ScenePlacementHeightMode::FollowTerrain;
            record.terrain_offset = 2.0;
        }
        let before = draft_record(&app).clone();
        begin_plane(&mut app, node);
        let cursor = point_on_plane(&app, Vec2::new(1.0, -1.0));
        step(&mut app, node, cursor);
        let heights = app
            .world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .draft
            .as_mut()
            .unwrap()
            .heights
            .take();
        let cursor = point_on_plane(&app, Vec2::new(2.2, -2.2));
        step(&mut app, node, cursor);
        assert!(
            !app.world()
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
                .contains("松手将取消")
        );
        if resumes {
            app.world_mut()
                .resource_mut::<EditorAuthoringSession>()
                .draft
                .as_mut()
                .unwrap()
                .heights = heights;
            step(&mut app, node, cursor);
            assert!(
                app.world()
                    .resource::<SceneEditorState>()
                    .status
                    .contains("移动预览")
            );
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        step(&mut app, node, cursor);
        let session = app.world().resource::<EditorAuthoringSession>();
        if resumes {
            let expected = Vec3::new(7.2, 4.0, 2.8);
            assert!(Vec3::from_array(draft_record(&app).translation).distance(expected) < 0.002);
            assert!(
                app.world()
                    .get::<Transform>(root)
                    .unwrap()
                    .translation
                    .distance(expected)
                    < 0.002
            );
            assert_eq!(session.undo.len(), 1);
        } else {
            assert_eq!(draft_record(&app), &before);
            assert_eq!(
                app.world().get::<Transform>(root).unwrap().translation,
                Vec3::from_array(before.translation)
            );
            assert!(session.undo.is_empty());
        }
    }
}
