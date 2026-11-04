//! 宿主线程拥有的无绘制输入层：只接四边缩放与标题空白，作者窗口始终完整绘制，不挖孔或跨进程补色。
//! NOREDIRECTIONBITMAP弹窗没有可见表面；原始NC按下在同一线程同步进入root，后续移动/松开由系统队列处理。

use super::types::{ChromeGeometry, PixelRect};
use std::{cell::Cell, rc::Rc, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, SetLastError, WPARAM},
    Graphics::Gdi::{
        BeginPaint, ClientToScreen, CombineRgn, CreateRectRgn, DeleteObject, EndPaint,
        GetWindowRgnBox, HRGN, PAINTSTRUCT, RGN_OR, SetWindowRgn,
    },
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        HiDpi::{
            AreDpiAwarenessContextsEqual, GetDpiForWindow, GetSystemMetricsForDpi,
            GetWindowDpiAwarenessContext,
        },
        Input::KeyboardAndMouse::IsWindowEnabled,
        WindowsAndMessaging::*,
    },
};

const CLASS_NAME: *const u16 = windows_sys::w!("Feathrise.Chrome.InputSurface");
static CLASS: OnceLock<Result<u16, u32>> = OnceLock::new();

#[derive(Clone, Copy, PartialEq, Eq)]
struct Shape {
    width: i32,
    height: i32,
    dpi: u32,
    border: i32,
    drag: PixelRect,
}

#[derive(Clone, Copy)]
struct SurfaceData {
    root: HWND,
    surface: HWND,
    pid: u32,
    thread: u32,
    child: HWND,
    child_pid: u32,
    child_thread: u32,
    geometry: ChromeGeometry,
    fullscreen: bool,
    applied: Option<Shape>,
    attached: bool,
}

/// Rc使所有权固定在创建HWND的Winit线程；原生回调另持一份引用，重入和owner销毁均不会留下悬垂指针。
pub(super) struct InputSurface {
    state: Rc<Cell<SurfaceData>>,
}

impl InputSurface {
    pub(super) unsafe fn new(root: HWND) -> Result<Self, String> {
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(root, &mut pid) };
        if root.is_null()
            || unsafe { IsWindow(root) } == 0
            || pid != std::process::id()
            || thread != unsafe { GetCurrentThreadId() }
            || unsafe { GetAncestor(root, GA_ROOT) } != root
        {
            return Err("输入层必须由主窗口所属进程与原生线程创建".into());
        }
        class()?;
        let state = Rc::new(Cell::new(SurfaceData {
            root,
            surface: std::ptr::null_mut(),
            pid,
            thread,
            child: std::ptr::null_mut(),
            child_pid: 0,
            child_thread: 0,
            geometry: ChromeGeometry::default(),
            fullscreen: false,
            applied: None,
            attached: false,
        }));
        // 先隐藏创建。区域与位置完整采用后才NOACTIVATE显露，避免初创抢走作者焦点。
        let surface = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                CLASS_NAME,
                std::ptr::null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                root,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                Rc::as_ptr(&state).cast(),
            )
        };
        if surface.is_null() {
            return Err(failure("无法创建主窗口透明输入层"));
        }
        let result = Self { state };
        if unsafe {
            AreDpiAwarenessContextsEqual(
                GetWindowDpiAwarenessContext(root),
                GetWindowDpiAwarenessContext(surface),
            )
        } == 0
        {
            return Err("主窗口与输入层DPI模式不同，已拒绝显示输入层".into());
        }
        Ok(result)
    }

    pub(super) fn hwnd(&self) -> HWND {
        self.state.get().surface
    }

    pub(super) unsafe fn sync(
        &self,
        child: HWND,
        geometry: ChromeGeometry,
        fullscreen: bool,
    ) -> Result<(), String> {
        let data = self.state.get();
        unsafe { update(data.root, data.surface, child, geometry, fullscreen) }
    }
}

impl Drop for InputSurface {
    fn drop(&mut self) {
        let data = self.state.get();
        if data.attached
            && data.thread == unsafe { GetCurrentThreadId() }
            && unsafe { IsWindow(data.surface) } != 0
            && unsafe { GetWindowLongPtrW(data.surface, GWLP_USERDATA) } as *const Cell<SurfaceData>
                == Rc::as_ptr(&self.state)
        {
            unsafe { DestroyWindow(data.surface) };
        }
    }
}

fn class() -> Result<u16, String> {
    match *CLASS.get_or_init(|| {
        let definition = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(callback),
            hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
            lpszClassName: CLASS_NAME,
            // 无背景刷、无DComp/交换链、无投影类样式；输入层不制造任何可见像素。
            ..unsafe { std::mem::zeroed() }
        };
        let atom = unsafe { RegisterClassW(&definition) };
        if atom != 0 {
            Ok(atom)
        } else {
            Err(unsafe { GetLastError() })
        }
    }) {
        Ok(atom) => Ok(atom),
        Err(error) => Err(format!(
            "无法注册透明输入窗口类：{}",
            std::io::Error::from_raw_os_error(error as i32)
        )),
    }
}

/// 完整几何批次采用后更新；child为空会立即撤去旧输入区域，不把上一工程的拖动矩形带到开始页。
pub(super) unsafe fn update(
    root: HWND,
    surface: HWND,
    child: HWND,
    geometry: ChromeGeometry,
    fullscreen: bool,
) -> Result<(), String> {
    let state = unsafe { shared(surface)? };
    let mut data = state.get();
    if data.root != root || !unsafe { valid_root(data) } {
        unsafe { hide(surface)? };
        return Err("输入层的主窗口身份或owner已失效，已隐藏输入层".into());
    }
    let mut child_pid = 0;
    let child_thread = if child.is_null() {
        0
    } else {
        unsafe { GetWindowThreadProcessId(child, &mut child_pid) }
    };
    data.child = child;
    data.child_pid = child_pid;
    data.child_thread = child_thread;
    data.geometry = geometry;
    data.fullscreen = fullscreen;
    state.set(data);
    unsafe { reposition(root, surface) }
}

/// 在root原生位置/尺寸消息内调用，系统移动循环暂停Bevy帧时也立即跟随；纯移动不重建HRGN。
pub(super) unsafe fn reposition(root: HWND, surface: HWND) -> Result<(), String> {
    let state = unsafe { shared(surface)? };
    let data = state.get();
    if data.root != root || !unsafe { valid_root(data) } {
        unsafe { hide(surface)? };
        return Err("输入层的主窗口身份或owner已失效，已隐藏输入层".into());
    }
    if data.child.is_null() {
        return unsafe { hide(surface) };
    }
    if !unsafe { valid_child(data) } {
        unsafe { hide(surface)? };
        return Err("作者窗口来源已失效，已隐藏输入层".into());
    }
    if data.fullscreen || unsafe { IsIconic(root) } != 0 || unsafe { IsWindowEnabled(root) } == 0 {
        return unsafe { hide(surface) };
    }
    let mut bounds = RECT::default();
    let mut origin = POINT::default();
    if unsafe { GetClientRect(root, &mut bounds) } == 0
        || unsafe { ClientToScreen(root, &mut origin) } == 0
    {
        unsafe { hide(surface)? };
        return Err(failure("无法读取主窗口输入区位置"));
    }
    let width = bounds.right - bounds.left;
    let height = bounds.bottom - bounds.top;
    if width <= 0 || height <= 0 {
        return unsafe { hide(surface) };
    }
    let dpi = unsafe { GetDpiForWindow(root) }.max(96);
    let border = if unsafe { IsZoomed(root) } != 0 {
        0
    } else {
        (unsafe {
            GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
                + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
        })
        .max(4)
    };
    let shape = Shape {
        width,
        height,
        dpi,
        border,
        drag: if data.geometry.visible {
            data.geometry.drag
        } else {
            PixelRect::default()
        },
    };
    if shape.border == 0 && clipped(shape.drag, width, height).is_none() {
        return unsafe { hide(surface) };
    }
    let mut region_bounds = RECT::default();
    if data.applied != Some(shape) || unsafe { GetWindowRgnBox(surface, &mut region_bounds) } == 0 {
        // 只在几何/DPI/尺寸改变或真实区域缺失时建固定五矩形；绝不对作者HWND设置region。
        let Some(region) = (unsafe { build_region(shape) }) else {
            let error = failure("无法构造窗口输入区域");
            unsafe { hide(surface)? };
            return Err(error);
        };
        if unsafe { SetWindowRgn(surface, region, 1) } == 0 {
            unsafe {
                DeleteObject(region);
                hide(surface)?;
            }
            return Err(failure("无法采用窗口输入区域"));
        }
        let mut latest = state.get();
        latest.applied = Some(shape);
        state.set(latest); // SetWindowRgn可能重入，不能用调用前快照回写其他身份字段。
    }
    let mut actual = RECT::default();
    let positioned = unsafe { GetWindowRect(surface, &mut actual) } != 0
        && actual.left == origin.x
        && actual.top == origin.y
        && actual.right - actual.left == width
        && actual.bottom - actual.top == height;
    let visible = unsafe { IsWindowVisible(root) != 0 && IsWindowVisible(data.child) != 0 };
    let shown = unsafe { IsWindowVisible(surface) } != 0;
    if !positioned || visible != shown {
        let visibility = if visible {
            SWP_SHOWWINDOW
        } else {
            SWP_HIDEWINDOW
        };
        if unsafe {
            SetWindowPos(
                surface,
                std::ptr::null_mut(),
                origin.x,
                origin.y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE | visibility,
            )
        } == 0
        {
            unsafe { hide(surface)? };
            return Err(failure("无法同步主窗口透明输入层"));
        }
    }
    Ok(())
}

unsafe fn shared(surface: HWND) -> Result<Rc<Cell<SurfaceData>>, String> {
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(surface, &mut pid) };
    if surface.is_null()
        || unsafe { IsWindow(surface) } == 0
        || thread != unsafe { GetCurrentThreadId() }
        || pid != std::process::id()
        || unsafe { GetClassLongPtrW(surface, GCW_ATOM) } != class()? as usize
    {
        return Err("拒绝操作其他进程、线程或类型的输入窗口".into());
    }
    let pointer = unsafe { GetWindowLongPtrW(surface, GWLP_USERDATA) } as *const Cell<SurfaceData>;
    if pointer.is_null() {
        return Err("输入层已经退役".into());
    }
    unsafe { Rc::increment_strong_count(pointer) };
    let state = unsafe { Rc::from_raw(pointer) };
    let data = state.get();
    if !data.attached || data.surface != surface || data.thread != thread || data.pid != pid {
        return Err("输入层实例身份已经改变".into());
    }
    Ok(state)
}

unsafe fn valid_root(data: SurfaceData) -> bool {
    let mut pid = 0;
    unsafe {
        IsWindow(data.root) != 0
            && GetWindowThreadProcessId(data.root, &mut pid) == data.thread
            && pid == data.pid
            && data.thread == GetCurrentThreadId()
            && data.pid == std::process::id()
            && GetAncestor(data.root, GA_ROOT) == data.root
            && GetWindow(data.surface, GW_OWNER) == data.root
    }
}

unsafe fn valid_child(data: SurfaceData) -> bool {
    let mut pid = 0;
    unsafe {
        !data.child.is_null()
            && IsWindow(data.child) != 0
            && GetWindowThreadProcessId(data.child, &mut pid) == data.child_thread
            && pid == data.child_pid
            && pid != 0
            && GetAncestor(data.child, GA_PARENT) == data.root
    }
}

unsafe fn hide(surface: HWND) -> Result<(), String> {
    if unsafe { IsWindowVisible(surface) } != 0
        && unsafe {
            SetWindowPos(
                surface,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        } == 0
    {
        return Err(failure("无法隐藏失效的输入层"));
    }
    Ok(())
}

fn clipped(rect: PixelRect, width: i32, height: i32) -> Option<PixelRect> {
    let value = PixelRect {
        left: rect.left.max(0),
        top: rect.top.max(0),
        right: rect.right.min(width),
        bottom: rect.bottom.min(height),
    };
    (value.left < value.right && value.top < value.bottom).then_some(value)
}

unsafe fn build_region(shape: Shape) -> Option<HRGN> {
    let region = unsafe { CreateRectRgn(0, 0, 0, 0) };
    if region.is_null() {
        return None;
    }
    for rect in [
        PixelRect {
            left: 0,
            top: 0,
            right: shape.width,
            bottom: shape.border,
        },
        PixelRect {
            left: 0,
            top: shape.height - shape.border,
            right: shape.width,
            bottom: shape.height,
        },
        PixelRect {
            left: 0,
            top: 0,
            right: shape.border,
            bottom: shape.height,
        },
        PixelRect {
            left: shape.width - shape.border,
            top: 0,
            right: shape.width,
            bottom: shape.height,
        },
        shape.drag,
    ] {
        let Some(rect) = clipped(rect, shape.width, shape.height) else {
            continue;
        };
        let part = unsafe { CreateRectRgn(rect.left, rect.top, rect.right, rect.bottom) };
        if part.is_null() {
            unsafe { DeleteObject(region) };
            return None;
        }
        let combined = unsafe { CombineRgn(region, region, part, RGN_OR) };
        unsafe { DeleteObject(part) };
        if combined == 0 {
            unsafe { DeleteObject(region) };
            return None;
        }
    }
    Some(region)
}

fn movement(hit: WPARAM) -> bool {
    hit == HTCAPTION as usize || (HTLEFT as usize..=HTBOTTOMRIGHT as usize).contains(&hit)
}

unsafe extern "system" fn callback(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        let pointer = create.lpCreateParams as *const Cell<SurfaceData>;
        if pointer.is_null() || unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } != 0 {
            return 0;
        }
        unsafe { SetLastError(0) };
        if unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, pointer as isize) } == 0
            && unsafe { GetLastError() } != 0
        {
            return 0;
        }
        unsafe { Rc::increment_strong_count(pointer) };
        let data = unsafe { &*pointer };
        data.set(SurfaceData {
            surface: window,
            attached: true,
            ..data.get()
        });
        return 1;
    }
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const Cell<SurfaceData>;
    if message == WM_NCDESTROY && !pointer.is_null() {
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, 0) };
        let state = unsafe { Rc::from_raw(pointer) };
        state.set(SurfaceData {
            attached: false,
            ..state.get()
        });
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    // 无论宿主身份是否仍有效，输入窗口都不激活、不绘制、不独自进入move/size loop。
    match message {
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_ERASEBKGND | WM_NCACTIVATE => return 1,
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = unsafe { std::mem::zeroed() };
            unsafe {
                BeginPaint(window, &mut paint);
                EndPaint(window, &paint);
            }
            return 0;
        }
        WM_NCPAINT | WM_CLOSE => return 0,
        WM_SYSCOMMAND if matches!((wparam as u32) & 0xfff0, SC_MOVE | SC_SIZE) => return 0,
        _ => (),
    }
    if matches!(
        message,
        WM_NCHITTEST | WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK | WM_NCRBUTTONDOWN | WM_NCRBUTTONUP
    ) {
        if pointer.is_null() {
            return if message == WM_NCHITTEST {
                HTNOWHERE as LRESULT
            } else {
                0
            };
        }
        unsafe { Rc::increment_strong_count(pointer) };
        let state = unsafe { Rc::from_raw(pointer) };
        let data = state.get(); // 进入SendMessage前只复制，不跨root重入持有借用或裸Rc所有权。
        if !data.attached
            || data.surface != window
            || data.fullscreen
            || !unsafe { valid_root(data) && valid_child(data) && IsWindowEnabled(data.root) != 0 }
        {
            return if message == WM_NCHITTEST {
                HTNOWHERE as LRESULT
            } else {
                0
            };
        }
        if message == WM_NCHITTEST {
            let hit = unsafe { SendMessageW(data.root, message, wparam, lparam) };
            return if movement(hit as usize) {
                hit
            } else {
                HTNOWHERE as LRESULT
            };
        }
        if movement(wparam) {
            // 这里只在同一Winit线程交原始NC入口；原GetMessagePos/屏幕坐标保持，绝不重放move/up。
            unsafe { SendMessageW(data.root, message, wparam, lparam) };
        }
        return 0;
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn failure(context: &str) -> String {
    format!("{context}：{}", std::io::Error::last_os_error())
}

#[cfg(test)]
#[path = "input_surface_tests.rs"]
mod tests;
