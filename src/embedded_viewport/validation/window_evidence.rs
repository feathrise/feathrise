//! 宿主独立读取仍存活的原生游戏窗口，核对裁剪、句柄归属与输入状态后保存阶段证据。
//! 只在显式验收的后台读取槽执行；最终聚合使用已留存证据，不查询已经销毁的窗口。

use crate::viewport_mode::ViewportGeometry;
use feathrise_editor_protocol::{Envelope, Limits, Message, Mode, SessionIdentity, ViewportUiShape};
use serde::Deserialize;
use serde_json::{Value, json};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{
        CombineRgn, CreateRectRgn, DeleteObject, ERROR, EqualRgn, GetWindowRgn, HRGN, PtInRegion,
        RGN_DIFF, ScreenToClient,
    },
    UI::{
        Input::KeyboardAndMouse::IsWindowEnabled,
        WindowsAndMessaging::{
            GetClientRect, GetParent, GetWindowRect, GetWindowThreadProcessId, IsWindow,
        },
    },
};

#[path = "native_window_evidence.rs"]
mod native_window;
#[path = "native_mouse_input.rs"]
mod native_mouse_input;
pub(super) use native_mouse_input::NativeMouseClick;

const HOST_SOURCE: &str = "host GetWindowRgn+EqualRgn+PtInRegion+HWND ownership";

#[derive(Deserialize)]
struct Claim {
    source: String,
    verified: bool,
    mode: Mode,
    hwnd: u64,
    parent_hwnd: u64,
    parent_pid: u32,
    window_enabled: bool,
    width: u32,
    height: u32,
    excluded_rects: Vec<[u32; 4]>,
    requested_excluded_rects: Vec<[u32; 4]>,
    #[serde(default)]
    excluded_shapes: Vec<ViewportUiShape>,
    #[serde(default)]
    requested_excluded_shapes: Vec<ViewportUiShape>,
    region_equal: bool,
    samples: Vec<Sample>,
}

#[derive(Deserialize)]
struct Sample {
    point: [i32; 2],
    included: bool,
    expected_included: bool,
}

struct Region(HRGN);
impl Region {
    /// 宿主独立重建实际作者轮廓，不能只相信游戏声称自己的区域正确。
    fn shape(shape: &ViewportUiShape) -> Result<Self, String> {
        use windows_sys::Win32::Graphics::Gdi::{CreateEllipticRgn, CreatePolygonRgn, WINDING};
        if !shape.valid() { return Err("宿主拒绝无效作者轮廓".into()); }
        let handle = match shape {
            ViewportUiShape::Ellipse { outer: [l, t, r, b], .. } => unsafe { CreateEllipticRgn(*l, *t, *r, *b) },
            ViewportUiShape::Quad { points } => {
                let points = points.map(|[x, y]| POINT { x, y });
                unsafe { CreatePolygonRgn(points.as_ptr(), 4, WINDING) }
            }
        };
        if handle.is_null() { return Err("宿主无法创建作者轮廓".into()); }
        let region = Self(handle);
        if let ViewportUiShape::Ellipse { inner: Some([l, t, r, b]), .. } = shape {
            let inner = Self(unsafe { CreateEllipticRgn(*l, *t, *r, *b) });
            if inner.0.is_null() || unsafe { CombineRgn(region.0, region.0, inner.0, RGN_DIFF) } == ERROR {
                return Err("宿主无法重建作者圆环".into());
            }
        }
        Ok(region)
    }

    fn rectangle(left: i32, top: i32, right: i32, bottom: i32) -> Result<Self, String> {
        let handle = unsafe { CreateRectRgn(left, top, right, bottom) };
        if handle.is_null() {
            Err("宿主无法创建验收区域".into())
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for Region {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.0);
        }
    }
}

fn read_claim(capture: &Value) -> Result<Claim, String> {
    let claim: Claim = serde_json::from_value(capture["window_region"].clone())
        .map_err(|error| format!("运行截图缺少完整原生区域证据：{error}"))?;
    let playing = capture["mode"] == "playing";
    if claim.source != "GetWindowRgn+EqualRgn+PtInRegion"
        || !claim.verified
        || !claim.region_equal
        || claim.mode
            != if playing {
                Mode::Playing
            } else {
                Mode::Editing
            }
        || claim.window_enabled != playing
        || claim.hwnd == 0
        || claim.parent_hwnd == 0
        || claim.parent_pid == 0
        || claim.width == 0
        || claim.height == 0
        || capture["capture"]["width"] != claim.width
        || capture["capture"]["height"] != claim.height
    {
        return Err("运行实例原生区域模式、尺寸、输入或句柄身份不符".into());
    }
    // 复用正式协议的数量、非零范围和溢出校验，不在验收中另写一套更宽松的矩形合同。
    Envelope::new(
        SessionIdentity {
            project_id: "owned-validation".into(),
            session: 1,
            generation: 1,
        },
        1,
        Message::SetViewport {
            x: 0,
            y: 0,
            width: claim.width,
            height: claim.height,
            visible: true,
            focused: false,
            excluded_rects: claim.requested_excluded_rects.clone(),
            excluded_shapes: claim.requested_excluded_shapes.clone(),
        },
    )
    .validate(&Limits::default())
    .map_err(|error| error.to_string())?;
    if playing {
        if !claim.excluded_rects.is_empty() || !claim.requested_excluded_rects.is_empty()
            || !claim.excluded_shapes.is_empty() || !claim.requested_excluded_shapes.is_empty() {
            return Err("预览区域仍排除了作者浮层".into());
        }
    } else if claim.excluded_rects.is_empty() && claim.excluded_shapes.is_empty()
        || claim.excluded_rects != claim.requested_excluded_rects
        || claim.excluded_shapes != claim.requested_excluded_shapes
    {
        return Err("编辑阶段必须真正让出至少一个作者浮层区域".into());
    }
    let points = sample_points(&claim);
    if points.len() != claim.samples.len()
        || claim.samples.iter().zip(points).any(|(sample, point)| {
            sample.point != point
                || sample.included != included(&claim, point)
                || sample.expected_included != included(&claim, point)
        })
    {
        return Err("运行实例原生区域取样点或半开矩形结果不符".into());
    }
    Ok(claim)
}

fn sample_points(claim: &Claim) -> Vec<[i32; 2]> {
    let mut points = vec![
        [0, 0],
        [(claim.width / 2) as i32, (claim.height / 2) as i32],
        [claim.width as i32 - 1, claim.height as i32 - 1],
    ];
    points.extend(
        claim
            .requested_excluded_rects
            .iter()
            .map(|[x, y, w, h]| [(x + w / 2) as i32, (y + h / 2) as i32]),
    );
    points.extend(claim.requested_excluded_shapes.iter().flat_map(ViewportUiShape::probe_points));
    points
}

fn included(claim: &Claim, [x, y]: [i32; 2]) -> bool {
    x >= 0
        && y >= 0
        && (x as u32) < claim.width
        && (y as u32) < claim.height
        && !claim
            .excluded_rects
            .iter()
            .any(|[left, top, width, height]| {
                x as u32 >= *left
                    && y as u32 >= *top
                    && (x as u32) < left + width
                    && (y as u32) < top + height
            })
        && !claim.excluded_shapes.iter().any(|shape| {
            Region::shape(shape).is_ok_and(|region| unsafe { PtInRegion(region.0, x, y) } != 0)
        })
}

fn window_pid(window: HWND) -> Option<u32> {
    let mut pid = 0;
    (unsafe { IsWindow(window) } != 0
        && unsafe { GetWindowThreadProcessId(window, &mut pid) } != 0
        && pid != 0)
        .then_some(pid)
}

/// 原生消息不会激活窗口；跨进程必须查询游戏线程的真实焦点，不能读取宿主线程的GetFocus。
pub(super) fn game_return_focus_ready(proof: &Value, pid: u32) -> bool {
    if proof["source"] == native_window::HOST_SOURCE { return native_window::focus_ready(proof, pid); }
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GUITHREADINFO, GetAncestor, GetForegroundWindow, GetGUIThreadInfo,
    };
    let Some(handle) = proof["hwnd"].as_u64().and_then(|value| usize::try_from(value).ok()) else { return false; };
    let Some(parent) = proof["parent_hwnd"].as_u64().and_then(|value| usize::try_from(value).ok()) else { return false; };
    let hwnd = handle as HWND;
    let parent = parent as HWND;
    if window_pid(hwnd) != Some(pid) || window_pid(parent) != Some(std::process::id())
        || unsafe { GetParent(hwnd) } != parent || unsafe { IsWindowEnabled(hwnd) } == 0 { return false; }
    let root = unsafe { GetAncestor(parent, GA_ROOT) };
    if root.is_null() || unsafe { GetForegroundWindow() } != root { return false; }
    let thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    if thread == 0 { return false; }
    let mut focus: GUITHREADINFO = unsafe { std::mem::zeroed() };
    focus.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    unsafe { GetGUIThreadInfo(thread, &mut focus) != 0 && focus.hwndFocus == hwnd }
}

/// 仅在首次拒绝等待后即时复读事实；不会激活窗口、移动鼠标或替代原焦点许可判断。
pub(super) fn describe_game_return_focus(proof: &Value, pid: u32) -> Value {
    use windows_sys::Win32::{Graphics::Gdi::GetWindowRgnBox, UI::WindowsAndMessaging::{
        GA_ROOT, GUITHREADINFO, GWL_STYLE, GetAncestor, GetCursorPos, GetForegroundWindow,
        GetGUIThreadInfo, GetWindowLongPtrW, IsIconic, IsWindowVisible,
    }};
    let native = proof["source"] == native_window::HOST_SOURCE;
    let handle = proof["hwnd"].as_u64().and_then(|value| usize::try_from(value).ok()).unwrap_or(0);
    let author_handle = proof[if native { "author_hwnd" } else { "parent_hwnd" }].as_u64()
        .and_then(|value| usize::try_from(value).ok()).unwrap_or(0);
    let hwnd = handle as HWND;
    let author = author_handle as HWND;
    let parent = unsafe { GetParent(hwnd) };
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let foreground = unsafe { GetForegroundWindow() };
    let thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    let mut focus: GUITHREADINFO = unsafe { std::mem::zeroed() };
    focus.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
    let focus_ok = thread != 0 && unsafe { GetGUIThreadInfo(thread, &mut focus) } != 0;
    let mut client = RECT::default();
    let client_ok = unsafe { GetClientRect(hwnd, &mut client) } != 0;
    let mut region = RECT::default();
    let region_type = unsafe { GetWindowRgnBox(hwnd, &mut region) };
    let mut cursor = POINT::default();
    let cursor_ok = unsafe { GetCursorPos(&mut cursor) } != 0;
    let mut cursor_client = cursor;
    let cursor_client_ok = cursor_ok && unsafe { ScreenToClient(hwnd, &mut cursor_client) } != 0;
    let contract = json!({"source":proof["source"],"verified":proof["verified"],"presentation":proof["presentation"],
        "mode":proof["mode"],"pid":proof["pid"],"observer_pid":proof["observer_pid"],
        "author_pid":proof["author_pid"],"parent_pid":proof["parent_pid"],"parent_hwnd":proof["parent_hwnd"],
        "root_hwnd":proof["root_hwnd"],"is_child":proof["is_child"],"width":proof["width"],"height":proof["height"]});
    let window = json!({"hwnd":handle,"valid":unsafe { IsWindow(hwnd) } != 0,"pid":window_pid(hwnd),
        "author_hwnd":author_handle,"author_pid":window_pid(author),"parent_hwnd":parent as usize as u64,
        "parent_pid":window_pid(parent),"root_hwnd":root as usize as u64,"root_pid":window_pid(root),
        "style":unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32,
        "enabled":unsafe { IsWindowEnabled(hwnd) } != 0,"visible":unsafe { IsWindowVisible(hwnd) } != 0,
        "minimized":unsafe { IsIconic(hwnd) } != 0});
    let focus = json!({"thread":thread,"query_ok":focus_ok,"active_hwnd":focus.hwndActive as usize as u64,
        "focus_hwnd":focus.hwndFocus as usize as u64,"foreground_hwnd":foreground as usize as u64,
        "foreground_pid":window_pid(foreground)});
    json!({"source":"read-only Win32 facts after rejected HUD focus gate","expected_pid":pid,
        "observer_pid":std::process::id(),"native_branch":native,"contract":contract,"window":window,"focus":focus,
        "client":{"query_ok":client_ok,"rect":[client.left,client.top,client.right,client.bottom]},
        "region_type":region_type,"cursor":{"query_ok":cursor_ok,"screen":[cursor.x,cursor.y],
            "client_query_ok":cursor_client_ok,"client":[cursor_client.x,cursor_client.y]}})
}

/// 只点击本轮已经独立核验的工程窗口及真实布局按钮；不模拟F12或直接写成功状态。
/// 这证明Winit/UI/IPC链，不代表实体鼠标手感、桌面焦点或全部游戏输入验收。
pub(super) fn request_game_return(proof: &Value, pid: u32) -> Result<super::resources::GameMouseRelease, String> {
    request_game_toolbar_button(proof, pid, "返回")
}

pub(super) fn request_game_toolbar_button(proof: &Value, pid: u32, label: &str) -> Result<super::resources::GameMouseRelease, String> {
    use windows_sys::Win32::UI::{
        WindowsAndMessaging::{PostMessageW, WM_MOUSEMOVE, WM_LBUTTONDOWN},
    };
    let handle = proof["hwnd"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or("没有本轮工程窗口原生输入证据")?;
    let hwnd = handle as HWND;
    if (proof["source"] != HOST_SOURCE && proof["source"] != native_window::HOST_SOURCE)
        || proof["verified"] != true
        || proof["pid"] != pid
        || proof["window_enabled"] != true
        || window_pid(hwnd) != Some(pid)
    {
        return Err("拒绝点击不属于本轮已核验工程的窗口".into());
    }
    if proof["source"] == native_window::HOST_SOURCE && !native_window::focus_ready(proof, pid) {
        return Err("原生预览工具条点击前已失去本轮真实顶层焦点或客户区尺寸".into());
    }
    let toolbar = &proof["preview_toolbar"];
    if toolbar["source"] != "production-Bevy-UI-ComputedNode" || toolbar["active"] != true
        || toolbar["root_visible"] != true || toolbar["camera_active"] != true
        || toolbar["f12_bound"] != false {
        return Err("实际预览工具条未呈现或仍绑定F12".into());
    }
    let buttons = toolbar["buttons"].as_array().filter(|buttons| buttons.len() == 3)
        .ok_or("预览工具条不是实际三个按钮")?;
    if !matches!(label, "返回" | "显示网格" | "显示碰撞网格") {
        return Err("不属于本轮预览工具条的按钮".into());
    }
    let button = buttons.iter().find(|button| button["label"] == label && button["visible"] == true)
        .ok_or_else(||format!("实际{label}按钮不可见"))?;
    let center = button["physical_center"].as_array().filter(|point| point.len() == 2)
        .ok_or("返回按钮缺少真实物理布局")?;
    let x = center[0].as_f64().filter(|value| value.is_finite() && *value > 0. && *value < 32768.)
        .ok_or("返回按钮横坐标无效")?.round() as i32;
    let y = center[1].as_f64().filter(|value| value.is_finite() && *value > 0. && *value < 32768.)
        .ok_or("返回按钮纵坐标无效")?.round() as i32;
    if i64::from(x) >= proof["width"].as_i64().unwrap_or(0)
        || i64::from(y) >= proof["height"].as_i64().unwrap_or(0) {
        return Err("实际返回按钮超出已核验运行窗口客户区".into());
    }
    if proof["source"] == native_window::HOST_SOURCE {
        // SendInput像素中心映射可有1px舍入差；真实按钮必须为落点保留明确的内部余量。
        if button["physical_size"].as_array().is_none_or(|size| size.len() != 2
            || size.iter().any(|value| value.as_f64().is_none_or(|value| !value.is_finite() || value < 4.))) {
            return Err("原生HUD按钮缺少足够的实际物理尺寸，拒绝系统鼠标点击".into());
        }
        return Ok(super::resources::GameMouseRelease { hwnd:handle as u64, pid, point:[x,y],
            pressed:std::time::Instant::now(), native:Some(NativeMouseClick::start(proof, pid, [x,y])?) });
    }
    let point = (x as u32 | ((y as u32) << 16)) as isize;
    if unsafe { PostMessageW(hwnd, WM_MOUSEMOVE, 0, point) } == 0
        || unsafe { PostMessageW(hwnd, WM_LBUTTONDOWN, 1, point) } == 0 {
        return Err(format!("本轮预览工具条鼠标投递失败：{}", std::io::Error::last_os_error()));
    }
    Ok(super::resources::GameMouseRelease { hwnd:handle as u64, pid, point:[x,y], pressed:std::time::Instant::now(), native:None })
}

pub(super) fn release_game_click(release: &mut super::resources::GameMouseRelease) -> Result<bool, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_LBUTTONUP};
    if let Some(native) = &mut release.native {
        return native.advance();
    }
    let hwnd = usize::try_from(release.hwnd).map_err(|_| "鼠标松开窗口无法表示")? as HWND;
    if window_pid(hwnd) != Some(release.pid) { return Err("鼠标松开前自有运行窗口身份已改变".into()); }
    let point = (release.point[0] as u32 | ((release.point[1] as u32) << 16)) as isize;
    if unsafe { PostMessageW(hwnd, WM_LBUTTONUP, 0, point) } == 0 {
        return Err(format!("本轮预览工具条鼠标松开失败：{}", std::io::Error::last_os_error()));
    }
    Ok(true)
}

fn matches_author_geometry(claim: &Claim, expected: &ViewportGeometry) -> bool {
    expected.visible
        && expected.parent_hwnd != 0
        && expected.parent_pid != 0
        && claim.parent_hwnd == expected.parent_hwnd as u64
        && claim.parent_pid == expected.parent_pid
        && claim.width == expected.rect[2].max(1) as u32
        && claim.height == expected.rect[3].max(1) as u32
        && claim.requested_excluded_rects == expected.excluded_rects
        && claim.requested_excluded_shapes == expected.excluded_shapes
}

fn fills_parent_client(child: [i32; 4], parent: [i32; 4]) -> bool {
    parent[0] == 0 && parent[1] == 0 && parent[2] > 0 && parent[3] > 0 && child == parent
}

fn fills_preview_content(child: [i32; 4], parent: [i32; 4]) -> bool {
    // 预览保留原三键的独立标题栏；不能退回中央小视口，也不能让游戏盖住标题栏。
    parent[0] == 0 && parent[1] == 0 && parent[2] > 0 && parent[3] > 0
        && child[0] == 0 && child[1] > 0 && child[1] < parent[3]
        && child[2] == parent[2] && child[3] == parent[3] - child[1]
}

pub(super) fn query_live(
    capture: &Value,
    author: &ViewportGeometry,
) -> Result<Option<Value>, String> {
    if capture["mode"] == "playing" { return native_window::query_live(capture, author); }
    // JSON来自上一稳定帧时可以重试，绝不将guest自己声明的矩形当作作者UI实际布局。
    let claim: Claim = serde_json::from_value(capture["window_region"].clone())
        .map_err(|error| format!("运行截图缺少完整原生区域证据：{error}"))?;
    if !matches_author_geometry(&claim, author) {
        return Ok(None);
    }
    // 激活窗口后首张PNG可能仍绑定之前的失焦输入许可；等实际启用的新帧，最终read_claim仍严格拒绝禁用输入。
    if claim.mode == Mode::Playing && (!author.focused || !claim.window_enabled) {
        return Ok(None);
    }
    let claim = read_claim(capture)?;
    let pid = capture["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("截图PID无效")?;
    let hwnd = usize::try_from(claim.hwnd).map_err(|_| "运行实例HWND无法表示")? as HWND;
    let parent = usize::try_from(claim.parent_hwnd).map_err(|_| "作者HWND无法表示")? as HWND;
    let actual_pid = window_pid(hwnd);
    let actual_parent_pid = window_pid(parent);
    let actual_parent = unsafe { GetParent(hwnd) };
    if actual_pid != Some(pid)
        || actual_parent_pid != Some(std::process::id())
        || claim.parent_pid != std::process::id()
        || actual_parent != parent
    {
        return Err(format!("宿主实时查询发现运行窗口或作者父窗口归属不符：运行实例HWND={} PID={actual_pid:?}/{pid}，父HWND={} PID={actual_parent_pid:?}/{}，实际父={}，声明父PID={}",
            claim.hwnd, claim.parent_hwnd, std::process::id(), actual_parent as usize, claim.parent_pid));
    }
    let mut client = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
        return Err("宿主无法查询运行窗口客户区".into());
    }
    let mut parent_client = RECT::default();
    if unsafe { GetClientRect(parent, &mut parent_client) } == 0 {
        return Err("宿主无法查询作者完整客户区".into());
    }
    let mut outer = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut outer) } == 0 {
        return Err("宿主无法查询运行子窗口位置".into());
    }
    let mut origin = POINT {
        x: outer.left,
        y: outer.top,
    };
    if unsafe { ScreenToClient(parent, &mut origin) } == 0 {
        return Err("宿主无法换算作者客户区位置".into());
    }
    if client.right - client.left != author.rect[2].max(1)
        || client.bottom - client.top != author.rect[3].max(1)
        || origin.x != author.rect[0]
        || origin.y != author.rect[1]
    {
        return Ok(None);
    }
    let physical_rect = [
        origin.x,
        origin.y,
        client.right - client.left,
        client.bottom - client.top,
    ];
    let parent_client_rect = [
        parent_client.left,
        parent_client.top,
        parent_client.right - parent_client.left,
        parent_client.bottom - parent_client.top,
    ];
    let fills_parent = fills_parent_client(physical_rect, parent_client_rect);
    let fills_content = fills_preview_content(physical_rect, parent_client_rect);
    // 标题栏高度由作者实际测量后进入ViewportGeometry，原生窗口必须精确避开该行。
    // 交接或缩放期间先重试，最终阶段不能采用上一布局的几何证据。
    if claim.mode == Mode::Playing && !fills_content {
        return Ok(None);
    }
    let enabled = unsafe { IsWindowEnabled(hwnd) } != 0;
    if enabled != claim.window_enabled {
        return Err("宿主实时查询发现编辑/预览原生输入状态不符".into());
    }
    let expected = Region::rectangle(0, 0, author.rect[2].max(1), author.rect[3].max(1))?;
    let exclusions: &[[u32; 4]] = if claim.mode == Mode::Editing {
        &author.excluded_rects
    } else {
        &[]
    };
    for &[x, y, width, height] in exclusions {
        let hole = Region::rectangle(x as i32, y as i32, (x + width) as i32, (y + height) as i32)?;
        if unsafe { CombineRgn(expected.0, expected.0, hole.0, RGN_DIFF) } == ERROR {
            return Err("宿主无法计算作者浮层预期区域".into());
        }
    }
    if claim.mode == Mode::Editing {
        for shape in &author.excluded_shapes {
            let hole = Region::shape(shape)?;
            if unsafe { CombineRgn(expected.0, expected.0, hole.0, RGN_DIFF) } == ERROR {
                return Err("宿主无法计算作者透明轮廓预期区域".into());
            }
        }
    }
    let actual = Region::rectangle(0, 0, 0, 0)?;
    let region_type = unsafe { GetWindowRgn(hwnd, actual.0) };
    if region_type == ERROR {
        return Err("宿主无法读取运行实例原生窗口区域".into());
    }
    // 几何任务可能跨过下一次布局发布；不把暂态不同的区域记为成功，等待下个一致快照。
    if unsafe { EqualRgn(actual.0, expected.0) } == 0 {
        return Ok(None);
    }
    let mut samples = Vec::new();
    for point in sample_points(&claim) {
        let actual_included = unsafe { PtInRegion(actual.0, point[0], point[1]) } != 0;
        let expected_included = included(&claim, point);
        if actual_included != expected_included {
            return Err("宿主实时取样发现作者浮层仍被原生子窗口覆盖".into());
        }
        samples.push(
            json!({"point":point,"included":actual_included,"expected_included":expected_included}),
        );
    }
    Ok(Some(
        json!({"source":HOST_SOURCE,"verified":true,"observer_pid":std::process::id(),
        "pid":pid,"world_id":capture["world_id"],"revision":capture["revision"],"capture_frame":capture["frame"],
        "mode":claim.mode,"hwnd":claim.hwnd,"parent_hwnd":claim.parent_hwnd,"parent_pid":claim.parent_pid,
        "preview_toolbar":capture["preview_toolbar"],
        "width":client.right-client.left,"height":client.bottom-client.top,"window_enabled":enabled,
        "physical_rect":physical_rect,"parent_client_source":"GetClientRect(parent)",
        "parent_client_rect":parent_client_rect,"fills_parent_client":fills_parent,
        "fills_preview_content":fills_content,
        "preview_header_bottom":if claim.mode==Mode::Playing {Some(physical_rect[1])} else {None},
        "region_equal":true,"region_type":region_type,"excluded_rects":claim.excluded_rects,
        "requested_excluded_rects":claim.requested_excluded_rects,
        "excluded_shapes":claim.excluded_shapes,"requested_excluded_shapes":claim.requested_excluded_shapes,"samples":samples,
        "host_expected":{"source":"ViewportGeometry","revision":author.revision,
            "parent_hwnd":author.parent_hwnd,"parent_pid":author.parent_pid,"physical_rect":author.rect,
            "excluded_rects":author.excluded_rects,"excluded_shapes":author.excluded_shapes,"visible":author.visible}}),
    ))
}

pub(super) fn verify_record(step: &Value, capture: &Value, host_pid: &Value) -> Result<(), String> {
    if capture["mode"] == "playing" { return native_window::verify_record(step, capture, host_pid); }
    let claim = read_claim(capture)?;
    let proof = &step["host_window_region"];
    let expected = &proof["host_expected"];
    let physical: [i32; 4] = serde_json::from_value(expected["physical_rect"].clone())
        .map_err(|error| format!("阶段缺少作者物理几何证据：{error}"))?;
    let parent_client: [i32; 4] = serde_json::from_value(proof["parent_client_rect"].clone())
        .map_err(|error| format!("阶段缺少作者完整客户区原生证据：{error}"))?;
    // 旧的无轮廓证据按空集合读取；缺省不能掩盖宿主或guest多出的裁剪轮廓。
    let proof_shapes: Vec<ViewportUiShape> = serde_json::from_value(
        proof.get("excluded_shapes").cloned().unwrap_or_else(|| json!([])),
    ).map_err(|error| format!("宿主轮廓证据无效：{error}"))?;
    let expected_shapes: Vec<ViewportUiShape> = serde_json::from_value(
        expected.get("excluded_shapes").cloned().unwrap_or_else(|| json!([])),
    ).map_err(|error| format!("作者轮廓证据无效：{error}"))?;
    if proof["source"] != HOST_SOURCE
        || proof["verified"] != true
        || proof["region_equal"] != true
        || &proof["observer_pid"] != host_pid
        || &proof["parent_pid"] != host_pid
        || proof["pid"] != step["pid"]
        || proof["world_id"] != step["world_id"]
        || proof["revision"] != step["applied_revision"]
        || proof["mode"] != step["mode"]
        || proof["hwnd"] != claim.hwnd
        || proof["parent_hwnd"] != claim.parent_hwnd
        || proof["window_enabled"] != claim.window_enabled
        || proof["width"] != claim.width
        || proof["height"] != claim.height
        || proof["excluded_rects"] != json!(claim.excluded_rects)
        || proof_shapes != claim.excluded_shapes
        || expected["source"] != "ViewportGeometry"
        || expected["visible"] != true
        || expected["physical_rect"] != proof["physical_rect"]
        || expected["parent_hwnd"] != claim.parent_hwnd
        || expected["parent_pid"] != claim.parent_pid
        || expected["excluded_rects"] != json!(claim.requested_excluded_rects)
        || expected_shapes != claim.requested_excluded_shapes
        || physical[2].max(1) as u32 != claim.width
        || physical[3].max(1) as u32 != claim.height
        || proof["parent_client_source"] != "GetClientRect(parent)"
        || proof["fills_parent_client"] != fills_parent_client(physical, parent_client)
        || proof["fills_preview_content"] != fills_preview_content(physical, parent_client)
        || (claim.mode == Mode::Playing && (!fills_preview_content(physical, parent_client)
            || proof["preview_header_bottom"] != physical[1]))
    {
        return Err("阶段缺少匹配本次截图身份的宿主实时原生区域证明".into());
    }
    let samples: Vec<Sample> =
        serde_json::from_value(proof["samples"].clone()).map_err(|error| error.to_string())?;
    let points = sample_points(&claim);
    if points.len() != samples.len()
        || samples.iter().zip(points).any(|(sample, point)| {
            sample.point != point
                || sample.included != included(&claim, point)
                || sample.expected_included != included(&claim, point)
        })
    {
        return Err("宿主留存的原生区域取样证据不完整".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture() -> Value {
        json!({"pid":12,"world_id":5,"revision":3,"frame":100,"mode":"editing",
        "capture":{"width":100,"height":100},"window_region":{
            "source":"GetWindowRgn+EqualRgn+PtInRegion","verified":true,"region_equal":true,
            "mode":"editing","hwnd":18,"parent_hwnd":19,"parent_pid":20,"window_enabled":false,
            "width":100,"height":100,"excluded_rects":[[10,10,20,20]],
            "requested_excluded_rects":[[10,10,20,20]],"samples":[
                {"point":[0,0],"included":true,"expected_included":true},
                {"point":[50,50],"included":true,"expected_included":true},
                {"point":[99,99],"included":true,"expected_included":true},
                {"point":[20,20],"included":false,"expected_included":false}
            ]}})
    }

    #[test]
    fn guest_verified_flag_cannot_replace_live_host_evidence_or_required_holes() {
        let original = capture();
        assert!(read_claim(&original).is_ok());
        assert!(
            verify_record(
                &json!({"pid":12,"world_id":5,"applied_revision":3,"mode":"editing"}),
                &original,
                &json!(20)
            )
            .is_err()
        );
        let mut missing_hole = original.clone();
        missing_hole["window_region"]["excluded_rects"] = json!([]);
        assert!(read_claim(&missing_hole).is_err());
        let mut enabled_editor = original.clone();
        enabled_editor["window_region"]["window_enabled"] = json!(true);
        assert!(read_claim(&enabled_editor).is_err());
        let mut wrong_sample = original;
        wrong_sample["window_region"]["samples"][3]["included"] = json!(true);
        assert!(read_claim(&wrong_sample).is_err());
    }

    #[test]
    fn playing_requires_full_region_and_enabled_native_input() {
        let mut playing = capture();
        playing["mode"] = json!("playing");
        playing["window_region"]["mode"] = json!("playing");
        playing["window_region"]["window_enabled"] = json!(true);
        assert!(read_claim(&playing).is_err());
        playing["window_region"]["excluded_rects"] = json!([]);
        playing["window_region"]["samples"][3]["included"] = json!(true);
        playing["window_region"]["samples"][3]["expected_included"] = json!(true);
        assert!(
            read_claim(&playing).is_err(),
            "预览不得继续携带作者浮层留孔请求"
        );
        playing["window_region"]["requested_excluded_rects"] = json!([]);
        playing["window_region"]["samples"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(read_claim(&playing).is_ok());
    }

    #[test]
    fn playing_extent_preserves_only_the_measured_header_not_the_old_central_viewport() {
        let parent = [0, 0, 1280, 800];
        assert!(fills_parent_client(parent, parent));
        assert!(!fills_preview_content(parent, parent), "游戏不能覆盖原窗口三键");
        assert!(fills_preview_content([0, 54, 1280, 746], parent));
        assert!(!fills_preview_content([240, 60, 800, 700], parent));
        assert!(!fills_preview_content([0, 54, 1279, 746], parent));
        assert!(!fills_preview_content([1, 54, 1280, 746], parent));
        assert!(!fills_preview_content([0, 54, 1280, 745], parent));
        assert!(!fills_preview_content([0, 0, 0, 0], [0, 0, 0, 0]));
    }

    #[test]
    fn guest_region_must_match_independent_author_geometry_before_native_queries() {
        let claim = read_claim(&capture()).unwrap();
        let mut author = ViewportGeometry {
            parent_hwnd: 19,
            parent_pid: 20,
            rect: [30, 40, 100, 100],
            excluded_rects: vec![[10, 10, 20, 20]],
            excluded_shapes: Vec::new(),
            visible: true,
            focused: false,
            revision: 1,
        };
        assert!(matches_author_geometry(&claim, &author));
        author.excluded_rects[0][0] = 11;
        assert!(
            !matches_author_geometry(&claim, &author),
            "guest自报一致不能替代实际作者留孔"
        );
        author.excluded_rects[0][0] = 10;
        author.rect[2] = 101;
        assert!(!matches_author_geometry(&claim, &author));
        author.rect[2] = 100;
        author.parent_hwnd = 21;
        assert!(!matches_author_geometry(&claim, &author));
    }
}
