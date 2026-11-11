//! 资产预览缩放、轨道、平移和安全裁剪的常数成本纯数学。

use bevy::camera::{CameraProjection, OrthographicProjection, Projection, ScalingMode};
use bevy::prelude::*; // 向量、旋转与相机变换。
use feathrise_editor_core::EditorSnapSettings;

use crate::types::EditorView;

pub(super) const MODEL_PREVIEW_FOV_RADIANS: f32 = 42.0_f32.to_radians(); // 延续原有小画布透视。
const SKY_VIEW_PITCH: f32 = 8.0_f32.to_radians(); // 让地平线上方进入默认放大的视野。
const MIN_ZOOM: f32 = 0.1; // 允许缩小到完整取景的十分之一。
const MAX_ZOOM: f32 = 32.0; // 放大检查细节，同时防止指数溢出。
const PITCH_LIMIT: f32 = std::f32::consts::FRAC_PI_2 - 0.01; // 允许看底面，但不越过上下极点。

/// 两种预览的纯观察参数；resize只改变适配基准，不覆盖作者正在看的方向。
#[derive(Clone, Copy, Debug, PartialEq)] // 小型值快照便于变更检测和回归测试。
pub(super) struct AssetPreviewView {
    pub(super) zoom: f32,       // 1为完整取景，数值增大表示放大。
    pub(super) yaw: f32,        // 模型绕世界Y轴的观察方位。
    pub(super) pitch: f32,      // 模型俯仰，负数可以查看底面。
    pub(super) orthographic: bool, // 方向球切换投影时继续沿用原焦点与缩放倍率。
    pub(super) model_pan: Vec3, // 模型观察目标相对原包围盒中心的世界偏移。
    pub(super) image_pan: Vec2, // 图片中心相对画布中心的归一化偏移，resize不依赖旧像素尺寸。
    pub(super) snapping: EditorSnapSettings, // 与相机同属文件标签快照，和地图编辑器偏好独立。
}

impl Default for AssetPreviewView {
    fn default() -> Self {
        Self {
            zoom: 1.0,                                       // 首次加载和重置都显示完整资产。
            yaw: std::f32::consts::FRAC_PI_4,                // 与原(1, 0.62, 1)观察方向一致。
            pitch: (0.62 / std::f32::consts::SQRT_2).atan(), // 同时可辨正面、侧面和顶面。
            orthographic: false,
            model_pan: Vec3::ZERO,                           // 原模型位置不影响居中取景。
            image_pan: Vec2::ZERO,                           // 图片初始居中，不依赖鼠标位置。
            snapping: EditorSnapSettings {
                show_grid: false,
                translation_meters: 0.1, // 人形编辑起始步长比地图更细。
                ..default()
            },
        } // 默认值不加载或复制任何资产。
    }
}

impl AssetPreviewView {
    pub(super) fn rotation(self) -> Quat {
        // 与地图轨道相机同一方位约定，正顶视仍有确定的上方向。
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(-self.pitch)
    }

    /// 方向键仅对准观察轴。投影和距离分别由正交开关与取景按钮控制。
    pub(super) fn set_direction(&mut self, direction: EditorView, front_yaw: f32) {
        use std::f32::consts::FRAC_PI_2;
        let initial = Self::default();
        let (yaw, pitch) = match direction {
            EditorView::Perspective => (initial.yaw, SKY_VIEW_PITCH),
            EditorView::Top => (0.0, FRAC_PI_2),
            EditorView::Front => (0.0, 0.0),
            EditorView::Side => (FRAC_PI_2, 0.0),
        };
        self.yaw = front_yaw + yaw;
        self.pitch = pitch;
    }

    pub(super) fn direction(self, front_yaw: f32) -> EditorView {
        // 高亮从真实相机姿态读取，方向球和右键旋转不产生过期的工具条选中态。
        let direction = Quat::from_rotation_y(-front_yaw) * (self.rotation() * Vec3::Z);
        if direction.y > 0.999_999 {
            EditorView::Top
        } else if direction.z > 0.999_999 {
            EditorView::Front
        } else if direction.x > 0.999_999 {
            EditorView::Side
        } else {
            EditorView::Perspective
        }
    }

    /// 复用已准备的资产范围。选中关节时围绕其位置收紧，全部取景还原完整边界。
    pub(super) fn frame_model(&mut self, minimum: Vec3, maximum: Vec3, selected: Option<Vec3>) {
        let center = minimum.midpoint(maximum);
        self.model_pan = selected.map_or(Vec3::ZERO, |point| point - center);
        self.zoom = if selected.is_some() { 3.0 } else { 1.0 };
    }

    pub(super) fn reset_camera(&mut self) {
        *self = Self {
            snapping: self.snapping,
            ..default()
        };
    }

    pub(super) fn reveal_default_sky(&mut self) {
        let initial = Self::default();
        if !self.orthographic && self.pitch == initial.pitch && self.yaw == initial.yaw {
            // 只调整小预览原始角度，作者已转动的视角和正交模式原样迁移。
            self.pitch = SKY_VIEW_PITCH;
        }
    }
}

/// 自动取景与交互导航共同使用的完整相机结果。
#[derive(Clone, Copy, Debug, PartialEq)] // 便于直接验证裁剪和距离。
pub(super) struct ModelPreviewFrame {
    vertical_span: f32, // 目标平面的可视高度，用于等比例切换投影与平移。
    pub(super) center: Vec3,          // 当前轨道目标，包含只读预览平移。
    pub(super) camera_position: Vec3, // 当前观察位置，不改模型Transform。
    pub(super) near: f32,             // 正近裁剪面，近距离查看不会沿用自动取景的大near。
    pub(super) far: f32,              // 随当前距离和平移更新的远裁剪面。
    pub(super) size: Vec3,            // 原始尺寸只供只读状态栏显示。
}

/// 滚轮采用指数倍率，有限上下界保证快速连续滚动仍可重置。
pub(super) fn zoom_after_scroll(zoom: f32, lines: f32) -> f32 {
    let zoom = if zoom.is_finite() && zoom > 0.0 {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }; // 坏旧状态也可恢复完整取景。
    if !lines.is_finite() {
        return zoom; // 输入设备非法增量不能污染视图。
    } // 正常鼠标与触控板均进入同一倍率路径。
    (zoom * (lines.clamp(-8.0, 8.0) * 0.12).exp()).clamp(MIN_ZOOM, MAX_ZOOM) // 不乘帧时间。
}

/// 只旋转观察方向；与场景视口统一为下拖增大俯仰、上拖减小俯仰并可进入底面视角。
pub(super) fn orbit_after_drag(view: &mut AssetPreviewView, delta: Vec2) {
    if !delta.is_finite() {
        return; // 坏设备事件不得把相机变成NaN。
    } // 物理像素直接决定角度，不随帧率改变。
    view.yaw = (view.yaw - delta.x * 0.006).rem_euclid(std::f32::consts::TAU); // 水平方位循环。
    if delta.y != 0.0 {
        view.pitch = (view.pitch + delta.y * 0.006).clamp(-PITCH_LIMIT, PITCH_LIMIT); // 精确顶视在没有纵向位移时保持对轴。
    }
}

/// 图片始终保持比例；小于画布时居中，大于画布时最多平移到任一边缘。
pub(super) fn clamp_image_pan(pan: Vec2, zoom: f32, image_size: Vec2, canvas_size: Vec2) -> Vec2 {
    if !canvas_size.is_finite() || canvas_size.min_element() <= 0.0 || !pan.is_finite() {
        return Vec2::ZERO; // 首帧没有有效布局时保留居中。
    } // 所有尺寸均来自同一物理像素坐标系。
    let limit = ((image_size * zoom - canvas_size) * 0.5).max(Vec2::ZERO) / canvas_size; // 不允许把整张图拖丢。
    pan.clamp(-limit, limit) // 两轴分别限制，非方形图片也不会被错误裁切。
}

/// 图片围绕当前鼠标位置缩放，已放大的细节不会每次跳回中心。
pub(super) fn image_pan_after_zoom(pan: Vec2, cursor: Vec2, old_zoom: f32, new_zoom: f32) -> Vec2 {
    cursor - (cursor - pan) * (new_zoom / old_zoom) // 变换前后鼠标下对应同一源图位置。
}

/// 两轴FOV共同决定适配距离；导航使用此基准而不是不断重扫模型。
pub(super) fn model_preview_frame(
    minimum: Vec3,          // 完整缓存世界范围下界。
    maximum: Vec3,          // 完整缓存世界范围上界。
    aspect_ratio: f32,      // 当前实际画布纵横比。
    view: AssetPreviewView, // 当前缩放、旋转和平移的纯值快照。
) -> ModelPreviewFrame {
    let size = (maximum - minimum).max(Vec3::ZERO); // 兼容退化平面范围。
    let radius = (size * 0.5).length().max(0.05); // 毫米模型仍拥有可操作的正距离。
    let vertical_tangent = (MODEL_PREVIEW_FOV_RADIANS * 0.5).tan(); // 垂直半视野。
    let horizontal_tangent = vertical_tangent * aspect_ratio.clamp(0.05, 20.0); // 窄栏也完整容纳模型。
    let fit_distance = radius / vertical_tangent.min(horizontal_tangent) * 1.18; // 沿用原有取景留边。
    let fit_zoomed = (fit_distance / view.zoom).max(radius * 0.03);
    let vertical_span = 2.0 * fit_zoomed * vertical_tangent;
    let distance = if view.orthographic {
        // 正交放大时仍把镜头放在模型前方，避免近裁面切掉肩膀或脚尖。
        fit_zoomed.max(radius * 2.0 + view.model_pan.length())
    } else {
        fit_zoomed
    };
    let direction = view.rotation() * Vec3::Z;
    let center = (minimum + maximum) * 0.5 + view.model_pan; // 模型本身保持原变换。
    let near = (radius * 0.002).max(0.0001).min(distance * 0.1); // 放大后不被旧自动取景near切掉。
    let far = (distance + radius * 3.0 + view.model_pan.length()).max(near + 1.0); // 平移后原模型仍在远裁剪范围内。
    ModelPreviewFrame {
        vertical_span,
        center,                                         // 轨道与平移共用同一观察目标。
        camera_position: center + direction * distance, // 不修改Scene0的任何作者参数。
        near,                                           // 当前正近裁剪。
        far,                                            // 当前远裁剪。
        size,                                           // 原资产世界尺度保持不变。
    } // 所有计算仅涉及固定数量向量。
}

/// 沿当前屏幕右/上方向移动目标，拖动距离按当前投影覆盖米数换算。
pub(super) fn pan_model_after_drag(
    view: &mut AssetPreviewView, // 只写临时观察偏移。
    frame: ModelPreviewFrame,    // 当前距离与朝向对应的固定数学结果。
    delta: Vec2,                 // 本帧物理像素位移。
    canvas_height: f32,          // 画布真实物理高度，窗口与全屏手感一致。
) {
    if !delta.is_finite() || canvas_height <= 0.0 {
        return; // 无布局或无效事件不移动观察目标。
    } // 采用相机平面而不是硬编码世界XZ。
    let transform = Transform::from_rotation(view.rotation()); // 当前屏幕基，正交和透视共用。
    let meters_per_pixel = frame.vertical_span / canvas_height;
    let shift = (-transform.right() * delta.x + transform.up() * delta.y) * meters_per_pixel; // 模型视觉跟随鼠标。
    let radius = (frame.size * 0.5).length().max(0.05); // 与自动取景使用同一个尺度。
    view.model_pan = (view.model_pan + shift).clamp_length_max(radius * 20.0); // 防止长拖导致数值无界和精度丢失。
}

/// 统一应用完整相机姿态；窗口resize和手势变化都不会重建离屏纹理。
pub(super) fn apply_model_preview_frame(
    minimum: Vec3,               // 已缓存世界范围下界。
    maximum: Vec3,               // 已缓存世界范围上界。
    aspect_ratio: f32,           // 当前真实画布比例。
    view: AssetPreviewView,      // 原样保留用户导航结果。
    transform: &mut Transform,   // 只作用于专用预览相机。
    projection: &mut Projection, // 与相机位置成对更新正近/远裁剪。
) -> ModelPreviewFrame {
    let frame = model_preview_frame(minimum, maximum, aspect_ratio, view); // 无资产扫描的固定成本。
    *transform = Transform::from_translation(frame.camera_position).with_rotation(view.rotation());
    *projection = if view.orthographic {
        let mut orthographic = OrthographicProjection {
            near: frame.near,
            far: frame.far,
            scaling_mode: ScalingMode::FixedVertical { viewport_height: frame.vertical_span },
            ..OrthographicProjection::default_3d()
        };
        // 点位投影在同帧读取 area，不能等待下一次渲染相机更新。
        orthographic.update(aspect_ratio, 1.0);
        Projection::Orthographic(orthographic)
    } else {
        Projection::Perspective(PerspectiveProjection {
            fov: MODEL_PREVIEW_FOV_RADIANS,
            aspect_ratio,
            near: frame.near,
            far: frame.far,
            ..default()
        })
    }; // 相机与投影在同一系统内完整采用。
    frame // 扫描完成时仍可复用原模型尺寸状态。
}

/// 固定缩略图复用大预览的安全取景，二维纹理从正面观察并保留宽高比。
pub(crate) fn frame_thumbnail_camera(minimum: Vec3, maximum: Vec3, flat: bool, transform: &mut Transform, projection: &mut Projection) {
    let mut view = AssetPreviewView::default();
    if flat {
        view.yaw = 0.0; view.pitch = 0.0;
        let size = maximum - minimum;
        view.zoom = size.length() * 1.18 / (size.max_element().max(0.0001) * 1.08); // 平面按最长边留8%余量。
    }
    apply_model_preview_frame(minimum, maximum, 1.0, view, transform, projection);
}

#[cfg(test)] // 数学测试无需窗口、GPU或实际模型。
#[path = "view_math_tests.rs"] // 让实现文件保持单一职责和可读长度。
mod tests; // 覆盖缩放、轨道、图片锚点和resize保持。
