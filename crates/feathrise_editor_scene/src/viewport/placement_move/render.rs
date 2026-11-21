//! 只发布原工具确认的移动、旋转与缩放语义；两种视口共用SDK几何，不复制输入或事务。

use super::super::modes::{ViewportMode, ViewportModeState};
use super::super::LayerCandidateState;
use super::{
    constants::PICK_LOGICAL_PIXELS,
    input::selected_record,
    math::MoveCamera,
    resources::PlacementMoveState,
    rotation::rotation_ring,
    types::{MoveAxis, MoveHandle},
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
use crate::viewport::tool_preview::AuthorToolFrame;

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
    let (Some(viewport), Ok(node)) = (viewport, nodes.single()) else {
        return;
    };
    if let Some(selection) = multi_selection.as_ref().filter(|selection| selection.members.len() > 1) {
        if mode.mode.transforms_placement() && (selection.ready_for(&session) || state.group.is_some())
            && let Some(camera) = MoveCamera::new(&editor, &viewport, node.inverse_scale_factor()) {
            let origin = selection.center + state.group_delta();
            let color = Color::srgb(0.92, 0.72, 0.40);
            let active = |handle| state.group.as_ref().map(|group| group.handle).or(state.hover) == Some(handle);
            if mode.mode == ViewportMode::Rotate {
                if let Some(ring) = rotation_ring(camera, origin) {
                    let angle = state.group.as_ref().map_or(0.0, |group| group.rotation_degrees);
                    let radius = ring.world[0].distance(origin);
                    tools.push(AuthorTool::Transform(TransformPreview::Rotate {
                        origin: origin.to_array(), radius, yaw_degrees: Some(angle),
                        before_yaw_degrees: None,
                        color: if active(MoveHandle::RotationY) { color } else { MoveAxis::Y.color() }.to_linear().to_f32_array(),
                    }));
                }
                return;
            }
            if mode.mode == ViewportMode::Scale {
                if let Some(corners) = camera.screen_square(origin, PICK_LOGICAL_PIXELS) {
                    let color = if active(MoveHandle::UniformScale) { color } else { Color::srgba(0.745, 0.529, 0.243, 0.8) };
                    if let Some(preview) = screen_scale_preview(origin, corners, color) {
                        tools.push(AuthorTool::Transform(preview));
                    }
                }
                return;
            }
            if let Some(length) = camera.handle_length(origin) {
                let axes = visible_axes(camera, origin, length, 0b111);
                let active_axes = MoveAxis::ALL.into_iter().fold(0, |mask, axis| {
                    let handle = MoveHandle::Axis(axis);
                    mask | if state.group.as_ref().is_some_and(|group| group.handle == handle)
                        || state.hover == Some(handle) { 1 << axis.index() } else { 0 }
                }) & axes;
                tools.push(AuthorTool::Transform(TransformPreview::Move {
                    spatial: false,
                    origin: origin.to_array(), length, axes, active_axes,
                    plane: camera.plane_quad(origin, length).is_some(), plane_active: active(MoveHandle::XzPlane), plane_cross: true,
                    plane_color: Color::srgba(0.745, 0.529, 0.243, 0.8).to_linear().to_f32_array(),
                    color_active: color.to_linear().to_f32_array(), axis_colors: axis_colors(),
                    anchor: None, marker_radius: None,
                }));
            }
        }
        return; // 多选始终绘制共同中心手柄，不借用单体选择。
    }
    let Some((_, record)) = selected_record(&editor, &session, &browser, &candidates, &viewport)
    else {
        return;
    };
    let Some(camera) = MoveCamera::new(&editor, &viewport, node.inverse_scale_factor()) else {
        return;
    };
    let preview = state
        .drag
        .as_ref()
        .or_else(|| state.pending_building.as_ref().map(|pending| &pending.drag))
        .filter(|drag| drag.before.id == record.id && drag.author_revision == session.revision);
    let animated = session.draft.as_ref().and_then(|draft| animation.as_ref()
        .filter(|workbench| workbench.owns(&draft.scene_id, &record.id))
        .map(|workbench| (&draft.scene_id, workbench)));
    let origin = if let Some((scene, workbench)) = animated {
        if mode.mode != ViewportMode::Move || !workbench.can_pose(scene, &record.id) { return; }
        let Some(pose) = state.animation_pose(scene, &record.id).or_else(|| workbench.pose_for(record, scene)) else { return; };
        Vec3::from_array(pose.translation)
    } else if let Some(drag) = preview { drag.preview_world_position() }
        else if let Some(origin) = session.draft.as_ref().and_then(|draft|
            crate::authoring::placement_pose::placement_record_world_position(draft, record).ok()) { origin }
        else { return; };
    let Some(length) = camera.handle_length(origin) else {
        return;
    };
    let building = record.building_preset_id.is_some();
    if mode.mode == ViewportMode::Rotate {
        if let Some(ring) = rotation_ring(camera, origin) {
            let active = preview.is_some_and(|drag| drag.handle == MoveHandle::RotationY)
                || state.hover == Some(MoveHandle::RotationY);
            let color = if active {
                Color::srgb(0.92, 0.72, 0.40)
            } else {
                MoveAxis::Y.color()
            };
            let yaw = preview.map_or(record.yaw_degrees, |drag| drag.preview.yaw_degrees);
            let radius = ring.world[0].distance(origin);
            tools.push(AuthorTool::Transform(TransformPreview::Rotate {
                origin: origin.to_array(), radius, yaw_degrees: Some(yaw),
                before_yaw_degrees: preview.map(|drag| drag.before.yaw_degrees),
                color: color.to_linear().to_f32_array(),
            }));
        }
        return; // 圆环和指示线共用冻结根与实际相机；端视退化门与拾取完全相同。
    }
    if mode.mode == ViewportMode::Scale {
        if !building && let Some(corners) = camera.screen_square(origin, PICK_LOGICAL_PIXELS) {
            let active = state
                .drag
                .as_ref()
                .is_some_and(|drag| drag.handle == MoveHandle::UniformScale)
                || state.hover == Some(MoveHandle::UniformScale);
            let color = if active {
                Color::srgb(0.92, 0.72, 0.40)
            } else {
                Color::srgba(0.745, 0.529, 0.243, 0.80)
            };
            if let Some(preview) = screen_scale_preview(origin, corners, color) {
                tools.push(AuthorTool::Transform(preview));
            }
        }
        return; // 建筑不提供缩放入口；普通模型倍率变化不改变中心方块的屏幕尺寸。
    }
    if mode.mode != ViewportMode::Move {
        return;
    }
    let axes = visible_axes(camera, origin, length, if building { 0b101 } else { 0b111 });
    let active = |handle| state.drag.as_ref().is_some_and(|drag| drag.handle == handle)
        || state.hover == Some(handle);
    let active_axes = MoveAxis::ALL.into_iter().fold(0, |mask, axis| {
        let handle = MoveHandle::Axis(axis);
        mask | if active(handle) { 1 << axis.index() } else { 0 }
    }) & axes;
    let plane = camera.plane_quad(origin, length).is_some();
    tools.push(AuthorTool::Transform(TransformPreview::Move {
        spatial: false,
        origin: origin.to_array(), length, axes, active_axes, plane,
        plane_active: plane && active(MoveHandle::XzPlane), plane_cross: true,
        plane_color: Color::srgba(0.745, 0.529, 0.243, 0.8).to_linear().to_f32_array(),
        color_active: Color::srgb(0.92, 0.72, 0.40).to_linear().to_f32_array(),
        axis_colors: axis_colors(), anchor: (building && preview.is_some()).then_some(record.translation),
        marker_radius: (building && preview.is_some()).then_some(length * 0.15),
    })); // 候选位移线与16段落点圈仍跟随原校验预览，不提前改作者记录。
}

/// 动画姿势支持完整世界轴，沿用网格和骨骼的圆环几何与同一套拾取坐标。
pub(crate) fn draw_animation_rotation(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    state: Res<PlacementMoveState>,
    mode: Res<ViewportModeState>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<&ComputedNode, With<SceneEditorViewport>>,
    browser: Res<EditorGltfBrowserState>,
    candidates: Res<LayerCandidateState>,
    animation: Res<Workbench>,
    mut gizmos: Gizmos<crate::viewport::SceneEditorBrushGizmos>,
) {
    use feathrise_editor_animation_model::gizmo::{MoveConstraint, rotation_rings};

    if !editor.open || !editor.placement_transform_enabled || editor.scene_transition_in_progress
        || mode.mode != ViewportMode::Rotate || editor.tool != EditorTool::Inspect
    {
        return;
    }
    let Some(draft) = session.draft.as_ref() else { return; };
    let Some(viewport) = viewport else { return; };
    let Some((_, record)) = selected_record(&editor, &session, &browser, &candidates, &viewport) else { return; };
    if !animation.can_pose(&draft.scene_id, &record.id) { return; }
    let Some(pose) = state.animation_pose(&draft.scene_id, &record.id)
        .or_else(|| animation.pose_for(record, &draft.scene_id)) else { return; };
    let Some(camera) = nodes.single().ok().and_then(|node| MoveCamera::new(&editor, &viewport, node.inverse_scale_factor()))
    else { return; };
    let active = state.drag.as_ref().map(|drag| drag.handle).or(state.hover);
    for ring in rotation_rings(*camera, Vec3::from_array(pose.translation)).into_iter().flatten() {
        let color = if active == Some(MoveHandle::AnimationRotation(ring.constraint)) {
            Color::srgb(0.92, 0.72, 0.40)
        } else {
            match ring.constraint {
                MoveConstraint::X => MoveAxis::X.color(),
                MoveConstraint::Y => MoveAxis::Y.color(),
                MoveConstraint::Z => MoveAxis::Z.color(),
                MoveConstraint::View => Color::srgb(0.82, 0.84, 0.87),
            }
        };
        for segment in ring.world.windows(2) {
            gizmos.line(segment[0], segment[1], color);
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
