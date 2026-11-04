//! 真实隐藏HWND上的原生回调回归；下层哨兵捕捉错误放行，避免启动系统modal拖动或打扰用户窗口。
use super::*;
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;

const PROBE_SUBCLASS: usize = SUBCLASS + 1;
const DEFAULT_REACHED: LRESULT = 0x4654;

struct OwnedWindow(HWND);
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) };
    }
}

// 字段按声明顺序释放，先销毁HWND，保证原callback清理Rc时状态和哨兵计数仍然有效。
struct HookedWindow {
    window: OwnedWindow,
    state: Rc<Cell<Snapshot>>,
    default_calls: Box<Cell<u32>>,
}

unsafe extern "system" fn default_probe(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    if matches!(message, WM_NCLBUTTONDOWN | WM_SYSCOMMAND) {
        let calls = unsafe { &*(data as *const Cell<u32>) };
        calls.set(calls.get() + 1);
        return DEFAULT_REACHED;
    }
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}

fn geometry() -> ChromeGeometry {
    // 放在固定隐藏客户区内部，避免测试机DPI改变系统边缘宽度而误中缩放边。
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

impl HookedWindow {
    fn new(parent: Option<HWND>, root: Option<HWND>) -> Self {
        let class: Vec<_> = "STATIC".encode_utf16().chain(Some(0)).collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                if parent.is_some() { WS_CHILD } else { WS_POPUP },
                if parent.is_some() { 0 } else { 200 },
                if parent.is_some() { 0 } else { 200 },
                1200,
                800,
                parent.unwrap_or(std::ptr::null_mut()),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null(), "应能创建本测试拥有的隐藏窗口");
        let window = OwnedWindow(hwnd);
        let default_calls = Box::new(Cell::new(0));
        assert_ne!(
            unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(default_probe),
                    PROBE_SUBCLASS,
                    (&*default_calls as *const Cell<u32>) as usize,
                )
            },
            0
        );
        let state = Rc::new(Cell::new(Snapshot {
            root: root.unwrap_or(hwnd) as usize,
            root_pid: std::process::id(),
            geometry: geometry(),
            attached: true,
            ..default()
        }));
        let raw = Rc::into_raw(Rc::clone(&state)) as usize;
        if unsafe { SetWindowSubclass(hwnd, Some(callback), SUBCLASS, raw) } == 0 {
            unsafe { drop(Rc::from_raw(raw as *const Cell<Snapshot>)) };
            panic!("必须安装生产Chrome callback");
        }
        Self {
            window,
            state,
            default_calls,
        }
    }

    fn screen(&self, x: i32, y: i32) -> LPARAM {
        let mut point = POINT { x, y };
        assert_ne!(unsafe { ClientToScreen(self.window.0, &mut point) }, 0);
        packed_point(point)
    }

    fn send(&self, message: u32, wparam: u32, point: LPARAM) -> LRESULT {
        unsafe { SendMessageW(self.window.0, message, wparam as WPARAM, point) }
    }
}

#[test]
fn real_child_hit_testing_never_advertises_move_or_resize_but_keeps_caption_buttons() {
    let root = HookedWindow::new(None, None);
    let child = HookedWindow::new(Some(root.window.0), Some(root.window.0));
    assert!(unsafe {
        valid_root(
            child.window.0 as usize,
            root.window.0 as usize,
            std::process::id(),
        )
    });
    for (x, y) in [(300, 140), (1, 300), (1199, 300), (600, 1), (600, 799)] {
        assert_eq!(
            child.send(WM_NCHITTEST, 0, child.screen(x, y)),
            HTCLIENT as LRESULT
        );
    }
    for (x, hit) in [(690, HTMINBUTTON), (730, HTMAXBUTTON), (778, HTCLOSE)] {
        assert_eq!(
            child.send(WM_NCHITTEST, 0, child.screen(x, 140)),
            hit as LRESULT
        );
    }
    assert_eq!(
        root.send(WM_NCHITTEST, 0, root.screen(300, 140)),
        HTCAPTION as LRESULT
    );
    assert_eq!(
        root.send(WM_NCHITTEST, 0, root.screen(1, 300)),
        HTLEFT as LRESULT
    );
}

#[test]
fn real_child_callback_blocks_default_move_chain_even_when_root_identity_is_invalid() {
    let root = HookedWindow::new(None, None);
    let child = HookedWindow::new(Some(root.window.0), Some(root.window.0));
    for invalid_root in [false, true] {
        let mut snapshot = child.state.get();
        snapshot.root_pid = if invalid_root {
            std::process::id().wrapping_add(1)
        } else {
            std::process::id()
        };
        child.state.set(snapshot);
        for hit in [HTCAPTION, HTLEFT, HTBOTTOMRIGHT] {
            assert_eq!(child.send(WM_NCLBUTTONDOWN, hit, child.screen(300, 140)), 0);
        }
        // 低四位是系统内部命中/方向位，不能仅按完整wParam相等拦截。
        for command in [SC_MOVE, SC_MOVE | 2, SC_SIZE, SC_SIZE | 8] {
            assert_eq!(child.send(WM_SYSCOMMAND, command, 0), 0);
        }
        assert_eq!(
            child.default_calls.get(),
            0,
            "子窗口动作不得抵达Winit/系统默认移动链"
        );
        if invalid_root {
            assert_eq!(
                child.send(WM_NCHITTEST, 0, child.screen(300, 140)),
                HTCLIENT as LRESULT
            );
        }
    }
}

#[test]
fn real_root_system_move_commands_keep_the_original_default_chain() {
    let root = HookedWindow::new(None, None);
    for command in [SC_MOVE, SC_MOVE | 2, SC_SIZE, SC_SIZE | 8] {
        assert_eq!(root.send(WM_SYSCOMMAND, command, 0), DEFAULT_REACHED);
    }
    assert_eq!(root.default_calls.get(), 4);
}
