//! 多选通过真实输入状态机、Transform传播和批量历史验证；不以局部父节点位移冒充模型预览。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::viewport::modes::{SelectionFilter, ViewportMode};
use crate::viewport::multi_select::{
    PlacementSelectionSet, SelectedMember, advance_multi_selection, handle_multi_select_pointer,
};
use crate::viewport::placement_move::group_types::GroupPhase;
use bevy::ecs::system::RunSystemOnce;
use feathrise_scene::MapPlacementInstance;

#[path = "multi_select_refresh_tests.rs"]
mod refresh;

#[path = "multi_select_formal_ownership_tests.rs"]
mod formal_ownership; // 正式根归保存候选拥有，多选作者变换只能绑定独立预览。

pub(super) fn undo_resources(app: &mut App) {
    use crate::authoring::*;
    app.init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<WaterMaskMaterializeTask>()
        .init_resource::<SurfaceLayerEditTask>()
        .init_resource::<SurfaceBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<WaterFileHandoff>()
        .init_resource::<crate::authoring::courtyards::CourtyardAuthoringState>()
        .insert_resource(EditorPreviewAssets {
            portal_base_mesh: default(),
            portal_arrow_head_mesh: default(),
            portal_arrow_shaft_mesh: default(),
            portal_edge_beam_mesh: default(),
            portal_edge_ring_mesh: default(),
            portal_base_material: default(),
            portal_arrow_material: default(),
            portal_edge_beam_material: default(),
            portal_edge_ring_material: default(),
            portal_label_font: default(),
        });
}

pub(super) fn group_app() -> (App, [Entity; 2], Entity, Entity) {
    let (mut app, first, node) = app();
    app.add_plugins(bevy::transform::TransformPlugin)
        .init_resource::<PlacementSelectionSet>();
    let mut second = record();
    second.id = "move-second".into();
    second.translation = [7.0, 6.0, 5.0];
    let map = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .map_id
        .clone();
    let parent = app
        .world_mut()
        .spawn((Transform::IDENTITY, Visibility::Inherited))
        .id();
    app.world_mut()
        .entity_mut(first)
        .insert((MapPlacementInstance::new(&map, &record()), ChildOf(parent),
            Transform::from_translation(Vec3::from_array(record().translation))
                .with_rotation(Quat::from_rotation_y(record().yaw_degrees.to_radians()))
                .with_scale(Vec3::from_array(record().scale))));
    let other = app
        .world_mut()
        .spawn((
            Transform::from_translation(Vec3::from_array(second.translation))
                .with_rotation(Quat::from_rotation_y(second.yaw_degrees.to_radians()))
                .with_scale(Vec3::from_array(second.scale)),
            Visibility::Inherited,
            MapPlacementInstance::new(&map, &second),
            ChildOf(parent),
        ))
        .id();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.placement_previews.insert(second.id.clone(), other);
        session
            .draft
            .as_mut()
            .unwrap()
            .placements
            .placements
            .push(second.clone());
        session
            .draft
            .as_mut()
            .unwrap()
            .placement_water_surface_count = 2;
    }
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .select_indexed_test_placement(&second.id, 1, 3);
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .selected_placement_id = None;
    app.world_mut()
        .resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
            selection.publish(
                vec![
                    SelectedMember {
                        id: record().id,
                        index: 0,
                        position: Vec3::from_array(record().translation),
                    },
                    SelectedMember {
                        id: second.id,
                        index: 1,
                        position: Vec3::from_array(second.translation),
                    },
                ],
                world.resource::<EditorAuthoringSession>(),
            );
        });
    (app, [first, other], node, parent)
}

pub(super) fn group_point(app: &App, axis: MoveAxis, delta: f32) -> Vec2 {
    let origin = app.world().resource::<PlacementSelectionSet>().center;
    let camera = camera(app);
    camera
        .project(origin + axis.direction() * (camera.handle_length(origin).unwrap() * 0.65 + delta))
        .unwrap()
}

#[test]
fn group_handle_hover_highlights_plane_rotation_and_scale_without_history() {
    use crate::viewport::tool_preview::AuthorToolFrame;
    use feathrise_author_tools::{AuthorTool, TransformPreview};
    let (mut app, _, node, _) = group_app();
    app.init_resource::<AuthorToolFrame>()
        .add_systems(First, |mut frame: ResMut<AuthorToolFrame>| *frame = default())
        .add_systems(Update, super::super::render::draw_placement_move.after(handle_placement_move));
    plane_tests::set_camera(&mut app, EditorView::Perspective, false);
    let origin = app.world().resource::<PlacementSelectionSet>().center;
    for handle in [MoveHandle::XzPlane, MoveHandle::RotationY, MoveHandle::UniformScale] {
        app.world_mut().resource_mut::<ViewportModeState>().mode = handle.mode();
        let camera = camera(&app);
        let point = match handle {
            MoveHandle::XzPlane => origin + Vec3::new(0.36, 0.0, 0.36) * camera.handle_length(origin).unwrap(),
            MoveHandle::RotationY => super::super::rotation::rotation_ring(camera, origin).unwrap().world[13],
            _ => origin,
        };
        step(&mut app, node, camera.project(point).unwrap());
        let state = app.world().resource::<PlacementMoveState>();
        assert_eq!(state.hover, Some(handle));
        assert!(!state.owns_mouse() && state.group.is_none());
        let color = Color::srgb(0.92, 0.72, 0.40).to_linear().to_f32_array();
        let preview = &app.world().resource::<AuthorToolFrame>().preview.items[0];
        match preview {
            AuthorTool::Transform(TransformPreview::Move {plane_active, active_axes, ..}) => {
                assert!(*plane_active); assert_eq!(*active_axes, 0);
            }
            AuthorTool::Transform(TransformPreview::Rotate {color: actual, ..} | TransformPreview::Scale {color: actual, ..}) => assert_eq!(*actual, color),
            _ => panic!("多选未发布对应手柄"),
        }
        step(&mut app, node, Vec2::new(-0.45, -0.45));
        assert!(app.world().resource::<PlacementMoveState>().hover.is_none());
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, 7); assert!(session.undo.is_empty() && session.redo.is_empty());
    }
}

pub(super) fn group_begin(app: &mut App, node: Entity, axis: MoveAxis) -> Vec2 {
    let start = group_point(app, axis, 0.0);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(app, node, start);
    step(app, node, start);
    assert!(
        app.world()
            .resource::<PlacementMoveState>()
            .group
            .as_ref()
            .is_some_and(|group| group.phase == GroupPhase::Dragging)
    );
    start
}

pub(super) fn finish(app: &mut App, node: Entity, cursor: Vec2) {
    for _ in 0..128 {
        step(app, node, cursor);
        let movement = app.world().resource::<PlacementMoveState>();
        if movement.group.is_none() && movement.group_task.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!(
        "整组状态应完整收尾：{}",
        app.world().resource::<MapEditorState>().status
    );
}

#[test]
fn persistent_relative_group_snap_preserves_spacing_and_has_one_undo_entry() {
    let (mut app, roots, node, _) = group_app();
    for record in &mut app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().placements.placements { record.translation[0] += 0.3; }
    for root in roots { app.world_mut().get_mut::<Transform>(root).unwrap().translation.x += 0.3; }
    app.world_mut().resource_mut::<PlacementSelectionSet>().center.x += 0.3;
    app.insert_resource(feathrise_editor_core::EditorSettings { editor_snapping: feathrise_editor_core::EditorSnapSettings { enabled: true, mode: feathrise_editor_core::EditorSnapMode::Relative, ..default() }, ..default() });
    let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
    group_begin(&mut app, node, MoveAxis::X);
    let cursor = group_point(&app, MoveAxis::X, 0.7);
    step(&mut app, node, cursor);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    finish(&mut app, node, cursor);
    let session = app.world().resource::<EditorAuthoringSession>();
    let after = &session.draft.as_ref().unwrap().placements.placements;
    assert_eq!(session.undo.len(), 1);
    for (old, new) in before.iter().zip(after) { assert!((new.translation[0] - old.translation[0] - 1.0).abs() < 0.002); }
    assert!(((after[1].translation[0] - after[0].translation[0]) - (before[1].translation[0] - before[0].translation[0])).abs() < 0.00001);
}

#[test]
fn multi_group_previews_world_positions_and_commits_one_reversible_complete_history() {
    let (mut app, roots, node, parent) = group_app();
    let before = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .clone();
    group_begin(&mut app, node, MoveAxis::X);
    let cursor = group_point(&app, MoveAxis::X, 1.37);
    step(&mut app, node, cursor);
    for (i, root) in roots.into_iter().enumerate() {
        assert!(
            (app.world()
                .get::<GlobalTransform>(root)
                .unwrap()
                .translation()
                .x
                - before[i].translation[0]
                - 1.37)
                .abs()
                < 0.002
        );
        assert_eq!(
            app.world()
                .get::<Transform>(root)
                .unwrap()
                .translation
                .to_array(),
            before[i].translation
        );
    }
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements,
        before
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, cursor);
    finish(&mut app, node, cursor);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, 8);
    assert_eq!(session.undo.len(), 1);
    assert!(app.world().resource::<MapEditorState>().dirty);
    let after = session
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .clone();
    for i in 0..2 {
        assert_eq!(after[i].water_surfaces, before[i].water_surfaces);
        assert_eq!(after[i].yaw_degrees, before[i].yaw_degrees);
        assert_eq!(after[i].scale, before[i].scale);
        assert_eq!(
            app.world().get::<ChildOf>(roots[i]).unwrap().parent(),
            parent
        );
        assert!(
            (app.world()
                .get::<GlobalTransform>(roots[i])
                .unwrap()
                .translation()
                .x
                - after[i].translation[0])
                .abs()
                < 0.002
        );
    }
    assert!((after[1].translation[0] - after[0].translation[0] - 2.0).abs() < 0.002);
    assert!(matches!(&session.undo[0], EditorCommand::CourtyardBatch(_)));
    undo_resources(&mut app);
    for undo in [true, false] {
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.undo_requested = undo;
            editor.redo_requested = !undo;
        }
        app.world_mut()
            .run_system_once(crate::authoring::apply_undo_redo)
            .unwrap();
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(
            draft.placements.placements,
            if undo { before.clone() } else { after.clone() }
        );
        assert_eq!(draft.placement_water_surface_count, 2);
        assert_eq!(session.undo.len(), usize::from(!undo));
        assert_eq!(session.redo.len(), usize::from(undo));
        app.world_mut()
            .run_system_once(crate::authoring::courtyards::sync_visuals)
            .unwrap();
        for i in 0..2 {
            let expected = if undo { &before[i] } else { &after[i] };
            assert!(
                app.world()
                    .get::<Transform>(roots[i])
                    .unwrap()
                    .translation
                    .distance(Vec3::from_array(expected.translation))
                    < 0.002
            );
        }
    }
    assert_eq!(
        app.world()
            .resource::<PlacementSelectionSet>()
            .members
            .len(),
        2
    );
}

#[test]
fn multi_group_ctrl_uses_frozen_step_and_preserves_spacing_beyond_old_bounds() {
    let (mut app, roots, node, _) = group_app();
    app.insert_resource(feathrise_editor_core::EditorSettings {
        editor_snapping: feathrise_editor_core::EditorSnapSettings {
            translation_meters: 2.5,
            ..default()
        },
        ..default()
    });
    group_begin(&mut app, node, MoveAxis::X);
    app.world_mut()
        .resource_mut::<feathrise_editor_core::EditorSettings>()
        .editor_snapping
        .translation_meters = 0.25;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlRight);
    let cursor = group_point(&app, MoveAxis::X, 1.37);
    step(&mut app, node, cursor);
    assert!(
        (app.world()
            .get::<GlobalTransform>(roots[0])
            .unwrap()
            .translation()
            .x
            - 6.5)
            .abs()
            < 0.002
    );
    let cursor = group_point(&app, MoveAxis::X, 20.0);
    step(&mut app, node, cursor);
    assert!(
        (app.world()
            .get::<GlobalTransform>(roots[1])
            .unwrap()
            .translation()
            .x
            - 26.0)
            .abs()
            < 0.002
    );
    assert!(
        (app.world()
            .get::<GlobalTransform>(roots[0])
            .unwrap()
            .translation()
            .x
            - 24.0)
            .abs()
            < 0.002
    );
}

#[test]
fn multi_group_cancel_and_stale_worker_keep_author_records_and_surviving_roots() {
    for waiting in [false, true] {
        for reason in 0..5 {
            let (mut app, roots, node, parent) = group_app();
            group_begin(&mut app, node, MoveAxis::X);
            let cursor = group_point(&app, MoveAxis::X, 1.0);
            step(&mut app, node, cursor);
            if waiting {
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .release(MouseButton::Left);
                step(&mut app, node, cursor);
            }
            match reason {
                0 => app
                    .world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Escape),
                1 => {
                    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Rotate
                }
                2 => {
                    app.world_mut()
                        .resource_mut::<EditorAuthoringSession>()
                        .revision += 1
                }
                3 => {
                    app.world_mut()
                        .resource_mut::<MapEditorState>()
                        .map_transition_in_progress = true
                }
                _ => {
                    app.world_mut()
                        .resource_mut::<EditorAuthoringSession>()
                        .terrain_revision += 1
                }
            }
            step(&mut app, node, cursor);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left);
            finish(&mut app, node, cursor);
            assert_eq!(draft_record(&app).translation, record().translation);
            assert!(
                app.world()
                    .resource::<EditorAuthoringSession>()
                    .undo
                    .is_empty()
            );
            assert!(!app.world().resource::<MapEditorState>().dirty);
            for root in roots {
                assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
            }
        }
    }
}

#[test]
fn multi_group_missing_height_rejects_whole_batch_without_using_old_preview() {
    let (mut app, roots, node, _) = group_app();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let record = &mut session.draft.as_mut().unwrap().placements.placements[1];
        record.height_mode = MapPlacementHeightMode::FollowTerrain;
        record.terrain_offset = 4.0;
    }
    let before = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .clone();
    group_begin(&mut app, node, MoveAxis::X);
    let cursor = group_point(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .heights = None;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, cursor);
    finish(&mut app, node, cursor);
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements,
        before
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert!(!app.world().resource::<MapEditorState>().dirty);
    for i in 0..2 {
        assert_eq!(
            app.world()
                .get::<GlobalTransform>(roots[i])
                .unwrap()
                .translation()
                .to_array(),
            before[i].translation
        );
    }
}

#[test]
fn multi_group_cleanup_never_cascade_deletes_root_whose_owner_was_revoked() {
    let (mut app, roots, node, _) = group_app();
    group_begin(&mut app, node, MoveAxis::X);
    let cursor = group_point(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    app.world_mut()
        .entity_mut(roots[1])
        .remove::<MapPlacementInstance>();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .placement_previews
        .remove("move-second");
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    step(&mut app, node, cursor);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    finish(&mut app, node, cursor);
    assert!(app.world().get_entity(roots[1]).is_ok());
    assert!(app.world().get::<ChildOf>(roots[1]).is_none());
}

#[test]
fn multi_mixed_preview_parents_preserve_batch_visibility_and_author_root_when_formal_batch_retires()
{
    use crate::ui::placement_water::{
        register_placement_instance_root, unregister_placement_instance_root,
    };
    let (mut app, roots, node, parent) = group_app();
    app.add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    for (index, root) in roots.into_iter().enumerate() {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        let instance =
            MapPlacementInstance::new(&draft.map_id, &draft.placements.placements[index]);
        app.world_mut()
            .entity_mut(root)
            .remove::<MapPlacementInstance>();
        app.world_mut().entity_mut(root).insert(instance);
    }
    app.world_mut().entity_mut(roots[1]).remove::<ChildOf>(); // 模拟新摆放的独立作者预览。
    group_begin(&mut app, node, MoveAxis::X);
    let formal_pivot = app.world().get::<ChildOf>(roots[0]).unwrap().parent();
    let author_pivot = app.world().get::<ChildOf>(roots[1]).unwrap().parent();
    assert_ne!(formal_pivot, author_pivot);
    assert_eq!(
        app.world().get::<ChildOf>(formal_pivot).unwrap().parent(),
        parent
    );
    assert!(app.world().get::<ChildOf>(author_pivot).is_none());
    let cursor = group_point(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    app.world_mut().despawn(parent); // 正式容器退休只拥有自己的临时父级和正式根。
    assert!(app.world().get_entity(roots[0]).is_err());
    assert!(app.world().get_entity(roots[1]).is_ok());
    step(&mut app, node, cursor);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    finish(&mut app, node, cursor);
    assert_eq!(
        app.world()
            .get::<GlobalTransform>(roots[1])
            .unwrap()
            .translation(),
        Vec3::new(7.0, 6.0, 5.0)
    );
    assert!(app.world().get::<ChildOf>(roots[1]).is_none());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn multi_group_follow_terrain_preserves_relative_world_height_and_updates_offset() {
    let (mut app, roots, node, _) = group_app();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let record = &mut session.draft.as_mut().unwrap().placements.placements[1];
        record.height_mode = MapPlacementHeightMode::FollowTerrain;
        record.terrain_offset = 4.0;
    }
    group_begin(&mut app, node, MoveAxis::Y);
    let cursor = group_point(&app, MoveAxis::Y, 1.5);
    step(&mut app, node, cursor);
    assert!(
        (app.world()
            .get::<GlobalTransform>(roots[1])
            .unwrap()
            .translation()
            .y
            - 7.5)
            .abs()
            < 0.002
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, cursor);
    finish(&mut app, node, cursor);
    let session = app.world().resource::<EditorAuthoringSession>();
    let records = &session.draft.as_ref().unwrap().placements.placements;
    assert!((records[1].translation[1] - records[0].translation[1] - 2.0).abs() < 0.002);
    assert!((records[1].terrain_offset - 5.5).abs() < 0.002);
}

fn box_systems(app: &mut App) {
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    app.add_systems(
        Update,
        handle_multi_select_pointer
            .after(handle_placement_move)
            .before(update_map_editor_numeric_scrub_mouse_guard),
    )
    .add_systems(
        Update,
        advance_multi_selection.after(update_map_editor_numeric_scrub_mouse_guard),
    );
}

#[test]
fn multi_rectangle_selects_both_origins_and_shift_click_toggle_does_not_dirty_map() {
    let (mut app, _, node, _) = group_app();
    box_systems(&mut app);
    let p0 = camera(&app)
        .project(Vec3::from_array(record().translation))
        .unwrap();
    let p1 = camera(&app).project(Vec3::new(7.0, 6.0, 5.0)).unwrap();
    let min = p0.min(p1) - Vec2::splat(0.02);
    let max = p0.max(p1) + Vec2::splat(0.02);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, min);
    step(&mut app, node, max);
    assert!(app.world().resource::<PlacementSelectionSet>().owns_mouse());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, max);
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert_eq!(selection.members.len(), 2);
    assert!(
        app.world()
            .resource::<EditorGltfBrowserState>()
            .selected_placement_id
            .is_none()
    );
    let member = selection.members[0].clone();
    app.world_mut()
        .resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
            selection
                .click(member, true, world.resource::<EditorAuthoringSession>())
                .unwrap();
        });
    assert_eq!(
        app.world()
            .resource::<PlacementSelectionSet>()
            .members
            .len(),
        1
    );
    assert!(!app.world().resource::<MapEditorState>().dirty);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn multi_short_click_freezes_shift_and_filter_change_cancels_rectangle() {
    let (mut app, _, node, _) = group_app();
    box_systems(&mut app);
    let cursor = Vec2::splat(0.5);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, cursor);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ShiftLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<PlacementSelectionSet>().click_ready);
    assert!(
        app.world()
            .resource::<PlacementSelectionSet>()
            .click_additive
    );
    step(&mut app, node, cursor);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, Vec2::splat(0.2));
    step(&mut app, node, Vec2::splat(0.8));
    app.world_mut().resource_mut::<ViewportModeState>().filter = SelectionFilter::Placements;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, Vec2::splat(0.8));
    assert!(!app.world().resource::<PlacementSelectionSet>().click_ready);
    assert_eq!(
        app.world()
            .resource::<PlacementSelectionSet>()
            .members
            .len(),
        2
    );
    step(&mut app, node, cursor);
    {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.press(MouseButton::Left);
        mouse.release(MouseButton::Left);
    }
    step(&mut app, node, cursor);
    assert!(app.world().resource::<PlacementSelectionSet>().click_ready);
}

#[test]
fn multi_rectangle_has_capture_budget_and_overflow_preserves_old_selection() {
    let (mut app, _, node, _) = group_app();
    box_systems(&mut app);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        for i in 2..300 {
            let mut item = record();
            item.id = format!("box-{i}");
            session
                .draft
                .as_mut()
                .unwrap()
                .placements
                .placements
                .push(item);
        }
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, Vec2::splat(-0.4));
    step(&mut app, node, Vec2::splat(0.4));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app, node, Vec2::splat(0.4));
    assert_eq!(
        app.world()
            .resource::<PlacementSelectionSet>()
            .members
            .len(),
        2
    );
    step(&mut app, node, Vec2::splat(0.4));
    assert!(app.world().resource::<PlacementSelectionSet>().box_busy());
    step(&mut app, node, Vec2::splat(0.4));
    assert_eq!(
        app.world()
            .resource::<PlacementSelectionSet>()
            .members
            .len(),
        2
    );
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .status
            .contains("超过")
    );
}

fn transform_cursor(app: &App, mode: ViewportMode, delta: f32) -> Vec2 {
    let center = app.world().resource::<PlacementSelectionSet>().center;
    let camera = camera(app);
    if mode == ViewportMode::Rotate {
        let radius = camera.handle_length(center).unwrap() * super::super::constants::ROTATION_RADIUS_FRACTION;
        camera.project(center + super::super::rotation::yaw_direction(delta) * radius).unwrap()
    } else {
        camera.project(center).unwrap() + Vec2::X * delta / camera.size / camera.inverse_scale
    }
}

#[test]
fn multi_rotation_and_scale_preview_actual_roots_and_commit_one_undo() {
    for mode in [ViewportMode::Rotate, ViewportMode::Scale] {
        for orthographic in [false, true] {
            let (mut app, roots, node, parent) = group_app();
            super::plane_tests::set_camera(&mut app, EditorView::Top, orthographic);
            app.world_mut().resource_mut::<ViewportModeState>().mode = mode;
            let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
            let center = app.world().resource::<PlacementSelectionSet>().center;
            let start = transform_cursor(&app, mode, 0.0);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut app, node, start); step(&mut app, node, start);
            assert!(app.world().resource::<PlacementMoveState>().group.as_ref().is_some_and(|group| group.phase == GroupPhase::Dragging));
            let cursor = transform_cursor(&app, mode, if mode == ViewportMode::Rotate { 90.0 } else { 120.0 });
            step(&mut app, node, cursor);
            let angle = if mode == ViewportMode::Rotate { 90.0_f32 } else { 0.0 };
            let factor = if mode == ViewportMode::Scale { 2.0 } else { 1.0 };
            let rotation = Quat::from_rotation_y(angle.to_radians());
            for (i, root) in roots.into_iter().enumerate() {
                let global = app.world().get::<GlobalTransform>(root).unwrap().compute_transform();
                let position = center + rotation * ((Vec3::from_array(before[i].translation) - center) * factor);
                assert!(global.translation.distance(position) < 0.002);
                assert!(global.scale.distance(Vec3::from_array(before[i].scale) * factor) < 0.002);
                assert!(global.rotation.angle_between(rotation * Quat::from_rotation_y(before[i].yaw_degrees.to_radians())) < 0.002);
            }
            assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements, before);
            assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            step(&mut app, node, cursor); finish(&mut app, node, cursor);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.undo.len(), 1);
            let after = session.draft.as_ref().unwrap().placements.placements.clone();
            for i in 0..2 {
                assert!(Vec3::from_array(after[i].scale).distance(Vec3::from_array(before[i].scale) * factor) < 0.002);
                assert!((after[i].yaw_degrees - (before[i].yaw_degrees + angle)).abs() < 0.002);
                assert_eq!(after[i].water_surfaces, before[i].water_surfaces);
                assert_eq!(app.world().get::<ChildOf>(roots[i]).unwrap().parent(), parent);
            }
            undo_resources(&mut app);
            for undo in [true, false] {
                app.world_mut().resource_mut::<MapEditorState>().undo_requested = undo;
                app.world_mut().resource_mut::<MapEditorState>().redo_requested = !undo;
                app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
                assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements, if undo { before.clone() } else { after.clone() });
                app.world_mut().run_system_once(crate::authoring::courtyards::sync_visuals).unwrap();
            }
        }
    }
}

#[test]
fn multi_rotation_and_scale_cancel_during_preview_or_pending_commit() {
    for mode in [ViewportMode::Rotate, ViewportMode::Scale] {
        for waiting in [false, true] {
            let (mut app, roots, node, parent) = group_app();
            super::plane_tests::set_camera(&mut app, EditorView::Top, false);
            app.world_mut().resource_mut::<ViewportModeState>().mode = mode;
            let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
            let start = transform_cursor(&app, mode, 0.0);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut app, node, start); step(&mut app, node, start);
            let cursor = transform_cursor(&app, mode, if mode == ViewportMode::Rotate { 60.0 } else { 100.0 });
            step(&mut app, node, cursor);
            if waiting {
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
                step(&mut app, node, cursor);
                app.world_mut().resource_mut::<EditorAuthoringSession>().terrain_revision += 1;
            } else {
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
            }
            step(&mut app, node, cursor);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            finish(&mut app, node, cursor);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.draft.as_ref().unwrap().placements.placements, before);
            assert!(session.undo.is_empty());
            for (i, root) in roots.into_iter().enumerate() {
                let transform = app.world().get::<Transform>(root).unwrap();
                assert_eq!(transform.translation.to_array(), before[i].translation);
                assert_eq!(transform.scale.to_array(), before[i].scale);
                assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
            }
        }
    }
}

#[test]
fn multi_assets_from_three_paste_containers_share_one_preview_pivot() {
    let (mut app, original_roots, node, _) = group_app();
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    let mut third = record(); third.id = "third-paste".into(); third.translation = [6.0, 5.0, 7.0];
    let third_root = app.world_mut().spawn((third.validated_transform(&map, Rect::from_corners(Vec2::ZERO, Vec2::splat(10.0))).unwrap(), Visibility::Inherited, MapPlacementInstance::new(&map, &third))).id();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.placement_previews.insert(third.id.clone(), third_root);
        session.draft.as_mut().unwrap().placements.placements.push(third);
        session.draft.as_mut().unwrap().placement_water_surface_count = 3;
    }
    let roots = [original_roots[0], original_roots[1], third_root];
    for (i, root) in roots.into_iter().enumerate() {
        let parent = app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited,
            crate::authoring::placement_batch::PastePreviewGroup::test_group(map.clone(), format!("container-{i}")))).id();
        app.world_mut().entity_mut(root).insert(ChildOf(parent));
    }
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        let session = world.resource::<EditorAuthoringSession>();
        let members = session.draft.as_ref().unwrap().placements.placements.iter().enumerate()
            .map(|(index, record)| SelectedMember { id: record.id.clone(), index, position: Vec3::from_array(record.translation) }).collect();
        selection.publish(members, session);
    });
    group_begin(&mut app, node, MoveAxis::X);
    assert_eq!(app.world().resource::<PlacementMoveState>().group.as_ref().unwrap().pivots.len(), 1);
    let cursor = group_point(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    step(&mut app, node, cursor); finish(&mut app, node, cursor);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    let records = &app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements;
    for (i, root) in roots.into_iter().enumerate() {
        assert!(app.world().get::<GlobalTransform>(root).unwrap().translation().distance(Vec3::from_array(records[i].translation)) < 0.002);
    }
}

#[test]
fn f12_during_multi_transform_requests_close_and_restores_whole_group() {
    let (mut app, _, node, _) = group_app();
    let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
    group_begin(&mut app, node, MoveAxis::X);
    let cursor = group_point(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::F12);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<MapEditorState>().close_requested);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    finish(&mut app, node, cursor);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, before);
    assert!(session.undo.is_empty());
}

#[test]
fn close_button_during_multi_transform_cancels_preview_and_requests_close() {
    let (mut app, _, node, _) = group_app();
    undo_resources(&mut app);
    app.init_resource::<crate::authoring::placement_batch::PlacementBatchState>();
    group_begin(&mut app, node, MoveAxis::X);
    let cursor = group_point(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    app.world_mut().spawn((crate::components::MapEditorActionButton(crate::types::EditorAction::Close), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    app.world_mut().run_system_once(crate::authoring::placement_batch::handle_placement_batch).unwrap();
    assert!(app.world().resource::<MapEditorState>().close_requested);
    step(&mut app, node, cursor);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    finish(&mut app, node, cursor);
    assert_eq!(draft_record(&app), &record());
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}
