//! Windows自绘非客户区适配；保留系统移动/缩放循环、任务栏与关闭事务，不销毁窗口或接管工程生命周期。
use super::components::ChromeButton;
use super::{resources::ChromeState, types::*};
use bevy::{
    app::MainScheduleOrder,
    ecs::{schedule::ScheduleLabel, system::NonSendMarker},
    prelude::*,
    ui::RelativeCursorPosition,
    window::{PrimaryWindow, RawHandleWrapper, WindowMode},
};
use feathrise_editor_ui::ButtonColors;
use std::{cell::Cell, rc::Rc};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{
            DWMNCRP_ENABLED, DWMWA_NCRENDERING_POLICY, DwmExtendFrameIntoClientArea,
            DwmSetWindowAttribute,
        },
        Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
            ScreenToClient,
        },
    },
    UI::{
        Controls::MARGINS,
        HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

const SUBCLASS: usize = 0x46544348;
// 固定矩形用消息值传递，绝不把跨进程指针交给宿主；只接受真实可见的直接作者子窗口。
const RECT_MESSAGE: u32 = WM_APP + 0x461;
const VISIBLE_MESSAGE: u32 = RECT_MESSAGE + 4;

#[derive(Clone, Copy, Default)]
struct Snapshot {
    root: usize,
    root_pid: u32,
    geometry: ChromeGeometry,
    child: usize,
    child_geometry: ChromeGeometry,
    pending_child: usize,
    pending_geometry: ChromeGeometry,
    pending_mask: u8,
    fullscreen: bool,
    attached: bool,
    caption_down: Option<WindowAction>,
    input_surface: usize,
}
struct Installation {
    hwnd: usize,
    state: Rc<Cell<Snapshot>>,
    raw: usize,
    published: Option<(ChromeGeometry, bool)>,
    frame_applied: bool,
    input_surface: Option<super::input_surface::InputSurface>,
    input_error: Option<String>,
}
#[derive(Default)]
struct NativeChrome {
    installation: Option<Installation>,
}

impl Drop for Installation {
    fn drop(&mut self) {
        // 输入popup与root同线程；先撤掉它，再解除root回调，避免销毁时使用迟到句柄。
        self.state.set(Snapshot {
            input_surface: 0,
            ..self.state.get()
        });
        drop(self.input_surface.take());
        let snapshot = self.state.get();
        if snapshot.attached {
            // SetWindowSubclass及其移除都在创建窗口的Winit线程执行；资源是NonSend。
            if unsafe { RemoveWindowSubclass(self.hwnd as HWND, Some(callback), SUBCLASS) } != 0 {
                self.state.set(Snapshot {
                    attached: false,
                    ..snapshot
                });
                unsafe {
                    drop(Rc::from_raw(self.raw as *const Cell<Snapshot>));
                }
            }
        }
    }
}
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ChromeNativeSync;

pub(super) fn install(app: &mut App) {
    #[cfg(feature = "validation")]
    app.init_resource::<super::resources::NativeSyncDiagnostics>();
    app.init_resource::<ChromeGeometry>()
        .insert_non_send(NativeChrome::default())
        // 在首次呈现前安装自绘框架，避免首次获取交换链图像后才改变非客户区。
        .add_systems(PreStartup, sync)
        .init_schedule(ChromeNativeSync)
        .add_systems(ChromeNativeSync, sync);
    // 与嵌入样式校正一样，在Winit的Last写回之后，仅在样式确实变化时恢复可缩放框架。
    app.world_mut()
        .resource_mut::<MainScheduleOrder>()
        .insert_after(Last, ChromeNativeSync);
    #[cfg(feature = "validation")]
    super::validation::install(app);
    #[cfg(feature = "validation")]
    super::frame_validation::install(app);
}

fn hwnd(raw: &RawHandleWrapper) -> Option<usize> {
    crate::workspace_window::primary_hwnd(raw)
}
unsafe fn valid_root(own: usize, root: usize, pid: u32) -> bool {
    if own == 0 || root == 0 || pid == 0 {
        return false;
    }
    let mut actual = 0;
    unsafe {
        IsWindow(root as HWND) != 0
            && GetWindowThreadProcessId(root as HWND, &mut actual) != 0
            && actual == pid
            && (own == root || GetAncestor(own as HWND, GA_ROOT) == root as HWND)
    }
}

fn sync(
    _main_thread: NonSendMarker,
    mut native: NonSendMut<NativeChrome>,
    geometry: Res<ChromeGeometry>,
    mut state: ResMut<ChromeState>,
    windows: Query<(&Window, &RawHandleWrapper), With<PrimaryWindow>>,
    mut buttons: Query<(
        Entity,
        &ChromeButton,
        &ButtonColors,
        &mut Interaction,
        &mut RelativeCursorPosition,
        &mut BackgroundColor,
    )>,
    #[cfg(feature = "validation")] mut diagnostics: ResMut<super::resources::NativeSyncDiagnostics>,
) {
    #[cfg(feature = "validation")]
    {
        *diagnostics = super::resources::NativeSyncDiagnostics {
            phase: "entered",
            geometry_visible: geometry.visible,
            drag: diagnostic_rect(geometry.drag),
            minimize: diagnostic_rect(geometry.minimize),
            maximize: diagnostic_rect(geometry.maximize),
            close: diagnostic_rect(geometry.close),
            ..default()
        };
    }
    let Ok((window, raw)) = windows.single() else {
        #[cfg(feature = "validation")]
        {
            diagnostics.phase = "missing_primary_window";
        }
        return;
    };
    #[cfg(feature = "validation")]
    {
        diagnostics.window_focused = window.focused;
    }
    let Some(own) = hwnd(raw) else {
        #[cfg(feature = "validation")]
        {
            diagnostics.phase = "missing_win32_handle";
        }
        return;
    };
    #[cfg(feature = "validation")]
    {
        diagnostics.own = own;
        diagnostics.parent = unsafe { GetParent(own as HWND) } as usize;
        diagnostics.ancestor = unsafe { GetAncestor(own as HWND, GA_ROOT) } as usize;
    }
    if native.installation.as_ref().is_some_and(|i| i.hwnd != own) {
        native.installation = None;
    }
    if native.installation.is_none() {
        let (root, root_pid) = match crate::workspace_window::EmbedTarget::from_env() {
            Ok(Some(target)) => (target.parent, target.pid),
            Ok(None) => (own, std::process::id()),
            Err(error) => {
                #[cfg(feature = "validation")]
                {
                    diagnostics.phase = "invalid_embed_target";
                }
                error!("标题栏拒绝无效宿主身份：{error}");
                return;
            }
        };
        // 隐藏作者在SetParent之前也可安装回调，但不能在验证父链前向外部窗口投递动作。
        let state = Rc::new(Cell::new(Snapshot {
            root,
            root_pid,
            attached: true,
            ..default()
        }));
        let pointer = Rc::into_raw(Rc::clone(&state)) as usize;
        if unsafe { SetWindowSubclass(own as HWND, Some(callback), SUBCLASS, pointer) } == 0 {
            #[cfg(feature = "validation")]
            {
                diagnostics.phase = "subclass_install_failed";
            }
            unsafe {
                drop(Rc::from_raw(pointer as *const Cell<Snapshot>));
            }
            error!(
                "无法安装编辑器自绘窗口框架：{}",
                std::io::Error::last_os_error()
            );
            return;
        }
        native.installation = Some(Installation {
            hwnd: own,
            state,
            raw: pointer,
            published: None,
            frame_applied: false,
            input_surface: None,
            input_error: None,
        });
    }
    let installed = native.installation.as_mut().unwrap();
    let before = installed.state.get();
    #[cfg(feature = "validation")]
    {
        diagnostics.root = before.root;
        diagnostics.root_pid = before.root_pid;
        unsafe { GetWindowThreadProcessId(before.root as HWND, &mut diagnostics.actual_root_pid) };
        diagnostics.caption_down = before.caption_down.map(diagnostic_action);
    }
    let fullscreen = !matches!(window.mode, WindowMode::Windowed);
    installed.state.set(Snapshot {
        geometry: *geometry,
        fullscreen,
        ..before
    });
    let root_valid = unsafe { valid_root(own, before.root, before.root_pid) };
    #[cfg(feature = "validation")]
    {
        diagnostics.valid_root = root_valid;
        diagnostics.phase = "invalid_root";
    }
    if !root_valid {
        return;
    }
    let root = before.root as HWND;
    state.maximized = unsafe { IsZoomed(root) } != 0;
    let minimized = unsafe { IsIconic(root) } != 0;
    if state.minimized != minimized {
        debug!(root = before.root, minimized, "编辑器原生最小化状态已变化");
        state.minimized = minimized;
    }
    state.focused = unsafe { GetForegroundWindow() == root && IsIconic(root) == 0 };
    #[cfg(feature = "validation")]
    {
        diagnostics.focused = state.focused;
        diagnostics.foreground = unsafe { GetForegroundWindow() } as usize;
    }
    let mut cursor = POINT::default();
    let cursor_valid =
        unsafe { GetCursorPos(&mut cursor) != 0 && ScreenToClient(own as HWND, &mut cursor) != 0 };
    let held =
        unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x01) } < 0;
    #[cfg(feature = "validation")]
    {
        diagnostics.cursor_valid = cursor_valid;
        diagnostics.cursor = [cursor.x, cursor.y];
        diagnostics.held = held;
        diagnostics.capture = unsafe {
            windows_sys::Win32::UI::Input::KeyboardAndMouse::GetCapture()
        } as usize;
    }
    for (_entity, button, colors, mut interaction, mut relative, mut background) in &mut buttons {
        #[cfg(feature = "validation")]
        {
            diagnostics.query_matches += 1;
        }
        let ChromeButton::Window(action) = button else {
            continue;
        };
        #[cfg(feature = "validation")]
        let (before_interaction, background_before) = (
            diagnostic_interaction(*interaction),
            diagnostic_color(background.0),
        );
        let rect = match action {
            WindowAction::Minimize => geometry.minimize,
            WindowAction::Maximize => geometry.maximize,
            WindowAction::Close => geometry.close,
        };
        let (wanted, position, color) = caption_feedback(
            rect,
            state.focused && geometry.visible,
            cursor_valid.then_some((cursor.x, cursor.y)),
            held,
            before.caption_down == Some(*action),
            colors,
        );
        interaction.set_if_neq(wanted);
        relative.set_if_neq(position);
        background.set_if_neq(BackgroundColor(color));
        #[cfg(feature = "validation")]
        {
            diagnostics.caption_matches += 1;
            let index = match action {
                WindowAction::Minimize => 0,
                WindowAction::Maximize => 1,
                WindowAction::Close => 2,
            };
            diagnostics.buttons[index] = Some(super::resources::NativeCaptionDiagnostics {
                action: diagnostic_action(*action),
                entity: _entity.to_bits(),
                rect: diagnostic_rect(rect),
                before_interaction,
                wanted: diagnostic_interaction(wanted),
                relative_over: relative.cursor_over(),
                relative_normalized: relative.normalized.map(|point| point.to_array()),
                background_before,
                background_after: diagnostic_color(background.0),
                colors: [
                    diagnostic_color(colors.normal),
                    diagnostic_color(colors.hovered),
                    diagnostic_color(colors.pressed),
                ],
            });
        }
    }
    #[cfg(feature = "validation")]
    {
        diagnostics.phase = "feedback_written";
    }
    // 捕获只由原生释放/取消消息结束；不能用异步按键状态抢先清除尚未处理的快速松开。
    if own == before.root && !fullscreen {
        unsafe {
            let style = GetWindowLongPtrW(root, GWL_STYLE) as u32;
            // 保留系统标题能力位以支持移动循环、最大化拖出还原和贴靠；WM_NCCALCSIZE负责移除原生外观。
            let desired =
                style | WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
            if desired != style || !installed.frame_applied {
                SetWindowLongPtrW(root, GWL_STYLE, desired as isize);
                let policy = DWMNCRP_ENABLED;
                DwmSetWindowAttribute(
                    root,
                    DWMWA_NCRENDERING_POLICY as u32,
                    &policy as *const _ as *const _,
                    std::mem::size_of_val(&policy) as u32,
                );
                let margin = MARGINS {
                    cxLeftWidth: 1,
                    cxRightWidth: 1,
                    cyTopHeight: 1,
                    cyBottomHeight: 1,
                };
                DwmExtendFrameIntoClientArea(root, &margin);
                SetWindowPos(
                    root,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
                installed.frame_applied = true;
            }
        }
    }
    if own == before.root {
        if installed.input_surface.is_none() && installed.input_error.is_none() {
            match unsafe { super::input_surface::InputSurface::new(root) } {
                Ok(surface) => {
                    installed.state.set(Snapshot {
                        input_surface: surface.hwnd() as usize,
                        ..installed.state.get()
                    });
                    installed.input_surface = Some(surface);
                }
                Err(error) => {
                    error!("无法创建编辑器原生命中层：{error}");
                    installed.input_error = Some(error);
                }
            }
        }
        let latest = installed.state.get();
        if latest.child != 0 && unsafe {
            IsWindow(latest.child as HWND) == 0
                || GetAncestor(latest.child as HWND, GA_PARENT) != root
        } {
            // 原关闭事务销毁/脱离作者后，正常退役旧来源；隐藏但有效的作者仍保留，供最小化恢复。
            installed.state.set(Snapshot {
                child: 0,
                child_geometry: ChromeGeometry::default(),
                pending_child: 0,
                pending_geometry: ChromeGeometry::default(),
                pending_mask: 0,
                ..latest
            });
        }
        let latest = installed.state.get();
        if let Some(surface) = installed.input_surface.as_mut() {
            let result = unsafe {
                surface.sync(latest.child as HWND, latest.child_geometry, fullscreen)
            };
            if let Err(error) = result {
                if installed.input_error.as_ref() != Some(&error) {
                    error!("编辑器原生命中层同步失败：{error}");
                }
                installed.input_error = Some(error);
            } else {
                installed.input_error = None;
            }
        }
    } else {
        // 作者始终保留完整客户区绘制。仅发布输入几何，不再挖绘制孔或跨进程猜测补底颜色。
        let visible = unsafe { IsWindowVisible(own as HWND) } != 0;
        if installed.published != Some((*geometry, visible)) {
            let rects = [
                geometry.drag,
                geometry.minimize,
                geometry.maximize,
                geometry.close,
            ];
            let all_sent = rects.iter().enumerate().all(|(index, rect)| {
                pack_rect(*rect).is_some_and(|packed|
                unsafe {PostMessageW(root,RECT_MESSAGE+index as u32,own,packed as isize)}!=0)
            });
            if all_sent
                && unsafe {
                    PostMessageW(
                        root,
                        VISIBLE_MESSAGE,
                        own,
                        (visible && geometry.visible) as isize,
                    )
                } != 0
            {
                installed.published = Some((*geometry, visible));
            }
        }
    }
}

pub(super) fn request(world: &mut World, action: WindowAction) {
    let Some(native) = world.get_non_send::<NativeChrome>() else {
        return;
    };
    let Some(installed) = native.installation.as_ref() else {
        return;
    };
    let snapshot = installed.state.get();
    if !unsafe { valid_root(installed.hwnd, snapshot.root, snapshot.root_pid) } {
        return;
    }
    let root = snapshot.root as HWND;
    unsafe {
        post_action(root, action);
    }
}

unsafe fn post_action(root: HWND, action: WindowAction) {
    unsafe {
        match action {
            WindowAction::Minimize => {
                PostMessageW(root, WM_SYSCOMMAND, SC_MINIMIZE as usize, 0);
            }
            WindowAction::Maximize => {
                PostMessageW(
                    root,
                    WM_SYSCOMMAND,
                    if IsZoomed(root) != 0 {
                        SC_RESTORE
                    } else {
                        SC_MAXIMIZE
                    } as usize,
                    0,
                );
            }
            // 只请求关闭。宿主转交原保存全部/放弃/取消事务，不能发送AppExit或DestroyWindow。
            WindowAction::Close => {
                PostMessageW(root, WM_CLOSE, 0, 0);
            }
        }
    }
}

fn point(value: LPARAM) -> POINT {
    POINT {
        x: (value as u16 as i16) as i32,
        y: ((value as usize >> 16) as u16 as i16) as i32,
    }
}
fn packed_point(point: POINT) -> LPARAM {
    ((point.x as u16 as u32) | ((point.y as u16 as u32) << 16)) as isize
}
fn pack_rect(rect: PixelRect) -> Option<u64> {
    let values = [rect.left, rect.top, rect.right, rect.bottom];
    if values.iter().any(|v| !(0..=u16::MAX as i32).contains(v)) {
        return None;
    }
    Some(
        values
            .iter()
            .enumerate()
            .fold(0, |v, (i, x)| v | ((*x as u64) << (i * 16))),
    )
}
fn unpack_rect(value: LPARAM) -> PixelRect {
    let value = value as u64;
    PixelRect {
        left: (value & 65535) as i32,
        top: ((value >> 16) & 65535) as i32,
        right: ((value >> 32) & 65535) as i32,
        bottom: ((value >> 48) & 65535) as i32,
    }
}

fn edge_hit(x: i32, y: i32, width: i32, height: i32, border: i32) -> Option<u32> {
    if x < 0 || y < 0 || x >= width || y >= height {
        return None;
    }
    let left = x < border;
    let right = x >= width - border;
    let top = y < border;
    let bottom = y >= height - border;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(HTTOPLEFT),
        (_, true, true, _) => Some(HTTOPRIGHT),
        (true, _, _, true) => Some(HTBOTTOMLEFT),
        (_, true, _, true) => Some(HTBOTTOMRIGHT),
        (true, _, _, _) => Some(HTLEFT),
        (_, true, _, _) => Some(HTRIGHT),
        (_, _, true, _) => Some(HTTOP),
        (_, _, _, true) => Some(HTBOTTOM),
        _ => None,
    }
}
fn caption_hit(geometry: ChromeGeometry, x: i32, y: i32) -> u32 {
    if !geometry.visible {
        return HTCLIENT;
    }
    if geometry.minimize.contains(x, y) {
        HTMINBUTTON
    } else if geometry.maximize.contains(x, y) {
        HTMAXBUTTON
    } else if geometry.close.contains(x, y) {
        HTCLOSE
    } else if geometry.drag.contains(x, y) {
        HTCAPTION
    } else {
        HTCLIENT
    }
}
fn move_or_resize_hit(hit: u32) -> bool {
    hit == HTCAPTION || (HTLEFT..=HTBOTTOMRIGHT).contains(&hit)
}
unsafe fn native_hit(window: HWND, snapshot: Snapshot, value: LPARAM) -> u32 {
    if snapshot.fullscreen {
        return HTCLIENT;
    }
    let mut cursor = point(value);
    let mut outer = RECT::default();
    let root = snapshot.root as HWND;
    unsafe {
        if window == root && GetWindowRect(root, &mut outer) != 0 && IsZoomed(root) == 0 {
            let dpi = GetDpiForWindow(root).max(96);
            let border = (GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
                + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi))
            .max(4);
            if let Some(hit) = edge_hit(
                cursor.x - outer.left,
                cursor.y - outer.top,
                outer.right - outer.left,
                outer.bottom - outer.top,
                border,
            ) {
                return hit;
            }
        }
        if ScreenToClient(window, &mut cursor) == 0 {
            return HTCLIENT;
        }
        let geometry = if window == root
            && snapshot.child != 0
            && IsWindowVisible(snapshot.child as HWND) != 0
            && GetAncestor(snapshot.child as HWND, GA_PARENT) == root
        {
            snapshot.child_geometry
        } else {
            snapshot.geometry
        };
        let hit = caption_hit(geometry, cursor.x, cursor.y);
        // 系统拖动由宿主线程的独立输入层承接；作者完整绘制，也不能获得自己的移动入口。
        // 三键仍保留原生命中与根窗口动作。
        if window != root && move_or_resize_hit(hit) { HTCLIENT } else { hit }
    }
}
unsafe fn client_mouse(window: HWND, message: u32, value: LPARAM) {
    let mut cursor = point(value);
    unsafe {
        if ScreenToClient(window, &mut cursor) != 0 {
            PostMessageW(
                window,
                message,
                if message == WM_LBUTTONDOWN { 1 } else { 0 },
                packed_point(cursor),
            );
        }
    }
}

unsafe extern "system" fn callback(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // Cell只在该HWND所属Winit线程读取/写入；进入Win32前已经复制快照，重入不持有可变借用。
    let state = unsafe { &*(data as *const Cell<Snapshot>) };
    let snapshot = state.get();
    if message == WM_NCDESTROY {
        state.set(Snapshot {
            attached: false,
            ..snapshot
        });
        unsafe {
            RemoveWindowSubclass(window, Some(callback), SUBCLASS);
        }
        let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
        unsafe {
            drop(Rc::from_raw(data as *const Cell<Snapshot>));
        }
        return result;
    }
    let child = window as usize != snapshot.root;
    if child && (
        message == WM_NCLBUTTONDOWN && move_or_resize_hit(wparam as u32)
        || message == WM_SYSCOMMAND && matches!((wparam as u32) & 0xfff0, SC_MOVE | SC_SIZE)
    ) {
        // 先于父链/PID校验保护已声明的嵌入窗口：样式改写或区域孔缺失时也不能
        // 落入Winit/DefWindowProc的子窗口移动循环。不跨进程补发拖动消息。
        return 0;
    }
    if !unsafe { valid_root(window as usize, snapshot.root, snapshot.root_pid) } {
        if child && message == WM_NCHITTEST {
            return HTCLIENT as LRESULT;
        }
        return unsafe { DefSubclassProc(window, message, wparam, lparam) };
    }
    let root = snapshot.root as HWND;
    if (RECT_MESSAGE..=VISIBLE_MESSAGE).contains(&message) && window == root {
        let child = wparam as HWND;
        if !child.is_null()
            && unsafe { IsWindowVisible(child) != 0 && GetAncestor(child, GA_PARENT) == root }
        {
            let mut next = snapshot;
            if message == RECT_MESSAGE || next.pending_child != wparam {
                next.pending_child = wparam;
                next.pending_geometry = ChromeGeometry::default();
                next.pending_mask = 0;
            }
            let rect = unpack_rect(lparam);
            let index = message - RECT_MESSAGE;
            match index {
                0 => next.pending_geometry.drag = rect,
                1 => next.pending_geometry.minimize = rect,
                2 => next.pending_geometry.maximize = rect,
                3 => next.pending_geometry.close = rect,
                _ => {}
            }
            if index < 4 {
                next.pending_mask |= 1 << index;
            } else if next.pending_mask == 15 {
                // 四矩形与显隐完整到齐才采用，跨窗口只传输入几何，不传画面颜色。
                next.pending_geometry.visible = lparam != 0;
                next.child = next.pending_child;
                next.child_geometry = next.pending_geometry;
                next.pending_mask = 0;
            }
            state.set(next);
            if next.input_surface != 0 {
                // 与输入几何在同一原生线程采用，不等下一次Bevy帧，也不改变作者的绘制区域。
                let _ = unsafe {
                    super::input_surface::update(root, next.input_surface as HWND,
                        next.child as HWND, next.child_geometry, next.fullscreen)
                };
            }
        }
        return 0;
    }
    match message {
        WM_WINDOWPOSCHANGED | WM_SIZE | WM_DPICHANGED | WM_SHOWWINDOW | WM_ENABLE if window == root => {
            let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
            if snapshot.input_surface != 0 {
                // 系统move/size循环会暂停普通App帧；owned输入popup必须在原生位置消息中立即跟随。
                let _ = unsafe {
                    super::input_surface::reposition(root, snapshot.input_surface as HWND)
                };
            }
            result
        }
        WM_NCCALCSIZE if window == root && !snapshot.fullscreen => {
            // 最小化时保留系统的零客户区语义，不能把图标占位矩形作为作者布局尺寸。
            if unsafe { IsIconic(root) } != 0 && lparam != 0 {
                let rect = unsafe { &mut *(lparam as *mut RECT) };
                rect.right = rect.left;
                rect.bottom = rect.top;
            } else if unsafe { IsZoomed(root) } != 0 && lparam != 0 {
                // 带系统标题能力的最大化外框会伸出工作区；客户区必须显式落在工作区内，避免自绘栏被裁边。
                let monitor = unsafe { MonitorFromWindow(root, MONITOR_DEFAULTTONEAREST) };
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..unsafe { std::mem::zeroed() }
                };
                if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
                    unsafe {
                        *(lparam as *mut RECT) = info.rcWork;
                    }
                }
            }
            0
        }
        WM_NCPAINT if window == root && !snapshot.fullscreen => 0,
        WM_NCACTIVATE if window == root && !snapshot.fullscreen => 1,
        WM_GETMINMAXINFO if window == root && !snapshot.fullscreen => {
            let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
            let monitor = unsafe { MonitorFromWindow(root, MONITOR_DEFAULTTONEAREST) };
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..unsafe { std::mem::zeroed() }
            };
            if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 && lparam != 0 {
                let limits = unsafe { &mut *(lparam as *mut MINMAXINFO) };
                limits.ptMaxPosition = POINT {
                    x: info.rcWork.left - info.rcMonitor.left,
                    y: info.rcWork.top - info.rcMonitor.top,
                };
                limits.ptMaxSize = POINT {
                    x: info.rcWork.right - info.rcWork.left,
                    y: info.rcWork.bottom - info.rcWork.top,
                };
            }
            result
        }
        WM_NCHITTEST => unsafe { native_hit(window, snapshot, lparam) as i32 as isize },
        WM_NCMOUSEMOVE if matches!(wparam as u32, HTMINBUTTON | HTMAXBUTTON | HTCLOSE) => {
            unsafe {
                client_mouse(window, WM_MOUSEMOVE, lparam);
            }
            if window != root {
                unsafe {
                    PostMessageW(root, message, wparam, lparam);
                }
                return 0;
            }
            // Windows11通过真实顶层HTMAXBUTTON提供贴靠菜单；仍由系统处理悬停。
            unsafe { DefSubclassProc(window, message, wparam, lparam) }
        }
        WM_NCLBUTTONDOWN if matches!(wparam as u32, HTMINBUTTON | HTMAXBUTTON | HTCLOSE) => {
            // 在原生输入线程当场捕获，子进程的快速松开也不会落在异步转发之前。
            // 只在原按钮内松开时投递一次系统命令，普通作者按钮的输入协议完全保留。
            let action = match wparam as u32 {
                HTMINBUTTON => WindowAction::Minimize,
                HTMAXBUTTON => WindowAction::Maximize,
                _ => WindowAction::Close,
            };
            state.set(Snapshot {
                caption_down: Some(action),
                ..snapshot
            });
            unsafe {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::SetCapture(window);
            }
            0
        }
        WM_LBUTTONUP | WM_NCLBUTTONUP if snapshot.caption_down.is_some() => {
            let action = snapshot.caption_down.unwrap();
            let mut cursor = POINT::default();
            let inside = unsafe {
                GetCursorPos(&mut cursor) != 0 && ScreenToClient(window, &mut cursor) != 0
            };
            let rect = match action {
                WindowAction::Minimize => snapshot.geometry.minimize,
                WindowAction::Maximize => snapshot.geometry.maximize,
                WindowAction::Close => snapshot.geometry.close,
            };
            state.set(Snapshot {
                caption_down: None,
                ..snapshot
            });
            unsafe {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
            }
            if inside && snapshot.geometry.visible && rect.contains(cursor.x, cursor.y) {
                unsafe {
                    post_action(root, action);
                }
            }
            0
        }
        WM_CANCELMODE | WM_KILLFOCUS | WM_CAPTURECHANGED if snapshot.caption_down.is_some() => {
            state.set(Snapshot {
                caption_down: None,
                ..snapshot
            });
            if unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetCapture() } == window {
                unsafe {
                    windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                }
            }
            unsafe { DefSubclassProc(window, message, wparam, lparam) }
        }
        WM_KEYDOWN if wparam == 0x1b && snapshot.caption_down.is_some() => {
            state.set(Snapshot {
                caption_down: None,
                ..snapshot
            });
            unsafe {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
            }
            0
        }
        WM_NCLBUTTONDOWN
            if window == root && move_or_resize_hit(wparam as u32) =>
        {
            // Winit在HTCAPTION按下后追加(0,0)的假鼠标移动来提前结束其默认等待；
            // 该消息可能进入已经启动的系统移动循环，导致窗口跳到屏幕左上。
            // 此处仅将原生移动入口直接交给系统，尺寸/退出循环等其余消息仍走Winit。
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_NCLBUTTONDBLCLK if wparam as u32 == HTCAPTION => {
            unsafe {
                post_action(root, WindowAction::Maximize);
            }
            0
        }
        WM_NCRBUTTONUP if window != root => {
            unsafe {
                PostMessageW(root, WM_SYSCOMMAND, SC_MOUSEMENU as usize, lparam);
            }
            0
        }
        WM_SYSKEYDOWN if window != root && (wparam == 0x20 || wparam == 0x73) => {
            unsafe {
                PostMessageW(
                    root,
                    WM_SYSCOMMAND,
                    if wparam == 0x73 { SC_CLOSE } else { SC_KEYMENU } as usize,
                    if wparam == 0x20 { 0x20 } else { 0 },
                );
            }
            0
        }
        _ => unsafe { DefSubclassProc(window, message, wparam, lparam) },
    }
}

/// 非客户区鼠标不会可靠更新Winit客户区光标；反馈必须与原生命中采用同一位置。
/// 此阶段晚于Update按钮着色，因此同步原ButtonColors的当前背景，提取可立即绘制。
/// 不合成输入/点击，不改变原生捕获和松开动作；拖出、失焦或隐藏时回到透明普通态。
fn caption_feedback(
    rect: PixelRect,
    available: bool,
    cursor: Option<(i32, i32)>,
    held: bool,
    captured: bool,
    colors: &ButtonColors,
) -> (Interaction, RelativeCursorPosition, Color) {
    let over = available && cursor.is_some_and(|(x, y)| rect.contains(x, y));
    let normalized = cursor
        .filter(|_| available && rect.right > rect.left && rect.bottom > rect.top)
        .map(|(x, y)| {
            Vec2::new(
                (x as f32 - rect.left as f32) / (rect.right as f32 - rect.left as f32),
                (y as f32 - rect.top as f32) / (rect.bottom as f32 - rect.top as f32),
            ) - Vec2::splat(0.5)
        });
    let interaction = if over && held && captured {
        Interaction::Pressed
    } else if over {
        Interaction::Hovered
    } else {
        Interaction::None
    };
    let color = match interaction {
        Interaction::Pressed => colors.pressed,
        Interaction::Hovered => colors.hovered,
        Interaction::None => colors.normal,
    };
    (
        interaction,
        RelativeCursorPosition {
            cursor_over: over,
            normalized,
        },
        color,
    )
}

#[cfg(feature = "validation")]
fn diagnostic_rect(rect: PixelRect) -> [i32; 4] {
    [rect.left, rect.top, rect.right, rect.bottom]
}

#[cfg(feature = "validation")]
fn diagnostic_action(action: WindowAction) -> &'static str {
    match action {
        WindowAction::Minimize => "minimize",
        WindowAction::Maximize => "maximize",
        WindowAction::Close => "close",
    }
}

#[cfg(feature = "validation")]
fn diagnostic_interaction(interaction: Interaction) -> &'static str {
    match interaction {
        Interaction::None => "none",
        Interaction::Hovered => "hovered",
        Interaction::Pressed => "pressed",
    }
}

#[cfg(feature = "validation")]
fn diagnostic_color(color: Color) -> [f32; 4] {
    let color = color.to_srgba();
    [color.red, color.green, color.blue, color.alpha]
}

#[cfg(test)]
#[path = "native_feedback_tests.rs"]
mod feedback_tests;

#[cfg(test)]
#[path = "native_drag_tests.rs"]
mod drag_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn borders_cover_all_eight_directions_and_keep_center_client() {
        for (x, y, expected) in [
            (0, 0, HTTOPLEFT),
            (400, 0, HTTOP),
            (799, 0, HTTOPRIGHT),
            (0, 300, HTLEFT),
            (799, 300, HTRIGHT),
            (0, 599, HTBOTTOMLEFT),
            (400, 599, HTBOTTOM),
            (799, 599, HTBOTTOMRIGHT),
        ] {
            assert_eq!(edge_hit(x, y, 800, 600, 8), Some(expected));
        }
        assert_eq!(edge_hit(400, 300, 800, 600, 8), None);
    }
    #[test]
    fn physical_layout_has_distinct_controls_drag_and_ordinary_ui() {
        let geometry = ChromeGeometry {
            visible: true,
            drag: PixelRect {
                left: 200,
                top: 0,
                right: 600,
                bottom: 36,
            },
            minimize: PixelRect {
                left: 668,
                top: 0,
                right: 712,
                bottom: 36,
            },
            maximize: PixelRect {
                left: 712,
                top: 0,
                right: 756,
                bottom: 36,
            },
            close: PixelRect {
                left: 756,
                top: 0,
                right: 800,
                bottom: 36,
            },
        };
        for (x, y, expected) in [
            (50, 15, HTCLIENT),
            (300, 15, HTCAPTION),
            (680, 15, HTMINBUTTON),
            (730, 15, HTMAXBUTTON),
            (780, 15, HTCLOSE),
            (780, 50, HTCLIENT),
        ] {
            assert_eq!(caption_hit(geometry, x, y), expected);
        }
        assert_eq!(
            caption_hit(
                ChromeGeometry {
                    visible: false,
                    ..geometry
                },
                730,
                15
            ),
            HTCLIENT
        );
    }
    #[test]
    fn child_geometry_messages_are_pointer_free_and_reject_overflow() {
        let rect = PixelRect {
            left: 100,
            top: 0,
            right: 3840,
            bottom: 54,
        };
        assert_eq!(unpack_rect(pack_rect(rect).unwrap() as isize), rect);
        assert!(pack_rect(PixelRect { left: -1, ..rect }).is_none());
        assert!(
            pack_rect(PixelRect {
                right: 65536,
                ..rect
            })
            .is_none()
        );
        assert_eq!(point(packed_point(POINT { x: -1920, y: -1080 })).x, -1920);
    }
}
