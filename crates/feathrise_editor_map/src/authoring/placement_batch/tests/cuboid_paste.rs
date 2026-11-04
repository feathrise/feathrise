//! 使用真实复制/粘贴快捷键、相机射线与后台worker验证空地图工作平面。

use super::*;
use feathrise_map_content::{MapPlacementAssetTarget, MapTerrainDescriptor};

fn cubes() -> (App, Entity) {
    let (mut app, node) = app(2);
    app.init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_resource::<feathrise_editor_core::EditorSettings>();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.placement_water_surface_count = 0;
        for (index, record) in draft.placements.placements.iter_mut().enumerate() {
            record.asset_path.clear();
            record.asset_target = MapPlacementAssetTarget::Cuboid {
                size_mm: [2000, if index == 0 { 4000 } else { 2000 }, 2000],
                color_srgb: [160; 3],
                roughness_milli: 700,
                collision: true,
            };
            record.water_surfaces.clear();
            record.translation = [4.0 + index as f32 * 2.0, 5.0 + index as f32 * 3.0, 5.0];
            record.scale[1] = if index == 0 { 1.5 } else { 2.0 };
            record.height_mode = MapPlacementHeightMode::FollowTerrain;
            record.terrain_offset = record.translation[1] - 2.0;
        }
    }
    (app, node)
}

fn empty_target(app: &mut App) {
    target(app);
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let revision = session.terrain_revision;
    let draft = session.draft.as_mut().unwrap();
    draft.manifest.terrains = Some(Arc::from([]));
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    draft.terrain_mesh = None;
    draft.terrain_editing_index = None;
    draft.heights = None;
    draft.terrain_ground = Some(
        crate::authoring::terrain_ground::TerrainGroundSnapshot::empty(
            draft.map_id.clone(),
            revision,
        ),
    );
}

#[test]
fn empty_map_ctrl_copy_paste_snaps_bottom_to_plane_and_commits_one_complete_group() {
    let (mut app, node) = cubes();
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    assert_eq!(
        app.world()
            .resource::<PlacementBatchState>()
            .clipboard
            .as_ref()
            .unwrap()
            .origin,
        Vec3::new(5.0, 2.0, 5.0)
    );
    empty_target(&mut app);
    app.world_mut()
        .resource_mut::<feathrise_editor_core::EditorSettings>()
        .editor_snapping
        .enabled = true;
    let cursor = camera(&app).project(Vec3::new(4.4, 0.0, 4.6)).unwrap();
    key(&mut app, node, KeyCode::KeyV, cursor);
    until(&mut app, node, cursor, |state| {
        state
            .paste
            .as_ref()
            .is_some_and(|paste| paste.point.is_some() && paste.roots.len() == 2)
    });
    {
        let paste = app
            .world()
            .resource::<PlacementBatchState>()
            .paste
            .as_ref()
            .unwrap();
        assert!(paste.builtin_only);
        assert!(
            paste
                .point
                .unwrap()
                .1
                .abs_diff_eq(Vec3::new(4.0, 0.0, 5.0), 0.002)
        );
        assert!(
            app.world()
                .get::<avian3d::prelude::RigidBodyDisabled>(paste.parent)
                .is_some()
        );
        for root in &paste.roots {
            assert!(app.world().get::<Children>(*root).is_some_and(|children| {
                children
                    .iter()
                    .any(|child| app.world().get::<Mesh3d>(child).is_some())
            }));
        }
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app, node, cursor);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    until(&mut app, node, cursor, |state| !state.busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    let records = session
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .clone();
    assert_eq!(records.len(), 2);
    assert!(Vec3::from_array(records[0].translation).abs_diff_eq(Vec3::new(3.0, 3.0, 5.0), 0.002));
    assert!(Vec3::from_array(records[1].translation).abs_diff_eq(Vec3::new(5.0, 6.0, 5.0), 0.002));
    assert!(records.iter().all(
        |record| record.height_mode == MapPlacementHeightMode::FixedHeight
            && record.terrain_offset == 0.0
            && record.terrain_binding.is_none()
    ));
    assert_eq!(session.undo.len(), 1);
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
            if undo { Vec::new() } else { records.clone() }
        );
        app.world_mut()
            .run_system_once(crate::authoring::courtyards::sync_visuals)
            .unwrap();
    }
}

#[test]
fn empty_map_external_paste_does_not_gain_the_cuboid_work_plane() {
    let (mut app, node) = app(1);
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    empty_target(&mut app);
    let cursor = camera(&app).project(Vec3::new(4.0, 0.0, 4.0)).unwrap();
    key(&mut app, node, KeyCode::KeyV, cursor);
    until(&mut app, node, cursor, |state| {
        state
            .paste
            .as_ref()
            .is_some_and(|paste| paste.started.is_some() && !paste.builtin_only)
            && state.job.is_none()
    });
    let state = app.world().resource::<PlacementBatchState>();
    assert!(state.paste.as_ref().unwrap().point.is_none());
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
