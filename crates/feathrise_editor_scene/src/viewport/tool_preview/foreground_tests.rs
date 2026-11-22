//! 用真实作者相机回读近距离立方体，验证被实体挡住的手柄仍有填色和悬停反馈。

use super::{Foreground, MoveAxis, MoveCamera, SceneEditorCamera, TransformPreview, draw_move, install};
use bevy::{
    camera::{Hdr, RenderTarget},
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{TextureFormat, TextureUsages},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use std::{sync::mpsc, time::{Duration, Instant}};

const EDGE: u32 = 256; // BGRA 行宽已按 GPU 回读要求对齐。
const SIZE: Vec2 = Vec2::splat(EDGE as f32);

impl Foreground {
    /// 按实际提交的三角形采样，允许交互夹具检验特定前景笔画是否出现。
    pub(crate) fn covers_white_at(&self, canvas_size: Vec2, point: Vec2) -> bool {
        let point = point / canvas_size * Vec2::new(2., -2.);
        self.0.chunks_exact(3).any(|triangle| {
            if triangle[0][5] <= 0. || triangle[0][2..5].iter().any(|&channel| channel < 0.75) {
                return false;
            }
            let [a, b, c] = [0, 1, 2].map(|index| Vec2::new(triangle[index][0], triangle[index][1]));
            let sides = [(a, b), (b, c), (c, a)].map(|(a, b)| (b - a).perp_dot(point - a));
            sides.iter().all(|&side| side >= 0.) || sides.iter().all(|&side| side <= 0.)
        })
    }
}

fn sampled_colors(
    app: &mut App,
    target: &Handle<Image>,
    points: [Vec2; 5],
    ready: impl Fn([[u8; 3]; 5]) -> bool,
) -> [[u8; 3]; 5] {
    let (send, receive) = mpsc::channel();
    let readback = app.world_mut().spawn(Readback::texture(target.clone()))
        .observe(move |event: On<ReadbackComplete>| { let _ = send.send(event.data.clone()); })
        .id();
    let started = Instant::now();
    let mut colors = [[0; 3]; 5];
    loop {
        app.update();
        for bytes in receive.try_iter() {
            assert_eq!(bytes.len(), (EDGE * EDGE * 4) as usize);
            colors = points.map(|point| {
                let UVec2 { x, y } = (point + SIZE * 0.5).as_uvec2();
                let offset = ((y * EDGE + x) * 4) as usize;
                [bytes[offset + 2], bytes[offset + 1], bytes[offset]]
            });
            if ready(colors) {
                app.world_mut().despawn(readback);
                return colors;
            }
        }
        assert!(started.elapsed() < Duration::from_secs(15), "手柄 GPU 回读未取得预期颜色 {colors:?}");
        std::thread::sleep(Duration::from_millis(16));
    }
}

#[test]
fn constraint_guide_crosses_both_viewport_edges() {
    let pose = Transform::from_xyz(0., 2., 5.).looking_at(Vec3::ZERO, Vec3::Y);
    let camera = MoveCamera::from_projection(&Projection::default(), &GlobalTransform::from(pose), SIZE, 1.).unwrap();
    let mut frame = Foreground::default();
    // 约束起点偏向一边，短箭头仍应生成贯穿视口两端的 2px 直线。
    frame.canvas(SIZE).guide(camera, Vec3::X, Vec3::X, 0.5, Color::WHITE);
    assert_eq!(frame.0.len(), 6);
    assert!(frame.0.iter().any(|vertex| vertex[0] < -1.));
    assert!(frame.0.iter().any(|vertex| vertex[0] > 1.));
    assert!(frame.0.iter().all(|vertex| (vertex[1].abs() * SIZE.y * 0.5 - 1.).abs() < 0.001));
}

#[test]
fn radial_scale_guide_has_gaps_and_opposing_arrowheads() {
    let size = Vec2::new(360., 240.);
    let center = Vec2::new(-70., -80.);
    let direction = Vec2::new(0.6, 0.8);
    let pointer = center + direction * 120.;
    let radius = 12.;
    let mut frame = Foreground::default();
    frame.canvas(size).radial_scale_guide(center, pointer, radius);

    // 斜拖穿过非方形视口，交替采样虚线段和间隔，还要给中心圆留空。
    for (distance, covered) in [(radius + 1., false), (17., true), (22., false), (26., true), (31., false)] {
        assert_eq!(frame.covers_white_at(size, center + direction * distance), covered);
    }
    assert!(!frame.contains_viewport_guide());
    assert!(frame.covers_white_at(size, pointer));

    let across = direction.perp();
    for sign in [-1., 1.] {
        assert!(frame.covers_white_at(size, pointer + direction * (sign * 9.)));
        for side in [-1., 1.] {
            // 两个尖端都应有径向两侧的箭翼。只画一条短横线会漏掉这些采样点。
            let wing = pointer + direction * (sign * 7.) + across * (side * 2.5);
            assert!(frame.covers_white_at(size, wing), "缺少径向 {sign}、侧向 {side} 的箭翼");
        }
    }
}

#[test]
#[ignore = "需要真实 GPU。只启动离屏作者相机，回读实体遮挡后的手柄像素"]
fn gpu_move_foreground_fills_handles_over_opaque_mesh() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.build()
        .set(WindowPlugin { primary_window: None, exit_condition: ExitCondition::DontExit, ..default() })
        .disable::<WinitPlugin>()
        .disable::<PipelinedRenderingPlugin>()
        .disable::<bevy::log::LogPlugin>())
        .insert_resource(ClearColor(Color::BLACK));
    install(&mut app);
    app.finish();
    app.cleanup();

    let world = app.world_mut();
    let mut image = Image::new_target_texture(EDGE, EDGE, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = world.resource_mut::<Assets<Image>>().add(image);
    let projection = Projection::Perspective(PerspectiveProjection { fov: std::f32::consts::FRAC_PI_4, ..default() });
    let pose = Transform::from_xyz(3., 4., 5.).looking_at(Vec3::ZERO, Vec3::Y);
    let camera = MoveCamera::from_projection(&projection, &GlobalTransform::from(pose), SIZE, 1.).unwrap();
    world.spawn((Camera3d::default(), SceneEditorCamera, Hdr, RenderTarget::from(target.clone()), projection, pose, Msaa::Sample4));
    let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(4., 4., 4.));
    let material = world.resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial { base_color: Color::linear_rgb(0.25, 0.25, 0.25), unlit: true, ..default() });
    world.spawn((Mesh3d(mesh), MeshMaterial3d(material)));

    let origin = Vec3::ZERO;
    let length = camera.handle_length(origin).unwrap();
    assert!(length < 2., "箭杆、箭头与平面应完全位于立方体内部");
    let world_point = |point| camera.project(point).unwrap() * SIZE;
    let (shaft_start, tip) = camera.axis_segment(origin, MoveAxis::X, length).unwrap();
    let base = world_point(origin + Vec3::X * length * 0.8);
    let shaft = shaft_start.lerp(base, 0.55);
    // 两侧采样点都避开轴杆中线和锥头轮廓。只画箭翼时无法通过。
    let head = base.lerp(tip, 0.2);
    let side = (tip - base).perp().normalize() * 2.0;
    let plane = camera.plane_quad(origin, length).unwrap().into_iter().sum::<Vec2>() * 0.25;
    let quiet = world_point(-Vec3::X * 0.5);
    let points = [shaft, head + side, head - side, plane, quiet];
    let baseline = sampled_colors(&mut app, &target, points, |colors| {
        colors.into_iter().all(|[r, g, b]| r > 48 && r.abs_diff(g) <= 4 && r.abs_diff(b) <= 4)
    });

    draw_move(&mut app.world_mut().resource_mut::<Foreground>(), camera, &TransformPreview::Move {
        spatial: false, origin: origin.to_array(), length, axes: 1, active_axes: 1,
        plane: true, plane_active: false, plane_cross: false, plane_color: [0., 1., 0., 1.],
        color_active: [1., 0., 0., 0.75], axis_colors: [[0., 0., 1., 1.]; 3],
        anchor: None, marker_radius: None,
    });
    // 红色代表悬停高亮，半透明平面为绿色。保留部分底色才算实际混合。
    let blended = |pixel: [u8; 3], previous: [u8; 3], main: usize| {
        pixel[main] > previous[main].saturating_add(24)
            && (0..3).filter(|channel| *channel != main)
                .all(|channel| pixel[channel] > 12 && pixel[channel].saturating_add(8) < previous[channel])
    };
    sampled_colors(&mut app, &target, points, |colors| {
        (0..3).all(|index| blended(colors[index], baseline[index], 0))
            && blended(colors[3], baseline[3], 1)
            && colors[4] == baseline[4]
    });
    app.world_mut().resource_mut::<Foreground>().clear();
    sampled_colors(&mut app, &target, points, |colors| colors == baseline);

    let radius = 8.0;
    let [a, b, _, d] = camera.screen_square(origin, radius).unwrap();
    let center = world_point(origin);
    let points = [
        center + Vec2::new(-7.5, 0.5), center + Vec2::splat(0.5), center + Vec2::new(13.5, 0.5),
        center + Vec2::new(18.5, 0.5), center + Vec2::new(60.5, 0.5),
    ];
    let baseline = sampled_colors(&mut app, &target, points, |colors| {
        colors.into_iter().all(|[r, g, b]| r > 48 && r.abs_diff(g) <= 4 && r.abs_diff(b) <= 4)
    });
    {
        let mut foreground = app.world_mut().resource_mut::<Foreground>();
        let pivot = center + Vec2::splat(0.5);
        foreground.canvas(SIZE).radial_scale_guide(pivot, pivot + Vec2::X * 60.0, radius);
        super::draw_scale_center(&mut foreground, camera, &TransformPreview::Scale {
            origin: origin.to_array(), right: (b - a).normalize().to_array(),
            up: (d - a).normalize().to_array(), half_size: (b - a).length() * 0.5,
            color: [1.; 4], cube: false,
        });
    }
    // 圆环、虚线实段和双箭头可见，环心和虚线间隔仍为原底色。
    sampled_colors(&mut app, &target, points, |colors| {
        [0, 2, 4].into_iter().all(|index| colors[index].into_iter().all(|channel| channel > 240))
            && colors[1] == baseline[1] && colors[3] == baseline[3]
    });
    app.world_mut().resource_mut::<Foreground>().clear();
    sampled_colors(&mut app, &target, points, |colors| colors == baseline);
}
