//! 复用视口的轴和旋转环操作场景动画当前帧，释放时仅采用临时姿势。

use bevy::prelude::*;
use feathrise_editor_animation_model::gizmo::{MeshEditTool, rotation_delta, pick_transform_handle};
use feathrise_scene_content::SceneId;

use crate::ui::scene_motion::{SceneAnimationFramePose, Workbench};

use super::{math::MoveCamera, types::MoveHandle};

pub(super) struct AnimationDrag {
    time: f64,
    before: SceneAnimationFramePose,
    pub(super) pose: SceneAnimationFramePose,
    raw_angle: f32,
    angle: f32,
}

impl AnimationDrag {
    pub(super) fn new(workbench: &Workbench, pose: SceneAnimationFramePose) -> Self {
        Self { time: workbench.frame_time(), before: pose, pose, raw_angle: 0.0, angle: 0.0 }
    }

    pub(super) fn current(&self, workbench: &Workbench, scene: &SceneId, id: &str) -> bool {
        workbench.frame_time() == self.time && workbench.can_pose(scene, id)
    }

    pub(super) fn translate(&mut self, position: Vec3) {
        self.pose.translation = position.to_array();
    }

    pub(super) fn rotate(
        &mut self, handle: MoveHandle, camera: MoveCamera, origin: Vec3,
        start: Vec2, cursor: Vec2, moved: bool, step_degrees: f32,
    ) -> Result<(), String> {
        let MoveHandle::AnimationRotation(constraint) = handle else { return Ok(()); };
        if !moved && self.angle == 0.0 {
            return Ok(());
        }
        let (axis, raw_angle) = rotation_delta(*camera, start, cursor, origin, constraint)?;
        // 圆环数学返回有向主角。同一笔绕过半圈时累积相邻角差，关键帧可以表达整圈。
        self.angle += angle_difference(raw_angle - self.raw_angle);
        self.raw_angle = raw_angle;
        let step = step_degrees.to_radians();
        let angle = if step > 0.0 { (self.angle / step).round() * step } else { self.angle };
        if angle.abs() <= f32::EPSILON {
            self.pose.rotation_degrees = self.before.rotation_degrees;
            return Ok(());
        }
        let before = Vec3::from_array(self.before.rotation_degrees.map(f32::to_radians));
        let rotated = rotate_euler(before, axis, angle);
        if !rotated.is_finite() {
            return Err("场景动画的旋转不是有限数值".into());
        }
        self.pose.rotation_degrees = rotated.to_array().map(f32::to_degrees);
        Ok(())
    }

    pub(super) fn publish(self, workbench: &mut Workbench, scene: &SceneId, id: &str) -> bool {
        if self.pose.translation != self.before.translation {
            workbench.set_position(scene, id, self.pose.translation)
        } else if self.pose.rotation_degrees != self.before.rotation_degrees {
            workbench.set_rotation(scene, id, self.pose.rotation_degrees)
        } else {
            false
        }
    }
}

pub(super) fn pick_rotation(camera: MoveCamera, origin: Vec3, cursor: Vec2) -> Option<MoveHandle> {
    pick_transform_handle(MeshEditTool::Rotate, *camera, origin, cursor).map(MoveHandle::AnimationRotation)
}

fn angle_difference(value: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (value + PI).rem_euclid(TAU) - PI
}

fn rotate_euler(before: Vec3, axis: Vec3, angle: f32) -> Vec3 {
    if angle == 0.0 {
        return before;
    }
    // 单轴曲线直接保留圈数。组合姿势按实际世界轴旋转并选取离原欧拉值近的等价分支。
    if axis.abs().max_element() == 1.0 && before.cross(axis) == Vec3::ZERO {
        return before + axis * angle;
    }
    let result = Quat::from_axis_angle(axis, angle) * Quat::from_euler(EulerRot::XYZ, before.x, before.y, before.z);
    let (x, y, z) = result.to_euler(EulerRot::XYZ);
    let pi = std::f32::consts::PI;
    let nearest = |angles: Vec3| before + (angles - before).map(angle_difference);
    let regular = nearest(Vec3::new(x, y, z));
    let other = nearest(Vec3::new(x + pi, pi - y, z + pi));
    if regular.distance_squared(before) <= other.distance_squared(before) { regular } else { other }
}

/// 验收只读取真实视口投影。调用方仍需要发送原指针和原按键，通过鼠标事务才能改变姿势。
#[cfg(feature = "validation")]
pub(crate) fn validation_animation_drag_points(
    world: &mut World,
    mode: crate::viewport::modes::ViewportMode,
    delta: f32,
) -> Option<(Vec2, Vec2)> {
    use crate::{
        authoring::EditorAuthoringSession, components::SceneEditorViewport,
        resources::{SceneEditorState, SceneEditorViewportState}, ui::gltf_browser::EditorGltfBrowserState,
        viewport::{LayerCandidateState, modes::ViewportMode},
    };
    use feathrise_editor_animation_model::gizmo::{MoveAxis, MoveConstraint, rotation_rings};
    let node_scale = world.query_filtered::<&ComputedNode, With<SceneEditorViewport>>()
        .single(world).ok()?.inverse_scale_factor();
    let editor = world.resource::<SceneEditorState>();
    let viewport = world.resource::<SceneEditorViewportState>();
    let session = world.resource::<EditorAuthoringSession>();
    let browser = world.resource::<EditorGltfBrowserState>();
    let scene = &session.draft.as_ref()?.scene_id;
    let (_, record) = super::input::selected_record(editor, session, browser, world.resource::<LayerCandidateState>(), viewport)?;
    let pose = world.resource::<Workbench>().pose_for(record, scene)?;
    let origin = Vec3::from_array(pose.translation);
    let camera = MoveCamera::new(editor, viewport, node_scale)?;
    let inside = |p: Vec2| p.abs().max_element() < 0.49;
    match mode {
        ViewportMode::Move => {
            let grabbed = origin + Vec3::Y * camera.handle_length(origin)? * 0.67;
            let from = camera.project(grabbed)?;
            let to = camera.project(grabbed + Vec3::Y * delta)?;
            (inside(from) && inside(to) && camera.pick_handle(origin, from, false) == Some(MoveHandle::Axis(MoveAxis::Y)))
                .then_some((from, to))
        }
        ViewportMode::Rotate => {
            let ring = rotation_rings(*camera, origin).into_iter().flatten()
                .find(|ring| ring.constraint == MoveConstraint::Y && !ring.edge_on)?;
            ring.world.into_iter().find_map(|point| {
                let from = camera.project(point)?;
                let to = camera.project(origin + Quat::from_rotation_y(delta.to_radians()) * (point - origin))?;
                (inside(from) && inside(to) && pick_rotation(camera, origin, from) == Some(MoveHandle::AnimationRotation(MoveConstraint::Y)))
                    .then_some((from, to))
            })
        }
        _ => None,
    }
}

#[cfg(feature = "validation")]
pub(crate) fn validation_animation_drag_active(world: &World) -> bool {
    world.get_resource::<super::PlacementMoveState>()
        .and_then(|movement| movement.drag.as_ref())
        .is_some_and(|drag| drag.animation.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_pose_rotation_preserves_a_world_axis_and_unwrapped_single_axis_keys() {
        let start = Vec3::new(28.0_f32, 96.0, -16.0).map(f32::to_radians);
        let angle = 65.0_f32.to_radians();
        let rotate = |value: Vec3| Quat::from_euler(EulerRot::XYZ, value.x, value.y, value.z);
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            let actual = rotate(rotate_euler(start, axis, angle));
            let expected = Quat::from_axis_angle(axis, angle) * rotate(start);
            assert!(actual.dot(expected).abs() > 0.99999);
        }
        let before = Vec3::Y * 315.0_f32.to_radians();
        assert!((rotate_euler(before, Vec3::Y, angle).y.to_degrees() - 380.0).abs() < 0.001);
    }
}
