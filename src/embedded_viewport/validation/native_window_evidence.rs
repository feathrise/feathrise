//! Playing原生顶层窗口的独立宿主证据；Editing继续使用原子窗口、作者几何与HRGN验收门。

use super::{Region, window_pid};
use crate::viewport_mode::ViewportGeometry;
use feathrise_editor_protocol::Mode;
use serde::Deserialize;
use serde_json::{Value, json};
use windows_sys::Win32::{Foundation::{HWND, RECT}, Graphics::Gdi::{GetWindowRgn, GetWindowRgnBox},
    UI::{Input::KeyboardAndMouse::IsWindowEnabled, WindowsAndMessaging::*}};

pub(super) const HOST_SOURCE: &str = "host native-Win32-window+client+focus readback";

#[derive(Deserialize)]
struct NativeClaim {
    source: String, verified: bool, presentation: String, mode: Mode,
    hwnd: u64, pid: u32, parent_hwnd: u64, parent_pid: u32, root_hwnd: u64,
    is_child: bool, style: u32, extended_style: u32, window_enabled: bool,
    width: u32, height: u32, visible: bool, focused: bool, minimized: bool,
    active_hwnd: u64, focus_hwnd: u64, window_rect: [i32; 4], region_equal: bool,
    region_type: i32, excluded_rects: Vec<[u32; 4]>, requested_excluded_rects: Vec<[u32; 4]>,
    excluded_shapes: Vec<feathrise_editor_protocol::ViewportUiShape>,
    requested_excluded_shapes: Vec<feathrise_editor_protocol::ViewportUiShape>,
}

fn read_claim(capture: &Value) -> Result<NativeClaim, String> {
    let claim: NativeClaim = serde_json::from_value(capture["window_region"].clone())
        .map_err(|error| format!("原生预览截图缺少完整顶层窗口证据：{error}"))?;
    if capture["mode"] != "playing" || claim.source != "native-Win32-window-readback"
        || !claim.verified || claim.presentation != "native" || claim.mode != Mode::Playing
        || claim.hwnd == 0 || claim.pid == 0 || capture["pid"] != claim.pid
        || claim.parent_hwnd != 0 || claim.parent_pid != 0 || claim.root_hwnd != claim.hwnd
        || claim.is_child || claim.style & WS_CHILD != 0 || !claim.window_enabled
        || !claim.visible || claim.minimized || !claim.region_equal || claim.region_type != 0
        || claim.width == 0 || claim.height == 0
        || capture["capture"]["width"] != claim.width || capture["capture"]["height"] != claim.height
        || claim.window_rect[2] <= claim.window_rect[0] || claim.window_rect[3] <= claim.window_rect[1]
        || !claim.excluded_rects.is_empty() || !claim.requested_excluded_rects.is_empty()
        || !claim.excluded_shapes.is_empty() || !claim.requested_excluded_shapes.is_empty() {
        return Err("Playing截图不是同一可见、启用且无作者裁剪的原生顶层窗口".into());
    }
    Ok(claim)
}

fn native_handle(value: u64) -> Option<HWND> { usize::try_from(value).ok().filter(|value| *value != 0).map(|value| value as HWND) }

fn top_level(hwnd: HWND, pid: u32) -> bool {
    window_pid(hwnd) == Some(pid) && unsafe {
        GetParent(hwnd).is_null() && GetAncestor(hwnd, GA_ROOT) == hwnd
            && GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CHILD == 0
            && IsWindowEnabled(hwnd) != 0 && IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) == 0
    }
}

fn real_focus(hwnd: HWND) -> bool {
    let thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    if thread == 0 || unsafe { GetForegroundWindow() } != hwnd { return false; }
    let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    unsafe { GetGUIThreadInfo(thread, &mut info) != 0 && info.hwndActive == hwnd && info.hwndFocus == hwnd }
}

/// 后台只读实际原生状态；不能用宿主冻结的Editing几何或focused代替Playing窗口的自身事实。
pub(super) fn query_live(capture: &Value, author: &ViewportGeometry) -> Result<Option<Value>, String> {
    let claim = read_claim(capture)?;
    let hwnd = native_handle(claim.hwnd).ok_or("原生预览HWND无法表示")?;
    let author_hwnd = native_handle(author.parent_hwnd as u64).ok_or("原生预览缺少仍存活的作者窗口身份")?;
    if author.parent_pid != std::process::id() || window_pid(author_hwnd) != Some(std::process::id()) {
        return Err("原生预览截图不属于当前作者会话".into());
    }
    if window_pid(hwnd) != Some(claim.pid) { return Err("宿主发现原生预览HWND不属于截图PID".into()); }
    if !top_level(hwnd, claim.pid) || !real_focus(hwnd) || !claim.focused
        || claim.active_hwnd != claim.hwnd || claim.focus_hwnd != claim.hwnd { return Ok(None); }
    let mut client = RECT::default();
    let mut outer = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut client) } == 0 || unsafe { GetWindowRect(hwnd, &mut outer) } == 0 {
        return Err("宿主不能读取原生预览的实际客户区/外框".into());
    }
    let actual_rect = [outer.left, outer.top, outer.right, outer.bottom];
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let extended = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    if client.right - client.left != claim.width as i32 || client.bottom - client.top != claim.height as i32
        || client.left != 0 || client.top != 0 || actual_rect != claim.window_rect
        || style != claim.style || extended != claim.extended_style { return Ok(None); }
    let region = Region::rectangle(0, 0, 0, 0)?;
    if unsafe { GetWindowRgn(hwnd, region.0) } != 0 {
        return Err("宿主发现原生预览仍带作者HRGN裁剪".into());
    }
    Ok(Some(json!({"source":HOST_SOURCE,"verified":true,"observer_pid":std::process::id(),
        "presentation":"native","pid":claim.pid,"world_id":capture["world_id"],"revision":capture["revision"],
        "capture_frame":capture["frame"],"mode":"playing","hwnd":claim.hwnd,"parent_hwnd":0,"parent_pid":0,
        "root_hwnd":claim.hwnd,"is_child":false,"style":style,"extended_style":extended,
        "width":claim.width,"height":claim.height,"window_enabled":true,"visible":true,"minimized":false,
        "window_rect":actual_rect,"client_rect":[client.left,client.top,client.right,client.bottom],
        "region_equal":true,"region_type":0,"focused":true,"active_hwnd":claim.hwnd,"focus_hwnd":claim.hwnd,
        "foreground_hwnd":claim.hwnd,"preview_toolbar":capture["preview_toolbar"],
        "author_hwnd":author.parent_hwnd,"author_pid":author.parent_pid,
        "host_expected":{"source":"author-session-reference","revision":author.revision,
            "parent_hwnd":author.parent_hwnd,"parent_pid":author.parent_pid}})))
}

/// 输入前再次读当前顶层/焦点和客户区，旧截图不授予对其他窗口或旧尺寸的点击权限。
pub(super) fn focus_ready(proof: &Value, pid: u32) -> bool {
    let Some(hwnd) = proof["hwnd"].as_u64().and_then(native_handle) else { return false; };
    let Some(author) = proof["author_hwnd"].as_u64().and_then(native_handle) else { return false; };
    if proof["source"] != HOST_SOURCE || proof["verified"] != true || proof["presentation"] != "native"
        || proof["observer_pid"] != std::process::id() || proof["author_pid"] != std::process::id()
        || window_pid(author) != Some(std::process::id())
        || proof["mode"] != "playing" || proof["pid"] != pid || proof["parent_hwnd"] != 0
        || proof["parent_pid"] != 0 || proof["root_hwnd"] != proof["hwnd"] || proof["is_child"] != false
        || !top_level(hwnd, pid) || !real_focus(hwnd) { return false; }
    let mut client = RECT::default();
    let mut region = RECT::default();
    (unsafe { GetClientRect(hwnd, &mut client) != 0 && GetWindowRgnBox(hwnd, &mut region) == 0 })
        && proof["width"].as_i64() == Some(i64::from(client.right - client.left))
        && proof["height"].as_i64() == Some(i64::from(client.bottom - client.top))
}

pub(super) fn verify_record(step: &Value, capture: &Value, host_pid: &Value) -> Result<(), String> {
    let claim = read_claim(capture)?;
    let proof = &step["host_window_region"];
    let reference = &proof["host_expected"];
    if !claim.focused || claim.active_hwnd != claim.hwnd || claim.focus_hwnd != claim.hwnd
        || step["mode"] != "playing" || proof["source"] != HOST_SOURCE || proof["verified"] != true || &proof["observer_pid"] != host_pid
        || &proof["author_pid"] != host_pid || proof["author_hwnd"].as_u64().unwrap_or(0) == 0
        || reference["source"] != "author-session-reference" || &reference["parent_pid"] != host_pid
        || reference["parent_hwnd"] != proof["author_hwnd"] || reference["revision"].as_u64().is_none()
        || proof["presentation"] != "native" || proof["mode"] != "playing"
        || proof["pid"] != step["pid"] || proof["pid"] != capture["pid"]
        || proof["world_id"] != step["world_id"] || proof["world_id"] != capture["world_id"]
        || proof["revision"] != step["applied_revision"] || proof["revision"] != capture["revision"]
        || proof["capture_frame"] != capture["frame"] || proof["hwnd"] != claim.hwnd
        || proof["parent_hwnd"] != 0 || proof["parent_pid"] != 0 || proof["root_hwnd"] != claim.hwnd
        || proof["is_child"] != false || proof["style"] != claim.style || proof["extended_style"] != claim.extended_style
        || proof["window_enabled"] != true || proof["visible"] != true || proof["minimized"] != false
        || proof["width"] != claim.width || proof["height"] != claim.height
        || proof["window_rect"] != json!(claim.window_rect)
        || proof["client_rect"] != json!([0, 0, claim.width, claim.height])
        || proof["region_equal"] != true || proof["region_type"] != 0
        || proof["focused"] != true || proof["active_hwnd"] != claim.hwnd
        || proof["focus_hwnd"] != claim.hwnd || proof["foreground_hwnd"] != claim.hwnd {
        return Err("阶段缺少与本次GPU身份一致的宿主原生顶层/尺寸/焦点证明".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture() -> Value {
        json!({"mode":"playing","pid":12,"world_id":5,"revision":3,"frame":100,
            "capture":{"width":800,"height":600},"window_region":{
                "source":"native-Win32-window-readback","verified":true,"presentation":"native","mode":"playing",
                "hwnd":18,"pid":12,"parent_hwnd":0,"parent_pid":0,"root_hwnd":18,"is_child":false,
                "style":0,"extended_style":0,"window_enabled":true,"width":800,"height":600,
                "visible":true,"focused":true,"minimized":false,"active_hwnd":18,"focus_hwnd":18,
                "window_rect":[10,20,826,659],"region_equal":true,"region_type":0,
                "excluded_rects":[],"requested_excluded_rects":[],"excluded_shapes":[],"requested_excluded_shapes":[]}})
    }

    #[test]
    fn native_claim_rejects_child_identity_disabled_or_cropped_and_wrong_gpu_dimensions() {
        let original = capture(); assert!(read_claim(&original).is_ok());
        for (field, value) in [("source", json!("GetWindowRgn+EqualRgn+PtInRegion")), ("parent_hwnd", json!(19)),
            ("parent_pid", json!(20)), ("root_hwnd", json!(19)), ("is_child", json!(true)),
            ("style", json!(WS_CHILD)), ("window_enabled", json!(false)), ("width", json!(801)),
            ("region_type", json!(1)), ("excluded_rects", json!([[0,0,10,10]]))] {
            let mut bad = original.clone(); bad["window_region"][field] = value;
            assert!(read_claim(&bad).is_err(), "{field}");
        }
    }

    #[test]
    fn guest_verified_or_playing_status_cannot_replace_native_host_record() {
        let capture = capture();
        let mut step = json!({"pid":12,"world_id":5,"applied_revision":3,"mode":"playing","host_window_region":{}});
        assert!(verify_record(&step, &capture, &json!(20)).is_err());
        step["host_window_region"] = json!({"source":HOST_SOURCE,"verified":true,"observer_pid":20,
            "author_pid":20,"author_hwnd":19,"presentation":"native","mode":"playing",
            "pid":12,"world_id":5,"revision":3,"capture_frame":100,"hwnd":18,"parent_hwnd":0,"parent_pid":0,
            "root_hwnd":18,"is_child":false,"style":0,"extended_style":0,"window_enabled":true,
            "visible":true,"minimized":false,"width":800,"height":600,"window_rect":[10,20,826,659],
            "client_rect":[0,0,800,600],"region_equal":true,"region_type":0,"focused":true,
            "active_hwnd":18,"focus_hwnd":18,"foreground_hwnd":18,
            "host_expected":{"source":"author-session-reference","revision":1,"parent_hwnd":19,"parent_pid":20}});
        assert!(verify_record(&step, &capture, &json!(20)).is_ok());
        for (field, value) in [("observer_pid", json!(21)), ("pid", json!(13)), ("capture_frame", json!(99)),
            ("parent_hwnd", json!(19)), ("region_type", json!(1)), ("focus_hwnd", json!(19)),
            ("client_rect", json!([0,0,799,600]))] {
            let mut bad = step.clone(); bad["host_window_region"][field] = value;
            assert!(verify_record(&bad, &capture, &json!(20)).is_err(), "{field}");
        }
    }
}
