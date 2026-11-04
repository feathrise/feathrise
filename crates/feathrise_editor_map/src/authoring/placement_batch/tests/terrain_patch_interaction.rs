//! 普通静态三维地形沿真实快捷键、鼠标射线和历史链完成预览与采用；不提前安装地形碰撞。

use super::*;
use feathrise_map_content::{
    MapPlacementRecord, MapPlacementTerrainPatch, MapTerrainCutout, MapTerrainDescriptor,
    MapTerrainKind, SparseHeightSpec, TerrainId, TerrainObjectTransform,
};

fn legacy_descriptor(map: &MapId) -> MapTerrainDescriptor {
    let spec = SparseHeightSpec::new([-32.0; 2], [32.0; 2], 1.0, -8.0, 16.0).unwrap();
    MapTerrainDescriptor {
        kind: MapTerrainKind::SparseClipmap,
        height_path: Some(format!("maps/{map}/terrain/height_tiles.map_height_manifest.json")),
        resolution: [65; 2], base_height: spec.base_height, height_scale: spec.height_scale,
        collision_lods: feathrise_map_content::default_collision_lods_for_resolution([65; 2]),
        display_lods: Some(feathrise_map_content::default_display_lods_for_spec(&spec).unwrap()),
        ..MapTerrainDescriptor::empty()
    }
}

fn pose(record: &MapPlacementRecord) -> Transform {
    Transform::from_translation(record.translation.into())
        .with_rotation(Quat::from_rotation_y(record.yaw_degrees.to_radians()))
        .with_scale(record.scale.into())
}

fn assert_world_matrix(app: &App, root: Entity, expected: Mat4) {
    let actual = app.world().get::<GlobalTransform>(root).unwrap().to_matrix();
    // 同时比较中心外的八角，避免仅位置正确而旋转、三轴比例或父子组合矩阵丢失。
    for x in [-1.0, 1.0] { for y in [-1.0, 1.0] { for z in [-1.0, 1.0] {
        let point = Vec3::new(x, y, z);
        assert!(actual.transform_point3(point).abs_diff_eq(expected.transform_point3(point), 0.004));
    } } }
}

fn history_shortcut(app: &mut App, undo: bool) {
    let code = if undo { KeyCode::KeyZ } else { KeyCode::KeyY };
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(code);
    }
    app.world_mut().run_system_once(crate::ui::actions::handle_editor_shortcuts).unwrap();
    let editor = app.world().resource::<MapEditorState>();
    assert_eq!(editor.undo_requested, undo);
    assert_eq!(editor.redo_requested, !undo);
    crate::authoring::drive_undo_redo_to_completion(app);
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(code);
        keys.release(KeyCode::ControlLeft);
        keys.clear();
    }
    app.world_mut().run_system_once(crate::authoring::courtyards::sync_visuals).unwrap();
}

#[test]
fn legacy_static_patch_ctrl_copy_paste_preserves_world_pose_and_ctrl_history_without_early_collision() {
    let (mut app, node) = app(1);
    let source_frame = TerrainObjectTransform {
        translation: [80.0, 7.0, -40.0], yaw_degrees: 90.0,
        scale: [2.0, 3.0, 4.0], ..default()
    };
    let before = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.world_min = [-32.0; 2];
        draft.manifest.world_max = [32.0; 2];
        draft.manifest.terrain = legacy_descriptor(&draft.map_id);
        let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
        source.id = TerrainId::parse("source-transformed").unwrap();
        source.transform = source_frame;
        source.terrain.height_path = Some(format!("{}/height_tiles.map_height_manifest.json", source.private_folder(&draft.map_id)));
        draft.manifest.terrains = Some(Arc::from([source.clone()]));
        draft.manifest.terrain = MapTerrainDescriptor::empty();
        draft.placement_water_surface_count = 0;
        let record = &mut draft.placements.placements[0];
        record.water_surfaces.clear();
        record.yaw_degrees = 90.0;
        record.height_mode = MapPlacementHeightMode::FixedHeight;
        record.terrain_offset = 0.0;
        record.terrain_patch = Some(MapPlacementTerrainPatch {
            terrain_id: Some(source.id),
            cutout: Some(MapTerrainCutout { min: [-0.25; 2], max: [0.25; 2] }),
            ..default()
        });
        record.validated_transform(&draft.map_id, Rect::from_corners(Vec2::splat(-32.0), Vec2::splat(32.0))).unwrap();
        record.clone()
    };
    let source_world = source_frame.as_transform().to_matrix() * pose(&before).to_matrix();
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        selection.publish(vec![SelectedMember {
            id: before.id.clone(), index: 0, position: source_world.transform_point3(Vec3::ZERO),
        }], world.resource::<EditorAuthoringSession>());
    });
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    let clipboard = app.world().resource::<PlacementBatchState>().clipboard.clone().unwrap();
    assert_eq!(clipboard.records, vec![before.clone()]);
    assert_eq!(clipboard.patch_frames.as_ref(), &vec![Some(source_frame)]);

    target(&mut app);
    let map = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.world_min = [-32.0; 2];
        draft.manifest.world_max = [32.0; 2];
        draft.manifest.terrain = legacy_descriptor(&draft.map_id);
        draft.heights = None;
        draft.map_id.clone()
    };
    // 此无窗口夹具保留现有真实三角网格，走正式mesh ray fallback及旧主Clipmap采用分支。
    // GPU Clipmap快照和独立地形的已采用私有实例表仍需真实场景验证，不以测试setter绕过它们。
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
        .insert_resource(feathrise_map_content::ActiveDynamicMap { map_id: Some(map) })
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<crate::authoring::WaterNetworkEditState>()
        .init_resource::<crate::ui::surface_preset_editor::SurfacePresetEditorState>()
        .init_resource::<crate::ui::water_preset_editor::WaterPresetEditorState>()
        .init_resource::<crate::ui::building_presets::BuildingPresetEditorState>()
        .init_resource::<crate::game_authoring::map_interaction::MapInteractionState>();
    app.world_mut().resource_mut::<feathrise_editor_core::EditorSettings>().editor_snapping.enabled = false;
    let clicked = Vec3::new(4.0, 2.0, 4.0);
    let cursor = camera(&app).project(clicked).unwrap();
    key(&mut app, node, KeyCode::KeyV, cursor);
    until(&mut app, node, cursor, |state| state.paste.as_ref()
        .is_some_and(|paste| paste.point.is_some() && paste.roots.len() == 1));
    let (ghost_root, ghost_frame, expected_world) = {
        let paste = app.world().resource::<PlacementBatchState>().paste.as_ref().unwrap();
        let point = paste.point.unwrap().1;
        assert!(point.abs_diff_eq(clicked, 0.002));
        let root = paste.roots[0];
        let frame = paste.frames[0].expect("独立源的非identity父框架必须保留到确认时");
        assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), frame);
        assert_eq!(app.world().get::<Transform>(frame), Some(&source_frame.as_transform()));
        assert!(app.world().get::<feathrise_scene::MapPlacementInstance>(root).is_none());
        assert!(app.world().get::<feathrise_scene::TerrainPatchCollisionRequest>(root).is_none());
        assert!(app.world().get::<avian3d::prelude::Collider>(root).is_none());
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.is_empty());
        (root, frame, Mat4::from_translation(point - clipboard.origin) * source_world)
    };
    assert_world_matrix(&app, ghost_root, expected_world);

    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut app, node, cursor);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, node, cursor, |state| !state.busy());
    let placed = {
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.undo.len(), 1);
        let records = &session.draft.as_ref().unwrap().placements.placements;
        assert_eq!(records.len(), 1);
        assert_ne!(records[0].id, before.id);
        assert!(records[0].terrain_binding.is_none());
        let mut patch = before.terrain_patch.clone().unwrap();
        patch.terrain_id = None;
        assert_eq!(records[0].terrain_patch.as_ref(), Some(&patch));
        assert_eq!(session.placement_previews[&records[0].id], ghost_root);
        records[0].clone()
    };
    assert!(app.world().get_entity(ghost_frame).is_err());
    assert!(app.world().get::<feathrise_scene::MapPlacementInstance>(ghost_root).unwrap().is_terrain_patch());
    assert!(app.world().get::<feathrise_scene::TerrainPatchCollisionRequest>(ghost_root).is_none());
    assert_world_matrix(&app, ghost_root, expected_world);

    history_shortcut(&mut app, true);
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.is_empty());
    assert!(app.world().get_entity(ghost_root).is_err());
    history_shortcut(&mut app, false);
    step(&mut app, node, cursor); // 正式TransformPlugin传播重建根；不手工拼接GlobalTransform。
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, vec![placed.clone()]);
    let restored = session.placement_previews[&placed.id];
    assert_eq!(app.world().get::<Transform>(restored), Some(&pose(&placed)));
    assert!(app.world().get::<feathrise_scene::TerrainPatchCollisionRequest>(restored).is_none());
    assert_world_matrix(&app, restored, expected_world);
    assert_eq!(clipboard.records, vec![before]);
}
