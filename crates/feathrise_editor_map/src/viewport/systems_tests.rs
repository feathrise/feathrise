// 编辑器视口导航与射线命中的纯算法回归测试。
use super::*;
const APPLICATION_MODEL_LAYER: usize = 24; // 中性应用夹具，不依赖任何游戏角色层定义。
const APPLICATION_TRANSIENT_LAYER: usize = 25;

#[test]
fn editor_camera_requires_explicit_application_layers_without_adding_transient_views() {
    let layers = editor_viewport_render_layers(None);
    assert!(!layers.intersects(&RenderLayers::layer(APPLICATION_MODEL_LAYER)));
    assert_eq!(layers, editor_viewport_render_layers(Some(&EditorApplicationViewLayers::default())));

    let application = EditorApplicationViewLayers {
        camera: RenderLayers::layer(APPLICATION_MODEL_LAYER),
        framing: RenderLayers::layer(APPLICATION_TRANSIENT_LAYER),
    };
    let layers = editor_viewport_render_layers(Some(&application));
    assert!(layers.intersects(&RenderLayers::layer(APPLICATION_MODEL_LAYER)));
    assert!(layers.intersects(&RenderLayers::layer(0))); // 应用输入只能追加，地图与环境原集合仍在。
    assert!(layers.intersects(&RenderLayers::layer(WEATHER_EFFECT_LAYER)));
    assert!(!layers.intersects(&RenderLayers::layer(APPLICATION_TRANSIENT_LAYER))); // 取景资格不会隐式变为相机可见资格。
}

#[test]
fn f1_air_wall_layer_does_not_enter_f12_authoring_camera() {
    let layers = editor_viewport_render_layers(None); // 验证实际相机配置，不复制层集合。
    assert!(!layers.intersects(&RenderLayers::layer(AIR_WALL_GAMEPLAY_LAYER)));
    assert!(!layers.intersects(&RenderLayers::layer(AIR_WALL_OUTLINE_GAMEPLAY_LAYER)));
    assert!(layers.intersects(&RenderLayers::layer(MAP_EDITOR_BRUSH_RENDER_LAYER))); // F12空气墙辅助线继续独立可见。
    let application = EditorApplicationViewLayers {
        camera: RenderLayers::from_layers(&[AIR_WALL_GAMEPLAY_LAYER, AIR_WALL_OUTLINE_GAMEPLAY_LAYER]),
        ..default()
    };
    assert_eq!(layers, editor_viewport_render_layers(Some(&application))); // 应用追加不能突破作者/运行调试轮廓的隔离。
}

#[test]
fn right_drag_vertical_motion_uses_direct_pitch_direction() {
    let pitch = 0.64; // 使用默认透视俯仰角验证两个拖动方向。
    assert!(orbit_pitch_after_drag(pitch, 10.0) > pitch); // 鼠标向下拖时俯仰角应增加。
    assert!(orbit_pitch_after_drag(pitch, -10.0) < pitch); // 鼠标向上拖时俯仰角应减小。
}

#[test]
fn right_drag_can_orbit_below_terrain_and_continuously_cross_camera_poles() {
    let underside_pitch = orbit_pitch_after_drag(0.64, -200.0); // 持续向上拖必须越过水平线进入地形下方。
    assert!(underside_pitch < 0.0); // 负俯仰是相机位于轨道中心下方的明确身份。
    let top = std::f32::consts::FRAC_PI_2;
    assert!(orbit_pitch_after_drag(top, 10.0) > top); // 顶视向下拖也能继续旋转，不能停在原极点保护夹角。
    assert!(orbit_pitch_after_drag(top, -10.0) < top);
    for motion in [-10_000.0, 10_000.0] {
        let pitch = orbit_pitch_after_drag(0.64, motion);
        assert!(pitch.is_finite() && pitch.abs() <= std::f32::consts::PI); // 完整转圈后角度有界，姿态仍连续。
    }
}

#[test]
fn changing_projection_preserves_focus_plane_screen_scale_at_every_direction() {
    let center = Vec3::new(10.0, 20.0, 30.0);
    for view in [EditorView::Perspective, EditorView::Top, EditorView::Front, EditorView::Side] {
        let (yaw, pitch) = orbit_angles_from_view(view);
        for extent in [MIN_EXTENT_METERS, 120.0, MAX_EXTENT_METERS] {
            let viewport = MapEditorViewportState {
                _image: Handle::default(), camera: Entity::PLACEHOLDER,
                center, scene_center: Vec3::ZERO, scene_half_extents: Vec3::new(500.0, 80.0, 300.0),
                extent, yaw, pitch, framed_map_id: None, render_size: UVec2::new(800, 600),
            };
            let (mut perspective, perspective_pose) = editor_camera_pose(false, &viewport);
            let (mut orthographic, orthographic_pose) = editor_camera_pose(true, &viewport);
            perspective.update(800.0, 600.0);
            orthographic.update(800.0, 600.0);
            assert!(perspective_pose.rotation.dot(orthographic_pose.rotation).abs() > 0.99999);
            let point = center + perspective_pose.right() * (extent * 0.1)
                + perspective_pose.up() * (extent * 0.05); // 在焦点平面取偏离中心的点，不能只检查必为屏幕中心的焦点。
            let to_screen = |projection: &Projection, transform: Transform| {
                let view_point = transform.rotation.inverse() * (point - transform.translation);
                let clip = projection.get_clip_from_view() * view_point.extend(1.0);
                Vec2::new(clip.x, clip.y) / clip.w
            };
            assert!(to_screen(&perspective, perspective_pose)
                .distance(to_screen(&orthographic, orthographic_pose)) < 0.0001); // 包括缩放上下限，来回切换不能累积缩放误差。
        }
    }
}

// 实际GLB三角形射线命中由共享TerrainEditingIndex测试覆盖；视口不再拥有第二套规则高度场算法。
