//! 将有限ECS鼠标轨迹交给原相机与地面查询；不伪造命中、像素或作者Mesh。

use super::{resources::TerrainWorkflow, types::*};
use crate::{
    components::SceneEditorViewport,
    resources::{SceneEditorState, SceneEditorViewportState},
    types::{EditorTool, EditorView},
    viewport::modes::{ViewportMode, ViewportModeState},
};
use bevy::{prelude::*, ui::RelativeCursorPosition};

pub(super) fn inject(world: &mut World) {
    let (phase, domain, tail, release) = {
        let mut state = world.resource_mut::<TerrainWorkflow>();
        let release = state.mouse_owned && state.phase != Phase::Paint;
        if release {
            state.mouse_owned = false;
        }
        (state.phase, state.domain, state.tail, release)
    };
    if release {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
    }
    if !matches!(phase, Phase::Hover | Phase::Paint | Phase::Release) {
        return;
    }
    if let Some(mut mode) = world.get_resource_mut::<ViewportModeState>() {
        mode.mode = ViewportMode::Edit;
        mode.pointer_over_controls = false;
    }
    {
        let mut editor = world.resource_mut::<SceneEditorState>();
        editor.tool = match domain {
            Domain::Height => EditorTool::Raise,
            Domain::Surface => EditorTool::Surface,
            Domain::Blade => EditorTool::Vegetation,
        };
        editor.view = EditorView::Top;
        editor.orthographic = true;
        editor.brush_radius = if domain == Domain::Height { 3.5 } else { 2. };
        editor.brush_strength = 1.;
        editor.terrain_sculpt_speed = 4.;
    }
    {
        let mut viewport = world.resource_mut::<SceneEditorViewportState>();
        viewport.center = domain.center();
        viewport.extent = 16.;
        viewport.yaw = 0.;
        viewport.pitch = std::f32::consts::FRAC_PI_2;
    }
    for mut cursor in world
        .query_filtered::<&mut RelativeCursorPosition, With<SceneEditorViewport>>()
        .iter_mut(world)
    {
        cursor.cursor_over = true;
        cursor.normalized = Some(Vec2::new(if tail { 0.12 } else { -0.12 }, 0.));
    }
    if phase == Phase::Paint {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        world.resource_mut::<TerrainWorkflow>().mouse_owned = true;
    }
}

/// 只在已观察原松手并退出笔刷后恢复视口；不清历史、不重置作者草稿。
pub(super) fn restore(world: &mut World, state: &mut TerrainWorkflow) {
    if let Some((view, ortho, center, extent, yaw, pitch)) = state.view.take() {
        let mut editor = world.resource_mut::<SceneEditorState>();
        editor.view = view;
        editor.orthographic = ortho;
        editor.tool = EditorTool::Inspect;
        let mut viewport = world.resource_mut::<SceneEditorViewportState>();
        viewport.center = center;
        viewport.extent = extent;
        viewport.yaw = yaw;
        viewport.pitch = pitch;
    }
}
