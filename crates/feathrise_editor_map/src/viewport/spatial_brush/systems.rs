//! 固定世界位置的手柄与真实落点；预览始终只改会话坐标，不写地图或作者历史。
use super::{resources::SpatialBrushState, types::*};
use crate::{
    authoring::{EditorAuthoringSession, volume_sculpt::VolumeAuthoringState},
    components::MapEditorViewport,
    resources::{MapEditorNumericScrubMouseGuard, MapEditorState, MapEditorViewportState},
    viewport::{
        VegetationGroundCursor,
        brush_cursor::{MAP_EDITOR_BRUSH_RENDER_LAYER, spatial_volume_brush_preview},
        placement_move::{
            MoveAxis, MoveCamera, MoveHandle, MoveInputContext, handle_point, handle_translation,
            axis_colors, visible_axes,
        },
    },
};
use bevy::{prelude::*, ui::RelativeCursorPosition};
use feathrise_author_tools::{AuthorTool, TransformPreview};
use feathrise_scene::{ClipmapGroundHit, DynamicClipmapGroundSurface};

pub(super) fn configure_gizmos(mut store: ResMut<bevy::gizmos::config::GizmoConfigStore>) {
    let (config, _) = store.config_mut::<SpatialBrushGizmos>();
    config.line.width = 2.;
    config.line.perspective = false;
    config.depth_bias = -1.; // 地下预览仍可见；独立组只进入F12相机。
    config.render_layers =
        bevy::camera::visibility::RenderLayers::layer(MAP_EDITOR_BRUSH_RENDER_LAYER);
}

fn cancel_drag(state: &mut SpatialBrushState) {
    if let Some(drag) = state.drag.take() {
        state.center = Some(drag.origin);
    }
}

pub(super) fn handles(
    mut state: ResMut<SpatialBrushState>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    context: MoveInputContext,
    viewport: Option<Res<MapEditorViewportState>>,
    nodes: Query<(&RelativeCursorPosition, &ComputedNode), With<MapEditorViewport>>,
    volume: Res<VolumeAuthoringState>,
    time: Res<Time<Real>>,
) {
    state.sync_scope(&editor, &session);
    if state.drag.is_none()
        && !state.click_pending
        && !mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
        && !mouse.any_just_released([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
    {
        state.owns_mouse = false;
    }
    state.hover = None;
    let node = nodes.single().ok();
    let camera = viewport.as_ref().and_then(|v| {
        node.and_then(|(_, n)| MoveCamera::new(&editor, v, n.inverse_scale_factor()))
    });
    let identity = Identity::capture(&editor, &session);
    let available = state.enabled
        && (state.mode != ApplyMode::Path || state.drag.is_some() || !state.path.busy())
        && context.sculpt_available(&editor)
        && identity.is_some()
        && !session.saving
        && !volume.busy()
        && state.pending.is_none();
    let escaped = keyboard.just_pressed(KeyCode::Escape);
    if escaped && state.path_active() && volume.busy() {
        state.enabled = false;
        state.pending = None;
        keyboard.clear();
        editor.status = "已取消路径雕刻，等待在途任务结束；原完整地形保持不变".into();
        return;
    }
    let chord = mouse.any_pressed([MouseButton::Right, MouseButton::Middle]);
    if state.drag.is_some() {
        let current = available
            && state.drag.as_ref().is_some_and(|d| {
                Some(&d.identity) == identity.as_ref()
                    && Some(d.camera) == camera
                    && node.is_some_and(|(_, n)| n.size() == d.ui_size)
            });
        if escaped || chord || !current {
            cancel_drag(&mut state);
            editor.status = "已取消笔刷移动并恢复原位置".into();
            keyboard.clear();
            return;
        }
        let drag = state.drag.as_ref().unwrap();
        let draft = session.draft.as_ref().unwrap();
        let position = node
            .and_then(|(c, _)| c.normalized)
            .and_then(|p| drag.camera.ray(p))
            .and_then(|r| handle_point(r, drag.origin, drag.handle))
            .and_then(|p| {
                handle_translation(
                    drag.origin,
                    p - drag.grabbed,
                    drag.handle,
                    if keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
                        drag.grid
                    } else {
                        0.
                    },
                    Vec2::from_array(draft.manifest.world_min),
                    Vec2::from_array(draft.manifest.world_max),
                )
            });
        let Some(position) = position.filter(|p| p.abs().max_element() <= 100_000.) else {
            cancel_drag(&mut state);
            editor.status = "笔刷移动射线失效，已恢复原位置".into();
            keyboard.clear();
            return;
        };
        state.center = Some(position);
        if !mouse.pressed(MouseButton::Left) {
            if !mouse.just_released(MouseButton::Left) {
                cancel_drag(&mut state);
            } else {
                state.drag = None;
                editor.status = "笔刷位置已确定；点击执行雕刻才修改地形".into();
            }
        }
        keyboard.clear();
        return;
    }
    if !available || escaped || chord {
        let cancel_placement = available && escaped && (state.armed || state.click_pending);
        state.click_pending = false;
        state.click_identity = None;
        if cancel_placement {
            state.armed = false;
            keyboard.clear();
        }
        return;
    }
    if state.click_pending {
        if state.click_identity.as_ref() != identity.as_ref() {
            state.click_pending = false;
            state.click_identity = None;
        }
        return;
    }
    if state.owns_mouse || guard.blocked() || context.pointer_over_ui() {
        return;
    }
    let Some((cursor, node)) = node.filter(|(c, _)| c.cursor_over()) else {
        return;
    };
    let Some(normalized) = cursor.normalized else {
        return;
    };
    if state.armed {
        if mouse.just_pressed(MouseButton::Left) {
            state.owns_mouse = true;
            state.click_pending = true;
            state.click_identity = identity;
            state.click_started = time.elapsed_secs_f64();
        }
        return;
    }
    let (Some(origin), Some(camera)) = (state.center, camera) else {
        return;
    };
    if state.path_active()
        && mouse.just_pressed(MouseButton::Left)
        && let Some(index) = super::path::pick(&state.path, camera, normalized)
        && state.path.selected != Some(index)
    {
        state.path.selected = Some(index);
        state.center = Some(Vec3::from_array(state.path.nodes[index]));
        state.path.observed_center = state.center;
        state.owns_mouse = true;
        editor.status = format!(
            "已选中路径节点 {}；拖动XYZ轴或XZ方框修改底部位置",
            index + 1
        );
        return;
    }
    let Some(handle) = camera.pick_handle(origin, normalized, false) else {
        return;
    };
    state.hover = Some(handle);
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(grabbed) = camera
        .ray(normalized)
        .and_then(|r| handle_point(r, origin, handle))
    else {
        return;
    };
    state.owns_mouse = true;
    state.drag = Some(SpatialDrag {
        identity: identity.unwrap(),
        origin,
        grabbed,
        handle,
        camera,
        ui_size: node.size(),
        grid: context
            .preferences
            .as_ref()
            .map_or(1., |p| p.editor_snapping.sanitized().translation_meters),
    });
    editor.status = "拖动笔刷XYZ轴或XZ方框；Ctrl吸附，Esc取消".into();
    keyboard.clear();
}

/// 使用已有真实表面点击的冻结身份，缺页时只等待这一点击，不能借用后来的鼠标落点。
pub(super) fn place(
    mut state: ResMut<SpatialBrushState>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    mut ground: ResMut<VegetationGroundCursor>,
    surface: Option<Res<DynamicClipmapGroundSurface>>,
    context: MoveInputContext,
    time: Res<Time<Real>>,
) {
    state.authoring_available = state.enabled
        && context.sculpt_parameters_available(&editor)
        && !session.saving
        && Identity::capture(&editor, &session).is_some();
    if state.pending.is_some() && !state.authoring_available {
        state.pending = None;
        editor.status = "编辑状态已变化，空间雕刻已取消；请重新确认执行".into();
    }
    if !state.click_pending {
        return;
    }
    if !state.enabled
        || !context.sculpt_available(&editor)
        || session.saving
        || state.click_identity.as_ref() != Identity::capture(&editor, &session).as_ref()
    {
        state.click_pending = false;
        state.click_identity = None;
        return;
    }
    if time.elapsed_secs_f64() - state.click_started > 30. {
        state.click_pending = false;
        state.click_identity = None;
        editor.status = "笔刷定位等待地形超时，请重新点击".into();
        return;
    }
    let Some(snapshot) = surface
        .as_ref()
        .and_then(|s| s.snapshot.as_ref())
        .filter(|s| {
            s.ready
                && session.draft.as_ref().is_some_and(|d| d.map_id == s.map_id)
                && s.authoring_height_revision() == Some(session.terrain_revision)
        })
    else {
        return;
    };
    let result =
        ground.take_volume_click(snapshot, &snapshot.map_id, editor.tool, session.revision);
    match result {
        Some(ClipmapGroundHit::Hit(p)) => {
            state.center = Some(p);
            state.armed = false;
            state.click_pending = false;
            state.click_identity = None;
            editor.status = "已放置空间笔刷；移动手柄或修改XYZ，确认后执行雕刻".into();
        }
        Some(_) => {
            state.click_pending = false;
            state.click_identity = None;
            editor.status = "笔刷定位未命中真实地形，请重新点击地形表面".into();
        }
        None => {}
    }
}

pub(super) fn draw(
    state: Res<SpatialBrushState>,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    viewport: Option<Res<MapEditorViewportState>>,
    nodes: Query<&ComputedNode, With<MapEditorViewport>>,
    mut gizmos: Gizmos<SpatialBrushGizmos>,
    mut frame: ResMut<crate::viewport::tool_preview::AuthorToolFrame>,
) {
    if !state.enabled || Identity::capture(&editor, &session).is_none() {
        return;
    }
    let Some(center) = state.center else {
        return;
    };
    let radius = editor.brush_radius.clamp(0.5, 8.);
    let height = editor.volume_brush_half_height.clamp(0.5, 8.);
    if !radius.is_finite() || !height.is_finite() || !center.is_finite() {
        return;
    }
    let color = Color::srgb(0.18, 0.95, 1.);
    if state.mode == ApplyMode::Path {
        super::path::draw(&state.path, center, &mut gizmos);
    } else {
        frame.push(AuthorTool::Brush(spatial_volume_brush_preview(
            center,
            radius,
            height,
            editor.volume_brush_shape,
            color,
        )));
    }
    if state.mode == ApplyMode::Line {
        if let Some(a) = state.start {
            frame.push(AuthorTool::Brush(spatial_volume_brush_preview(
                a,
                radius,
                height,
                editor.volume_brush_shape,
                Color::srgb(0.35, 1., 0.35),
            )));
        }
        if let Some(b) = state.end {
            frame.push(AuthorTool::Brush(spatial_volume_brush_preview(
                b,
                radius,
                height,
                editor.volume_brush_shape,
                Color::srgb(1., 0.65, 0.2),
            )));
        }
        if let (Some(a), Some(b)) = (state.start, state.end) {
            // 固定八段预览，不随长度/地图规模增长。端点之间确实执行连续刷印。
            gizmos.line(a, b, color);
            for i in 1..8 {
                frame.push(AuthorTool::Brush(spatial_volume_brush_preview(
                    a.lerp(b, i as f32 / 8.),
                    radius,
                    height,
                    editor.volume_brush_shape,
                    Color::srgba(0.18, 0.95, 1., 0.45),
                )));
            }
            let h =
                if editor.volume_brush_shape == feathrise_map_terrain::volume::VolumeBrushShape::Sphere {
                    radius
                } else {
                    height
                };
            for offset in [
                Vec3::X * radius,
                -Vec3::X * radius,
                Vec3::Y * h,
                -Vec3::Y * h,
                Vec3::Z * radius,
                -Vec3::Z * radius,
            ] {
                gizmos.line(a + offset, b + offset, color);
            }
        }
    }
    let Some(camera) = viewport.as_ref().and_then(|v| {
        nodes
            .single()
            .ok()
            .and_then(|n| MoveCamera::new(&editor, v, n.inverse_scale_factor()))
    }) else {
        return;
    };
    let Some(length) = camera.handle_length(center) else {
        return;
    };
    let selected = state.drag.as_ref().map(|d| d.handle).or(state.hover);
    let axes = visible_axes(camera, center, length, 0b111);
    let active_axes = MoveAxis::ALL.into_iter().enumerate().fold(0, |mask, (index, axis)| {
        mask | if selected == Some(MoveHandle::Axis(axis)) { 1 << index } else { 0 }
    }) & axes;
    frame.push(AuthorTool::Transform(TransformPreview::Move {
        spatial: true, origin: center.to_array(), length, axes, active_axes,
        plane: camera.plane_quad(center, length).is_some(),
        plane_active: selected == Some(MoveHandle::XzPlane), plane_cross: false,
        plane_color: color.to_linear().to_f32_array(),
        color_active: Color::srgb(1., 0.8, 0.2).to_linear().to_f32_array(),
        axis_colors: axis_colors(), anchor: None, marker_radius: None,
    }));
}
