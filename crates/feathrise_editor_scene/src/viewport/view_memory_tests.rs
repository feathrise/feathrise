//! 真实取景与RTT生命周期回归，覆盖源/目标身份、往返恢复、迟到清单及新深度采用。

use super::*;
use crate::types::SceneEditorViewSnapshot;
use feathrise_scene_content::{
    ActiveDynamicScene, SceneContentHandles, SceneId, SceneTerrainDescriptor, SceneTerrainKind,
};
use feathrise_scene_core::state::ScenePhase;
use std::collections::BTreeMap;

fn manifest(id: &SceneId, offset: f32) -> SceneManifestAsset {
    SceneManifestAsset {
        vegetation_paint_groups: Default::default(),
        terrains: None,
        subscenes: Default::default(),
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        id: id.clone(),
        display_name: id.to_string(),
        notes: String::new(),
        runtime_scene: "data_driven".to_owned(),
        world_min: [offset, offset],
        world_max: [offset + 1_000.0, offset + 600.0],
        default_spawn: [offset + 100.0, 0.0, offset + 100.0],
        environment: default(),
        puddles: None,
        asset_roots: Vec::new(),
        terrain: SceneTerrainDescriptor {resolved_stitches: Default::default(),
            kind: SceneTerrainKind::Gltf,
            collision_mode: default(),
            collision_lods: Vec::new(),
            display_lods: None,
            player_collision_preload_radius_meters:
                feathrise_scene_content::DEFAULT_PLAYER_COLLISION_PRELOAD_RADIUS_METERS,
            role_collision_preload_radius_meters: None,
            player_collision_chunk_cells: None,
            role_collision_chunk_cells: None,
            player_collision_span_meters: None,
            role_collision_span_meters: None,
            glb_path: format!("scenes/{id}/terrain.glb"),
            height_path: None,
            height_texture_path: None,
            resolution: [129, 129],
            base_height: -80.0,
            height_scale: 160.0,
        },
    } // 只构造清单，不生成地形、磁盘资产或整图草稿。
}

fn fixture(recreate_camera: bool) -> (App, SceneId, SceneId) {
    let a = SceneId::parse("MAP-view-a").unwrap();
    let b = SceneId::parse("MAP-view-b").unwrap();
    let mut app = App::new();
    app.init_resource::<Assets<SceneManifestAsset>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<SceneEditorViewMemory>()
        .insert_resource(SceneEditorState {
            open: true,
            selected_scene_id: Some(a.clone()),
            ..default()
        });
    let mut scenes = BTreeMap::new();
    for (id, offset) in [(&a, 0.0), (&b, 2_000.0)] {
        let manifest = app
            .world_mut()
            .resource_mut::<Assets<SceneManifestAsset>>()
            .add(manifest(id, offset));
        scenes.insert(
            id.clone(),
            SceneContentHandles {
                manifest,
                portals: Handle::default(),
                placements: Handle::default(),
                surface: None,
                vegetation: None,
                water: None,
                characters_path: None,
                boundaries: None,
            },
        );
    }
    app.insert_resource(SceneContentRegistry {
        catalog: Handle::default(),
        revision: 1,
        scenes,
        last_error: None,
    });
    if recreate_camera {
        // 正式视口创建鼠标指针；最小夹具也要为 PointerId 的注册钩子提供索引。
        app.init_resource::<bevy::picking::pointer::PointerMap>()
            .insert_resource(State::new(ScenePhase::Active))
            .insert_resource(ActiveDynamicScene {
                scene_id: Some(a.clone()),
            })
            .add_systems(Update, (setup_editor_viewport, frame_selected_scene).chain());
        app.world_mut().spawn(SceneEditorViewport);
    } else {
        let (yaw, pitch) = orbit_angles_from_view(EditorView::Perspective);
        app.insert_resource(SceneEditorViewportState {
            _image: Handle::default(),
            camera: Entity::PLACEHOLDER,
            center: Vec3::ZERO,
            scene_center: Vec3::ZERO,
            scene_half_extents: Vec3::ONE,
            extent: 1_200.0,
            yaw,
            pitch,
            framed_scene_id: None,
            render_size: UVec2::new(800, 600),
        })
        .add_systems(Update, frame_selected_scene);
    }
    app.update();
    (app, a, b)
}

fn snapshot(app: &App) -> SceneEditorViewSnapshot {
    let viewport = app.world().resource::<SceneEditorViewportState>();
    SceneEditorViewSnapshot {
        view: app.world().resource::<SceneEditorState>().view,
        orthographic: app.world().resource::<SceneEditorState>().orthographic,
        center: viewport.center,
        extent: viewport.extent,
        yaw: viewport.yaw,
        pitch: viewport.pitch,
    }
}

fn set_view(
    app: &mut App, view: EditorView, orthographic: bool, center: Vec3, extent: f32,
) -> SceneEditorViewSnapshot {
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.view = view;
        editor.orthographic = orthographic;
    }
    let mut viewport = app.world_mut().resource_mut::<SceneEditorViewportState>();
    viewport.center = center;
    viewport.extent = extent;
    (viewport.yaw, viewport.pitch) = orbit_angles_from_view(view);
    if view == EditorView::Perspective {
        viewport.yaw = 0.8;
        viewport.pitch = -0.6; // 自由正交也能记住任意旋转姿态。
    }
    snapshot(app)
}

fn select(app: &mut App, target: &SceneId) {
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = Some(target.clone());
    app.update();
}

fn enter_via_loading(app: &mut App, target: &SceneId) {
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = Some(target.clone());
    app.world_mut().resource_mut::<ActiveDynamicScene>().scene_id = Some(target.clone());
    app.insert_resource(State::new(ScenePhase::Loading));
    app.update();
    assert!(!app.world().contains_resource::<SceneEditorViewportState>());
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
    app.insert_resource(State::new(ScenePhase::Active));
    app.update(); // 真实创建新RTT后才运行同一个正式取景系统。
}

#[test]
fn scene_round_trip_restores_each_view_without_overwriting_later_navigation() {
    let (mut app, a, b) = fixture(false);
    assert_eq!(
        app.world().resource::<SceneEditorState>().view,
        EditorView::Perspective
    );
    assert!(
        app.world()
            .resource::<SceneEditorViewMemory>()
            .scenes
            .is_empty()
    );
    let saved_a = set_view(&mut app, EditorView::Top, true, Vec3::new(10.0, 20.0, 30.0), 64.0);
    app.update();
    assert_eq!(snapshot(&app), saved_a); // 相同场景稳定帧不能反复自动居中。
    select(&mut app, &b);
    assert!(!app.world().resource::<SceneEditorState>().orthographic); // 初访场景默认透视，不能继承源图的正交开关。
    assert_eq!(
        app.world().resource::<SceneEditorViewMemory>().scenes.get(&a),
        Some(&saved_a)
    );
    assert_eq!(
        app.world().resource::<SceneEditorState>().view,
        EditorView::Perspective
    );
    assert_eq!(
        app.world().resource::<SceneEditorViewportState>().center,
        Vec3::ZERO // 尚无实际内容时不使用清单中的虚拟矩形。
    );
    let saved_b = set_view(
        &mut app,
        EditorView::Side,
        false,
        Vec3::new(200.0, -40.0, 300.0),
        240.0,
    );
    select(&mut app, &a);
    assert_eq!(snapshot(&app), saved_a);
    assert_eq!(
        app.world().resource::<SceneEditorViewMemory>().scenes.get(&b),
        Some(&saved_b)
    );
    let newer_a = set_view(
        &mut app,
        EditorView::Perspective,
        true,
        Vec3::new(70.0, 80.0, 90.0),
        120.0,
    );
    app.update();
    assert_eq!(snapshot(&app), newer_a); // 缓存只在进图时恢复，不能把新导航持续压回旧观察记录。
    select(&mut app, &b);
    assert_eq!(snapshot(&app), saved_b);
    select(&mut app, &a);
    assert_eq!(snapshot(&app), newer_a);
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn loading_caches_source_before_rtt_teardown_then_restores_into_a_new_camera() {
    let (mut app, a, b) = fixture(true);
    let old_camera = app.world().resource::<SceneEditorViewportState>().camera;
    let old_image = app.world().resource::<SceneEditorViewportState>()._image.id();
    let saved_a = set_view(
        &mut app,
        EditorView::Front,
        false,
        Vec3::new(15.0, 25.0, 35.0),
        180.0,
    );
    enter_via_loading(&mut app, &b); // selected_scene_id已经指向B，缓存仍必须归入旧视口A。
    assert_eq!(
        app.world().resource::<SceneEditorViewMemory>().scenes.get(&a),
        Some(&saved_a)
    );
    assert!(app.world().get_entity(old_camera).is_err());
    assert!(
        app.world()
            .resource::<Assets<Image>>()
            .get(old_image)
            .is_none()
    );
    let saved_b = set_view(
        &mut app,
        EditorView::Perspective,
        true,
        Vec3::new(2015.0, 125.0, 2035.0),
        360.0,
    );
    enter_via_loading(&mut app, &a);
    assert_eq!(snapshot(&app), saved_a);
    let viewport = app.world().resource::<SceneEditorViewportState>();
    assert_ne!(viewport.camera, old_camera);
    assert_ne!(viewport._image.id(), old_image);
    assert_eq!(viewport.framed_scene_id.as_ref(), Some(&a));
    assert_eq!(
        app.world().resource::<SceneEditorViewMemory>().scenes.get(&b),
        Some(&saved_b)
    );
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 1); // 相机与纹理仍只有当前一份，不为视角记忆保留旧世界。
}

#[test]
fn restoring_view_requests_actual_content_depth_and_preserves_current_render_size() {
    let (mut app, a, b) = fixture(false);
    let saved_a = set_view(&mut app, EditorView::Top, true, Vec3::new(10.0, 20.0, 30.0), 64.0);
    select(&mut app, &b);
    let handle = app
        .world()
        .resource::<SceneContentRegistry>()
        .handles(&a)
        .unwrap()
        .manifest
        .clone();
    {
        let mut assets = app.world_mut().resource_mut::<Assets<SceneManifestAsset>>();
        let mut manifest = assets.get_mut(&handle).unwrap();
        manifest.world_min = [-500.0, -300.0];
        manifest.world_max = [1_500.0, 900.0];
        manifest.terrain.base_height = -200.0;
        manifest.terrain.height_scale = 600.0;
    }
    app.world_mut()
        .resource_mut::<SceneEditorViewportState>()
        .render_size = UVec2::new(1_000, 700);
    select(&mut app, &a);
    assert_eq!(snapshot(&app), saved_a);
    let viewport = app.world().resource::<SceneEditorViewportState>();
    assert_eq!(viewport.scene_center, Vec3::ZERO);
    assert_eq!(
        viewport.scene_half_extents,
        Vec3::new(16.0, 0.0, 16.0)
    );
    assert_eq!(app.world().resource::<SceneEditorState>().viewport_framing_requested,
        Some(crate::types::EditorViewportFraming::RefreshDepth)); // 真正内容统计由后序系统按预算执行。
    assert_eq!(viewport.render_size, UVec2::new(1_000, 700)); // 旧相机快照不能覆盖新布局或场景更新后的裁剪量程。
}

#[test]
fn unready_or_mismatched_target_keeps_source_pose_until_valid_adoption() {
    let (mut app, a, b) = fixture(false);
    let saved_a = set_view(&mut app, EditorView::Top, true, Vec3::new(10.0, 20.0, 30.0), 64.0);
    let handles = app
        .world_mut()
        .resource_mut::<SceneContentRegistry>()
        .scenes
        .remove(&b)
        .unwrap();
    select(&mut app, &b);
    assert_eq!(snapshot(&app), saved_a);
    assert_eq!(
        app.world()
            .resource::<SceneEditorViewportState>()
            .framed_scene_id
            .as_ref(),
        Some(&a)
    );
    assert!(
        app.world()
            .resource::<SceneEditorViewMemory>()
            .scenes
            .is_empty()
    );
    app.world_mut()
        .resource_mut::<Assets<SceneManifestAsset>>()
        .get_mut(&handles.manifest)
        .unwrap()
        .id = a.clone();
    app.world_mut()
        .resource_mut::<SceneContentRegistry>()
        .scenes
        .insert(b.clone(), handles.clone());
    app.update();
    assert_eq!(snapshot(&app), saved_a); // 句柄存在但清单身份错误，不能提前覆盖源视角或采用目标缓存。
    {
        let mut assets = app.world_mut().resource_mut::<Assets<SceneManifestAsset>>();
        let mut manifest = assets.get_mut(&handles.manifest).unwrap();
        manifest.id = b.clone();
        manifest.world_max = manifest.world_min;
    }
    app.update();
    assert_eq!(snapshot(&app).center, Vec3::ZERO); // 旧尺寸不再是内容取景的资格门。
    {
        let mut assets = app.world_mut().resource_mut::<Assets<SceneManifestAsset>>();
        let mut manifest = assets.get_mut(&handles.manifest).unwrap();
        manifest.world_max = [3_000.0, 2_600.0];
    }
    app.update();
    assert_eq!(
        app.world()
            .resource::<SceneEditorViewportState>()
            .framed_scene_id
            .as_ref(),
        Some(&b)
    );
    assert_eq!(
        app.world().resource::<SceneEditorViewMemory>().scenes.get(&a),
        Some(&saved_a)
    );
}

#[test]
fn reopening_workbench_on_same_scene_keeps_the_current_camera() {
    let (mut app, _, _) = fixture(true);
    let saved = set_view(
        &mut app,
        EditorView::Side,
        true,
        Vec3::new(70.0, 80.0, 90.0),
        120.0,
    );
    let camera = app.world().resource::<SceneEditorViewportState>().camera;
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    app.update();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    app.update();
    assert_eq!(snapshot(&app), saved);
    assert_eq!(
        app.world().resource::<SceneEditorViewportState>().camera,
        camera
    );
}
