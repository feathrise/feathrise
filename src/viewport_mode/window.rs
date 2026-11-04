//! 仅测量作者编辑视口与校验本地窗口身份；原生试玩窗口自行拥有尺寸、全屏与鼠标。
//! 不从作者帧调用跨进程 SetParent/SetWindowPos，也不在后台窗口恢复时抢占用户焦点。

use super::{ViewportGeometry, ViewportMode, ViewportModeState};
use bevy::{
    prelude::*,
    window::PrimaryWindow,
};
#[cfg(windows)]
use bevy::window::RawHandleWrapper;
use feathrise_editor_map::{
    standalone::viewport_projection::{ViewportControlShape, ViewportHostProjection},
    workbench::WorkbenchHostPageState,
};
use feathrise_editor_protocol::ViewportUiShape;
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow,
        IsWindowVisible,
    },
};

#[cfg(windows)]
pub(super) fn measure(
    mut selection: ResMut<ViewportModeState>,
    mut geometry: ResMut<ViewportGeometry>,
    windows: Query<(&Window, &RawHandleWrapper), With<PrimaryWindow>>,
    projection: Res<ViewportHostProjection>,
    game: Option<Res<crate::embedded_viewport::EmbeddedStatus>>,
    page: Option<Res<WorkbenchHostPageState>>,
    overlay: Option<Res<feathrise_editor_core::EditorHostOverlay>>,
    chrome:Option<Res<crate::window_chrome::ChromeState>>,
) {
    let mut next = geometry.clone();
    next.visible = false;
    next.focused = false;
    next.excluded_rects.clear();
    next.excluded_shapes.clear();
    // 预览已归还工程原生顶层窗口；冻结最后有效编辑租约，供同一HWND返回时重新嵌入。
    // 不能因游戏取得前台而把作者失焦当作游戏输入禁用，也不能以旧标题栏尺寸约束原生全屏。
    let playing = game
        .as_ref()
        .is_some_and(|game| game.active && game.playing);
    if playing { return; }
    if selection.selected == ViewportMode::EmbeddedWorld
        && !playing
        && let Some(error) = projection.error.as_ref()
        && selection.status != *error
    {
        selection.status.clone_from(error);
    }
    if let Ok((window, raw)) = windows.single()
        && let Some(hwnd) = crate::workspace_window::primary_hwnd(raw)
        && valid_local_parent(hwnd)
    {
        next.parent_hwnd = hwnd;
        next.parent_pid = std::process::id();
        // obscured只表示整窗模态；提示、详情和帮助已经投影为局部孔，不能因输入被UI占有而隐藏child。
        let blocked = overlay.as_ref().is_some_and(|overlay| overlay.active)
            || chrome.as_ref().is_some_and(|chrome| chrome.captures_input())
            || page
                .as_ref()
                .is_some_and(|page| page.active || page.settings_open || page.menu_open)
            || (!playing && projection.obscured);
        let bounds = [window.physical_width(), window.physical_height()];
        let rect = projection.layout.and_then(|(size, center)| physical_rect(size, center, bounds));
        if let Some(rect) = rect {
            next.rect = rect;
            let mut invalid_shapes = false;
            if !playing {
                next.excluded_rects = child_exclusions(&projection.excluded_rects, rect);
                match child_shapes(&projection.excluded_shapes, rect) {
                    Ok(shapes) => next.excluded_shapes = shapes,
                    Err(error) => {
                        invalid_shapes = true;
                        if selection.selected == ViewportMode::EmbeddedWorld && selection.status != error {
                            selection.status = error;
                        }
                    }
                }
            }
            next.visible = selection.selected == ViewportMode::EmbeddedWorld
                && !blocked
                && !invalid_shapes
                && native_visible(hwnd);
            next.focused = next.visible && native_foreground(hwnd);
        }
    }
    // 只有小型快照实际改变才增加修订，宿主可合并 resize/隐藏而不每帧排队消息。
    if next != *geometry {
        next.revision = geometry.revision.wrapping_add(1).max(1);
        *geometry = next;
    }
}

/// Unix作者由同主窗共享帧呈现，抽象PrimaryWindow没有RawHandleWrapper；只缓存真实布局。
#[cfg(not(windows))]
pub(super) fn measure(
    mut geometry: ResMut<ViewportGeometry>,
    windows: Query<&Window, With<PrimaryWindow>>,
    projection: Res<ViewportHostProjection>,
) {
    let mut next = geometry.clone();
    next.parent_hwnd = 0;
    next.parent_pid = 0;
    // 不把抽象作者窗口冒充Win32工程子窗；F12仍走独立工程进程。
    next.visible = false;
    next.focused = false;
    next.excluded_rects.clear();
    next.excluded_shapes.clear();
    if let Ok(window) = windows.single() {
        let bounds = [window.physical_width(), window.physical_height()];
        if let Some(rect) = projection.layout.and_then(|(size, center)| physical_rect(size, center, bounds)) {
            next.rect = rect;
        }
    }
    if next != *geometry {
        next.revision = geometry.revision.wrapping_add(1).max(1);
        *geometry = next;
    }
}

#[cfg(windows)]
pub(crate) fn valid_local_parent(hwnd: usize) -> bool {
    let mut pid = 0;
    unsafe {
        hwnd != 0
            && IsWindow(hwnd as HWND) != 0
            && GetWindowThreadProcessId(hwnd as HWND, &mut pid) != 0
            && pid == std::process::id()
    }
}

#[cfg(windows)]
fn native_visible(hwnd: usize) -> bool {
    unsafe {
        let root = GetAncestor(hwnd as HWND, GA_ROOT);
        !root.is_null() && IsWindowVisible(hwnd as HWND) != 0 && IsIconic(root) == 0
    }
}

#[cfg(windows)]
fn native_foreground(hwnd: usize) -> bool {
    unsafe {
        let root = GetAncestor(hwnd as HWND, GA_ROOT);
        !root.is_null() && GetForegroundWindow() == root && IsIconic(root) == 0
    }
}

fn physical_rect(size: Vec2, center: Vec2, bounds: [u32; 2]) -> Option<[i32; 4]> {
    if !size.is_finite() || !center.is_finite() || size.min_element() <= 0. || bounds.contains(&0) {
        return None;
    }
    let minimum = (center - size * 0.5).ceil().max(Vec2::ZERO);
    let maximum = (center + size * 0.5)
        .floor()
        .min(Vec2::new(bounds[0] as f32, bounds[1] as f32));
    let extent = maximum - minimum;
    if extent.min_element() < 1. || maximum.max_element() > i32::MAX as f32 {
        return None;
    }
    Some([
        minimum.x as i32,
        minimum.y as i32,
        extent.x as i32,
        extent.y as i32,
    ])
}

fn child_exclusions(excluded: &[Rect], child: [i32; 4]) -> Vec<[u32; 4]> {
    let origin = Vec2::new(child[0] as f32, child[1] as f32);
    let maximum = origin + Vec2::new(child[2] as f32, child[3] as f32);
    excluded
        .iter()
        .filter_map(|rect| {
            // 向外取整覆盖控件抗锯齿边缘；再次裁剪避免窗口边缘舍入产生越界HRGN。
            let minimum = rect.min.floor().max(origin);
            let end = rect.max.ceil().min(maximum);
            if !minimum.cmplt(end).all() {
                return None;
            }
            let offset = minimum - origin;
            let extent = end - minimum;
            Some([
                offset.x as u32,
                offset.y as u32,
                extent.x as u32,
                extent.y as u32,
            ])
        })
        .collect()
}

fn child_shapes(excluded: &[ViewportControlShape], child: [i32; 4]) -> Result<Vec<ViewportUiShape>, String> {
    if excluded.len() > feathrise_editor_protocol::MAX_VIEWPORT_EXCLUDED_SHAPES {
        return Err("XYZ 方向轴超过10个固定轮廓，已隐藏工程视口".into());
    }
    let origin = Vec2::new(child[0] as f32, child[1] as f32);
    let mut shapes = Vec::with_capacity(excluded.len());
    for shape in excluded {
        let converted = match shape {
            ViewportControlShape::Ellipse { outer, inner } => {
                if !outer.min.is_finite() || !outer.max.is_finite()
                    || inner.is_some_and(|rect| !rect.min.is_finite() || !rect.max.is_finite()) {
                    return Err("XYZ 方向轴包含非法坐标，已隐藏工程视口".into());
                }
                // 外轮廓向外取整保留抗锯齿像素，内孔向内取整保留真实边线；
                // 不裁小椭圆的外接框，部分越过 child 时由完整窗口区域做最终相交。
                let minimum = (outer.min - origin).floor();
                let maximum = (outer.max - origin).ceil();
                let outer = [minimum.x as i32, minimum.y as i32, maximum.x as i32, maximum.y as i32];
                let inner = inner.and_then(|rect| {
                    let minimum = (rect.min - origin).ceil();
                    let maximum = (rect.max - origin).floor();
                    minimum.cmplt(maximum).all().then_some([
                        minimum.x as i32, minimum.y as i32, maximum.x as i32, maximum.y as i32])
                });
                ViewportUiShape::Ellipse { outer, inner }
            }
            ViewportControlShape::Quad { points } => {
                if points.iter().any(|point| !point.is_finite()) {
                    return Err("XYZ 方向轴包含非法坐标，已隐藏工程视口".into());
                }
                let rounded = points.map(|point| {
                    let point = (point - origin).round();
                    [point.x as i32, point.y as i32]
                });
                let shape = ViewportUiShape::Quad { points: rounded };
                if !shape.valid() {
                    let a = points[1] - points[0];
                    let b = points[2] - points[0];
                    let c = points[3] - points[0];
                    let area = (a.perp_dot(b) + b.perp_dot(c)).abs() * 0.5;
                    // 几乎正对相机的短轴可在整数像素上退化；跳过这类不足4像素的线，
                    // 不以它的外接矩形代替真实斜线，也不丢弃其余完整端点。
                    if area <= 4. { continue; }
                }
                shape
            }
        };
        if !converted.valid() {
            return Err("XYZ 方向轴轮廓超出有效物理像素范围，已隐藏工程视口".into());
        }
        shapes.push(converted);
    }
    Ok(shapes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn native_preview_keeps_the_last_editor_lease_when_the_game_owns_foreground() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<ViewportModeState>();
        world.init_resource::<ViewportHostProjection>();
        world.insert_resource(crate::embedded_viewport::EmbeddedStatus {
            active: true, ready: true, playing: true, ..default()
        });
        let lease = ViewportGeometry {
            parent_hwnd: 17, parent_pid: 23, rect: [100, 50, 600, 400],
            visible: true, focused: true, revision: 42,
            excluded_rects: vec![[2, 3, 40, 50]], ..default()
        };
        world.insert_resource(lease.clone());
        // 此时作者UI已隐藏，且游戏是另一活动顶层；不得据此把最后编辑租约改成隐藏或整窗。
        world.run_system_once(measure).unwrap();
        assert_eq!(*world.resource::<ViewportGeometry>(), lease);
        world.resource_mut::<crate::embedded_viewport::EmbeddedStatus>().active = false;
        world.run_system_once(measure).unwrap();
        assert!(!world.resource::<ViewportGeometry>().visible, "失效会话不得冻结旧可见性");
    }

    #[test]
    #[cfg(not(windows))]
    fn abstract_unix_window_keeps_author_layout_without_a_native_handle() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<ViewportGeometry>();
        world.insert_resource(ViewportHostProjection {
            layout: Some((Vec2::new(320., 240.), Vec2::new(200., 150.))),
            ..default()
        });
        let window = world.spawn((Window::default(), PrimaryWindow)).id();
        world.run_system_once(measure).unwrap();
        let geometry = world.resource::<ViewportGeometry>();
        assert_eq!(geometry.rect, [40, 30, 320, 240]);
        assert_eq!(geometry.parent_hwnd, 0);
        assert!(!geometry.visible && !geometry.focused);
        assert!(world.get::<bevy::window::RawHandleWrapper>(window).is_none());
    }
    #[test]
    fn physical_bounds_are_clipped_and_never_publish_invalid_surface_sizes() {
        assert_eq!(
            physical_rect(Vec2::new(800., 600.), Vec2::new(500., 400.), [1270, 800]),
            Some([100, 100, 800, 600])
        );
        assert_eq!(
            physical_rect(Vec2::new(800., 600.), Vec2::new(10., 20.), [300, 200]),
            Some([0, 0, 300, 200])
        );
        assert!(physical_rect(Vec2::ZERO, Vec2::ONE, [300, 200]).is_none());
        assert!(physical_rect(Vec2::ONE, Vec2::splat(f32::NAN), [300, 200]).is_none());
        assert!(physical_rect(Vec2::ONE, Vec2::ONE, [0, 200]).is_none());
    }

    #[test]
    fn author_holes_use_child_local_pixels_without_changing_the_viewport() {
        let child = [101, 51, 399, 299];
        assert_eq!(
            child_exclusions(
                &[
                    Rect::new(100.4, 50.2, 130.1, 75.6),
                    Rect::new(480.5, 330.5, 510.4, 360.8),
                    Rect::new(700., 500., 710., 510.),
                ],
                child
            ),
            vec![[0, 0, 30, 25], [379, 279, 20, 20]]
        );
        assert_eq!(child, [101, 51, 399, 299]);
    }

    #[test]
    fn xyz_round_holes_keep_the_hollow_center_and_unclipped_edge_curve() {
        let child = [100, 50, 399, 299];
        let source = vec![ViewportControlShape::Ellipse {
            outer: Rect::new(95.4, 48.2, 109.4, 62.2),
            inner: Some(Rect::new(97.4, 50.2, 107.4, 60.2)),
        }];
        let expected = vec![ViewportUiShape::Ellipse {
            outer: [-5, -2, 10, 13], inner: Some([-2, 1, 7, 10]),
        }];
        assert_eq!(child_shapes(&source, child).unwrap(), expected);
        assert_eq!(child_shapes(&source, child).unwrap(), expected, "静止轮廓应产生相同几何，不推进修订");
        assert_eq!(child, [100, 50, 399, 299]);
    }

    #[test]
    fn xyz_diagonal_holes_preserve_four_corners_and_skip_only_subpixel_degeneracy() {
        let source = vec![
            ViewportControlShape::Quad { points: [Vec2::new(108., 50.), Vec2::new(120., 62.),
                Vec2::new(118., 64.), Vec2::new(106., 52.)] },
            ViewportControlShape::Quad { points: [Vec2::new(102.01, 52.01), Vec2::new(102.1, 52.1),
                Vec2::new(102.12, 52.12), Vec2::new(102.02, 52.02)] },
        ];
        assert_eq!(child_shapes(&source, [100, 50, 399, 299]).unwrap(), vec![ViewportUiShape::Quad {
            points: [[8, 0], [20, 12], [18, 14], [6, 2]],
        }]);
        assert!(child_shapes(&[ViewportControlShape::Ellipse {
            outer: Rect::new(0., 0., 100_000., 100_000.), inner: None,
        }], [0, 0, 399, 299]).is_err());
    }
}
