//! 内容取景回归：越过旧矩形、空图、对象姿态、有界收集以及换代/导航取消。
use super::*;
use feathrise_scene_core::residency::ResidentMapToken;
use std::sync::Arc;

fn fixture() -> App {
    AsyncComputeTaskPool::get_or_init(Default::default);
    let mut app = App::new();
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.terrain = feathrise_map_content::MapTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([]));
    draft.manifest.world_min = [-0.5; 2];
    draft.manifest.world_max = [0.5; 2];
    let map = draft.map_id.clone();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(map.clone()),
        ..default()
    })
    .insert_resource(MapEditorViewportState {
        _image: Handle::default(),
        camera: Entity::PLACEHOLDER,
        center: Vec3::splat(123.0),
        extent: 50.0,
        scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::ONE,
        yaw: 0.8,
        pitch: 0.4,
        framed_map_id: Some(map),
        render_size: UVec2::new(800, 600),
    })
    .insert_resource(SceneResidencyActivity {
        active: Some(ResidentMapToken(1)),
        ..default()
    })
    .init_resource::<ResidentEntityIndex>()
    .add_systems(Update, frame_content);
    app
}
fn mesh(app: &mut App, pose: Transform, half: Vec3, token: u64) -> Entity {
    let entity = app
        .world_mut()
        .spawn((
            Mesh3d::default(),
            GlobalTransform::from(pose),
            Aabb::from_min_max(-half, half),
        ))
        .id();
    app.world_mut()
        .resource_mut::<ResidentEntityIndex>()
        .insert(entity, ResidentMapToken(token));
    entity
}
fn request(app: &mut App) {
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .viewport_framing_requested = Some(EditorViewportFraming::Map);
    app.update();
}
#[test]
fn frames_transformed_contents_outside_old_bounds_and_ignores_other_maps() {
    let mut app = fixture();
    let pose = Transform::from_xyz(50_000.0, 31.0, -40_000.0)
        .with_rotation(Quat::from_rotation_y(0.7))
        .with_scale(Vec3::new(3.0, 2.0, 0.5));
    let half = Vec3::new(12.0, 3.0, 8.0);
    mesh(&mut app, pose, half, 1);
    mesh(
        &mut app,
        Transform::from_xyz(-99_000.0, 0.0, 0.0),
        Vec3::splat(50_000.0),
        2,
    );
    let hidden = mesh(&mut app, Transform::IDENTITY, Vec3::splat(90_000.0), 1);
    app.world_mut()
        .entity_mut(hidden)
        .insert(Visibility::Hidden);
    request(&mut app);
    let expected = Bounds::transformed(-half, half, pose.to_matrix());
    let view = app.world().resource::<MapEditorViewportState>();
    assert_eq!(view.center, pose.translation);
    assert!(
        view.scene_half_extents
            .abs_diff_eq((expected.max - expected.min) * 0.5, 0.01)
    );
    assert!(view.extent < 200.0);
}
#[test]
fn empty_container_uses_small_origin_view_and_ignores_legacy_metadata() {
    let mut app = fixture();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .world_max = [80_000.0; 2];
    request(&mut app);
    let view = app.world().resource::<MapEditorViewportState>();
    assert_eq!(view.center, Vec3::ZERO);
    assert_eq!(view.scene_half_extents, Vec3::new(16.0, 0.0, 16.0));
}
#[test]
fn large_scene_is_atomic_and_navigation_cancels_pending_frame() {
    let mut app = fixture();
    for i in 0..(VISITS_PER_FRAME + 7) {
        mesh(
            &mut app,
            Transform::from_xyz(i as f32, 0.0, 0.0),
            Vec3::ONE,
            1,
        );
    }
    request(&mut app);
    assert_eq!(
        app.world().resource::<MapEditorViewportState>().center,
        Vec3::splat(123.0)
    );
    app.world_mut()
        .resource_mut::<MapEditorViewportState>()
        .center = Vec3::splat(99.0);
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<MapEditorViewportState>().center,
        Vec3::splat(99.0)
    );
}
#[test]
fn large_scene_updates_only_after_last_entity() {
    let mut app = fixture();
    for _ in 0..VISITS_PER_FRAME {
        mesh(&mut app, Transform::IDENTITY, Vec3::ONE, 1);
    }
    mesh(
        &mut app,
        Transform::from_xyz(1000.0, 0.0, 0.0),
        Vec3::ONE,
        1,
    );
    request(&mut app);
    assert_eq!(
        app.world().resource::<MapEditorViewportState>().center,
        Vec3::splat(123.0)
    );
    app.update();
    assert_eq!(
        app.world().resource::<MapEditorViewportState>().center,
        Vec3::new(500.0, 0.0, 0.0)
    );
}
#[test]
fn loading_mesh_does_not_publish_partial_bounds() {
    let mut app = fixture();
    let entity = mesh(&mut app, Transform::from_xyz(100.0, 0.0, 0.0), Vec3::ONE, 1);
    app.world_mut().entity_mut(entity).remove::<Aabb>();
    request(&mut app);
    assert_eq!(
        app.world().resource::<MapEditorViewportState>().center,
        Vec3::splat(123.0)
    );
    app.world_mut()
        .entity_mut(entity)
        .insert(Aabb::from_min_max(-Vec3::ONE, Vec3::ONE));
    app.update();
    assert_eq!(
        app.world().resource::<MapEditorViewportState>().center,
        Vec3::new(100.0, 0.0, 0.0)
    );
}

#[test]
fn sparse_frame_uses_full_author_heights_and_unsaved_overrides() {
    use feathrise_map_content::{
        MapId, SparseHeightDocument, SparseHeightSpec, SparseHeightTile, SparseHeightTileKey,
    };
    let root = std::env::temp_dir().join(format!(
        "bevy-content-frame-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let map = MapId::parse("MAP-framing-heights").unwrap();
    let spec = SparseHeightSpec::new([-64.0; 2], [64.0; 2], 1.0, -8.0, 16.0).unwrap();
    let mut document =
        SparseHeightDocument::create_empty(&root, map.clone(), spec.clone()).unwrap();
    document.save_dirty().unwrap();
    let frame = Transform::from_xyz(20_000.0, 15.0, -30_000.0)
        .with_scale(Vec3::new(2.0, 3.0, 0.5))
        .to_matrix();
    let source = || SparseSource {
        entity: None,
        map: map.clone(),
        path: feathrise_map_content::sparse_height_manifest_asset_path(&map),
        frame,
        live: Default::default(),
        last: None,
    };
    let flat = sparse::bounds(&root, source()).unwrap();
    assert_eq!(flat.min.y, 15.0);
    assert_eq!(flat.max.y, 15.0); // 旧编码量程不能制造虚构山高。
    document.set_grid_height(50, 60, 50.0).unwrap();
    document.save_dirty().unwrap();
    let sculpted = sparse::bounds(&root, source()).unwrap();
    assert!((sculpted.max.y - 165.0).abs() < 0.04);
    let key = SparseHeightTileKey { x: 0, z: 0 };
    let mut live = source();
    live.live
        .insert(key, Arc::new(SparseHeightTile::blank(&spec, key).unwrap()));
    assert_eq!(sparse::bounds(&root, live).unwrap(), flat); // 未保存撤销优先于磁盘凸起。
    for z in 0..=spec.cells[1] {
        for x in 0..=spec.cells[0] {
            document.set_grid_height(x, z, 50.0).unwrap();
        }
    }
    document.save_dirty().unwrap();
    let raised = sparse::bounds(&root, source()).unwrap();
    assert!((raised.min.y - 165.0).abs() < 0.04);
    assert_eq!(raised.min.y, raised.max.y); // 完整抬高的平地只包含自身高度。
    document.set_grid_height(50, 60, -1000.125).unwrap();
    document.save_dirty().unwrap();
    let deep = sparse::bounds(&root, source()).unwrap();
    assert_eq!(deep.min.y, 15.0 - 1000.125 * 3.0); // 深坑超出旧-8米范围，真实取景仍完整包含。
    assert_eq!(deep.max.y, raised.max.y);
    std::fs::remove_dir_all(&root).unwrap(); // 仅回收本测试新建的唯一临时目录。
}

#[test]
fn depth_refresh_preserves_remembered_camera() {
    let mut app = fixture();
    mesh(
        &mut app,
        Transform::from_xyz(40_000.0, 0.0, 0.0),
        Vec3::splat(20.0),
        1,
    );
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .viewport_framing_requested = Some(EditorViewportFraming::RefreshDepth);
    app.update();
    let view = app.world().resource::<MapEditorViewportState>();
    assert_eq!(view.center, Vec3::splat(123.0));
    assert_eq!(view.extent, 50.0);
    assert_eq!(view.scene_center, Vec3::new(40_000.0, 0.0, 0.0));
}

#[test]
fn water_with_explicit_bounds_is_framed_even_without_frustum_culling() {
    let mut app = fixture();
    let entity = mesh(
        &mut app,
        Transform::from_xyz(30_000.0, 5.0, 0.0),
        Vec3::new(50.0, 0.0, 40.0),
        1,
    );
    app.world_mut().entity_mut(entity).insert(NoFrustumCulling);
    request(&mut app);
    assert_eq!(
        app.world().resource::<MapEditorViewportState>().center,
        Vec3::new(30_000.0, 5.0, 0.0)
    );
}

#[test]
fn framing_excludes_celestial_weather_asset_previews_and_surface_warmup() {
    use feathrise_scene_core::constants::render_layers::*;
    use feathrise_editor_core::constants::render_layers::*;
    let mut app = fixture();
    let half = Vec3::new(50.5, 31.0, 53.0); // a地图约101×106米；XYZ为零时仍不能被环境远景放大取景范围。
    mesh(&mut app, Transform::IDENTITY, half, 1);
    let grass_window = mesh(&mut app, Transform::IDENTITY, Vec3::splat(4_096.0), 1);
    app.world_mut().entity_mut(grass_window).insert(
        feathrise_environment::vegetation::blade::GpuBladePatch::square(Vec3::ZERO, 8_192.0, 8, 1),
    ); // 草窗保守剔除范围属于相机采样预算，即使显式有AABB也不能放大101米的地图。
    for layer in [CELESTIAL_AUTHORING_LAYER, CELESTIAL_GAMEPLAY_LAYER, WEATHER_EFFECT_LAYER,
        MAP_EDITOR_ASSET_PREVIEW_LAYER, MAP_EDITOR_ASSET_THUMBNAIL_LAYER,
        MAP_EDITOR_WATER_PRESET_PREVIEW_LAYER] {
        let entity = mesh(&mut app, Transform::from_xyz(0.0, 10_000.0, 0.0), Vec3::splat(5_000.0), 1);
        app.world_mut().entity_mut(entity).insert(RenderLayers::layer(layer));
    }
    let warmup = mesh(&mut app, Transform::from_xyz(0.0, -100_000.0, 0.0), Vec3::splat(0.000_1), 1);
    app.world_mut().entity_mut(warmup).insert((
        NoFrustumCulling, Visibility::Visible,
        feathrise_environment::terrain_surface::TerrainSurfaceWarmup,
    )); // 预热仍在世界渲染层且有AABB，必须按用途排除，不能仅依赖隐藏/剔除状态。
    request(&mut app);
    let view = app.world().resource::<MapEditorViewportState>();
    assert_eq!(view.center, Vec3::ZERO);
    assert_eq!(view.scene_half_extents, half);
    assert!(view.extent < 250.0);
}

#[test]
fn framing_requires_explicit_application_content_without_using_camera_layers() {
    const APPLICATION_CONTENT: usize = 24;
    const APPLICATION_CAMERA_ONLY: usize = 25;
    let mut app = fixture();
    mesh(&mut app, Transform::IDENTITY, Vec3::ONE, 1);
    for (layer, x) in [(APPLICATION_CONTENT, 1000.0), (APPLICATION_CAMERA_ONLY, 9000.0)] {
        let entity = mesh(&mut app, Transform::from_xyz(x, 0.0, 0.0), Vec3::ONE, 1);
        app.world_mut().entity_mut(entity).insert(RenderLayers::layer(layer));
    }
    request(&mut app);
    assert_eq!(app.world().resource::<MapEditorViewportState>().scene_center, Vec3::ZERO);

    app.insert_resource(feathrise_editor_core::EditorApplicationViewLayers {
        camera: RenderLayers::layer(APPLICATION_CAMERA_ONLY),
        framing: RenderLayers::layer(APPLICATION_CONTENT),
    });
    request(&mut app);
    let view = app.world().resource::<MapEditorViewportState>();
    assert_eq!(view.scene_center, Vec3::new(500.0, 0.0, 0.0));
    assert_eq!(view.scene_half_extents, Vec3::new(501.0, 1.0, 1.0));
}

#[test]
fn application_layer_change_invalidates_an_incomplete_capture() {
    const APPLICATION_CONTENT: usize = 24;
    let mut app = fixture();
    app.insert_resource(feathrise_editor_core::EditorApplicationViewLayers {
        framing: RenderLayers::layer(APPLICATION_CONTENT),
        ..default()
    });
    for _ in 0..VISITS_PER_FRAME {
        mesh(&mut app, Transform::IDENTITY, Vec3::ONE, 1);
    }
    let entity = mesh(&mut app, Transform::from_xyz(2000.0, 0.0, 0.0), Vec3::ONE, 1);
    app.world_mut().entity_mut(entity).insert(RenderLayers::layer(APPLICATION_CONTENT));
    request(&mut app);
    assert_eq!(app.world().resource::<MapEditorViewportState>().center, Vec3::splat(123.0));

    app.world_mut().remove_resource::<feathrise_editor_core::EditorApplicationViewLayers>();
    for _ in 0..200 {
        app.update();
        let view = app.world().resource::<MapEditorViewportState>();
        assert_ne!(view.center, Vec3::new(1000.0, 0.0, 0.0)); // 旧资格的完整结果也不能先发布一帧。
        if view.center == Vec3::ZERO {
            assert_eq!(view.scene_half_extents, Vec3::ONE);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("应用取景层换代后未采用新的完整范围");
}

#[test]
fn changing_resident_index_restarts_capture_before_adoption() {
    let mut app = fixture();
    for _ in 0..VISITS_PER_FRAME {
        mesh(&mut app, Transform::IDENTITY, Vec3::ONE, 1);
    }
    request(&mut app);
    mesh(
        &mut app,
        Transform::from_xyz(2000.0, 0.0, 0.0),
        Vec3::ONE,
        1,
    );
    // 新登记内容使旧游标失效，释放旧捕获后重新遍历，不能丢失新增远处对象。
    for _ in 0..200 {
        app.update();
        if app.world().resource::<MapEditorViewportState>().center == Vec3::new(1000.0, 0.0, 0.0) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("索引换代后的完整取景未完成");
}
