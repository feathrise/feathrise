//! 自有原生HUD的真实鼠标验收：先移动，跨帧核验落点后按下，再跨帧松开；从不抢焦点。

use super::{native_window, window_pid};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use windows_sys::Win32::{Foundation::{HWND, POINT, RECT}, Graphics::Gdi::{ClientToScreen, ScreenToClient},
    UI::{Input::KeyboardAndMouse::{GetAsyncKeyState, INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT,
        MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
        MOUSEEVENTF_VIRTUALDESK, SendInput, VK_LBUTTON}, WindowsAndMessaging::{
        GetClientRect, GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
        SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WindowFromPoint}}};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage { Moved, Pressed, Released }

/// 只有本对象成功发出的按下才由Drop兜底松开；不释放验收开始前用户已按住的鼠标。
pub(crate) struct NativeMouseClick {
    proof: Value, pid: u32, hwnd: u64, point: [i32; 2], screen: [i32; 2], size: [i32; 2],
    stage: Stage, changed: Instant, started: Instant,
}

impl NativeMouseClick {
    pub(super) fn start(proof: &Value, pid: u32, point: [i32; 2]) -> Result<Self, String> {
        let hwnd = proof["hwnd"].as_u64().ok_or("原生HUD缺少本轮HWND")?;
        let handle = usize::try_from(hwnd).map_err(|_| "原生HUD句柄越界")? as HWND;
        if !native_window::focus_ready(proof, pid) || left_pressed() {
            return Err("原生HUD移动前失去自有窗口焦点，或鼠标左键已被外部按住".into());
        }
        let mut screen = POINT { x: point[0], y: point[1] };
        if unsafe { ClientToScreen(handle, &mut screen) } == 0
            || unsafe { WindowFromPoint(screen) } != handle {
            return Err("原生HUD目标当前不属于已核验客户区，拒绝移动或点击其他窗口".into());
        }
        let origin = unsafe { [GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN)] };
        let extent = unsafe { [GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN)] };
        let absolute = absolute_point([screen.x, screen.y], origin, extent)
            .ok_or("原生HUD目标不在当前虚拟桌面内")?;
        send_mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, absolute)?;
        let click = Self { proof: proof.clone(), pid, hwnd, point, screen: [screen.x, screen.y],
            size: [proof["width"].as_i64().ok_or("原生HUD宽度缺失")? as i32,
                proof["height"].as_i64().ok_or("原生HUD高度缺失")? as i32],
            stage: Stage::Moved, changed: Instant::now(), started: Instant::now() };
        click.record("move_submitted");
        Ok(click)
    }

    pub(super) fn advance(&mut self) -> Result<bool, String> {
        if self.stage == Stage::Released { return Ok(true); }
        if self.started.elapsed() > Duration::from_secs(3) {
            return Err("原生HUD分帧鼠标操作超过3秒；本轮按住状态将立即收口".into());
        }
        if self.changed.elapsed() < Duration::from_millis(100) { return Ok(false); }
        if self.stage == Stage::Moved {
            let observed = self.read_position();
            if !press_allowed(self.hwnd, self.pid, self.point, self.screen, self.size, &observed) {
                return Err(format!("原生HUD按下前焦点、窗口尺寸、归属或真实鼠标落点已变化：{observed:?}"));
            }
            send_mouse(MOUSEEVENTF_LEFTDOWN, [0, 0])?;
            self.stage = Stage::Pressed;
            self.changed = Instant::now();
            self.record("down_submitted");
            return Ok(false);
        }
        // 返回按钮可在按下后立即重嵌入并失焦；释放自己持有的按钮不能再受Playing焦点门阻断。
        send_mouse(MOUSEEVENTF_LEFTUP, [0, 0])?;
        self.stage = Stage::Released;
        self.record("up_submitted");
        Ok(true)
    }

    fn read_position(&self) -> Position {
        let hwnd = self.hwnd as usize as HWND;
        let mut cursor = POINT::default();
        let cursor_ok = unsafe { GetCursorPos(&mut cursor) } != 0;
        let screen = [cursor.x, cursor.y];
        let hit = unsafe { WindowFromPoint(cursor) };
        let client_ok = cursor_ok && unsafe { ScreenToClient(hwnd, &mut cursor) } != 0;
        let mut rect = RECT::default();
        let size_ok = unsafe { GetClientRect(hwnd, &mut rect) } != 0;
        Position { focused: native_window::focus_ready(&self.proof, self.pid),
            cursor_ok: client_ok, screen, client: [cursor.x, cursor.y], size_ok,
            client_rect: [rect.left, rect.top, rect.right, rect.bottom],
            hit: hit as usize as u64, hit_pid: window_pid(hit), left_pressed: left_pressed() }
    }

    fn record(&self, phase: &str) {
        println!("FEATHRISE_CODE_ITERATION_RETURN_INPUT {}", json!({"phase":phase,"input":"Win32 SendInput",
            "pid":self.pid,"hwnd":self.hwnd,"point":self.point,"screen":self.screen}));
    }
}

impl Drop for NativeMouseClick {
    fn drop(&mut self) {
        if self.stage == Stage::Pressed {
            match send_mouse(MOUSEEVENTF_LEFTUP, [0, 0]) {
                Ok(()) => { self.stage = Stage::Released; self.record("cleanup_up_submitted"); }
                Err(error) => eprintln!("原生HUD验收释放自有鼠标失败：{error}"),
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Position {
    focused: bool, cursor_ok: bool, screen: [i32; 2], client: [i32; 2], size_ok: bool,
    client_rect: [i32; 4], hit: u64, hit_pid: Option<u32>, left_pressed: bool,
}

fn press_allowed(hwnd: u64, pid: u32, point: [i32; 2], screen: [i32; 2], size: [i32; 2], value: &Position) -> bool {
    hwnd != 0 && pid != 0 && size[0] > 0 && size[1] > 0 && value.focused && value.cursor_ok && value.size_ok
        && !value.left_pressed && value.hit == hwnd && value.hit_pid == Some(pid)
        && value.client_rect == [0, 0, size[0], size[1]]
        && (0..2).all(|axis| point[axis] > 1 && point[axis] < size[axis] - 1
            && (i64::from(value.client[axis]) - i64::from(point[axis])).abs() <= 1
            && (i64::from(value.screen[axis]) - i64::from(screen[axis])).abs() <= 1)
}

/// Win32绝对坐标覆盖整个虚拟桌面；按像素中心映射，支持负原点且拒绝桌面外目标。
fn absolute_point(point: [i32; 2], origin: [i32; 2], extent: [i32; 2]) -> Option<[i32; 2]> {
    let mut result = [0; 2];
    for axis in 0..2 {
        let offset = i64::from(point[axis]) - i64::from(origin[axis]);
        let extent = i64::from(extent[axis]);
        if extent <= 0 || !(0..extent).contains(&offset) { return None; }
        result[axis] = ((offset * 65536 + 32768) / extent).min(65535) as i32;
    }
    Some(result)
}

fn left_pressed() -> bool { unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON)) < 0 } }

fn send_mouse(flags: u32, point: [i32; 2]) -> Result<(), String> {
    let input = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT {
        dx: point[0], dy: point[1], mouseData: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } } };
    if unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) } != 1 {
        return Err(format!("自有原生HUD的SendInput未提交：{}", std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_point_requires_current_focus_dimensions_window_and_an_unheld_button() {
        let valid = Position { focused: true, cursor_ok: true, screen: [-50, 51], client: [100, 50],
            size_ok: true, client_rect: [0, 0, 800, 600], hit: 17, hit_pid: Some(23), left_pressed: false };
        let allowed = |value: &Position| press_allowed(17, 23, [100, 50], [-50, 51], [800, 600], value);
        assert!(allowed(&valid));
        for invalid in [Position { focused: false, ..valid.clone() }, Position { cursor_ok: false, ..valid.clone() },
            Position { size_ok: false, ..valid.clone() }, Position { client_rect: [0, 0, 801, 600], ..valid.clone() },
            Position { hit: 18, ..valid.clone() }, Position { hit_pid: Some(24), ..valid.clone() },
            Position { left_pressed: true, ..valid.clone() }, Position { screen: [-40, 51], ..valid.clone() },
            Position { client: [110, 50], ..valid.clone() }] { assert!(!allowed(&invalid), "{invalid:?}"); }
    }

    #[test]
    fn physical_pixels_scene_to_virtual_desktop_without_accepting_outside_points() {
        assert_eq!(absolute_point([-1920, 0], [-1920, 0], [3840, 1080]), Some([8, 30]));
        assert_eq!(absolute_point([1919, 1079], [-1920, 0], [3840, 1080]), Some([65527, 65505]));
        assert_eq!(absolute_point([1920, 0], [-1920, 0], [3840, 1080]), None);
        assert_eq!(absolute_point([-1921, 0], [-1920, 0], [3840, 1080]), None);
        assert_eq!(absolute_point([0, 0], [0, 0], [0, 1080]), None);
    }
}
