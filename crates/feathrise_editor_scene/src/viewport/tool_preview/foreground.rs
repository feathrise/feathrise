//! 在作者相机输出中合成手柄的实心图形和约束线。坐标仍来自原拾取相机。

use crate::components::SceneEditorCamera;
use crate::viewport::placement_move::{MoveAxis, MoveCamera, plane_corners};
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{Core3dSystems, schedule::Core3d, tonemapping::tonemapping, upscaling::upscaling},
    mesh::VertexBufferLayout,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_component::ExtractComponentPlugin,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        view::ViewTarget,
    },
};
use feathrise_author_tools::TransformPreview;

#[derive(Clone, Default, Resource, ExtractResource)]
#[extract_app(RenderApp)]
pub(crate) struct Foreground(Vec<[f32; 6]>);

impl Foreground {
    pub(super) fn clear(&mut self) { self.0.clear(); }

    pub(crate) fn canvas(&mut self, size: Vec2) -> Canvas<'_> { Canvas { vertices: &mut self.0, size } }

    #[cfg(test)]
    pub(crate) fn contains_viewport_guide(&self) -> bool {
        self.0.iter().any(|vertex| vertex[0].abs() > 1. || vertex[1].abs() > 1.)
    }
}

/// 所有尺寸使用视口逻辑像素，最终按真实离屏纹理大小投影。
pub(crate) struct Canvas<'a> {
    vertices: &'a mut Vec<[f32; 6]>,
    size: Vec2,
}

impl Canvas<'_> {
    pub(crate) fn point(&self, camera: MoveCamera, world: Vec3) -> Option<Vec2> {
        Some(camera.project(world)? * self.size)
    }

    pub(crate) fn quad_points(&self, camera: MoveCamera, [a, b, c, d]: [Vec3; 4]) -> Option<[Vec2; 4]> {
        Some([self.point(camera, a)?, self.point(camera, b)?, self.point(camera, c)?, self.point(camera, d)?])
    }

    pub(crate) fn triangle(&mut self, points: [Vec2; 3], tint: Color) {
        let [r, g, b, a] = tint.to_linear().to_f32_array();
        self.vertices.extend(points.map(|point| {
            let ndc = (point / self.size) * Vec2::new(2., -2.);
            [ndc.x, ndc.y, r, g, b, a]
        }));
    }

    pub(crate) fn quad(&mut self, [a, b, c, d]: [Vec2; 4], tint: Color) {
        self.triangle([a, b, c], tint);
        self.triangle([a, c, d], tint);
    }

    pub(crate) fn line(&mut self, start: Vec2, end: Vec2, width: f32, tint: Color) {
        if let Some(offset) = (end - start).perp().try_normalize() {
            let offset = offset * (width * 0.5);
            self.quad([start + offset, end + offset, end - offset, start - offset], tint);
        }
    }

    pub(crate) fn plane(&mut self, corners: [Vec2; 4], tint: Color) {
        self.quad(corners, tint.with_alpha(tint.alpha() * 0.5));
        for i in 0..4 { self.line(corners[i], corners[(i + 1) % 4], 1., tint); }
    }

    pub(crate) fn world_line(&mut self, camera: MoveCamera, start: Vec3, end: Vec3, width: f32, tint: Color) {
        if let Some((start, end)) = self.point(camera, start).zip(self.point(camera, end)) {
            self.line(start, end, width, tint);
        }
    }

    pub(crate) fn radial_scale_guide(&mut self, center: Vec2, pointer: Vec2, radius: f32) {
        let ray = pointer - center;
        let Some(direction) = ray.try_normalize() else { return; };
        let side = direction.perp() * 4.0;
        let outline = Color::srgb_u8(24, 24, 24);
        let dash_start = radius + 3.0;
        let dash_end = ray.length() - 12.0;
        // 一次仅有一条拖动线。普通视口保留短节奏，高分屏或移出视口时也只提交有界的线段。
        const MAX_DASHES: usize = 256;
        let period = 9.0_f32.max((dash_end - dash_start) / MAX_DASHES as f32);
        for index in 0..MAX_DASHES {
            let start = dash_start + index as f32 * period;
            if start >= dash_end { break; }
            let a = center + direction * start;
            let b = center + direction * (start + 5.0).min(dash_end);
            self.line(a, b, 3.0, outline);
            self.line(a, b, 1.0, Color::WHITE);
        }
        let inner = pointer - direction * 10.0;
        let outer = pointer + direction * 10.0;
        for (tint, width) in [(outline, 4.0), (Color::WHITE, 2.0)] {
            self.line(inner, outer, width, tint);
            for (tip, base) in [(inner, inner + direction * 5.0), (outer, outer - direction * 5.0)] {
                self.line(base + side, tip, width, tint);
                self.line(base - side, tip, width, tint);
            }
        }
    }

    pub(crate) fn guide(&mut self, camera: MoveCamera, origin: Vec3, axis: Vec3, length: f32, tint: Color) {
        let Some(start) = self.point(camera, origin) else { return; };
        let Some(end) = self.point(camera, origin + axis * length)
            .or_else(|| self.point(camera, origin - axis * length)) else { return; };
        let projected = end - start;
        if projected.length_squared() <= 1. { return; } // 端视轴不到一个逻辑像素，避免把浮点误差放大成斜线。
        // 透视投影的世界直线仍为屏幕直线，延伸到视口之外后由裁剪器收口。
        let span = projected.normalize() * (self.size.length() + start.length());
        self.line(start - span, start + span, 2., tint);
    }
}

pub(super) fn draw_scale_center(foreground: &mut Foreground, camera: MoveCamera, handle: &TransformPreview) {
    let TransformPreview::Scale { origin, right, half_size, color, .. } = *handle else { return; };
    let origin = Vec3::from_array(origin);
    let mut canvas = foreground.canvas(camera.size * camera.inverse_scale);
    let Some(center) = canvas.point(camera, origin) else { return; };
    let Some(edge) = canvas.point(camera, origin + Vec3::from_array(right) * half_size) else { return; };
    let radius = edge.distance(center);
    let tint = LinearRgba::from_f32_array(color).into();
    let rim = |i| center + Vec2::from_angle(i as f32 * std::f32::consts::TAU / 32.0) * radius;
    for i in 0..32 { canvas.line(rim(i), rim(i + 1), 2.0, tint); }
}

pub(super) fn draw_move(foreground: &mut Foreground, camera: MoveCamera, handle: &TransformPreview) {
    let TransformPreview::Move {
        origin, length, axes, active_axes, plane, plane_active, plane_cross,
        plane_color, color_active, axis_colors, anchor, marker_radius, ..
    } = *handle else { return; };
    let origin = Vec3::from_array(origin);
    let mut canvas = foreground.canvas(camera.size * camera.inverse_scale);
    let active = Color::from(LinearRgba::from_f32_array(color_active));
    for axis in MoveAxis::ALL {
        let bit = 1 << axis.index();
        if axes & bit == 0 { continue; }
        let Some((start, tip)) = camera.axis_segment(origin, axis, length) else { continue; };
        let Some(base) = canvas.point(camera, origin + axis.direction() * (length * 0.8)) else { continue; };
        let tint = if active_axes & bit != 0 { active } else { LinearRgba::from_f32_array(axis_colors[axis.index()]).into() };
        // 实心锥头的屏幕轮廓。尖端沿用原拾取端点，轴杆只画到锥底。
        let side = (tip - start).perp().normalize() * 4.;
        canvas.line(start, base, 2., tint);
        canvas.triangle([tip, base + side, base - side], tint);
    }
    if plane && let Some(corners) = camera.project_quad(plane_corners(origin, length)) {
        let tint = if plane_active { active } else { LinearRgba::from_f32_array(plane_color).into() };
        canvas.plane(corners, tint);
        if plane_cross {
            canvas.line(corners[0], corners[2], 1., tint);
            canvas.line(corners[1], corners[3], 1., tint);
        }
    }
    if let Some(anchor) = anchor { canvas.world_line(camera, anchor.into(), origin, 2., active); }
    if let Some(radius) = marker_radius {
        let circle = |index: u32| {
            let angle = index as f32 / 16. * std::f32::consts::TAU;
            origin + Vec3::new(angle.cos(), 0., angle.sin()) * radius
        };
        for i in 0..16 { canvas.world_line(camera, circle(i), circle(i + 1), 2., active); }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Foreground>();
    if app.get_sub_app(RenderApp).is_none() { return; }
    embedded_asset!(app, "foreground.wesl");
    app.add_plugins((ExtractResourcePlugin::<Foreground>::default(), ExtractComponentPlugin::<SceneEditorCamera>::default()));
    app.sub_app_mut(RenderApp)
        .init_resource::<SpecializedRenderPipelines<ForegroundPipeline>>()
        .add_systems(RenderStartup, init_pipeline)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
        // 和工作底色、真实深度解耦。色调映射完成后合成，轴色不随场景曝光变化。
        .add_systems(Core3d, draw.in_set(Core3dSystems::PostProcess).after(tonemapping).before(upscaling));
}

#[derive(Resource)]
struct ForegroundPipeline(Handle<Shader>);

fn init_pipeline(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(ForegroundPipeline(load_embedded_asset!(assets.as_ref(), "foreground.wesl")));
    commands.insert_resource(ForegroundBuffer(RawBufferVec::new(BufferUsages::VERTEX)));
}

impl SpecializedRenderPipeline for ForegroundPipeline {
    type Key = TextureFormat;

    fn specialize(&self, format: TextureFormat) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("editor_transform_foreground".into()),
            vertex: VertexState {
                shader: self.0.clone(), entry_point: Some("vertex".into()),
                buffers: vec![VertexBufferLayout::from_vertex_formats(VertexStepMode::Vertex,
                    vec![VertexFormat::Float32x2, VertexFormat::Float32x4])],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: self.0.clone(), entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format, blend: Some(BlendState::ALPHA_BLENDING), write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

#[derive(Resource)]
struct ForegroundBuffer(RawBufferVec<[f32; 6]>);

#[cfg(test)]
#[path = "foreground_tests.rs"]
mod tests;

fn prepare(frame: Res<Foreground>, mut buffer: ResMut<ForegroundBuffer>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    if !frame.is_changed() { return; }
    buffer.0.clear();
    // 只上传本帧手柄投影。没有地形、对象遍历或临时网格资产。
    for vertex in &frame.0 { buffer.0.push(*vertex); }
    buffer.0.write_buffer(&device, &queue);
}

fn draw(
    view: ViewQuery<&ViewTarget, With<SceneEditorCamera>>,
    buffer: Res<ForegroundBuffer>,
    pipeline: Res<ForegroundPipeline>,
    mut variants: ResMut<SpecializedRenderPipelines<ForegroundPipeline>>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let target = view.into_inner();
    let pipeline = variants.specialize(&cache, &pipeline, target.main_texture_format());
    let (Some(pipeline), Some(vertices)) = (cache.get_render_pipeline(pipeline), buffer.0.buffer()) else { return; };
    if buffer.0.len() == 0 { return; }
    let attachments = [Some(target.get_unsampled_color_attachment())];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("editor_transform_foreground"),
        color_attachments: &attachments,
        ..default()
    });
    pass.set_render_pipeline(pipeline);
    pass.set_vertex_buffer(0, vertices.slice(..));
    pass.draw(0..buffer.0.len() as u32, 0..1);
}
