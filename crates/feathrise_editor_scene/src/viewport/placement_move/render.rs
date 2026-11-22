//! 场景手柄几何与拾取共用同一实际相机；工程视口沿用已有语义预览。

use super::super::modes::{ViewportMode, ViewportModeState};
use super::super::LayerCandidateState;
use super::{
    group_types::GroupPhase,
    input::{placement_scale_basis, selected_record, yaw_only},
    math::{MoveCamera, move_plane_corners},
    resources::PlacementMoveState,
    rotation::rotation_ring,
    scale::{ScaleGizmo, UNIFORM_SCALE_RADIUS, frame_scale_basis},
    types::{MoveAxis, MoveHandle, MovePlane},
};
use crate::authoring::EditorAuthoringSession;
use crate::components::SceneEditorViewport;
use crate::resources::{SceneEditorState, SceneEditorViewportState};
use crate::types::EditorTool;
use crate::ui::{
    gltf_browser::EditorGltfBrowserState,
    placement_transform::PlacementTransformInputState,
    scene_motion::Workbench,
    vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
};
use bevy::{prelude::*, ui::RelativeCursorPosition};
use feathrise_author_tools::{AuthorTool, TransformPreview};
use crate::viewport::tool_preview::{AuthorToolFrame, foreground::{Canvas, Foreground}};
use crate::standalone::embedded_snapshot::EmbeddedSnapshotState;
use feathrise_editor_animation_model::gizmo::{MoveConstraint, rotation_rings};

pub(crate) fn draw_placement_move(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    state: Res<PlacementMoveState>,
    mode: Res<ViewportModeState>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<(&ComputedNode, &RelativeCursorPosition), With<SceneEditorViewport>>,
    browser: Res<EditorGltfBrowserState>,
    candidates: Res<LayerCandidateState>,
    workspace: Res<VegetationPresetEditorState>,
    transform: Res<PlacementTransformInputState>,
    mut tools: ResMut<AuthorToolFrame>,
    multi_selection: Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>,
    animation: Option<Res<Workbench>>,
    snapshot: Option<Res<EmbeddedSnapshotState>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut foreground: ResMut<Foreground>,
) {
    if !editor.open
        || !editor.placement_transform_enabled
        || editor.tool != EditorTool::Inspect
        || editor.scene_transition_in_progress
        || editor.new_scene_dialog_open
        || workspace.active_tab != PresetWorkspaceTab::Scene
        || transform.relocation_armed()
    {
        return;
    }
    let (Some(viewport), Ok((node, relative))) = (viewport, nodes.single()) else { return; };
    let Some(camera) = MoveCamera::new(&editor, &viewport, node.inverse_scale_factor()) else { return; };
    let local = !snapshot.is_some_and(|snapshot| snapshot.enabled);
    let held = mouse.pressed(MouseButton::Left);
    let size = camera.size * camera.inverse_scale;
    let pointer = relative.normalized.map(|point| point * size);
    let mut canvas = foreground.canvas(size);
    if let Some(selection) = multi_selection.as_ref().filter(|selection| selection.members.len() > 1) {
        if selection.ready_for(&session) || state.group.is_some() {
            PlacementGizmo {
                camera, origin: selection.center + state.group_delta(), basis: Mat3::IDENTITY,
                active: state.group.as_ref().map(|group| group.handle).or(state.hover),
                guide: state.group.as_ref().filter(|group| held && group.phase == GroupPhase::Dragging).map(|group| group.handle),
                building: false, local, animated: false,
                yaw_only: selection.terrain_patch_selected || !crate::authoring::placement_pose::placement_supports_full_rotation(),
                yaw: state.group.as_ref().map_or(0.0, |group| group.rotation.to_euler(EulerRot::YXZ).0.to_degrees()),
                before_yaw: None, anchor: None,
            }.draw(mode.mode, pointer, &mut tools, &mut canvas);
        }
        return; // 多选始终绘制共同中心手柄，不借用单体选择。
    }
    if crate::ui::scene_source_glb::owns_selection(&browser) {
        if local && mode.mode.accepts_transform(ViewportMode::Move)
            && let Some(display) = state.source_display.as_ref().filter(|display| display.current(&browser, session.revision))
        {
            PlacementGizmo {
                camera, origin: display.origin, basis: Mat3::IDENTITY, building: false, local,
                animated: false, yaw_only: false, active: state.hover,
                guide: state.source_drag.as_ref().filter(|_| held).map(|drag| drag.handle),
                yaw: 0.0, before_yaw: None, anchor: None,
            }.draw(ViewportMode::Move, pointer, &mut tools, &mut canvas);
        }
        return;
    }
    let Some((_, record)) = selected_record(&editor, &session, &browser, &candidates, &viewport)
    else {
        return;
    };
    let preview = state
        .drag
        .as_ref()
        .or_else(|| state.pending_building.as_ref().map(|pending| &pending.drag))
        .filter(|drag| drag.before.id == record.id && drag.author_revision == session.revision);
    let Some(draft) = session.draft.as_ref() else { return; };
    let animated = animation.as_ref().filter(|workbench| workbench.owns(&draft.scene_id, &record.id));
    let (origin, basis) = if let Some(workbench) = animated {
        if !workbench.can_pose(&draft.scene_id, &record.id) { return; }
        let Some(pose) = state.animation_pose(&draft.scene_id, &record.id)
            .or_else(|| workbench.pose_for(record, &draft.scene_id)) else { return; };
        (Vec3::from_array(pose.translation), frame_scale_basis(pose.rotation_degrees))
    } else {
        let Ok(frame) = crate::authoring::placement_pose::placement_record_frame(draft, record) else { return; };
        let origin = preview.map_or_else(|| frame.as_transform().transform_point(record.translation.into()),
            |drag| drag.preview_world_position());
        (origin, placement_scale_basis(frame, preview.map_or(record, |drag| &drag.preview)))
    };
    let building = record.building_preset_id.is_some();
    PlacementGizmo {
        camera, origin, basis, building, local, animated: animated.is_some(),
        yaw_only: animated.is_none() && yaw_only(record),
        active: preview.map(|drag| drag.handle).or(state.hover),
        guide: preview.filter(|_| held && state.drag.is_some()).map(|drag| drag.handle),
        yaw: preview.map_or(record.yaw_degrees, |drag| drag.preview.yaw_degrees),
        before_yaw: preview.map(|drag| drag.before.yaw_degrees),
        anchor: (building && preview.is_some()).then_some(record.translation),
    }.draw(mode.mode, pointer, &mut tools, &mut canvas);
}

struct PlacementGizmo {
    camera: MoveCamera,
    origin: Vec3,
    basis: Mat3,
    building: bool,
    local: bool,
    animated: bool,
    yaw_only: bool,
    active: Option<MoveHandle>,
    guide: Option<MoveHandle>,
    yaw: f32,
    before_yaw: Option<f32>,
    anchor: Option<[f32; 3]>,
}

const COPPER: Color = Color::srgba(0.745, 0.529, 0.243, 0.8);

impl PlacementGizmo {
    fn tint(&self, handle: MoveHandle, normal: Color) -> Color {
        handle_color(normal, self.active == Some(handle))
    }

    fn draw(self, mode: ViewportMode, pointer: Option<Vec2>, tools: &mut AuthorToolFrame, canvas: &mut Canvas) {
        if self.local && self.guide == Some(MoveHandle::UniformScale) {
            // 等比拖动期间凸显中心和径向反馈，松开后恢复可选择的轴、平面和外环。
            self.publish_scale_center(tools);
            if let Some((center, pointer)) = canvas.point(self.camera, self.origin).zip(pointer) {
                canvas.radial_scale_guide(center, pointer, UNIFORM_SCALE_RADIUS);
            }
            return;
        }
        if self.local && let Some(handle) = self.guide && let Some(length) = self.camera.handle_length(self.origin) {
            // 单轴、平面与局部缩放按已捕获的约束绘制。自由旋转和等比缩放没有轴线。
            for axis in MoveAxis::ALL {
                let direction = axis.direction();
                let direction = if handle.scales_axis(axis) {
                    self.basis * direction
                } else if handle.moves_axis(axis)
                    || handle == MoveHandle::RotationY && axis == MoveAxis::Y
                    || matches!(handle, MoveHandle::AnimationRotation(constraint) if constraint.axis() == Some(direction))
                {
                    direction
                } else { continue; };
                // 按参考图调成同一轴色的浅色贯穿线，长线经过模型表面时仍易识别。
                let tint = transform_axis_color(axis, true).mix(&Color::srgb_u8(198, 198, 198), 0.5);
                canvas.guide(self.camera, self.origin, direction, length, tint);
            }
        }
        if mode.accepts_transform(ViewportMode::Move) { self.draw_move(tools, canvas); }
        if mode.accepts_transform(ViewportMode::Rotate) { self.draw_rotate(tools, canvas); }
        if mode.accepts_transform(ViewportMode::Scale) && !self.building {
            self.draw_scale(mode == ViewportMode::Transform, tools, canvas);
        }
    }

    fn draw_move(&self, tools: &mut AuthorToolFrame, canvas: &mut Canvas) {
        let Some(length) = self.camera.handle_length(self.origin) else { return; };
        let axes = visible_axes(self.camera, self.origin, length, if self.building { 0b101 } else { 0b111 });
        let active_axes = match self.active {
            Some(MoveHandle::Axis(axis)) => axes & (1 << axis.index()),
            _ => 0,
        };
        let plane = self.camera.plane_quad(self.origin, length).is_some();
        tools.push(AuthorTool::Transform(TransformPreview::Move {
            spatial: false, origin: self.origin.to_array(), length, axes, active_axes, plane,
            plane_active: plane && self.active == Some(MoveHandle::XzPlane), plane_cross: false,
            plane_color: transform_axis_color(MoveAxis::Y, false).to_linear().to_f32_array(),
            color_active: active_move_color(self.active),
            axis_colors: axis_colors(), anchor: self.anchor, marker_radius: self.anchor.map(|_| length * 0.15),
        }));
        if self.local && !self.building {
            for plane in [MovePlane::Xy, MovePlane::Yz] {
                let corners = move_plane_corners(self.origin, length, plane);
                if let Some(points) = self.camera.project_quad(corners) {
                    canvas.plane(points, transform_axis_color(plane.normal(), self.active == Some(plane.move_handle())));
                }
            }
        }
    }

    fn draw_rotate(&self, tools: &mut AuthorToolFrame, canvas: &mut Canvas) {
        if !self.local || self.yaw_only {
            if !self.animated && let Some(ring) = rotation_ring(self.camera, self.origin) {
                tools.push(AuthorTool::Transform(TransformPreview::Rotate {
                    origin: self.origin.to_array(), radius: ring.world[0].distance(self.origin),
                    yaw_degrees: Some(self.yaw), before_yaw_degrees: self.before_yaw,
                    color: transform_axis_color(MoveAxis::Y, self.active == Some(MoveHandle::RotationY)).to_linear().to_f32_array(),
                }));
            }
            return;
        }
        for ring in rotation_rings(*self.camera, self.origin).into_iter().flatten() {
            let active = self.active == Some(MoveHandle::AnimationRotation(ring.constraint));
            let color = match ring.constraint {
                MoveConstraint::X => transform_axis_color(MoveAxis::X, active),
                MoveConstraint::Y => transform_axis_color(MoveAxis::Y, active),
                MoveConstraint::Z => transform_axis_color(MoveAxis::Z, active),
                MoveConstraint::View => handle_color(Color::WHITE, active),
            };
            for segment in ring.world.windows(2) {
                canvas.world_line(self.camera, segment[0], segment[1], 3., color);
            }
        }
    }

    fn publish_scale_center(&self, tools: &mut AuthorToolFrame) {
        if let Some(corners) = self.camera.screen_square(self.origin, UNIFORM_SCALE_RADIUS)
            && let Some(preview) = screen_scale_preview(self.origin, corners, self.tint(MoveHandle::UniformScale, Color::WHITE))
        {
            tools.push(AuthorTool::Transform(preview));
        }
    }

    fn draw_scale(&self, combined: bool, tools: &mut AuthorToolFrame, canvas: &mut Canvas) {
        self.publish_scale_center(tools);
        if !self.local { return; }
        let Some(geometry) = ScaleGizmo::new(self.camera, self.origin, self.basis, combined) else { return; };
        if let Some(ring) = geometry.outer {
            let color = self.tint(MoveHandle::UniformScale, Color::WHITE);
            for segment in ring.windows(2) { canvas.world_line(self.camera, segment[0], segment[1], 2., color); }
        }
        for (axis, shape) in MoveAxis::ALL.into_iter().zip(geometry.axes) {
            let Some(shape) = shape else { continue; };
            let color = transform_axis_color(axis, self.active == Some(MoveHandle::AxisScale(axis)));
            if !combined { canvas.world_line(self.camera, self.origin, shape.tip, 2., color); }
            if let Some(corners) = canvas.quad_points(self.camera, shape.cap) { canvas.quad(corners, color); }
        }
        for (plane, shape) in MovePlane::ALL.into_iter().zip(geometry.planes) {
            if let Some(points) = shape.and_then(|corners| self.camera.project_quad(corners)) {
                canvas.plane(points, transform_axis_color(plane.normal(), self.active == Some(MoveHandle::PlaneScale(plane))));
            }
        }
    }
}

/// 发送前保留原相机的逐轴退化门；共享绘制端不能把不可拾取的轴重新画出来。
pub(crate) fn visible_axes(camera: MoveCamera, origin: Vec3, length: f32, allowed: u8) -> u8 {
    MoveAxis::ALL.into_iter().fold(0, |mask, axis| {
        let bit = 1 << axis.index();
        mask | if allowed & bit != 0 && camera.axis_segment(origin, axis, length).is_some() { bit } else { 0 }
    })
}

pub(crate) fn axis_colors() -> [[f32; 4]; 3] {
    MoveAxis::ALL.map(|axis| transform_axis_color(axis, false).to_linear().to_f32_array())
}

pub(crate) fn transform_axis_color(axis: MoveAxis, active: bool) -> Color {
    // Blender 默认轴色。悬停和捕获只提高不透明度，轴与平面保持其方向色。
    let color = match axis {
        MoveAxis::X => Color::srgb_u8(255, 51, 82),
        MoveAxis::Y => Color::srgb_u8(139, 220, 0),
        MoveAxis::Z => Color::srgb_u8(40, 144, 255),
    };
    handle_color(color, active)
}

pub(crate) fn handle_color(color: Color, active: bool) -> Color {
    color.with_alpha(if active { 1. } else { 0.6 })
}

pub(crate) fn active_move_color(handle: Option<MoveHandle>) -> [f32; 4] {
    let tint = match handle {
        Some(MoveHandle::Axis(axis)) => transform_axis_color(axis, true),
        Some(MoveHandle::XzPlane) => transform_axis_color(MoveAxis::Y, true),
        _ => COPPER,
    };
    tint.to_linear().to_f32_array()
}

/// 继续使用原screen_square的实际四角，只把显示转换为正交基与世界半边长。
pub(super) fn screen_scale_preview(origin: Vec3, corners: [Vec3; 4], color: Color) -> Option<TransformPreview> {
    let right = (corners[1] - corners[0]) * 0.5;
    let up = (corners[3] - corners[0]) * 0.5;
    Some(TransformPreview::Scale {
        origin: origin.to_array(), right: right.try_normalize()?.to_array(), up: up.try_normalize()?.to_array(),
        half_size: right.length(), color: color.to_linear().to_f32_array(), cube: false,
    })
}
