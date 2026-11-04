//! 只向原ECS输入提供有限按下/松开；河流及建筑的落点、身份与后台校验仍由生产系统决定。

use super::{
    resources::Fixture,
    types::{Phase, RIVER_POINTERS},
};
use crate::{
    components::MapEditorViewport,
    resources::{MapEditorState, MapEditorViewportState},
    types::{EditorTool, EditorView},
    viewport::modes::{ViewportMode, ViewportModeState},
};
use bevy::{prelude::*, ui::RelativeCursorPosition};

pub(super) fn inject(world: &mut World) {
    let (phase, point, release) = {
        let mut fixture = world.resource_mut::<Fixture>();
        let pressed = matches!(fixture.phase, Phase::PressRiver | Phase::PressBuilding);
        let release = fixture.mouse_owned && !pressed;
        if release {
            fixture.mouse_owned = false;
        }
        (fixture.phase, fixture.point, release)
    };
    if release {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
    }
    let river = matches!(
        phase,
        Phase::HoverRiver | Phase::PressRiver | Phase::WaitPoint
    );
    let building = matches!(
        phase,
        Phase::HoverBuilding | Phase::PressBuilding | Phase::WaitBuilding
    );
    if !river && !building {
        return;
    }
    if let Some(mut mode) = world.get_resource_mut::<ViewportModeState>() {
        mode.mode = ViewportMode::Edit;
        mode.pointer_over_controls = false;
    }
    {
        let mut editor = world.resource_mut::<MapEditorState>();
        editor.tool = if river {
            EditorTool::RiverSpline
        } else {
            EditorTool::Building
        };
        editor.view = EditorView::Top;
        editor.orthographic = true;
    }
    {
        let mut viewport = world.resource_mut::<MapEditorViewportState>();
        viewport.center = if river {
            Vec3::new(-12., 0., 8.)
        } else {
            Vec3::new(-16., 0., -16.)
        };
        viewport.extent = 16.;
        viewport.yaw = 0.;
        viewport.pitch = std::f32::consts::FRAC_PI_2;
    }
    let normalized = if river {
        Vec2::from_array(RIVER_POINTERS[point.min(2)])
    } else {
        Vec2::ZERO
    };
    for mut cursor in world
        .query_filtered::<&mut RelativeCursorPosition, With<MapEditorViewport>>()
        .iter_mut(world)
    {
        cursor.cursor_over = true;
        cursor.normalized = Some(normalized);
    }
    if matches!(phase, Phase::PressRiver | Phase::PressBuilding) {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        world.resource_mut::<Fixture>().mouse_owned = true;
    }
}

pub(super) fn restore(world: &mut World, fixture: &mut Fixture) {
    if let Some((view, ortho, center, extent, yaw, pitch)) = fixture.view.take() {
        let mut editor = world.resource_mut::<MapEditorState>();
        editor.view = view;
        editor.orthographic = ortho;
        editor.tool = EditorTool::Inspect;
        let mut viewport = world.resource_mut::<MapEditorViewportState>();
        viewport.center = center;
        viewport.extent = extent;
        viewport.yaw = yaw;
        viewport.pitch = pitch;
    }
    if let Some(controls) = fixture.controls.take() {
        *world.resource_mut::<crate::ui::building_presets::BuildingPlacementControls>() = controls;
    }
}
