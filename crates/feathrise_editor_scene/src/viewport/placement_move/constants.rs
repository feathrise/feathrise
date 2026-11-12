//! 旋转/缩放手柄的固定几何预算与逻辑像素规格，避免随场景规模扩大工作量。

pub(super) const ROTATION_SEGMENTS: usize = feathrise_author_tools::AUTHOR_ROTATION_SEGMENTS as usize;
pub(super) const ROTATION_RADIUS_FRACTION: f32 = 0.75;
pub(super) const PICK_LOGICAL_PIXELS: f32 = 8.0;
pub(super) const ROTATION_MIN_THICKNESS: f32 = 16.0; // 端视圆环折成一条线时无法可靠求水平角。
pub(super) use feathrise_editor_animation_model::gizmo::SCALE_DOUBLE_LOGICAL_PIXELS;
