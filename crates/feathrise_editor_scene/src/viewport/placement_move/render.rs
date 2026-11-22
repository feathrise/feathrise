//! 场景手柄几何与拾取共用同一实际相机；工程视口沿用已有语义预览。

use super::super::modes::{ViewportMode, ViewportModeState};
use super::super::LayerCandidateState;
use super::{
    constants::PICK_LOGICAL_PIXELS,
    input::{placement_scale_basis, selected_record, yaw_only},
    math::{MoveCamera, move_plane_corners},
    resources::PlacementMoveState,
    rotation::rotation_ring,
    scale::{ScaleGizmo, frame_scale_basis},
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
use bevy::prelude::*;
use feathrise_author_tools::{AuthorTool, TransformPreview};
use crate::viewport::{SceneEditorBrushGizmos, tool_preview::AuthorToolFrame};
use crate::standalone::embedded_snapshot::EmbeddedSnapshotState;
use feathrise_editor_animation_model::gizmo::{MoveConstraint, rotation_rings};

pub(crate) fn draw_placement_move(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    state: Res<PlacementMoveState>,
    mode: Res<ViewportModeState>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<&ComputedNode, With<SceneEditorViewport>>,
    browser: Res<EditorGltfBrowserState>,
    candidates: Res<LayerCandidateState>,
    workspace: Res<VegetationPresetEditorState>,
    transform: Res<PlacementTransformInputState>,
    mut tools: ResMut<AuthorToolFrame>,
    multi_selection: Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>,
    animation: Option<Res<Workbench>>,
    snapshot: Option<Res<EmbeddedSnapshotState>>,
    mut gizmos: Gizmos<SceneEditorBrushGizmos>,
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
    let (Some(viewport), Ok(node)) = (viewport, nodes.single()) else { return; };
    let Some(camera) = MoveCamera::new(&editor, &viewport, node.inverse_scale_factor()) else { return; };
    let local = !snapshot.is_some_and(|snapshot| snapshot.enabled);
    if let Some(selection) = multi_selection.as_ref().filter(|selection| selection.members.len() > 1) {
        if selection.ready_for(&session) || state.group.is_some() {
            PlacementGizmo {
                camera, origin: selection.center + state.group_delta(), basis: Mat3::IDENTITY,
                active: state.group.as_ref().map(|group| group.handle).or(state.hover),
                building: false, local, animated: false,
                yaw_only: selection.terrain_patch_selected || !crate::authoring::placement_pose::placement_supports_full_rotation(),
                yaw: state.group.as_ref().map_or(0.0, |group| group.rotation.to_euler(EulerRot::YXZ).0.to_degrees()),
                before_yaw: None, anchor: None,
            }.draw(mode.mode, &mut tools, &mut gizmos);
        }
        return; // 多选始终绘制共同中心手柄，不借用单体选择。
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
        yaw: preview.map_or(record.yaw_degrees, |drag| drag.preview.yaw_degrees),
        before_yaw: preview.map(|drag| drag.before.yaw_degrees),
        anchor: (building && preview.is_some()).then_some(record.translation),
    }.draw(mode.mode, &mut tools, &mut gizmos);
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
    yaw: f32,
    before_yaw: Option<f32>,
    anchor: Option<[f32; 3]>,
}

const HIGHLIGHT: Color = Color::srgb(0.92, 0.72, 0.40);
const COPPER: Color = Color::srgba(0.745, 0.529, 0.243, 0.8);

impl PlacementGizmo {
    fn tint(&self, handle: MoveHandle, normal: Color) -> Color {
        if self.active == Some(handle) { HIGHLIGHT } else { normal }
    }

    fn draw(self, mode: ViewportMode, tools: &mut AuthorToolFrame, gizmos: &mut Gizmos<SceneEditorBrushGizmos>) {
        if mode.accepts_transform(ViewportMode::Move) { self.draw_move(tools, gizmos); }
        if mode.accepts_transform(ViewportMode::Rotate) { self.draw_rotate(tools, gizmos); }
        if mode.accepts_transform(ViewportMode::Scale) && !self.building {
            self.draw_scale(mode == ViewportMode::Transform, tools, gizmos);
        }
    }

    fn draw_move(&self, tools: &mut AuthorToolFrame, gizmos: &mut Gizmos<SceneEditorBrushGizmos>) {
        let Some(length) = self.camera.handle_length(self.origin) else { return; };
        let axes = visible_axes(self.camera, self.origin, length, if self.building { 0b101 } else { 0b111 });
        let active_axes = match self.active {
            Some(MoveHandle::Axis(axis)) => axes & (1 << axis.index()),
            _ => 0,
        };
        let plane = self.camera.plane_quad(self.origin, length).is_some();
        tools.push(AuthorTool::Transform(TransformPreview::Move {
            spatial: false, origin: self.origin.to_array(), length, axes, active_axes, plane,
            plane_active: plane && self.active == Some(MoveHandle::XzPlane), plane_cross: true,
            plane_color: COPPER.to_linear().to_f32_array(), color_active: HIGHLIGHT.to_linear().to_f32_array(),
            axis_colors: axis_colors(), anchor: self.anchor, marker_radius: self.anchor.map(|_| length * 0.15),
        }));
        if self.local && !self.building {
            for plane in [MovePlane::Xy, MovePlane::Yz] {
                let corners = move_plane_corners(self.origin, length, plane);
                if self.camera.project_quad(corners).is_some() {
                    draw_quad(gizmos, corners, self.tint(plane.move_handle(), plane.normal().color()));
                }
            }
        }
    }

    fn draw_rotate(&self, tools: &mut AuthorToolFrame, gizmos: &mut Gizmos<SceneEditorBrushGizmos>) {
        if !self.local || self.yaw_only {
            if !self.animated && let Some(ring) = rotation_ring(self.camera, self.origin) {
                tools.push(AuthorTool::Transform(TransformPreview::Rotate {
                    origin: self.origin.to_array(), radius: ring.world[0].distance(self.origin),
                    yaw_degrees: Some(self.yaw), before_yaw_degrees: self.before_yaw,
                    color: self.tint(MoveHandle::RotationY, MoveAxis::Y.color()).to_linear().to_f32_array(),
                }));
            }
            return;
        }
        for ring in rotation_rings(*self.camera, self.origin).into_iter().flatten() {
            let color = match ring.constraint {
                MoveConstraint::X => MoveAxis::X.color(),
                MoveConstraint::Y => MoveAxis::Y.color(),
                MoveConstraint::Z => MoveAxis::Z.color(),
                MoveConstraint::View => Color::srgb(0.82, 0.84, 0.87),
            };
            let color = self.tint(MoveHandle::AnimationRotation(ring.constraint), color);
            for segment in ring.world.windows(2) {
                gizmos.line(segment[0], segment[1], color);
            }
        }
    }

    fn draw_scale(&self, combined: bool, tools: &mut AuthorToolFrame, gizmos: &mut Gizmos<SceneEditorBrushGizmos>) {
        if let Some(corners) = self.camera.screen_square(self.origin, PICK_LOGICAL_PIXELS)
            && let Some(preview) = screen_scale_preview(self.origin, corners, self.tint(MoveHandle::UniformScale, COPPER))
        {
            tools.push(AuthorTool::Transform(preview));
        }
        if !self.local { return; }
        let Some(geometry) = ScaleGizmo::new(self.camera, self.origin, self.basis, combined) else { return; };
        if let Some(ring) = geometry.outer {
            let color = self.tint(MoveHandle::UniformScale, Color::srgb(0.82, 0.84, 0.87));
            for segment in ring.windows(2) { gizmos.line(segment[0], segment[1], color); }
        }
        for (axis, shape) in MoveAxis::ALL.into_iter().zip(geometry.axes) {
            let Some(shape) = shape else { continue; };
            let color = self.tint(MoveHandle::AxisScale(axis), axis.color());
            if !combined { gizmos.line(self.origin, shape.tip, color); }
            draw_quad(gizmos, shape.cap, color);
        }
        for (plane, shape) in MovePlane::ALL.into_iter().zip(geometry.planes) {
            if let Some(shape) = shape {
                draw_quad(gizmos, shape, self.tint(MoveHandle::PlaneScale(plane), plane.normal().color()));
            }
        }
    }
}

fn draw_quad(gizmos: &mut Gizmos<SceneEditorBrushGizmos>, points: [Vec3; 4], color: Color) {
    for index in 0..4 { gizmos.line(points[index], points[(index + 1) % 4], color); }
}

/// 发送前保留原相机的逐轴退化门；共享绘制端不能把不可拾取的轴重新画出来。
pub(crate) fn visible_axes(camera: MoveCamera, origin: Vec3, length: f32, allowed: u8) -> u8 {
    MoveAxis::ALL.into_iter().fold(0, |mask, axis| {
        let bit = 1 << axis.index();
        mask | if allowed & bit != 0 && camera.axis_segment(origin, axis, length).is_some() { bit } else { 0 }
    })
}

pub(crate) fn axis_colors() -> [[f32; 4]; 3] {
    MoveAxis::ALL.map(|axis| axis.color().to_linear().to_f32_array())
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
