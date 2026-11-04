//! 隐藏自有HWND上的生产输入层回归：真实消息经过原WinProc，哨兵截住root系统循环，不注入桌面键鼠。
use super::*;
use std::cell::RefCell;
use windows_sys::Win32::{
    Graphics::Gdi::{GetWindowRgn, PtInRegion},
    UI::Shell::{DefSubclassProc, SetWindowSubclass},
};

const PROBE: usize = 0x46544950;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Event {
    message: u32,
    wparam: usize,
    lparam: isize,
    position: u32,
    thread: u32,
}

#[derive(Default)]
struct Probe {
    events: RefCell<Vec<Event>>,
    surface: Cell<HWND>,
}

unsafe extern "system" fn root_probe(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    raw: usize,
) -> LRESULT {
    let probe = unsafe { &*(raw as *const Probe) };
    if matches!(
        message,
        WM_NCHITTEST
            | WM_NCLBUTTONDOWN
            | WM_NCLBUTTONDBLCLK
            | WM_NCRBUTTONDOWN
            | WM_NCRBUTTONUP
            | WM_MOUSEMOVE
            | WM_LBUTTONUP
            | WM_NCLBUTTONUP
            | WM_SYSCOMMAND
    ) {
        probe.events.borrow_mut().push(Event {
            message,
            wparam,
            lparam,
            position: unsafe { GetMessagePos() },
            thread: unsafe { GetCurrentThreadId() },
        });
        return if message == WM_NCHITTEST {
            HTCAPTION as LRESULT
        } else {
            0x4654
        };
    }
    let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
    if matches!(message, WM_WINDOWPOSCHANGED | WM_SIZE) && !probe.surface.get().is_null() {
        // 与正式root相同：在真实原生消息内跟随，不等下一帧sync。
        let _ = unsafe { reposition(window, probe.surface.get()) };
    }
    result
}

struct OwnedWindow(HWND);
impl OwnedWindow {
    fn new(parent: Option<HWND>) -> Self {
        let window = unsafe {
            CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                std::ptr::null(),
                if parent.is_some() { WS_CHILD } else { WS_POPUP },
                if parent.is_some() { 0 } else { 250 },
                if parent.is_some() { 0 } else { 200 },
                1200,
                800,
                parent.unwrap_or(std::ptr::null_mut()),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!window.is_null());
        Self(window)
    }
}
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) };
    }
}

// 先销毁输入层和作者，再销毁root，最后释放其回调哨兵；owner自动销毁输入层也保持Rc正确。
struct Fixture {
    surface: InputSurface,
    child: OwnedWindow,
    root: OwnedWindow,
    probe: Box<Probe>,
}
impl Fixture {
    fn new() -> Self {
        let root = OwnedWindow::new(None);
        let child = OwnedWindow::new(Some(root.0));
        let probe = Box::<Probe>::default();
        assert_ne!(
            unsafe {
                SetWindowSubclass(
                    root.0,
                    Some(root_probe),
                    PROBE,
                    (&*probe as *const Probe) as usize,
                )
            },
            0
        );
        let surface = unsafe { InputSurface::new(root.0) }.unwrap();
        probe.surface.set(surface.hwnd());
        unsafe { surface.sync(child.0, geometry(), false) }.unwrap();
        Self {
            surface,
            child,
            root,
            probe,
        }
    }
    fn send(&self, message: u32, wparam: usize, lparam: isize) -> LRESULT {
        unsafe { SendMessageW(self.surface.hwnd(), message, wparam, lparam) }
    }
}

fn geometry() -> ChromeGeometry {
    ChromeGeometry {
        visible: true,
        drag: PixelRect {
            left: 200,
            top: 100,
            right: 600,
            bottom: 180,
        },
        minimize: PixelRect {
            left: 668,
            top: 100,
            right: 712,
            bottom: 180,
        },
        maximize: PixelRect {
            left: 712,
            top: 100,
            right: 756,
            bottom: 180,
        },
        close: PixelRect {
            left: 756,
            top: 100,
            right: 800,
            bottom: 180,
        },
    }
}

struct Region(HRGN);
impl Drop for Region {
    fn drop(&mut self) {
        unsafe { DeleteObject(self.0) };
    }
}

#[test]
fn input_region_only_covers_edges_and_drag_without_clipping_the_author_window() {
    let fixture = Fixture::new();
    let hwnd = fixture.surface.hwnd();
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let extended = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    assert_ne!(style & WS_POPUP, 0);
    assert_eq!(style & (WS_CHILD | WS_CAPTION | WS_THICKFRAME), 0);
    assert_eq!(
        extended & (WS_EX_NOREDIRECTIONBITMAP | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW),
        WS_EX_NOREDIRECTIONBITMAP | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW
    );
    assert_eq!(
        extended & (WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT),
        0
    );
    assert_eq!(unsafe { GetWindow(hwnd, GW_OWNER) }, fixture.root.0);
    assert_eq!(
        unsafe { IsWindowVisible(hwnd) },
        0,
        "隐藏夹具不能抢桌面焦点或显示输入层"
    );
    let region = Region(unsafe { CreateRectRgn(0, 0, 0, 0) });
    assert_ne!(unsafe { GetWindowRgn(hwnd, region.0) }, 0);
    for (x, y) in [
        (0, 0),
        (1199, 0),
        (0, 799),
        (1199, 799),
        (600, 0),
        (600, 799),
        (0, 400),
        (1199, 400),
        (400, 140),
    ] {
        assert_ne!(
            unsafe { PtInRegion(region.0, x, y) },
            0,
            "缩放或标题空白未被输入层覆盖"
        );
    }
    for (x, y) in [(100, 140), (690, 140), (730, 140), (778, 140), (600, 400)] {
        assert_eq!(
            unsafe { PtInRegion(region.0, x, y) },
            0,
            "输入层不能拦截原作者控件/正文"
        );
    }
    assert_eq!(
        unsafe { GetWindowRgn(fixture.child.0, region.0) },
        0,
        "作者必须保留完整绘制区，不能把输入区域施加到作者HWND"
    );
    unsafe {
        fixture.surface.sync(
            fixture.child.0,
            ChromeGeometry {
                visible: false,
                ..geometry()
            },
            false,
        )
    }
    .unwrap();
    assert_ne!(unsafe { GetWindowRgn(hwnd, region.0) }, 0);
    assert_eq!(
        unsafe { PtInRegion(region.0, 400, 140) },
        0,
        "隐藏顶栏只撤掉标题拖动区，不给正文留下旧命中"
    );
    assert_ne!(
        unsafe { PtInRegion(region.0, 0, 400) },
        0,
        "顶栏隐藏时，普通窗口仍保留系统边缘缩放"
    );
}

#[test]
fn production_popup_callback_routes_original_nc_messages_on_the_root_thread_only() {
    let fixture = Fixture::new();
    // signed负屏幕坐标也必须原样传递，不把GetMessagePos或当前鼠标位置替换成合成输入。
    let point = ((-900_i16 as u16 as u32) | ((-300_i16 as u16 as u32) << 16)) as isize;
    assert_eq!(fixture.send(WM_MOUSEACTIVATE, 0, 0), MA_NOACTIVATE as isize);
    assert_eq!(fixture.send(WM_NCHITTEST, 0, point), HTCAPTION as isize);
    let position = unsafe { GetMessagePos() };
    for message in [
        WM_NCLBUTTONDOWN,
        WM_NCLBUTTONDBLCLK,
        WM_NCRBUTTONDOWN,
        WM_NCRBUTTONUP,
    ] {
        assert_eq!(fixture.send(message, HTCAPTION as usize, point), 0);
    }
    let events = fixture.probe.events.borrow();
    assert_eq!(events.len(), 5, "每个真实NC入口只同步路由一次");
    for event in events.iter() {
        assert_eq!(event.lparam, point);
        assert_eq!(event.position, position);
        assert_eq!(event.thread, unsafe { GetCurrentThreadId() });
        assert_eq!(
            event.wparam,
            if event.message == WM_NCHITTEST {
                0
            } else {
                HTCAPTION as usize
            }
        );
    }
    drop(events);
    for message in [WM_MOUSEMOVE, WM_LBUTTONUP, WM_NCLBUTTONUP] {
        fixture.send(message, 0, point);
    }
    for command in [SC_MOVE | 2, SC_SIZE | 5] {
        assert_eq!(fixture.send(WM_SYSCOMMAND, command as usize, point), 0);
    }
    assert_eq!(
        fixture.probe.events.borrow().len(),
        5,
        "移动/松开及输入层自身系统循环不得被转交或重放"
    );
}

#[test]
fn native_root_messages_reposition_the_popup_immediately_and_keep_the_shape_on_plain_moves() {
    let fixture = Fixture::new();
    let before = fixture.surface.state.get().applied;
    assert!(before.is_some());
    assert_ne!(
        unsafe {
            SetWindowPos(
                fixture.root.0,
                std::ptr::null_mut(),
                -1400,
                350,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE,
            )
        },
        0
    );
    let mut origin = POINT::default();
    let mut rect = RECT::default();
    assert_ne!(unsafe { ClientToScreen(fixture.root.0, &mut origin) }, 0);
    assert_ne!(
        unsafe { GetWindowRect(fixture.surface.hwnd(), &mut rect) },
        0
    );
    assert_eq!(
        (rect.left, rect.top),
        (origin.x, origin.y),
        "不能依赖普通帧修正popup位置"
    );
    assert!(
        fixture.surface.state.get().applied == before,
        "纯移动不改变区域缓存"
    );
    assert_ne!(
        unsafe {
            SetWindowPos(
                fixture.root.0,
                std::ptr::null_mut(),
                0,
                0,
                1300,
                850,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOMOVE,
            )
        },
        0
    );
    assert_ne!(
        unsafe { GetWindowRect(fixture.surface.hwnd(), &mut rect) },
        0
    );
    assert_eq!(
        (rect.right - rect.left, rect.bottom - rect.top),
        (1300, 850)
    );
    let shape = fixture.surface.state.get().applied.unwrap();
    assert_eq!((shape.width, shape.height), (1300, 850));
}

#[test]
fn stale_source_wrong_root_and_retired_owner_fail_closed_without_leaking_the_callback_rc() {
    let fixture = Fixture::new();
    let alien = OwnedWindow::new(None);
    assert!(unsafe { reposition(alien.0, fixture.surface.hwnd()) }.is_err());
    let mut invalid = fixture.surface.state.get();
    invalid.child_pid = invalid.child_pid.wrapping_add(1);
    fixture.surface.state.set(invalid);
    assert!(unsafe { reposition(fixture.root.0, fixture.surface.hwnd()) }.is_err());
    assert_eq!(fixture.send(WM_NCHITTEST, 0, 0), HTNOWHERE as isize);
    assert_eq!(fixture.send(WM_NCLBUTTONDOWN, HTCAPTION as usize, 0), 0);
    assert!(fixture.probe.events.borrow().is_empty());
    unsafe { fixture.surface.sync(fixture.child.0, geometry(), true) }.unwrap();
    assert_eq!(fixture.send(WM_NCHITTEST, 0, 0), HTNOWHERE as isize);
    unsafe { fixture.surface.sync(fixture.child.0, geometry(), false) }.unwrap();
    let weak = Rc::downgrade(&fixture.surface.state);
    assert_eq!(
        Rc::strong_count(&fixture.surface.state),
        2,
        "对象与WinProc各拥有一份Rc"
    );
    fixture.probe.surface.set(std::ptr::null_mut());
    assert_ne!(unsafe { DestroyWindow(fixture.root.0) }, 0); // Win32自动销毁owned popup并归还原生引用。
    assert!(!fixture.surface.state.get().attached);
    assert_eq!(Rc::strong_count(&fixture.surface.state), 1);
    drop(fixture);
    assert!(weak.upgrade().is_none());
}

#[test]
fn constructor_rejects_a_root_owned_by_another_win32_thread() {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let (release, hold) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let root = OwnedWindow::new(None);
        send.send(root.0 as usize).unwrap();
        let _ = hold.recv();
    });
    let root = receive.recv().unwrap() as HWND;
    let rejected = unsafe { InputSurface::new(root) }.is_err();
    release.send(()).unwrap();
    worker.join().unwrap();
    assert!(rejected, "不得创建跨Winit线程路由的输入层");
}
