//! 吸附真实地形、异步身份、一次消费和按钮隔离回归，不使用场景文件或GPU。
use feathrise_editor_ui::UiInteraction;
use super::{
    components::{SnapAction, SnapButton},
    resources::MeshSnapCursor,
    surface,
    types::MeshSnapKey,
};
use crate::{
    resources::{SceneEditorSettingsState, SceneEditorState},
    viewport::vegetation_ground::GroundCursorTarget,
};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use feathrise_scene::ClipmapGroundHit;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorSnapSettings;
use feathrise_editor_core::EditorSnapMode;

fn key() -> MeshSnapKey {
    MeshSnapKey {
        scene_id: feathrise_scene_content::SceneId("snap-test".into()),
        target: GroundCursorTarget::PlaceAsset {
            asset_path: "gltf/test.glb".into(),
            asset_target: Default::default(),
        },
        author: 7,
        terrain: 3,
        mesh: 11,
        index: 12,
        object: None,
        bounds: [0.0_f32, 0.0, 10.0, 10.0].map(f32::to_bits),
        camera: Entity::PLACEHOLDER,
        view: crate::types::EditorView::Top,
        projection: true,
        camera_pose: [0; 7],
        render_size: UVec2::new(800, 600),
        point: [0; 3],
        snap: EditorSnapSettings::default().translation_snap(true, [0.0; 3]),
    }
}

#[test]
fn confirmed_mesh_snap_survives_control_release_but_rejects_input_changes() {
    let clicked = key();
    let mut next = clicked.clone();
    next.point = [1; 3];
    next.snap = EditorSnapSettings::default().translation_snap(false, [0.0; 3]);
    assert!(clicked.same_scope(&next)); // 已确认的鼠标射线和Ctrl状态被冻结，后续悬停不改写。
    for field in 0..6 {
        let mut next = clicked.clone();
        match field {
            0 => next.author += 1,
            1 => next.terrain += 1,
            2 => next.mesh += 1,
            3 => next.index += 1,
            4 => next.render_size.x += 1,
            _ => {
                next.snap = EditorSnapSettings {
                    mode: EditorSnapMode::Relative,
                    ..default()
                }
                .translation_snap(true, [0.0; 3])
            }
        }
        assert!(!clicked.same_scope(&next));
    }
}

#[test]
fn mesh_snap_completion_is_consumed_once_and_never_crosses_scenes() {
    let clicked = key();
    let point = Vec3::new(1.0, -5.0, 2.0);
    let mut state = MeshSnapCursor::default();
    state.completed = Some((clicked.clone(), ClipmapGroundHit::Hit(point)));
    assert_eq!(
        state.take_click(
            &clicked.scene_id,
            &clicked.target,
            clicked.author,
            clicked.terrain
        ),
        Some(ClipmapGroundHit::Hit(point))
    );
    assert_eq!(
        state.take_click(
            &clicked.scene_id,
            &clicked.target,
            clicked.author,
            clicked.terrain
        ),
        None
    );
    state.completed = Some((clicked.clone(), ClipmapGroundHit::Hit(point)));
    assert_eq!(
        state.take_click(
            &feathrise_scene_content::SceneId("other".into()),
            &clicked.target,
            clicked.author,
            clicked.terrain
        ),
        None
    );
}

#[test]
fn cancelled_mesh_snap_keeps_the_real_cpu_slot_until_it_finishes() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let clicked = key();
    let mut state = MeshSnapCursor::default();
    state.task = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
        std::future::pending::<()>().await;
        (clicked, ClipmapGroundHit::Failed)
    }));
    state.pending = Some(key());
    state.requires_query = true;
    state.cancel();
    state.cancel();
    assert!(state.task.is_some());
    assert!(!state.requires_query());
}

#[test]
fn mesh_snap_stays_on_the_selected_cave_floor_and_rejects_missing_support() {
    let mut mesh = feathrise_terrain::TerrainMeshInput {
        positions: Vec::new(),
        indices: Vec::new(),
    };
    for height in [0.0, 10.0] {
        let base = mesh.positions.len() as u32;
        mesh.positions.extend([
            [-2.0, height, -2.0],
            [2.0, height, -2.0],
            [2.0, height, 2.0],
            [-2.0, height, 2.0],
        ]);
        mesh.indices
            .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    let index = feathrise_terrain::build_terrain_editing_index(&mesh).unwrap();
    let snap = EditorSnapSettings {
        enabled: true,
        ..default()
    }
    .translation_snap(false, [0.0; 3]);
    assert_eq!(
        surface::mesh_hit(&mesh, &index, Vec3::new(0.6, 0.0, -0.6), snap, None),
        ClipmapGroundHit::Hit(Vec3::new(1.0, 0.0, -1.0))
    );
    assert_eq!(
        surface::mesh_hit(&mesh, &index, Vec3::new(2.6, 0.0, 0.0), snap, None),
        ClipmapGroundHit::Miss
    );
}

#[test]
fn toolbar_switches_preferences_without_dirtying_author_data_and_respects_modals() {
    let (mut app, _, _) = crate::viewport::placement_move::interaction_tests::app();
    app.init_resource::<EditorSettings>();
    let before_revision = app
        .world()
        .resource::<crate::authoring::EditorAuthoringSession>()
        .revision;
    for action in [
        SnapAction::Enabled,
        SnapAction::Mode,
        SnapAction::Step,
        SnapAction::Grid,
    ] {
        let button = app
            .world_mut()
            .spawn((SnapButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
            .id();
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .suppress_world_pick_this_frame = false;
        app.world_mut()
            .run_system_once(super::handle_buttons)
            .unwrap();
        app.world_mut().despawn(button);
    }
    let snap = app.world().resource::<EditorSettings>().editor_snapping;
    assert!(snap.enabled);
    assert!(!snap.show_grid);
    assert_eq!(snap.mode, EditorSnapMode::Relative);
    assert_eq!(snap.translation_meters, 2.0);
    assert_eq!(
        app.world()
            .resource::<crate::authoring::EditorAuthoringSession>()
            .revision,
        before_revision
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .suppress_world_pick_this_frame = false;
    app.world_mut()
        .resource_mut::<SceneEditorSettingsState>()
        .open = true;
    app.world_mut()
        .spawn((SnapButton(SnapAction::Enabled), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    app.world_mut()
        .run_system_once(super::handle_buttons)
        .unwrap();
    assert_eq!(app.world().resource::<EditorSettings>().editor_snapping, snap);
}

#[test]
fn mesh_snap_follows_sloped_support_and_rejects_a_disconnected_floor_below() {
    let build = |edge: f32| {
        let mut mesh = feathrise_terrain::TerrainMeshInput {
            positions: Vec::new(),
            indices: Vec::new(),
        };
        for (max_x, slope, offset) in [(edge, 2.0, 0.0), (2.0, 2.0, 0.2), (2.0, 0.0, -10.0)] {
            let base = mesh.positions.len() as u32;
            mesh.positions.extend([
                [-2.0, -2.0 * slope + offset, -2.0],
                [max_x, max_x * slope + offset, -2.0],
                [max_x, max_x * slope + offset, 2.0],
                [-2.0, -2.0 * slope + offset, 2.0],
            ]);
            mesh.indices
                .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        }
        let index = feathrise_terrain::build_terrain_editing_index(&mesh).unwrap();
        (mesh, index)
    };
    let snap = EditorSnapSettings {
        enabled: true,
        ..default()
    }
    .translation_snap(false, [0.0; 3]);
    let (mesh, index) = build(2.0);
    assert_eq!(
        surface::mesh_hit(&mesh, &index, Vec3::new(0.6, 1.2, 0.0), snap, None),
        ClipmapGroundHit::Hit(Vec3::new(1.0, 2.0, 0.0))
    );
    assert_eq!(
        surface::mesh_hit(&mesh, &index, Vec3::new(0.4, 0.8, 0.0), snap, None),
        ClipmapGroundHit::Hit(Vec3::ZERO)
    );
    let (mesh, index) = build(0.8);
    assert_eq!(
        surface::mesh_hit(&mesh, &index, Vec3::new(0.6, 1.2, 0.0), snap, None),
        ClipmapGroundHit::Miss
    );
    // 悬空目标下方有真实第二层，但当前所选面的断边不能被补查解释成一次合法跨层摆放。
}

#[test]
fn independent_terrain_snap_rejects_changed_object_pose_and_source_identity() {
    let mut clicked = key();
    clicked.object = Some(super::types::ObjectSnapIdentity {
        id: feathrise_scene_content::TerrainId::parse("snap-island-a").unwrap(),
        pose: [0.0_f32, 3.0, 0.0, 90.0, 2.0, 3.0, 4.0].map(f32::to_bits),
        generation: 11,
        snapshot: 101,
        visibility: 201,
    });
    for field in 0..5 {
        let mut next = clicked.clone();
        let object = next.object.as_mut().unwrap();
        match field {
            0 => object.id = feathrise_scene_content::TerrainId::parse("snap-island-b").unwrap(),
            1 => object.pose[0] = 20.0_f32.to_bits(),
            2 => object.generation += 1,
            3 => object.snapshot += 1,
            _ => object.visibility += 1,
        }
        assert!(!clicked.same_scope(&next)); // 作者修订相同也不能把不同承托身份的结果混用。
    }
    let mut state = MeshSnapCursor::default();
    state.completed = Some((clicked.clone(), ClipmapGroundHit::Hit(Vec3::ONE)));
    assert_eq!(state.frozen_terrain().unwrap().as_str(), "snap-island-a");
    assert_eq!(state.take_click(&clicked.scene_id, &clicked.target, clicked.author, clicked.terrain),
        Some(ClipmapGroundHit::Hit(Vec3::ONE)));
    assert!(state.frozen_terrain().is_none()); // TerrainId与落点属于同一次完整消费。
}