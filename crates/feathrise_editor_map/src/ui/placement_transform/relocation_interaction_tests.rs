//! 多地形重定位的真实Collider、吸附补查与单记录作者事务回归；不加载窗口或修改地图资产。
use super::*;
use crate::{
    authoring::terrain_ground::{
        CursorIdentity, CursorRay, TerrainGroundCursor, TerrainGroundSnapshot,
        cursor_for_tests, cursor_running_for_tests, hold_cursor_result_for_tests,
    },
    ui::{asset_preview::AssetPreviewState, numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard},
    viewport::{modes::{ViewportMode, ViewportModeState}, snapping::{MeshSnapCursor, update_mesh_cursor}},
};
use feathrise_map_content::{MapTerrainDescriptor, TerrainId, TerrainObjectTransform};
use feathrise_editor_core::EditorSettings;
use std::{sync::Arc, time::{Duration, Instant}};

fn arm(app: &mut App, index: usize) {
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let target = super::super::PlacementTransformTarget {
        map_id: draft.map_id.clone(), placement_id: draft.placements.placements[index].id.clone(),
        record_index: index, structure_revision: session.placement_structure_revision,
        authoring_revision: session.revision,
    };
    app.world_mut().resource_mut::<EditorGltfBrowserState>()
        .select_indexed_test_placement(&target.placement_id, index, target.structure_revision);
    app.world_mut().resource_mut::<PlacementTransformInputState>().relocation_target = Some(target);
}

fn identity(app: &App) -> CursorIdentity {
    let session = app.world().resource::<EditorAuthoringSession>();
    let editor = app.world().resource::<MapEditorState>();
    let viewport = app.world().resource::<crate::resources::MapEditorViewportState>();
    let transform = app.world().resource::<PlacementTransformInputState>();
    let (_, index, id, structure, author) = transform.relocation_ground_identity().unwrap();
    let draft = session.draft.as_ref().unwrap();
    let origin = crate::authoring::placement_pose::placement_record_world_position(
        draft, &draft.placements.placements[index]).unwrap().to_array();
    CursorIdentity {
        map: session.draft.as_ref().unwrap().map_id.clone(), revision: session.revision,
        terrain_revision: session.terrain_revision, ground_snapshot: session.draft.as_ref().unwrap().terrain_ground.as_ref()
            .map(crate::authoring::terrain_ground::CursorGroundSnapshot::capture),
        tool: editor.tool, asset: None, asset_target: None, portal: None,
        camera: Mat4::IDENTITY, size: viewport.render_size, camera_entity: viewport.camera,
        projection: Mat4::IDENTITY, view: editor.view, orthographic: editor.orthographic,
        extent: viewport.extent.to_bits(), visibility: Arc::as_ptr(&editor.terrain_hidden) as usize,
        relocation: Some((index, id.into(), structure, author)),
        translation_snap: Some(app.world().resource::<EditorSettings>().editor_snapping.translation_snap(false, origin)),
    }
}

fn fixture(enabled: bool) -> (App, Entity, TerrainId) {
    let (mut app, root, _) = crate::viewport::placement_move::interaction_tests::app();
    let terrain = TerrainId::parse("relocation-real-island").unwrap();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.terrain_revision = 0;
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = terrain.clone(); object.transform = TerrainObjectTransform::default();
        draft.manifest.terrains = Some(Arc::from([object]));
        draft.manifest.terrain = MapTerrainDescriptor::empty();
        draft.terrain_mesh = None; draft.terrain_editing_index = None; draft.heights = None;
        let collider = avian3d::prelude::Collider::trimesh(
            vec![Vec3::new(0.0, 2.0, 0.0), Vec3::new(10.0, 2.0, 0.0),
                Vec3::new(10.0, 2.0, 10.0), Vec3::new(0.0, 2.0, 10.0)],
            vec![[0, 2, 1], [0, 3, 2]],
        );
        draft.terrain_ground = Some(TerrainGroundSnapshot::test_mesh(
            draft.map_id.clone(), terrain.clone(), TerrainObjectTransform::default(), collider));
        let mut other = draft.placements.placements[0].clone();
        other.id = "same-pose-other".into();
        draft.placements.placements.push(other); // 已存在的另一记录位置相同，目标身份不能只靠origin区分。
        draft.placement_water_surface_count = 2;
    }
    app.world_mut().resource_mut::<MapEditorState>().tool = crate::types::EditorTool::Inspect;
    app.world_mut().resource_mut::<MapEditorState>().placement_transform_enabled = false;
    app.world_mut().resource_mut::<MapEditorState>().cursor_world = None;
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    let mut settings = EditorSettings::default(); settings.editor_snapping.enabled = enabled;
    app.insert_resource(settings)
        .init_resource::<AssetPreviewState>()
        .init_resource::<VegetationGroundCursor>()
        .init_resource::<MeshSnapCursor>()
        .init_resource::<TerrainGroundCursor>()
        .add_systems(Update, (update_mesh_cursor, apply_selected_placement_relocation).chain()
            .after(update_map_editor_numeric_scrub_mouse_guard));
    arm(&mut app, 0);
    let context = identity(&app);
    let mut cursor = app.world_mut().resource_mut::<TerrainGroundCursor>();
    cursor.identity = Some(context);
    cursor.ray = Some(CursorRay { origin: Vec3::new(6.3, 20.0, 6.3), direction: Vec3::NEG_Y, epoch: 1 });
    (app, root, terrain)
}

fn step(app: &mut App) {
    app.world_mut().resource_mut::<MapEditorState>().ground_click_world = None;
    cursor_for_tests(app.world_mut());
    app.update();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
}

fn begin(app: &mut App, control: bool) -> std::sync::mpsc::Sender<()> {
    let context = identity(app);
    let origin = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        crate::authoring::placement_pose::placement_record_world_position(draft, &draft.placements.placements[0]).unwrap().to_array()
    };
    let snap = app.world().resource::<EditorSettings>().editor_snapping.translation_snap(control,
        origin);
    {
        let mut cursor = app.world_mut().resource_mut::<TerrainGroundCursor>();
        let ray = cursor.ray.unwrap();
        cursor.capture_click(context, ray, Some(snap));
    }
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    if control { app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft); }
    cursor_for_tests(app.world_mut());
    let release = hold_cursor_result_for_tests(app.world_mut());
    app.update(); // 第一个真实点击仍在屏障后；不能把上一hover误用为重定位落点。
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::ControlLeft);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    release
}

#[test]
fn terrain_patch_relocation_uses_real_click_in_source_space_without_ordinary_binding() {
    let (mut app, root, terrain) = fixture(false);
    app.add_plugins(bevy::transform::TransformPlugin);
    let frame = TerrainObjectTransform { translation: [-18.0, 5.0, 27.0], yaw_degrees: 31.0,
        pitch_degrees: 17.0, roll_degrees: -12.0, scale: [2.0, 3.0, 5.0] };
    let (map, object, before) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let source = &mut Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0];
        source.transform = frame;
        source.terrain.kind = feathrise_map_content::MapTerrainKind::SparseClipmap;
        source.world_min = [-20.0; 2]; source.world_max = [20.0; 2];
        let object = source.clone();
        let collider = avian3d::prelude::Collider::trimesh(
            vec![Vec3::new(0.0, 2.0, 0.0), Vec3::new(10.0, 2.0, 0.0),
                Vec3::new(10.0, 2.0, 10.0), Vec3::new(0.0, 2.0, 10.0)],
            vec![[0, 2, 1], [0, 3, 2]],
        );
        draft.terrain_ground = Some(TerrainGroundSnapshot::test_mesh(draft.map_id.clone(), terrain.clone(), frame, collider));
        draft.placement_water_surface_count = 1;
        let record = &mut draft.placements.placements[0];
        record.translation = [2.0, 3.0, 2.0]; record.water_surfaces.clear();
        record.terrain_binding = None;
        record.terrain_patch = Some(feathrise_map_content::MapPlacementTerrainPatch { terrain_id: Some(terrain.clone()), ..Default::default() });
        (draft.map_id.clone(), object, record.clone())
    };
    let parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(app.world_mut(), &map, &[object])[0];
    let local = |record: &MapPlacementRecord| Transform::from_translation(record.translation.into())
        .with_rotation(Quat::from_rotation_y(record.yaw_degrees.to_radians())).with_scale(record.scale.into());
    app.world_mut().entity_mut(root).insert((MapPlacementInstance::new(&map, &before), local(&before), ChildOf(parent)));
    arm(&mut app, 0);
    let hit = frame.as_transform().transform_point(Vec3::new(6.3, 2.0, 6.3));
    app.world_mut().resource_mut::<TerrainGroundCursor>().ray.as_mut().unwrap().origin = hit + Vec3::Y * 30.0;
    let context = identity(&app);
    app.world_mut().resource_mut::<TerrainGroundCursor>().identity = Some(context);
    let release = begin(&mut app, false);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements[0], before);
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.world().resource::<EditorAuthoringSession>().revision == 7 {
        assert!(Instant::now() < deadline, "补丁真实吸附必须在有限时间内完成");
        step(&mut app); std::thread::sleep(Duration::from_millis(1));
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    let after = &session.draft.as_ref().unwrap().placements.placements[0];
    assert!(Vec3::from_array(after.translation).abs_diff_eq(Vec3::new(6.3, 2.0, 6.3), 0.001));
    assert!(after.terrain_binding.is_none());
    assert_eq!(after.terrain_patch.as_ref().unwrap().terrain_id.as_ref(), Some(&terrain));
    assert!(!after.height_mode.follows_terrain());
    assert_eq!(session.undo.len(), 1);
    assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), Some(parent));
    let original_world = frame.as_transform().to_matrix() * local(&before).to_matrix();
    let expected = Mat4::from_translation(hit - original_world.w_axis.truncate()) * original_world;
    let actual = app.world().get::<GlobalTransform>(root).unwrap().to_matrix();
    for point in [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z] {
        assert!(actual.transform_point3(point).abs_diff_eq(expected.transform_point3(point), 0.001));
    }
}

#[test]
fn terrain_patch_relocation_rejects_retired_source_after_real_click_freezes() {
    use bevy::ecs::system::RunSystemOnce;

    fn runtime_parents(app: &mut App) -> (Option<Entity>, Option<Entity>) {
        app.world_mut().run_system_once(|session: Res<EditorAuthoringSession>, queries: feathrise_scene::TerrainObjectQueries| {
            let draft = session.draft.as_ref().unwrap();
            let objects = draft.manifest.terrains.as_ref().unwrap();
            (queries.instance(&draft.map_id, &objects[0].id), queries.instance(&draft.map_id, &objects[1].id))
        }).unwrap()
    }

    for missing_definition in [true, false] {
        let (mut app, root, source_id) = fixture(false);
        app.add_plugins(bevy::transform::TransformPlugin);
        let target_id = TerrainId::parse("relocation-ready-target").unwrap();
        let (map, objects, before, ground) = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            let mut source = draft.manifest.terrains.as_ref().unwrap()[0].clone();
            source.terrain.kind = feathrise_map_content::MapTerrainKind::SparseClipmap;
            source.world_min = [-20.0; 2]; source.world_max = [20.0; 2];
            source.transform.translation = [-30.0, 0.0, 0.0];
            let mut target = source.clone();
            target.id = target_id.clone(); target.transform = TerrainObjectTransform::default();
            let objects = [source, target];
            draft.manifest.terrains = Some(Arc::from(objects.clone()));
            let collider = avian3d::prelude::Collider::trimesh(
                vec![Vec3::new(0.0, 2.0, 0.0), Vec3::new(10.0, 2.0, 0.0),
                    Vec3::new(10.0, 2.0, 10.0), Vec3::new(0.0, 2.0, 10.0)],
                vec![[0, 2, 1], [0, 3, 2]],
            );
            let ground = TerrainGroundSnapshot::test_meshes(draft.map_id.clone(), &objects, collider);
            draft.terrain_ground = Some(ground.clone());
            draft.placement_water_surface_count = 1;
            let record = &mut draft.placements.placements[0];
            record.translation = [2.0, 3.0, 2.0]; record.water_surfaces.clear();
            record.terrain_binding = None;
            record.terrain_patch = Some(feathrise_map_content::MapPlacementTerrainPatch { terrain_id: Some(source_id.clone()), ..Default::default() });
            (draft.map_id.clone(), objects, record.clone(), ground)
        };
        let parents = feathrise_scene::runtime_test_support::install_active_terrain_instances(app.world_mut(), &map, &objects);
        let transform = Transform::from_translation(before.translation.into())
            .with_rotation(Quat::from_rotation_y(before.yaw_degrees.to_radians())).with_scale(before.scale.into());
        let path = feathrise_map_content::MapInstancePath::root(map.clone()).terrain(source_id.clone());
        app.world_mut().entity_mut(root).insert((MapPlacementInstance::new(&map, &before), transform,
            ChildOf(parents[0]), feathrise_scene::TerrainVolumeAuthorPreviewRoot(path.clone())));
        assert_eq!(runtime_parents(&mut app), (Some(parents[0]), Some(parents[1])));
        assert_eq!(ground.ray(Vec3::new(6.3, 20.0, 6.3), Vec3::NEG_Y).unwrap().0, target_id);
        arm(&mut app, 0);
        let context = identity(&app);
        app.world_mut().resource_mut::<TerrainGroundCursor>().identity = Some(context);
        let release = begin(&mut app, false);
        let previews = app.world().resource::<EditorAuthoringSession>().placement_previews.clone();
        let hidden = app.world().resource::<MapEditorState>().terrain_hidden.clone();
        let dirty = app.world().resource::<MapEditorState>().dirty;
        let visibility = *app.world().get::<Visibility>(root).unwrap();
        let world_pose = app.world().get::<GlobalTransform>(root).unwrap().to_matrix();

        // 两个源均已就绪时才冻结真实Collider点击；随后只使A的运行身份失效，B和作者修订保持有效。
        // 此例覆盖快照之后的运行时退役窗口，不绕过正常建快照对全部作者定义/姿态的校验。
        if missing_definition {
            app.world_mut().entity_mut(parents[0]).remove::<feathrise_scene::TerrainObjectInstance>();
        } else {
            app.world_mut().get_mut::<feathrise_scene::SubMapInstance>(parents[0]).unwrap().batch = Entity::PLACEHOLDER;
        }
        assert_eq!(runtime_parents(&mut app), (None, Some(parents[1])));
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while cursor_running_for_tests(app.world()) {
            assert!(Instant::now() < deadline, "源失效后的真实点击必须在有限时间内完成");
            step(&mut app); std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..3 { step(&mut app); }
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(draft.placements.placements[0], before);
        assert!(session.undo.is_empty() && session.redo.is_empty());
        assert_eq!((session.revision, session.terrain_revision, session.placement_structure_revision), (7, 0, 3));
        assert!(Arc::ptr_eq(draft.terrain_ground.as_ref().unwrap(), &ground));
        assert_eq!(session.placement_previews, previews);
        assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), Some(parents[0]));
        assert_eq!(app.world().get::<Transform>(root), Some(&transform));
        assert_eq!(app.world().get::<GlobalTransform>(root).unwrap().to_matrix(), world_pose);
        assert_eq!(app.world().get::<Visibility>(root), Some(&visibility));
        assert_eq!(app.world().get::<feathrise_scene::TerrainVolumeAuthorPreviewRoot>(root).unwrap().0, path);
        assert!(app.world().get::<MapPlacementInstance>(root).unwrap().matches_record(&map, &before, &before.water_surfaces));
        let editor = app.world().resource::<MapEditorState>();
        assert_eq!(editor.dirty, dirty);
        assert!(Arc::ptr_eq(&editor.terrain_hidden, &hidden));
        assert!(editor.status.contains("关联地形实例尚未就绪"), "{}", editor.status);
        assert_eq!(app.world().resource::<TerrainGroundCursor>().click_terrain.as_ref(), Some(&target_id));
    }
}

#[test]
fn object_relocation_uses_real_async_click_once_with_snap_enabled_or_disabled() {
    for (enabled, control) in [(false, false), (false, true), (true, false), (true, true)] {
        let (mut app, root, terrain) = fixture(enabled);
        let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements[0].clone();
        let release = begin(&mut app, control);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements[0], before);
        app.world_mut().resource_mut::<TerrainGroundCursor>().ray.as_mut().unwrap().origin.x = 50.0;
        release.send(()).unwrap(); // 松键且指针移入空白后，完整真实点击仍完成一次。
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.world().resource::<EditorAuthoringSession>().revision == 7 {
            assert!(Instant::now() < deadline, "重定位真实点击必须在有限时间内完成");
            step(&mut app);
            std::thread::sleep(Duration::from_millis(1));
        }
        let session = app.world().resource::<EditorAuthoringSession>();
        let after = &session.draft.as_ref().unwrap().placements.placements[0];
        let xz = if enabled != control { 6.0 } else { 6.3 };
        assert_eq!(after.translation, [xz, 2.0, xz]);
        assert_eq!(after.terrain_binding.as_ref().unwrap().terrain_id, terrain);
        assert_eq!(after.yaw_degrees, before.yaw_degrees);
        assert_eq!(after.scale, before.scale);
        assert_eq!(after.water_surfaces, before.water_surfaces);
        assert_eq!(session.undo.len(), 1);
        assert_eq!(session.revision, 8);
        assert_eq!(app.world().get::<Transform>(root).unwrap().translation, Vec3::from_array(after.translation));
        assert!(!app.world().resource::<PlacementTransformInputState>().relocation_armed());
        for _ in 0..3 { step(&mut app); }
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    }
}

#[test]
fn object_relocation_cancels_raw_click_after_map_author_terrain_or_same_pose_selection_changes() {
    for variant in 0..5 {
        let (mut app, _, _) = fixture(false);
        let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
        let release = begin(&mut app, false);
        let map = app.world().resource::<MapEditorState>().selected_map_id.clone();
        match variant {
            0 => app.world_mut().resource_mut::<MapEditorState>().selected_map_id = Some(feathrise_map_content::MapId::parse("MAP-other").unwrap()),
            1 => app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1,
            2 => app.world_mut().resource_mut::<EditorAuthoringSession>().terrain_revision += 1,
            3 => arm(&mut app, 1),
            _ => app.world_mut().resource_mut::<EditorAuthoringSession>().placement_structure_revision += 1,
        }
        if matches!(variant, 1 | 3 | 4) {
            arm(&mut app, if variant == 3 { 1 } else { 0 });
            let changed = identity(&app);
            app.world_mut().resource_mut::<TerrainGroundCursor>().identity = Some(changed);
        }
        step(&mut app); // 身份失配时任务仍在屏障后，必须此帧作废点击权限。
        app.world_mut().resource_mut::<MapEditorState>().selected_map_id = map;
        app.world_mut().resource_mut::<EditorAuthoringSession>().revision = 7;
        app.world_mut().resource_mut::<EditorAuthoringSession>().terrain_revision = 0;
        app.world_mut().resource_mut::<EditorAuthoringSession>().placement_structure_revision = 3;
        arm(&mut app, 0);
        let restored = identity(&app);
        app.world_mut().resource_mut::<TerrainGroundCursor>().identity = Some(restored);
        release.send(()).unwrap(); // 恢复原身份后也不能重新获得旧点击权限。
        let deadline = Instant::now() + Duration::from_secs(5);
        while cursor_running_for_tests(app.world()) {
            assert!(Instant::now() < deadline);
            step(&mut app);
            std::thread::sleep(Duration::from_millis(1));
        }
        for _ in 0..3 { step(&mut app); }
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, before, "case {variant}");
        assert!(session.undo.is_empty(), "case {variant}");
        assert_eq!(session.revision, 7);
    }
}
