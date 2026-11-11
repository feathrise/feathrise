//! 真实相机与Collider回归：自身高度更新保持刷圈，源姿态/场景/可见性换代立即撤销旧显示。
use super::*;
use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState, types::EditorTool};
use bevy::prelude::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn surface(session: &EditorAuthoringSession, height: f32) -> Arc<TerrainGroundSnapshot> {
    let draft = session.draft.as_ref().unwrap();
    let object = &draft.manifest.terrains.as_ref().unwrap()[0];
    let [min_x, min_z] = object.world_min;
    let [max_x, max_z] = object.world_max;
    let collider = avian3d::prelude::Collider::trimesh(
        vec![
            Vec3::new(min_x, height, min_z),
            Vec3::new(max_x, height, min_z),
            Vec3::new(max_x, height, max_z),
            Vec3::new(min_x, height, max_z),
        ],
        vec![[0, 2, 1], [0, 3, 2]],
    );
    let mut result = TerrainGroundSnapshot::test_meshes(
        draft.scene_id.clone(),
        draft.manifest.terrains.as_ref().unwrap(),
        collider,
    );
    let value = Arc::get_mut(&mut result).unwrap();
    value.revision = session.terrain_revision;
    value.generation = session.terrain_revision;
    result
}

fn step(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    std::thread::sleep(Duration::from_millis(1));
}

fn until(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        assert!(Instant::now() < deadline);
        step(app);
    }
}

fn fixture() -> App {
    fixture_for_source([-32.0; 2], [32.0; 2], Default::default())
}

fn fixture_for_source(min: [f32; 2], max: [f32; 2], pose: feathrise_scene_content::TerrainObjectTransform) -> App {
    let mut app = crate::authoring::vegetation_paint::object_tests::fixture();
    app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::Raise;
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    let object = &mut Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0];
    object.world_min = min;
    object.world_max = max;
    object.transform = pose;
    draft.terrain_documents.active.as_mut().unwrap().definition = Arc::new(object.clone());
    let snapshot = surface(&session, 2.0);
    session.draft.as_mut().unwrap().terrain_ground = Some(snapshot);
    drop(session);
    until(&mut app, |app| {
        app.world()
            .resource::<TerrainGroundCursor>()
            .display_hit()
            .is_some()
            && !cursor_running_for_tests(app.world())
    });
    app
}

#[test]
fn sculpt_display_survives_two_real_height_generations_without_granting_stale_author_hits() {
    let mut app = fixture();
    for height in [3.0, 5.0] {
        let old = app
            .world()
            .resource::<TerrainGroundCursor>()
            .display_hit()
            .unwrap()
            .point;
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.revision += 1;
            session.terrain_revision += 1;
            session.draft.as_mut().unwrap().terrain_ground = None;
        }
        for _ in 0..3 {
            step(&mut app);
            let cursor = app.world().resource::<TerrainGroundCursor>();
            assert_eq!(
                cursor.display_hit().unwrap().point,
                old,
                "新高度在途不能反复关闭刷圈"
            );
            assert!(cursor.paint_hit().is_none());
            assert!(
                app.world()
                    .resource::<SceneEditorState>()
                    .brush_cursor_world
                    .is_none()
            );
        }
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let snapshot = surface(&session, height);
            session.draft.as_mut().unwrap().terrain_ground = Some(snapshot);
        }
        until(&mut app, |app| {
            app.world()
                .resource::<TerrainGroundCursor>()
                .display_hit()
                .is_some_and(|hit| (hit.point.y - height).abs() < 0.01)
        });
    }
}

#[test]
fn sculpt_display_rejects_a_changed_pose_source_or_visibility_while_height_is_pending() {
    for case in 0..4 {
        let mut app = fixture();
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.revision += 1;
            session.terrain_revision += 1;
            let draft = session.draft.as_mut().unwrap();
            draft.terrain_ground = None;
            if case < 2 {
                let mut objects = draft.manifest.terrains.as_ref().unwrap().to_vec();
                if case == 0 {
                    objects[0].transform.yaw_degrees = 45.0;
                } else {
                    objects[0].terrain.height_path =
                        Some("scenes/new-height.scene_height_manifest.json".into());
                }
                let mut active = draft.terrain_documents.active.take().unwrap();
                active.definition = Arc::new(objects[0].clone());
                draft.manifest.terrains = Some(objects.into());
                draft.terrain_documents.active = Some(active);
            }
        }
        if case == 2 {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            let id = editor.terrain_canvas_target.clone().unwrap();
            Arc::make_mut(&mut editor.terrain_hidden).insert(id);
        } else if case == 3 {
            app.world_mut()
                .resource_mut::<SceneEditorState>()
                .selected_scene_id = None;
        }
        step(&mut app);
        assert!(
            app.world()
                .resource::<TerrainGroundCursor>()
                .display_hit()
                .is_none(),
            "case {case}: 纯显示不能跨姿态、来源、隐藏或场景变更保留旧圈"
        );
    }
}

#[test]
fn continuous_paint_keeps_a_real_inflight_ray_across_its_own_non_geometry_adoption() {
    let mut app = fixture();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    until(&mut app, |app| {
        app.world()
            .resource::<TerrainGroundCursor>()
            .paint_hit()
            .is_some()
            && !cursor_running_for_tests(app.world())
    });
    {
        let world = app.world_mut();
        let mut viewport = world.query_filtered::<&mut bevy::ui::RelativeCursorPosition,
            With<crate::components::SceneEditorViewport>>();
        viewport.single_mut(world).unwrap().normalized = Some(Vec2::new(0.25, 0.0));
    }
    step(&mut app);
    let release = hold_cursor_result_for_tests(app.world_mut());
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .revision += 1;
    {
        let world = app.world_mut();
        let mut viewport = world.query_filtered::<&mut bevy::ui::RelativeCursorPosition,
            With<crate::components::SceneEditorViewport>>();
        viewport.single_mut(world).unwrap().normalized = Some(Vec2::new(0.45, 0.0));
    }
    step(&mut app);
    assert!(
        app.world()
            .resource::<TerrainGroundCursor>()
            .paint_pending(),
        "同笔非几何采用不能把在途射线误作空白，让跨网格涂草每次采用都断笔"
    );
    release.send(()).unwrap();
    until(&mut app, |app| {
        app.world()
            .resource::<TerrainGroundCursor>()
            .paint_hit()
            .is_some_and(|hit| hit.point.x > 1.0)
    });
    let hit = app
        .world()
        .resource::<TerrainGroundCursor>()
        .paint_hit()
        .unwrap();
    assert!(
        hit.point.x < 5.0,
        "完成点仍属于原0.25射线，不能借当前0.45射线冒领落点"
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_none()
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .ground_click_world
            .is_none()
    );
}

fn move_pointer(app: &mut App, x: f32) {
    let world = app.world_mut();
    let mut viewport = world.query_filtered::<&mut bevy::ui::RelativeCursorPosition,
        With<crate::components::SceneEditorViewport>>();
    viewport.single_mut(world).unwrap().normalized = Some(Vec2::new(x, 0.0));
}

fn begin_planar_height_wait(app: &mut App) -> Vec3 {
    // 真相机/Collider结果均保持原样；Mesh夹具统一标成三维面，此处只设室外Clipmap的二维分类。
    // 后续鼠标射线、来源身份及作者写入门仍全部经过正式系统，不注入任何命中位置。
    let mut cursor = app.world_mut().resource_mut::<TerrainGroundCursor>();
    let sample = cursor.display.as_mut().unwrap();
    sample.hit.volume_surface = false;
    let old = sample.hit.point;
    drop(cursor);
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    session.revision += 1;
    session.terrain_revision += 1;
    session.draft.as_mut().unwrap().terrain_ground = None;
    old
}

#[test]
fn planar_display_follows_the_current_camera_ray_during_height_wait_without_author_or_click_hits() {
    let mut app = fixture();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    until(&mut app, |app| app.world().resource::<TerrainGroundCursor>().paint_hit().is_some()
        && !cursor_running_for_tests(app.world()));
    let old = begin_planar_height_wait(&mut app);
    let mut previous = None;
    for x in [-0.25, -0.10, 0.10, 0.25] {
        move_pointer(&mut app, x);
        step(&mut app);
        let cursor = app.world().resource::<TerrainGroundCursor>();
        let ray = cursor.ray.unwrap();
        let expected = ray.origin + ray.direction * ((old.y - ray.origin.y) / ray.direction.y);
        let displayed = cursor.display_point().expect("同来源高度等待仍须显示跟随当前鼠标的二维圈");
        assert!(displayed.distance(expected) < 0.0001);
        assert!(previous.is_none_or(|point: Vec3| point.distance(displayed) > 0.1));
        previous = Some(displayed);
        assert_eq!(cursor.display_hit().unwrap().point, old, "预览投影不能反写最后真实命中缓存");
        assert!(cursor.input_authority().is_some());
        assert!(cursor.paint_hit().is_none());
        assert!(cursor.pending_click.is_none() && cursor.click_terrain.is_none());
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.cursor_world.is_none() && editor.brush_cursor_world.is_none()
            && editor.ground_click_world.is_none(), "仅显示跟随绝不能给选择或作者输入补落点");
        assert_eq!(crate::viewport::scene_editor_brush_display_hit(editor, Some(cursor)), Some((displayed, false)),
            "刷圈和系统鼠标所有权必须消费同一个显示位置");
    }
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let snapshot = surface(&session, 5.0);
        session.draft.as_mut().unwrap().terrain_ground = Some(snapshot);
    }
    until(&mut app, |app| app.world().resource::<TerrainGroundCursor>().display_hit()
        .is_some_and(|hit| (hit.point.y - 5.0).abs() < 0.01));
    let cursor = app.world().resource::<TerrainGroundCursor>();
    assert_eq!(cursor.display_point(), Some(cursor.display_hit().unwrap().point),
        "新真命中完成后应立即接回真实高度");
}

#[test]
fn tilted_planar_display_follows_source_plane_during_height_wait() {
    let pose = feathrise_scene_content::TerrainObjectTransform {
        yaw_degrees: 25., pitch_degrees: 28., roll_degrees: -17., scale: [2., 3., 0.5], ..Default::default()
    };
    let mut app = fixture_for_source([-32.; 2], [32.; 2], pose);
    let old = begin_planar_height_wait(&mut app);
    let normal = pose.as_transform().rotation * Vec3::Y;
    for x in [-0.15, 0.10, 0.25] {
        move_pointer(&mut app, x); step(&mut app);
        let cursor = app.world().resource::<TerrainGroundCursor>();
        let ray = cursor.ray.unwrap();
        let expected = ray.origin + ray.direction * ((old - ray.origin).dot(normal) / ray.direction.dot(normal));
        let actual = cursor.display_point().expect("倾斜源等待期间仍显示本地XZ平面的刷圈");
        assert!(actual.distance(expected) < 0.0001);
        assert!((actual - old).dot(normal).abs() < 0.0001);
        assert!(cursor.paint_hit().is_none() && cursor.pending_click.is_none() && cursor.click_terrain.is_none());
        assert!(cursor.display_transform().unwrap().rotation.dot(pose.as_transform().rotation).abs() > 0.99999);
    }
}

#[test]
fn planar_display_rejects_source_local_bounds_and_parallel_rays_but_keeps_volume_true_points() {
    let mut app = fixture_for_source([-1.0; 2], [1.0; 2], feathrise_scene_content::TerrainObjectTransform {
        yaw_degrees: 90.0, scale: [2.0, 1.0, 0.5], ..Default::default()
    });
    let old = begin_planar_height_wait(&mut app);
    move_pointer(&mut app, 0.02);
    step(&mut app);
    assert!(app.world().resource::<TerrainGroundCursor>().display_point().is_some());
    move_pointer(&mut app, 0.05);
    step(&mut app);
    {
        let cursor = app.world().resource::<TerrainGroundCursor>();
        let ray = cursor.ray.unwrap();
        let preview = ray.origin + ray.direction * ((old.y - ray.origin.y) / ray.direction.y);
        assert!(preview.x > 0.5 && preview.x < 1.0,
            "世界点仍在未变换的[-1,1]范围，必须用旋转缩放后的来源局部边界拒绝");
        assert_eq!(cursor.display_point(), None);
        assert!(cursor.display_hit().is_some() && cursor.paint_hit().is_none());
        assert_eq!(crate::viewport::scene_editor_brush_display_hit(
            app.world().resource::<SceneEditorState>(), Some(cursor)), None);
    }
    move_pointer(&mut app, 0.02);
    step(&mut app);
    let mut cursor = app.world_mut().resource_mut::<TerrainGroundCursor>();
    let valid_ray = cursor.ray.unwrap();
    for direction in [Vec3::new(1.0, -0.0000001, 0.0), Vec3::Y, Vec3::splat(f32::NAN)] {
        cursor.ray = Some(super::resources::CursorRay { direction, ..valid_ray });
        assert_eq!(cursor.display_point(), None, "近平行、负交距和非有限投影不能隐藏系统鼠标");
    }
    cursor.ray = Some(valid_ray);
    cursor.display.as_mut().unwrap().hit.volume_surface = true;
    assert_eq!(cursor.display_point(), Some(old), "三维表面不按二维高度面猜测新位置");
    cursor.display.as_mut().unwrap().hit.volume_surface = false;
    cursor.display.as_mut().unwrap().identity.tool = EditorTool::VolumeDig;
    cursor.identity.as_mut().unwrap().tool = EditorTool::VolumeDig;
    assert_eq!(cursor.display_point(), Some(old), "三维雕刻工具继续使用真实点");
}

#[test]
fn planar_display_never_follows_after_authority_epoch_or_camera_changes() {
    for case in 0..3 {
        let mut app = fixture();
        begin_planar_height_wait(&mut app);
        move_pointer(&mut app, 0.10);
        step(&mut app);
        let mut cursor = app.world_mut().resource_mut::<TerrainGroundCursor>();
        assert!(cursor.display_point().is_some());
        match case {
            0 => cursor.input_available = false,
            1 => cursor.ray.as_mut().unwrap().epoch += 1,
            _ => cursor.identity.as_mut().unwrap().camera.w_axis.x += 1.0,
        }
        assert_eq!(cursor.display_point(), None, "case {case}: 旧显示不能跨输入或镜头换代跟随");
    }
}
