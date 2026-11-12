//! 等比缩放中心方块与倍率；同乘原三轴并整体限幅，不将已有比例重置为单位形状。

use super::{constants::*, math::MoveCamera};
use bevy::prelude::*;
use feathrise_scene_content::{MAX_SCENE_PLACEMENT_SCALE, MIN_SCENE_PLACEMENT_SCALE};

pub(crate) fn pick_scale(camera: MoveCamera, origin: Vec3, normalized: Vec2) -> bool {
    let Some(center) = camera.project(origin) else {
        return false;
    };
    let offset = (normalized - center) * camera.size * camera.inverse_scale;
    offset.is_finite() && offset.abs().max_element() <= PICK_LOGICAL_PIXELS
}

#[cfg(test)]
pub(super) fn uniform_scale(before: [f32; 3], logical_delta: Vec2, snap: bool) -> Option<[f32; 3]> {
    uniform_scale_step(before, logical_delta, if snap { 0.1 } else { 0.0 })
}

pub(super) fn uniform_scale_step(before: [f32; 3], logical_delta: Vec2, step: f32) -> Option<[f32; 3]> {
    let scale = Vec3::from_array(before);
    if !scale.is_finite()
        || !logical_delta.is_finite()
        || scale.min_element() < MIN_SCENE_PLACEMENT_SCALE
        || scale.max_element() > MAX_SCENE_PLACEMENT_SCALE
    {
        return None;
    }
    let exponent =
        ((logical_delta.x - logical_delta.y) / SCALE_DOUBLE_LOGICAL_PIXELS).clamp(-30.0, 30.0);
    let mut factor = 2.0_f32.powf(exponent);
    if step.is_finite() && step > 0.0 {
        factor = (factor / step).round() * step;
    }
    let min_factor = MIN_SCENE_PLACEMENT_SCALE / scale.min_element();
    let max_factor = MAX_SCENE_PLACEMENT_SCALE / scale.max_element();
    factor = factor.clamp(min_factor, max_factor);
    let result = (scale * factor).clamp(
        Vec3::splat(MIN_SCENE_PLACEMENT_SCALE),
        Vec3::splat(MAX_SCENE_PLACEMENT_SCALE),
    );
    result.is_finite().then(|| result.to_array()) // 分量clamp只抵消边界浮点舍入，共同倍率先保证比例与有限值。
}

/// 整组倍率由按下时汇总的共同上下界约束，拖动帧只做常数计算。
pub(crate) use feathrise_editor_animation_model::gizmo::group_scale_factor;
