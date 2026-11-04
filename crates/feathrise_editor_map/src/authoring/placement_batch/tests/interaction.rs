//! 实际键鼠、后台槽、地图换代与一次撤销的集成回归，复用现有无窗口编辑器夹具。
#[path = "cuboid_paste.rs"]
mod cuboid_paste;
#[path = "terrain_patch_interaction.rs"]
mod terrain_patch_interaction;
#[path = "derived_follow.rs"]
mod derived_follow;
#[path = "deletion_ownership.rs"]
mod deletion_ownership; // 真实整批删除的失败零发布及视觉预算前的迟到根保护。
#[path = "deletion_preparation.rs"]
mod deletion_preparation; // 近满整表、真实CPU取消槽及跨文档捕获拒收。
use super::super::{PlacementBatchState, handle_placement_batch, sync_batch_selection};
use crate::{
    authoring::{EditorAuthoringSession, test_heightfield_draft},
    resources::MapEditorState,
    viewport::{
        modes::{ViewportMode, ViewportModeState},
        multi_select::{PlacementSelectionSet, SelectedMember},
        placement_move::{
            handle_placement_move,
            interaction_tests::{
                app as base_app, batch_top_camera, batch_undo_resources, camera, record, step,
            },
        },
    },
};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use feathrise_map_content::{MapId, MapPlacementHeightMode};
use std::sync::Arc;

fn app(count: usize) -> (App, Entity) {
    let (mut app, _, node) = base_app();
    batch_undo_resources(&mut app);
    app.add_plugins(bevy::transform::TransformPlugin)
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .add_observer(super::super::retire_empty_paste_group)
        .add_systems(PreUpdate, |mut editor: ResMut<MapEditorState>| {
            editor.suppress_world_pick_this_frame = false;
        })
        .init_resource::<PlacementBatchState>()
        .init_resource::<PlacementSelectionSet>()
        .add_systems(
            Update,
            (handle_placement_batch, sync_batch_selection)
                .chain()
                .before(handle_placement_move),
        );
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    let mut members = vec![SelectedMember {
        id: record().id,
        index: 0,
        position: Vec3::from_array(record().translation),
    }];
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        for i in 1..count {
            let mut record = record();
            record.id = format!("source-{i}");
            record.translation = [5.0 + i as f32 * 0.05, 4.0 + i as f32, 5.0];
            record.height_mode = MapPlacementHeightMode::FollowTerrain;
            record.terrain_offset = record.translation[1] - 2.0;
            members.push(SelectedMember {
                id: record.id.clone(),
                index: i,
                position: Vec3::from_array(record.translation),
            });
            session
                .draft
                .as_mut()
                .unwrap()
                .placements
                .placements
                .push(record);
        }
        session
            .draft
            .as_mut()
            .unwrap()
            .placement_water_surface_count = count;
    }
    app.world_mut()
        .resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
            selection.publish(members, world.resource::<EditorAuthoringSession>())
        });
    (app, node)
}

fn key(app: &mut App, node: Entity, code: KeyCode, cursor: Vec2) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(code);
    step(app, node, cursor);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(code);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ControlLeft);
}
fn until(app: &mut App, node: Entity, cursor: Vec2, done: impl Fn(&PlacementBatchState) -> bool) {
    for _ in 0..256 {
        step(app, node, cursor);
        if done(app.world().resource::<PlacementBatchState>()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!(
        "操作应在有限帧内完成：{}",
        app.world().resource::<MapEditorState>().status
    );
}
fn target(app: &mut App) {
    let mut draft = test_heightfield_draft(Arc::new(vec![2; 121]));
    let map = MapId::parse("MAP-paste-target").unwrap();
    draft.map_id = map.clone();
    draft.manifest.id = map.clone();
    draft.placements.map_id = map.clone();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft = Some(draft);
        session.placement_previews.clear();
        session.undo.clear();
        session.redo.clear();
        session.revision += 1;
        session.placement_structure_revision += 1;
    }
    let mut editor = app.world_mut().resource_mut::<MapEditorState>();
    editor.selected_map_id = Some(map.clone());
    editor.dirty = false;
    editor.view = crate::types::EditorView::Top;
    let mut viewport = app
        .world_mut()
        .resource_mut::<crate::resources::MapEditorViewportState>();
    viewport.framed_map_id = Some(map);
    batch_top_camera(app);
    app.world_mut()
        .resource_mut::<PlacementSelectionSet>()
        .clear();
    app.world_mut()
        .resource_mut::<crate::ui::gltf_browser::EditorGltfBrowserState>()
        .selected_placement_id = None;
}

#[test]
fn cross_map_preview_preserves_full_records_then_commits_and_undoes_once() {
    let (mut app, node) = app(2);
    let source = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .clone();
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    target(&mut app);
    let cursor = camera(&app).project(Vec3::new(4.0, 2.0, 4.0)).unwrap();
    key(&mut app, node, KeyCode::KeyV, cursor);
    until(&mut app, node, cursor, |state| {
        state
            .paste
            .as_ref()
            .is_some_and(|paste| paste.point.is_some() && paste.roots.len() == 2)
    });
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert!(!app.world().resource::<MapEditorState>().dirty);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, cursor);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    until(&mut app, node, cursor, |state| !state.busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    let pasted = session
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .clone();
    assert_eq!(pasted.len(), 2);
    assert_eq!(session.undo.len(), 1);
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .placement_water_surface_count,
        2
    );
    for i in 0..2 {
        assert_ne!(pasted[i].id, source[i].id);
        assert_eq!(pasted[i].asset_path, source[i].asset_path);
        assert_eq!(pasted[i].asset_target, source[i].asset_target);
        assert_eq!(pasted[i].scale, source[i].scale);
        assert_eq!(pasted[i].yaw_degrees, source[i].yaw_degrees);
        assert_eq!(pasted[i].water_surfaces, source[i].water_surfaces);
        let root = session.placement_previews[&pasted[i].id];
        assert!(
            app.world()
                .get::<GlobalTransform>(root)
                .unwrap()
                .translation()
                .distance(Vec3::from_array(pasted[i].translation))
                < 0.002
        );
    }
    assert!(Vec3::from_array(pasted[0].translation).distance(Vec3::new(3.975, 2.0, 4.0)) < 0.002);
    assert!(Vec3::from_array(pasted[1].translation).distance(Vec3::new(4.025, 3.0, 4.0)) < 0.002);
    assert!((pasted[1].terrain_offset - 1.0).abs() < 0.002);
    assert_eq!(
        app.world()
            .resource::<PlacementBatchState>()
            .clipboard
            .as_ref()
            .unwrap()
            .records,
        source
    );
    assert_eq!(
        app.world()
            .resource::<PlacementSelectionSet>()
            .members
            .len(),
        2
    );
    assert!(
        app.world()
            .resource::<crate::ui::gltf_browser::EditorGltfBrowserState>()
            .selected_placement_id
            .is_none()
    );
    for undo in [true, false] {
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .undo_requested = undo;
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .redo_requested = !undo;
        crate::authoring::drive_undo_redo_to_completion(&mut app);
        assert_eq!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .unwrap()
                .placements
                .placements,
            if undo { Vec::new() } else { pasted.clone() }
        );
        app.world_mut()
            .run_system_once(crate::authoring::courtyards::sync_visuals)
            .unwrap();
    }
}

#[test]
fn paste_escape_cleans_ghosts_in_budget_and_keeps_clipboard() {
    let (mut app, node) = app(17);
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    assert_eq!(
        app.world()
            .resource::<PlacementBatchState>()
            .capture
            .as_ref()
            .unwrap()
            .records
            .len(),
        0
    );
    step(&mut app, node, Vec2::ZERO);
    assert_eq!(
        app.world()
            .resource::<PlacementBatchState>()
            .capture
            .as_ref()
            .unwrap()
            .records
            .len(),
        16
    );
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    target(&mut app);
    let cursor = camera(&app).project(Vec3::new(4.0, 2.0, 4.0)).unwrap();
    key(&mut app, node, KeyCode::KeyV, cursor);
    until(&mut app, node, cursor, |state| {
        state
            .paste
            .as_ref()
            .is_some_and(|paste| paste.roots.len() == 16)
    });
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    step(&mut app, node, cursor);
    assert!(
        app.world()
            .resource::<PlacementBatchState>()
            .paste
            .is_none()
    );
    assert_eq!(
        app.world().resource::<PlacementBatchState>().cleaning.len(),
        17
    );
    step(&mut app, node, cursor);
    assert_eq!(
        app.world().resource::<PlacementBatchState>().cleaning.len(),
        1
    );
    until(&mut app, node, cursor, |state| !state.busy());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert_eq!(
        app.world()
            .resource::<PlacementBatchState>()
            .clipboard
            .as_ref()
            .unwrap()
            .records
            .len(),
        17
    );
}

#[test]
fn batch_delete_is_one_history_and_restores_every_field_with_water_count() {
    let (mut app, node) = app(3);
    let before = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .clone();
    key(&mut app, node, KeyCode::Delete, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .is_empty()
    );
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    app.world_mut()
        .run_system_once(crate::authoring::courtyards::sync_visuals)
        .unwrap();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .undo_requested = true;
    crate::authoring::drive_undo_redo_to_completion(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements,
        before
    );
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .placement_water_surface_count,
        3
    );
}

#[test]
fn copy_respects_text_focus_and_f12_can_cancel_and_request_safe_close() {
    let (mut app, node) = app(17);
    app.world_mut()
        .resource_mut::<crate::ui::text_input::EditorTextInputGuard>()
        .block_shortcuts = true;
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    assert!(!app.world().resource::<PlacementBatchState>().busy());
    app.world_mut()
        .resource_mut::<crate::ui::text_input::EditorTextInputGuard>()
        .block_shortcuts = false;
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F12);
    app.world_mut()
        .run_system_once(handle_placement_batch)
        .unwrap();
    assert!(app.world().resource::<MapEditorState>().close_requested);
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::F12)
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .revision += 1;
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    assert!(
        app.world()
            .resource::<PlacementBatchState>()
            .clipboard
            .is_none()
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn late_paste_result_after_map_revision_change_cannot_commit_any_member() {
    let (mut app, node) = app(2);
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    target(&mut app);
    let cursor = camera(&app).project(Vec3::new(4.0, 2.0, 4.0)).unwrap();
    key(&mut app, node, KeyCode::KeyV, cursor);
    until(&mut app, node, cursor, |state| {
        state
            .paste
            .as_ref()
            .is_some_and(|paste| paste.point.is_some())
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<PlacementBatchState>().job.is_some());
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .revision += 1;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    until(&mut app, node, cursor, |state| !state.busy());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert_eq!(
        app.world()
            .resource::<PlacementBatchState>()
            .clipboard
            .as_ref()
            .unwrap()
            .records
            .len(),
        2
    );
}

#[test]
fn empty_paste_container_retires_when_all_children_are_removed() {
    let (mut app, node) = app(2);
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    target(&mut app);
    let cursor = camera(&app).project(Vec3::new(4.0, 2.0, 4.0)).unwrap();
    key(&mut app, node, KeyCode::KeyV, cursor);
    until(&mut app, node, cursor, |state| {
        state
            .paste
            .as_ref()
            .is_some_and(|paste| paste.point.is_some())
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, cursor);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    until(&mut app, node, cursor, |state| !state.busy());
    let roots = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .iter()
        .map(|record| {
            app.world()
                .resource::<EditorAuthoringSession>()
                .placement_previews[&record.id]
        })
        .collect::<Vec<_>>();
    let parent = app.world().get::<ChildOf>(roots[0]).unwrap().parent();
    for root in roots {
        app.world_mut().entity_mut(root).remove::<ChildOf>();
    }
    app.world_mut().flush();
    assert!(app.world().get_entity(parent).is_err());
    assert!(
        !app.world()
            .resource::<EditorAuthoringSession>()
            .placement_previews
            .values()
            .any(|entity| *entity == parent)
    );
}

#[test]
fn toolbar_copy_works_over_controls_and_late_selection_change_rejects_result() {
    let (mut app, node) = app(2);
    app.world_mut()
        .resource_mut::<ViewportModeState>()
        .pointer_over_controls = true;
    app.world_mut().spawn((
        super::super::PlacementBatchButton(super::super::PlacementBatchAction::Copy),
        feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed,
    ));
    step(&mut app, node, Vec2::ZERO);
    assert!(
        app.world()
            .resource::<PlacementBatchState>()
            .capture
            .is_some()
    );
    step(&mut app, node, Vec2::ZERO);
    assert!(app.world().resource::<PlacementBatchState>().job.is_some());
    app.world_mut()
        .resource_mut::<PlacementSelectionSet>()
        .clear();
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    assert!(
        app.world()
            .resource::<PlacementBatchState>()
            .clipboard
            .is_none()
    );
}

#[test]
fn close_button_cancels_paste_without_losing_clipboard_or_writing_map() {
    let (mut app, node) = app(2);
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    target(&mut app);
    let cursor = camera(&app).project(Vec3::new(4.0, 2.0, 4.0)).unwrap();
    key(&mut app, node, KeyCode::KeyV, cursor);
    until(&mut app, node, cursor, |state| {
        state
            .paste
            .as_ref()
            .is_some_and(|paste| paste.point.is_some())
    });
    app.world_mut().spawn((
        crate::components::MapEditorActionButton(crate::types::EditorAction::Close),
        feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed,
    ));
    step(&mut app, node, cursor);
    assert!(app.world().resource::<MapEditorState>().close_requested);
    assert!(
        app.world()
            .resource::<PlacementBatchState>()
            .paste
            .is_none()
    );
    until(&mut app, node, cursor, |state| !state.busy());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert_eq!(
        app.world()
            .resource::<PlacementBatchState>()
            .clipboard
            .as_ref()
            .unwrap()
            .records
            .len(),
        2
    );
}

#[test]
fn duplicate_keeps_original_group_and_existing_clipboard_independent() {
    let (mut app, node) = app(2);
    let before = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .clone();
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    batch_top_camera(&mut app);
    let cursor = camera(&app).project(Vec3::new(7.0, 2.0, 7.0)).unwrap();
    key(&mut app, node, KeyCode::KeyD, cursor);
    until(&mut app, node, cursor, |state| {
        state
            .paste
            .as_ref()
            .is_some_and(|paste| paste.point.is_some())
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, cursor);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    until(&mut app, node, cursor, |state| !state.busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    let records = &session.draft.as_ref().unwrap().placements.placements;
    assert_eq!(&records[..2], &before);
    assert_eq!(records.len(), 4);
    assert_ne!(records[2].id, records[0].id);
    assert_eq!(session.undo.len(), 1);
    assert_eq!(
        app.world()
            .resource::<PlacementBatchState>()
            .clipboard
            .as_ref()
            .unwrap()
            .records,
        before
    );
}

#[test]
fn clicking_a_missed_ground_ray_unfreezes_and_allows_a_new_valid_position() {
    let (mut app, node) = app(2);
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    target(&mut app);
    let miss = camera(&app).project(Vec3::new(-1.0, 2.0, 5.0)).unwrap();
    key(&mut app, node, KeyCode::KeyV, miss);
    until(&mut app, node, miss, |state| {
        state.job.is_none()
            && state
                .paste
                .as_ref()
                .is_some_and(|paste| paste.started.is_some() && paste.point.is_none())
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, miss);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    until(&mut app, node, miss, |state| {
        state.job.is_none()
            && state
                .paste
                .as_ref()
                .is_some_and(|paste| paste.clicked.is_none())
    });
    let valid = camera(&app).project(Vec3::new(4.0, 2.0, 4.0)).unwrap();
    until(&mut app, node, valid, |state| {
        state
            .paste
            .as_ref()
            .is_some_and(|paste| paste.point.is_some_and(|(key, _)| key.normalized == valid))
    });
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .is_empty()
    );
}
