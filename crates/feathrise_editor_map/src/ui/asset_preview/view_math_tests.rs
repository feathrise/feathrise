//! 预览导航数学回归：完整取景、近距离裁剪、负俯仰、图片锚点和resize保持。

use super::*; // 测试直接复用运行时同一纯数学实现。

#[test] // 保留原自动取景回归，增加导航默认参数。
fn model_frame_centers_an_offset_tall_asset_and_keeps_valid_clips() {
    let frame = model_preview_frame(
        Vec3::new(10.0, -2.0, 20.0), // 模型原点不在几何中心。
        Vec3::new(14.0, 18.0, 26.0), // 高模型要求按完整范围取景。
        1.0,                         // 方形画布。
        AssetPreviewView::default(), // 默认导航必须复现原有完整取景。
    ); // 不需要真实Scene或GPU。
    assert_eq!(frame.center, Vec3::new(12.0, 8.0, 23.0)); // 中心取自实际世界范围。
    assert_eq!(frame.size, Vec3::new(4.0, 20.0, 6.0)); // 状态栏保留真实尺寸。
    assert!(frame.near > 0.0 && frame.far > frame.near); // 正裁剪区间始终合法。
    assert!(frame.camera_position.distance(frame.center) > frame.size.max_element() * 0.5); // 默认能看到完整模型。
}

#[test] // 原窄栏取景回归继续存在。
fn narrow_tall_canvas_moves_camera_back_to_preserve_horizontal_fit() {
    let view = AssetPreviewView::default(); // 同一导航状态只改变画布比例。
    let square = model_preview_frame(Vec3::NEG_ONE, Vec3::ONE, 1.0, view); // 普通方形预览。
    let tall = model_preview_frame(Vec3::NEG_ONE, Vec3::ONE, 0.35, view); // 全屏可能出现很高的窄栏。
    assert!(
        tall.camera_position.distance(tall.center) > square.camera_position.distance(square.center)
    ); // 水平视野变窄时增加距离。
}

#[test] // 放大不能继续使用旧自动取景near而把模型切光。
fn zoom_changes_distance_and_preserves_safe_clipping_at_both_limits() {
    for zoom in [MIN_ZOOM, 1.0, MAX_ZOOM] {
        let view = AssetPreviewView { zoom, ..default() }; // 分别覆盖最小、默认与最大倍率。
        let frame = model_preview_frame(Vec3::NEG_ONE, Vec3::ONE, 0.5, view); // 窄画布同时考察横向适配。
        let distance = frame.camera_position.distance(frame.center); // 当前轨道目标距离。
        assert!(frame.near > 0.0 && frame.near < distance); // 观察目标不能落在近裁剪前。
        assert!(frame.far > distance && frame.far.is_finite()); // 缩小后远裁剪也必须覆盖目标。
    } // 三档均不产生NaN或反向投影。
    let base = model_preview_frame(Vec3::NEG_ONE, Vec3::ONE, 1.0, AssetPreviewView::default()); // 完整取景基准。
    let enlarged = model_preview_frame(
        Vec3::NEG_ONE,
        Vec3::ONE,
        1.0,
        AssetPreviewView {
            zoom: 2.0,
            ..default()
        },
    ); // 两倍放大。
    assert!(
        (base.camera_position.distance(base.center)
            / enlarged.camera_position.distance(enlarged.center)
            - 2.0)
            .abs()
            < 1e-5
    ); // 距离与倍率互逆。
}

#[test] // 快速滚动和非法设备输入都必须保持可操作。
fn zoom_is_bounded_reversible_and_rejects_invalid_input() {
    let enlarged = zoom_after_scroll(1.0, 2.0); // 连续向上滚动。
    assert!(enlarged > 1.0); // 向上即放大。
    assert!((zoom_after_scroll(enlarged, -2.0) - 1.0).abs() < 1e-6); // 等量反向滚动恢复倍率。
    assert_eq!(zoom_after_scroll(MAX_ZOOM, 1e9), MAX_ZOOM); // 上界不能溢出。
    assert_eq!(zoom_after_scroll(MIN_ZOOM, -1e9), MIN_ZOOM); // 下界不能穿过零。
    assert_eq!(zoom_after_scroll(2.0, f32::NAN), 2.0); // 坏事件被丢弃。
    assert_eq!(zoom_after_scroll(f32::NAN, 0.0), 1.0); // 坏状态仍可恢复。
}

#[test] // 预览与中央视口的右键上下拖动必须同向，不能只验证俯仰发生了变化。
fn orbit_vertical_drag_matches_map_viewport_direction() {
    let initial = AssetPreviewView::default(); // 从正常取景角度分别验证上拖和下拖。
    for motion_y in [-10.0, 10.0] {
        let mut view = initial; // 两个方向各自从同一姿态开始。
        orbit_after_drag(&mut view, Vec2::new(0.0, motion_y)); // Bevy原始鼠标Y向下为正。
        assert!((view.pitch - initial.pitch) * motion_y > 0.0); // 下拖增加俯仰，上拖减小俯仰。
        assert_eq!(view.yaw, initial.yaw); // 纯纵向拖动不能连带改变水平方位。
        assert_eq!(view.zoom, initial.zoom); // 修正方向不影响缩放。
        assert_eq!(view.model_pan, initial.model_pan); // 轨道旋转不串入中键平移。
        orbit_after_drag(&mut view, Vec2::new(0.0, -motion_y)); // 等量反向拖动应回到原俯仰。
        assert!((view.pitch - initial.pitch).abs() < 1e-6); // 不产生累积偏移。
    } // 两种纵向输入都走正式预览数学路径。
}

#[test] // 旋转可查看底面但不翻转上方向。
fn orbit_wraps_yaw_and_allows_negative_pitch_without_reaching_poles() {
    let mut view = AssetPreviewView::default(); // 默认向下观察模型。
    orbit_after_drag(&mut view, Vec2::new(100_000.0, -300.0)); // 水平多圈并向上拖动到模型下方，与中央视口一致。
    assert!(view.pitch < 0.0 && view.pitch > -std::f32::consts::FRAC_PI_2); // 底面视角合法。
    assert!((0.0..std::f32::consts::TAU).contains(&view.yaw)); // 方位保持有限标准周期。
    orbit_after_drag(&mut view, Vec2::new(0.0, 100_000.0)); // 极大向下拖动验证上方保护。
    assert_eq!(view.pitch, PITCH_LIMIT); // 上方极点前停止，不触发look_at奇点。
    orbit_after_drag(&mut view, Vec2::new(0.0, -100_000.0)); // 极大向上拖动验证下方保护。
    assert_eq!(view.pitch, -PITCH_LIMIT); // 修正方向后下方安全边界同样有效。
}

#[test] // UI变换要保持光标所指向的源图细节。
fn image_zoom_keeps_the_cursor_anchor_and_pan_never_loses_the_image() {
    let pan = Vec2::new(0.2, -0.1); // 已经平移过的图片。
    let cursor = Vec2::new(0.3, 0.2); // 非中心观察点。
    let next = image_pan_after_zoom(pan, cursor, 2.0, 3.0); // 在当前细节处继续放大。
    assert!(((cursor - pan) / 2.0 - (cursor - next) / 3.0).length() < 1e-6); // 鼠标对应同一源图坐标。
    let canvas = Vec2::new(300.0, 200.0); // 非方形画布。
    assert_eq!(
        clamp_image_pan(Vec2::splat(99.0), 1.0, canvas, canvas),
        Vec2::ZERO
    ); // 整图适配时禁止拖丢。
    assert_eq!(
        clamp_image_pan(Vec2::splat(99.0), 2.0, canvas, canvas),
        Vec2::splat(0.5)
    ); // 放大后可查看到完整边缘。
}

#[test] // resize只改变适配距离，不清除任何已调整观察参数。
fn resizing_preserves_orbit_zoom_and_panned_target() {
    let view = AssetPreviewView {
        zoom: 2.3,                            // 用户已放大。
        yaw: 2.4,                             // 用户已转到侧后方。
        pitch: -0.45,                         // 用户正在观察底面。
        model_pan: Vec3::new(0.5, 0.3, -0.2), // 用户已平移观察目标。
        ..default()                           // 图片偏移与本模型测试无关。
    }; // 这份状态不随窗口模式更改。
    let windowed = model_preview_frame(Vec3::NEG_ONE, Vec3::ONE, 0.8, view); // 窗口画布。
    let fullscreen = model_preview_frame(Vec3::NEG_ONE, Vec3::ONE, 0.3, view); // 全屏窄高画布。
    assert_eq!(windowed.center, fullscreen.center); // 平移目标严格保持。
    let a = (windowed.camera_position - windowed.center).normalize(); // 窗口观察方向。
    let b = (fullscreen.camera_position - fullscreen.center).normalize(); // 全屏观察方向。
    assert!((a - b).length() < 1e-6); // 方向不跳回默认值。
    assert_eq!(view.zoom, 2.3); // 相对取景倍率不重置。
}

#[test] // 屏幕平移应随当前旋转方向，而不是硬编码世界轴。
fn model_pan_moves_target_in_the_current_camera_plane() {
    let mut view = AssetPreviewView::default(); // 初始观察朝向。
    let frame = model_preview_frame(Vec3::NEG_ONE, Vec3::ONE, 1.0, view); // 当前屏幕覆盖米数。
    pan_model_after_drag(&mut view, frame, Vec2::new(20.0, 0.0), 300.0); // 中键向右拖动。
    assert!(view.model_pan.length() > 0.0); // 观察目标确实变化。
    let forward = (frame.center - frame.camera_position).normalize(); // 当前视线方向。
    assert!(view.model_pan.dot(forward).abs() < 1e-5); // 平移严格位于相机平面。
}
