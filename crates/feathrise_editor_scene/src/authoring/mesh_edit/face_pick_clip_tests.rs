//! 面拾取复用正式作者相机的裁剪范围；不可见远面拒选，部分可见面与原遮挡规则保留。

use super::*;
use crate::resources::SceneEditorViewportState;
use crate::viewport::placement_move::MoveCamera;

fn front_camera(orthographic: bool, inverse_scale: f32) -> MoveCamera {
    MoveCamera::new(
        &SceneEditorState { orthographic, ..default() },
        &SceneEditorViewportState {
            _image: default(), camera: Entity::PLACEHOLDER,
            center: Vec3::ZERO, scene_center: Vec3::ZERO, scene_half_extents: Vec3::splat(10.0),
            extent: 8.0, yaw: 0.0, pitch: 0.0, framed_scene_id: None,
            render_size: UVec2::new((800.0 / inverse_scale) as u32, (600.0 / inverse_scale) as u32),
        },
        inverse_scale,
    ).unwrap()
}

#[test]
fn face_pick_rejects_invisible_geometry_beyond_the_actual_orthographic_far_plane() {
    let mut before = record();
    before.translation = [0.0, 0.0, -100.0];
    let mesh = world_mesh(&before).unwrap();
    for inverse_scale in [1.0, 0.5] {
        // 正式正交公式给出相机z=74、far=149；目标全部位于世界z=-75的远面之后。
        let camera = front_camera(true, inverse_scale);
        assert!(mesh.vertices.iter().all(|&point| camera.project(point).is_none()));
        assert!(mesh.nearest_face(camera.ray(Vec2::ZERO).unwrap()).is_some(),
            "负对照必须证明无穷射线仍能打中已被相机裁掉的真实三角");
        assert_eq!(mesh.pick(MeshElementKind::Face, camera, Vec2::ZERO), None,
            "面模式不能选择正交远面之外的不可见几何，inverse_scale={inverse_scale}");
    }
}

#[test]
fn real_face_click_cannot_select_a_target_outside_the_orthographic_depth_range() {
    let (mut app, root, node) = app();
    let mut before = current(&app).clone();
    before.translation = [0.0, 0.0, -100.0];
    app.world_mut().resource_mut::<EditorAuthoringSession>()
        .draft.as_mut().unwrap().placements.placements[0] = before.clone();
    app.world_mut().get_mut::<Transform>(root).unwrap().translation = Vec3::from_array(before.translation);
    app.world_mut().resource_mut::<SceneEditorState>().orthographic = true;
    {
        let mut viewport = app.world_mut().resource_mut::<SceneEditorViewportState>();
        viewport.center = Vec3::ZERO;
        viewport.scene_center = Vec3::ZERO;
        viewport.scene_half_extents = Vec3::splat(10.0);
        viewport.extent = 8.0;
        viewport.yaw = 0.0;
        viewport.pitch = 0.0;
    }
    // 场景树已有的稳定ID选择允许进入网格会话；真正的视口点击仍必须服从相机深度。
    enter(&mut app, node);
    request(&mut app, MeshEditAction::Kind(MeshElementKind::Face));
    fixture::step(&mut app, node, Vec2::ZERO);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    fixture::step(&mut app, node, Vec2::ZERO);
    let state = app.world().resource::<MeshEditState>();
    assert!(state.active());
    assert_eq!(state.hover, None, "被裁掉的面不能产生悬停反馈");
    assert_eq!(state.selected_count(), 0, "空白点击应清除旧选集，不能命中远面外的对象");
    assert_eq!(current(&app), &before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn face_pick_keeps_partially_clipped_surfaces_in_both_actual_projections() {
    for orthographic in [false, true] {
        for inverse_scale in [1.0, 0.5] {
            let camera = front_camera(orthographic, inverse_scale);
            let mut before = record();
            before.translation = [0.0, 0.0, if orthographic { -75.0 } else { 7.8 }];
            before.yaw_degrees = 45.0;
            if !orthographic {
                before.asset_target = ScenePlacementAssetTarget::Cuboid {
                    size_mm: [200; 3], color_srgb: [160; 3], roughness_milli: 700, collision: true,
                };
            }
            let mesh = world_mesh(&before).unwrap();
            assert!(mesh.vertices.iter().any(|&point| camera.project(point).is_none()));
            assert!(mesh.vertices.iter().any(|&point| camera.project(point).is_some()));
            let local = if orthographic { Vec3::new(0.5, 0.2, 1.0) } else { Vec3::new(-0.05, 0.02, -0.1) };
            let point = Vec3::from_array(before.translation) + Quat::from_rotation_y(45.0_f32.to_radians()) * local;
            let cursor = camera.project(point).unwrap();
            assert!(cursor.abs().max_element() < 0.5, "样本必须在实际视口内");
            let ray = camera.ray(cursor).unwrap();
            let (face, distance) = mesh.nearest_face(ray).unwrap();
            assert!(camera.project(ray.get_point(distance)).is_some());
            assert_eq!(mesh.pick(MeshElementKind::Face, camera, cursor), Some(face),
                "只裁命中位置，不能因同面其他角被裁掉就拒绝可见部分");
        }
    }
}

#[test]
fn face_pick_keeps_near_clipping_and_front_surface_occlusion() {
    for orthographic in [false, true] {
        let camera = front_camera(orthographic, 1.0);
        let mut before = record();
        before.translation = [0.0; 3];
        let mesh = world_mesh(&before).unwrap();
        assert_eq!(mesh.pick(MeshElementKind::Face, camera, Vec2::ZERO), Some(5));
        assert!(!mesh.visible(camera, Vec3::new(0.0, 0.0, -1.0)), "后面仍被同一真实前面遮挡");

        before.translation[2] = if orthographic { 73.95 } else { 7.9 };
        before.asset_target = ScenePlacementAssetTarget::Cuboid {
            size_mm: [20; 3], color_srgb: [160; 3], roughness_milli: 700, collision: true,
        };
        let mesh = world_mesh(&before).unwrap();
        assert!(mesh.vertices.iter().all(|&point| camera.project(point).is_none()));
        assert_eq!(mesh.pick(MeshElementKind::Face, camera, Vec2::ZERO), None,
            "完全位于近面之前的几何仍不可选");
    }
}
