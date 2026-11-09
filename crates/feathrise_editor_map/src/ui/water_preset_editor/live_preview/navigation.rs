//! 水体样品画布的右键轨道、中键平移与滚轮缩放。
//!
//! 【功能设计】导航只修改独立样品相机的会话状态，不改变地图、预设草稿或撤销栈；
//! 手势必须从画布内部开始，并通过统一鼠标门阻止拖出画布后误操作地图。

use feathrise_editor_ui::UiInteraction;
use bevy::camera::Projection; // 导航同步透视裁剪与真实画布宽高比。
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel}; // 读取原始物理像素移动与滚轮。
use bevy::prelude::*; // Resource、Transform、Query与输入资源。
use bevy::ui::RelativeCursorPosition; // 只在实际可见画布内取得手势。
use bevy::window::PrimaryWindow; // 过滤其他窗口的滚轮事件。
use feathrise_editor_ui::ActiveNumericInputScrub; // 本帧新开始的专业数值拖动优先。

use super::{
    PREVIEW_LENGTH_METERS, WaterPresetLivePreviewCamera, WaterPresetLivePreviewRuntime,
    WaterPresetLivePreviewViewport,
}; // 样品尺寸、相机与当前活动标签共同决定导航边界。
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState}; // F12显隐和统一鼠标所有权。
use crate::ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState}; // 隐藏场景页不得后台接收输入。

pub(super) const PREVIEW_FOV_RADIANS: f32 = 42.0_f32.to_radians(); // 小画布固定视野只通过距离缩放。
const PREVIEW_MIN_DISTANCE_METERS: f32 = 1.0; // 允许近距离检查微法线但保持正裁剪空间。
const PREVIEW_MAX_DISTANCE_METERS: f32 = 800.0; // 防止连续滚轮把相机送入浮点精度不足的远处。
const PREVIEW_PITCH_LIMIT_RADIANS: f32 = std::f32::consts::FRAC_PI_2 - 0.01; // 上下极点前停止，避免looking_at退化。
const PREVIEW_MAX_PAN_METERS: f32 = PREVIEW_LENGTH_METERS * 10.0; // 足够检查河面边缘，同时限制无界长拖。

/// 当前由水体样品画布持有的鼠标手势。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaterPreviewCameraDrag {
    Orbit, // 右键围绕当前观察中心旋转。
    Pan,   // 中键沿当前屏幕平面移动观察中心。
}

impl WaterPreviewCameraDrag {
    /// 返回冻结用途对应的鼠标键，和弦期间不重新猜测手势。
    const fn button(self) -> MouseButton {
        match self {
            Self::Orbit => MouseButton::Right, // 与中央地图视口保持一致。
            Self::Pan => MouseButton::Middle,  // 与中央地图视口保持一致。
        }
    }
}

/// 独立样品相机的纯会话导航状态。
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct WaterPresetLivePreviewNavigation {
    center: Vec3,                         // 轨道和平移共享的世界观察点。
    distance: f32,                        // 相机到观察点的米制距离，滚轮只修改此值。
    yaw: f32,                             // 绕世界Y轴的水平方位。
    pitch: f32,                           // 允许从河面上方连续旋转到下方的俯仰角。
    drag: Option<WaterPreviewCameraDrag>, // 手势开始后即使拖出画布也保持原用途。
    wheel_captured: bool,                 // 本帧画布悬停时阻止外层右栏同时滚动。
    mouse_captured: bool,                 // 拖动与release边沿通过统一鼠标门隔离地图输入。
}

impl Default for WaterPresetLivePreviewNavigation {
    fn default() -> Self {
        Self {
            center: Vec3::new(0.0, -0.05, 0.0), // 初始观察河面几何中心并略向下看河床。
            distance: 87.2,                     // 与原48、44、58米完整构图近似一致。
            yaw: 0.69,                          // 同时观察十米河宽和五十米长轴。
            pitch: 0.53,                        // 保留足够顶面角度辨识宏观波形。
            drag: None,                         // 首帧没有鼠标所有权。
            wheel_captured: false,              // 未悬停时右栏正常滚动。
            mouse_captured: false,              // 未拖动时地图输入正常工作。
        }
    }
}

impl WaterPresetLivePreviewNavigation {
    /// 返回当前完整相机姿态，场景首次创建和交互同步共用同一入口。
    pub(super) fn camera_transform(self) -> Transform {
        let direction = Vec3::new(
            self.yaw.sin() * self.pitch.cos(), // X由方位和俯仰共同决定。
            self.pitch.sin(),                  // Y允许为负以检查水面底侧。
            self.yaw.cos() * self.pitch.cos(), // Z与X构成单位水平环绕方向。
        );
        Transform::from_translation(self.center + direction * self.distance)
            .looking_at(self.center, Vec3::Y) // 俯仰远离极点，世界Y上方向保持稳定。
    }

    /// 右栏滚动系统读取本帧所有权，避免一次滚轮同时缩放相机和滚动面板。
    pub(crate) const fn owns_wheel(&self) -> bool {
        self.wheel_captured // 只在实际可交互画布悬停帧为true。
    }

    /// 统一鼠标门读取拖动所有权，拖出画布后仍阻止地图补触发。
    pub(crate) const fn dragging(&self) -> bool {
        self.drag.is_some() || self.mouse_captured // release边沿所在帧继续保持所有权。
    }

    /// 测试读取纯导航值，不向运行时其他模块扩大字段写权限。
    #[cfg(test)]
    pub(super) const fn test_values(&self) -> (Vec3, f32, f32, f32) {
        (self.center, self.distance, self.yaw, self.pitch) // 中心、距离与两轴角足以验证三类手势。
    }
}

/// 仅在画布内部按下且没有其他鼠标键和弦时建立导航手势。
fn begin_drag(mouse: &ButtonInput<MouseButton>) -> Option<WaterPreviewCameraDrag> {
    let buttons = [MouseButton::Left, MouseButton::Right, MouseButton::Middle]; // 固定三枚作者鼠标键。
    for button in buttons {
        if !mouse.just_pressed(button) {
            continue; // 从画布外按住后移入不得取得样品相机所有权。
        }
        if buttons
            .into_iter()
            .any(|other| other != button && (mouse.pressed(other) || mouse.just_released(other)))
        {
            return None; // 和弦及另一按键release帧均不得开始新手势。
        }
        return match button {
            MouseButton::Right => Some(WaterPreviewCameraDrag::Orbit), // 右键轨道旋转。
            MouseButton::Middle => Some(WaterPreviewCameraDrag::Pan),  // 中键屏幕平移。
            _ => None, // 左键保持只读，不修改河面或地图选择。
        };
    }
    None // 普通悬停不取得鼠标所有权。
}

/// 把有限滚轮档位转换为米制观察距离。
fn distance_after_scroll(distance: f32, lines: f32) -> f32 {
    if !distance.is_finite() || !lines.is_finite() {
        return WaterPresetLivePreviewNavigation::default().distance; // 坏输入恢复安全完整取景。
    }
    (distance * (-lines.clamp(-8.0, 8.0) * 0.11).exp())
        .clamp(PREVIEW_MIN_DISTANCE_METERS, PREVIEW_MAX_DISTANCE_METERS) // 正滚轮放大，负滚轮缩小。
}

/// 应用一次有限鼠标位移；旋转和平移都只改变小型会话状态。
fn apply_drag(
    navigation: &mut WaterPresetLivePreviewNavigation, // 原位写入当前样品相机状态。
    drag: WaterPreviewCameraDrag,                      // 按下时冻结的手势用途。
    motion: Vec2,                                      // 本帧物理像素位移总和。
    canvas_height: f32,                                // 米制平移手感按真实画布高度换算。
) {
    if !motion.is_finite() {
        return; // 无效设备事件不能污染相机Transform。
    }
    match drag {
        WaterPreviewCameraDrag::Orbit => {
            navigation.yaw = (navigation.yaw - motion.x * 0.006).rem_euclid(std::f32::consts::TAU); // 水平方位可连续环绕。
            navigation.pitch = (navigation.pitch + motion.y * 0.006)
                .clamp(-PREVIEW_PITCH_LIMIT_RADIANS, PREVIEW_PITCH_LIMIT_RADIANS); // 下拖增加俯仰，上拖可进入河面下方。
        }
        WaterPreviewCameraDrag::Pan if canvas_height > 0.0 => {
            let transform = navigation.camera_transform(); // 当前相机屏幕基决定平移方向。
            let meters_per_pixel =
                2.0 * navigation.distance * (PREVIEW_FOV_RADIANS * 0.5).tan() / canvas_height; // 放大时相同物理拖动移动更少米数。
            let shift =
                (-transform.right() * motion.x + transform.up() * motion.y) * meters_per_pixel; // 视觉内容跟随鼠标移动。
            navigation.center =
                (navigation.center + shift).clamp_length_max(PREVIEW_MAX_PAN_METERS); // 长拖仍保持有限精度并可恢复河面。
        }
        WaterPreviewCameraDrag::Pan => {} // 首帧布局无高度时安全忽略平移。
    }
}

/// 每帧消费原始输入；禁用帧也读空事件，避免恢复后补执行旧移动。
#[allow(clippy::too_many_arguments)]
pub(in crate::ui::water_preset_editor) fn control_live_preview_camera(
    mut motions: MessageReader<MouseMotion>, // 独立游标读取器不清空其他相机的事件。
    mut wheels: MessageReader<MouseWheel>,   // Line与Pixel单位统一为滚轮档位。
    mouse: Res<ButtonInput<MouseButton>>,    // 按下起点、持续捕获与松开边沿。
    editor: Res<MapEditorState>,             // F12关闭或切图等待时立即失去输入资格。
    sidebar: Res<MapEditorSidebarState>,     // 只有右侧场景页可操作样品。
    runtime: Res<WaterPresetLivePreviewRuntime>, // 活动水体编辑标签决定面板真实资格。
    numeric: Res<MapEditorNumericScrubMouseGuard>, // 上一手势release门优先于新相机手势。
    active_numeric: Res<ActiveNumericInputScrub>, // 同帧刚开始的数值拖动立即优先。
    windows: Query<(Entity, &Window), With<PrimaryWindow>>, // 窗口失焦立即取消捕获。
    canvases: Query<
        (&ComputedNode, &RelativeCursorPosition, &UiInteraction),
        With<WaterPresetLivePreviewViewport>,
    >, // 同时核对真实布局、祖先裁剪和前方遮挡。
    mut navigation: ResMut<WaterPresetLivePreviewNavigation>, // 只发布真实变化的小型会话状态。
) {
    let motion = motions
        .read()
        .filter(|event| event.delta.is_finite())
        .fold(Vec2::ZERO, |sum, event| sum + event.delta); // 物理像素不乘帧时间。
    let window = windows.single().ok(); // 缺失唯一窗口时失败关闭输入。
    let wheel = wheels
        .read()
        .filter(|event| {
            window.is_some_and(|(entity, _)| entity == event.window) && event.y.is_finite()
        })
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y,         // 普通鼠标整档输入。
            MouseScrollUnit::Pixel => event.y / 27.0, // 触控板沿用编辑器每行二十七像素口径。
        })
        .sum::<f32>(); // 下方即使return也不会积压旧滚轮。
    let mut next = *navigation; // 小型值快照只在真实变化时发布。
    next.wheel_captured = false; // 本帧重新验证画布悬停。
    next.mouse_captured = false; // release边沿按当前真实手势重新发布。
    let canvas = canvases
        .single()
        .ok()
        .filter(|(node, _, _)| node.size().is_finite() && node.size().min_element() > 0.0); // 首帧布局未就绪时不做除法。
    let enabled = editor.open
        && !editor.close_requested
        && editor.pending_map_transition.is_none()
        && sidebar.right == EditorRightSidebarTab::Scene
        && runtime.active_tab.is_some()
        && window.is_some_and(|(_, window)| window.focused); // 关闭、切页、切图或失焦都取消导航。
    if !enabled || canvas.is_none() || (numeric.blocked() && next.drag.is_none()) {
        next.drag = None; // 输入资格失效立即终止旧手势。
        navigation.set_if_neq(next); // 稳定禁用帧不触发Changed。
        return;
    }
    let (canvas, cursor, interaction) = canvas.expect("已确认有效的水体样品画布必须存在"); // 固定唯一画布。
    let hovered = cursor.cursor_over() && *interaction != UiInteraction::None; // 被弹框遮挡时不抢输入。
    let previous_drag = next.drag; // 起始按下帧不误用按下前累计位移。
    if next.drag.is_none() && hovered && !numeric.blocked() && active_numeric.entity.is_none() {
        next.drag = begin_drag(&mouse); // 手势只能从画布内部开始。
    }
    next.mouse_captured = next.drag.is_some(); // 极短点击和release帧仍阻止地图补触发。
    next.wheel_captured = hovered; // 画布悬停滚轮只缩放相机，不滚动右栏。
    if hovered && wheel != 0.0 && !numeric.blocked() && active_numeric.entity.is_none() {
        next.distance = distance_after_scroll(next.distance, wheel); // 围绕当前观察中心缩放。
    }
    if let Some(drag) = next.drag {
        if previous_drag.is_some()
            && (mouse.pressed(drag.button()) || mouse.just_released(drag.button()))
        {
            apply_drag(&mut next, drag, motion, canvas.size().y); // 捕获可越过画布边界继续完成。
        }
        if !mouse.pressed(drag.button()) {
            next.drag = None; // 松开原键结束，其他键不能接力改变用途。
        }
    }
    navigation.set_if_neq(next); // 稳定帧不更新相机或输入门。
}

/// 导航或画布尺寸真实变化时，原位更新唯一样品相机姿态与裁剪。
pub(in crate::ui::water_preset_editor) fn sync_live_preview_camera(
    navigation: Res<WaterPresetLivePreviewNavigation>, // 读取当前轨道、平移和缩放状态。
    canvases: Query<Ref<ComputedNode>, With<WaterPresetLivePreviewViewport>>, // resize只更新投影，不重建RTT。
    mut cameras: Query<(&mut Transform, &mut Projection), With<WaterPresetLivePreviewCamera>>, // 只修改样品相机。
) {
    let Ok(canvas) = canvases.single() else {
        return; // UI壳尚未生成时等待下一帧。
    };
    if !navigation.is_changed() && !canvas.is_changed() {
        return; // 稳定帧保持零相机写入。
    }
    let Ok((mut transform, mut projection)) = cameras.single_mut() else {
        return; // 场景尚未建立或正在安全回收。
    };
    let size = canvas.size(); // 物理宽高来自当前右栏真实布局。
    let aspect_ratio = (size.x / size.y.max(1.0)).clamp(0.05, 20.0); // 极窄布局仍保持有限投影。
    *transform = navigation.camera_transform(); // 轨道、平移和缩放一次性原子应用。
    *projection = Projection::Perspective(PerspectiveProjection {
        fov: PREVIEW_FOV_RADIANS, // 滚轮只改变距离，不扭曲透视。
        aspect_ratio,             // RTT与画布保持同一比例，禁止非等比拉伸。
        near: (navigation.distance * 0.002).clamp(0.02, 0.5), // 近距离检查仍避免零裁剪面。
        far: (navigation.distance + PREVIEW_LENGTH_METERS * 8.0 + navigation.center.length())
            .max(400.0), // 缩小和平移后河面仍在正远裁剪范围内。
        ..default()               // 其余透视参数沿用Bevy正式默认值。
    });
}

#[cfg(test)]
pub(super) fn test_apply_navigation(
    navigation: &mut WaterPresetLivePreviewNavigation, // 测试直接验证纯会话数学。
    orbit_motion: Vec2,                                // 右键等价位移。
    pan_motion: Vec2,                                  // 中键等价位移。
    wheel_lines: f32,                                  // 滚轮等价档位。
    canvas_height: f32,                                // 真实画布高度口径。
) {
    navigation.distance = distance_after_scroll(navigation.distance, wheel_lines); // 验证缩放。
    apply_drag(
        navigation,
        WaterPreviewCameraDrag::Orbit,
        orbit_motion,
        canvas_height,
    ); // 验证轨道。
    apply_drag(
        navigation,
        WaterPreviewCameraDrag::Pan,
        pan_motion,
        canvas_height,
    ); // 验证屏幕平移。
}
