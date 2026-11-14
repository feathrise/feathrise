//! 显式实窗夹具只向原 ECS 鼠标源投递；命中、雕刻和种植仍由正式系统完成。

use super::{resources::GeometryFixture, types::*};
use crate::{components::SceneEditorViewport, resources::{SceneEditorState, SceneEditorViewportState},
    types::{EditorTool, EditorView}, viewport::modes::{ViewportMode, ViewportModeState}};
use bevy::{prelude::*, ui::RelativeCursorPosition};

pub(super) fn inject(world: &mut World) {
    let (phase, domain, center, release, tail) = {
        let mut state = world.resource_mut::<GeometryFixture>();
        let release = state.mouse_owned && state.phase != Phase::Paint;
        if release { state.mouse_owned = false; }
        (state.phase, state.domain, state.center(), release, state.phase_started.elapsed().as_millis() > 300)
    };
    if release { world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left); }
    if !matches!(phase, Phase::Hover | Phase::Paint | Phase::Release | Phase::Settle | Phase::Capture) { return; }
    if let Some(mut mode) = world.get_resource_mut::<ViewportModeState>() {
        mode.mode = ViewportMode::Edit; mode.pointer_over_controls = false;
    }
    {
        let mut editor = world.resource_mut::<SceneEditorState>();
        editor.tool = if matches!(phase, Phase::Settle | Phase::Capture) { EditorTool::Inspect }
            else if domain == Domain::Height { EditorTool::Raise } else { EditorTool::Vegetation };
        editor.view = EditorView::Top; editor.orthographic = true;
        editor.brush_radius = 0.75; editor.brush_strength = 1.; editor.terrain_sculpt_speed = 1.;
    }
    let normalized = {
        let mut viewport = world.resource_mut::<SceneEditorViewportState>();
        viewport.center = center; viewport.extent = 8.; viewport.yaw = 0.;
        // 底面必须由真实反向相机射线命中；不设置命中点、法线或字段成功状态。
        viewport.pitch = if domain == Domain::VoxelUnderside { -std::f32::consts::FRAC_PI_2 }
            else { std::f32::consts::FRAC_PI_2 };
        stroke_pointer(&viewport, phase == Phase::Paint && tail)
    };
    for mut cursor in world.query_filtered::<&mut RelativeCursorPosition, With<SceneEditorViewport>>().iter_mut(world) {
        cursor.cursor_over = true;
        cursor.normalized = Some(normalized);
    }
    if phase == Phase::Paint {
        world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        world.resource_mut::<GeometryFixture>().mouse_owned = true;
    }
}

/// 用正式相机投影瞄准左右半米，宽屏也不会把固定0.1归一化偏移放到四米来源之外。
fn stroke_pointer(viewport: &SceneEditorViewportState, tail: bool) -> Vec2 {
    let (mut projection, transform) = crate::viewport::editor_camera_pose(true, viewport);
    projection.update(viewport.render_size.x.max(1) as f32, viewport.render_size.y.max(1) as f32);
    let target = viewport.center + Vec3::X * if tail { 0.5 } else { -0.5 };
    let clip = projection.get_clip_from_view() * transform.to_matrix().inverse() * target.extend(1.);
    Vec2::new(clip.x, -clip.y) / (clip.w * 2.)
}

#[cfg(test)]
#[path = "pointer_tests.rs"]
mod tests;

pub(super) fn restore(world: &mut World, state: &mut GeometryFixture) {
    super::screenshot::restore(world);
    if state.mouse_owned {
        world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        state.mouse_owned = false;
    }
    if let Some((view, orthographic, center, extent, yaw, pitch)) = state.view.take() {
        let mut editor = world.resource_mut::<SceneEditorState>();
        editor.tool = EditorTool::Inspect; editor.view = view; editor.orthographic = orthographic;
        let mut viewport = world.resource_mut::<SceneEditorViewportState>();
        viewport.center = center; viewport.extent = extent; viewport.yaw = yaw; viewport.pitch = pitch;
    }
}
