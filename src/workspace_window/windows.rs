//! 将原完整作者App的呈现窗口嵌入持续存在的主工作台；工程World和不可变路径仍归本实例。
//!
//! 候选工程在隐藏子窗口中准备，父宿主确认旧工程退出后才显示它。这里只处理窗口归属、
//! 尺寸及真实前台恢复焦点，不接管作者文档、保存、工程运行或工程控制通道，也不隐藏或销毁父窗口。

use bevy::{
    app::{AppExit, MainScheduleOrder},
    ecs::{schedule::ScheduleLabel, system::NonSendMarker},
    prelude::*,
    window::{
        PrimaryWindow, RawHandleWrapper, WindowFocused, WindowResized, WindowScaleFactorChanged,
    },
};
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND, LPARAM, POINT, RECT, SetLastError},
    Graphics::Gdi::ScreenToClient,
    UI::{
        HiDpi::{AreDpiAwarenessContextsEqual, GetDpiForWindow, GetWindowDpiAwarenessContext},
        Input::KeyboardAndMouse::{GetFocus, SetFocus},
        WindowsAndMessaging::*,
    },
};

const PARENT_HWND_ENV: &str = "FEATHRISE_WORKSPACE_PARENT_HWND";
const PARENT_PID_ENV: &str = "FEATHRISE_WORKSPACE_PARENT_PID";
const CANDIDATE_ENV: &str = "FEATHRISE_WORKSPACE_CANDIDATE";
const MAX_CHILD_WINDOWS: usize = 64;
const PREPARATION_TIMEOUT: Duration = Duration::from_secs(90);

/// 由父宿主传入的窗口身份，句柄和PID必须成对出现，不能凭一个已被复用的HWND采用窗口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EmbedTarget {
    pub parent: usize,
    pub pid: u32,
}

impl EmbedTarget {
    pub(crate) fn from_env() -> Result<Option<Self>, String> {
        parse_target(
            std::env::var_os(PARENT_HWND_ENV)
                .map(|value| {
                    value
                        .into_string()
                        .map_err(|_| "父窗口句柄不是UTF-8文本".to_owned())
                })
                .transpose()?,
            std::env::var_os(PARENT_PID_ENV)
                .map(|value| {
                    value
                        .into_string()
                        .map_err(|_| "父窗口PID不是UTF-8文本".to_owned())
                })
                .transpose()?,
        )
    }
}

fn parse_target(hwnd: Option<String>, pid: Option<String>) -> Result<Option<EmbedTarget>, String> {
    match (hwnd, pid) {
        (None, None) => Ok(None),
        (Some(hwnd), Some(pid)) => {
            let parent = hwnd
                .parse::<usize>()
                .ok()
                .filter(|value| *value != 0)
                .ok_or("工作台父窗口句柄无效")?;
            let pid = pid
                .parse::<u32>()
                .ok()
                .filter(|value| *value != 0)
                .ok_or("工作台父窗口PID无效")?;
            Ok(Some(EmbedTarget { parent, pid }))
        }
        _ => Err("工作台父窗口句柄和PID必须同时提供".into()),
    }
}

/// Ready只表示窗口已正确嵌入且原字体/最小UI帧已就绪，地图与文档就绪仍由作者运行时确认。
#[derive(Resource, Debug, Default)]
pub(crate) struct EmbeddedAuthorStatus {
    pub hwnd: Option<usize>,
    pub attached: bool,
    pub ready: bool,
    pub activated: bool,
    pub error: Option<String>,
}

#[derive(Resource)]
struct EmbedState {
    target: Option<EmbedTarget>,
    candidate: bool,
    started: Instant,
    frames: u64,
    last_size: Option<(u32, u32)>,
    focus_settled: bool,
    root_was_foreground: bool,
    native_focused: bool,
    play_completed: u64,
    play_return_requested: bool,
    failed: bool,
}

/// 原工程运行完成会写Window.focused=true；子窗口不得让Winit按顶层激活路径执行该意图。
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct EmbeddedFocusPrepare;

/// Winit在Last阶段更新窗口时会重新计算原生样式，因此嵌入校正必须位于Last之后。
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct EmbeddedWindowSync;

pub(crate) struct EmbeddedAuthorPlugin;

impl Plugin for EmbeddedAuthorPlugin {
    fn build(&self, app: &mut App) {
        let (target, error) = match EmbedTarget::from_env() {
            Ok(None) => return, // 显式独立作者入口保持原窗口行为。
            Ok(Some(target)) => (Some(target), None),
            Err(error) => (None, Some(error)),
        };
        app.insert_resource(EmbeddedAuthorStatus { error, ..default() })
            .insert_resource(EmbedState {
                target,
                candidate: std::env::var_os(CANDIDATE_ENV).is_some_and(|value| value == "1"),
                started: Instant::now(),
                frames: 0,
                last_size: None,
                focus_settled: false,
                root_was_foreground: false,
                native_focused: false,
                play_completed: 0,
                play_return_requested: false,
                failed: false,
            })
            // 渲染尺寸先于本帧UI/相机计算发布；Last之后只校正原生窗口，不能再改交换链尺寸。
            .add_systems(First, publish_embedded_render_size)
            .init_schedule(EmbeddedFocusPrepare)
            .add_systems(EmbeddedFocusPrepare, prepare_embedded_focus)
            .init_schedule(EmbeddedWindowSync)
            .add_systems(EmbeddedWindowSync, sync_embedded_window);
        let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
        order.insert_before(Last, EmbeddedFocusPrepare);
        order.insert_after(Last, EmbeddedWindowSync);
    }
}

/// 嵌入身份由late阶段建立；这里只读已附着的原句柄，失败留给原late错误/超时出口处理。
fn publish_embedded_render_size(
    _main_thread: NonSendMarker,
    state: Res<EmbedState>,
    status: Res<EmbeddedAuthorStatus>,
    mut windows: Query<(Entity, &mut Window, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
    mut resized: MessageWriter<WindowResized>,
    mut scale_changed: MessageWriter<WindowScaleFactorChanged>,
) {
    if state.failed {
        return;
    }
    let Some(target) = state
        .target
        .filter(|target| target.pid != std::process::id())
    else {
        return;
    };
    let Ok((entity, mut window, raw)) = windows.single_mut() else {
        return;
    };
    let Some(hwnd) = raw.and_then(primary_hwnd) else {
        return;
    };
    let Some(size) = render_size_snapshot(target, &status, hwnd) else {
        return;
    };
    // 先只读比较，稳定帧不触发Window变更检测，避免Winit重新处理原生样式。
    if (window.physical_width(), window.physical_height()) == size.physical
        && window.resolution.scale_factor_override() == Some(size.scale)
    {
        return;
    }
    publish_render_size(entity, &mut window, size, &mut resized, &mut scale_changed);
}

#[derive(Clone, Copy)]
struct RenderSizeSnapshot {
    physical: (u32, u32),
    scale: f32,
}

fn render_size_snapshot(
    target: EmbedTarget,
    status: &EmbeddedAuthorStatus,
    hwnd: usize,
) -> Option<RenderSizeSnapshot> {
    if !status.attached
        || status.error.is_some()
        || status.hwnd != Some(hwnd)
        || !parent_is_valid(target.parent, target.pid)
        || !unsafe { child_belongs_to(hwnd as HWND, target.parent as HWND, std::process::id()) }
    {
        return None;
    }
    let physical = unsafe { client_size(target.parent as HWND) }.ok()?;
    let scale = unsafe { GetDpiForWindow(target.parent as HWND) };
    if scale == 0 {
        return None;
    }
    Some(RenderSizeSnapshot {
        physical,
        scale: scale.max(96) as f32 / 96.,
    })
}

fn publish_render_size(
    entity: Entity,
    window: &mut Window,
    size: RenderSizeSnapshot,
    resized: &mut MessageWriter<WindowResized>,
    scale_changed: &mut MessageWriter<WindowScaleFactorChanged>,
) {
    let old_size = (window.physical_width(), window.physical_height());
    let old_scale = window.scale_factor();
    if window.resolution.scale_factor_override() != Some(size.scale) {
        window
            .resolution
            .set_scale_factor_override(Some(size.scale));
    }
    if old_size != size.physical {
        window
            .resolution
            .set_physical_resolution(size.physical.0, size.physical.1);
    }
    // CameraUpdateSystems按窗口消息失效computed.target_info。只改Window会让默认视口相机
    // 继续沿用旧尺寸；First同时发出实际逻辑尺寸/DPI，使UI、相机和提取看到同一完整快照。
    if old_size != size.physical || old_scale != size.scale {
        resized.write(WindowResized {
            window: entity,
            width: window.width(),
            height: window.height(),
        });
    }
    if old_scale != size.scale {
        scale_changed.write(WindowScaleFactorChanged {
            window: entity,
            scale_factor: size.scale as f64,
        });
    }
}

fn prepare_embedded_focus(
    mut state: ResMut<EmbedState>,
    play: Option<Res<crate::native_play::NativePlayStatus>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if state.failed {
        return;
    }
    if let Some(play) = play {
        if play.completed != state.play_completed {
            state.play_completed = play.completed;
            state.play_return_requested = true;
        }
    }
    if let Ok(mut window) = windows.single_mut() {
        // Winit0.30的focus_window把自身当顶层调用SetForegroundWindow，并为激活注入Alt。
        // 嵌入后该HWND是子窗口，既不能正确恢复主窗口，也不能因后台工程运行结束抢占其他应用。
        // Last前只给Winit保留“不发激活请求”的缓存值；Last后按真实原生焦点恢复供作者系统读取。
        if window.focused {
            window.focused = false;
        }
    }
}

/// 只读取RawHandleWrapper，不转移Winit窗口所有权；调用方不得销毁这个借用句柄。
pub(crate) fn primary_hwnd(raw: &RawHandleWrapper) -> Option<usize> {
    match raw.get_window_handle() {
        raw_window_handle::RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as usize),
        _ => None,
    }
}

pub(crate) fn parent_is_valid(parent: usize, pid: u32) -> bool {
    parent != 0 && pid != 0 && unsafe { window_belongs_to(parent as HWND, pid) }
}

unsafe fn window_belongs_to(window: HWND, pid: u32) -> bool {
    let mut actual = 0;
    unsafe {
        !window.is_null()
            && IsWindow(window) != 0
            && GetWindowThreadProcessId(window, &mut actual) != 0
            && actual == pid
    }
}

unsafe fn child_belongs_to(window: HWND, parent: HWND, pid: u32) -> bool {
    // 父进程可能恰好读到Winit改样式、子线程尚未校正的中间态。GetParent随WS_CHILD解释
    // 顶层owner，样式位也不是工程身份；GA_PARENT只检查真实父链，仍须精确核对所属PID。
    unsafe { window_belongs_to(window, pid) && GetAncestor(window, GA_PARENT) == parent }
}

/// 只返回指定作者进程直接挂到父窗口的子窗口，枚举有界，也不会误取另一个候选工程。
pub(crate) fn find_child(parent: usize, pid: u32) -> Option<usize> {
    if parent == 0 || pid == 0 {
        return None;
    }
    struct Search {
        parent: HWND,
        pid: u32,
        visited: usize,
        found: Option<usize>,
    }
    unsafe extern "system" fn visit(window: HWND, value: LPARAM) -> i32 {
        // EnumChildWindows同步调用，栈上Search只在本次枚举期间借用。
        let search = unsafe { &mut *(value as *mut Search) };
        if search.visited >= MAX_CHILD_WINDOWS {
            return 0;
        }
        search.visited += 1;
        if unsafe { child_belongs_to(window, search.parent, search.pid) } {
            search.found = Some(window as usize);
            return 0;
        }
        1
    }
    let mut search = Search {
        parent: parent as HWND,
        pid,
        visited: 0,
        found: None,
    };
    unsafe {
        EnumChildWindows(
            parent as HWND,
            Some(visit),
            (&mut search as *mut Search) as LPARAM,
        )
    };
    search.found
}

/// 父宿主在旧工程实际退出后提交显示。跨进程尺寸/显示使用异步请求，不等待作者消息泵。
/// 焦点由子线程首次观察到可见时处理，父线程不跨输入队列强设焦点或抢占其他应用。
pub(crate) fn activate_child(parent: usize, pid: u32, child: usize) -> Result<(), String> {
    let parent = parent as HWND;
    let child = child as HWND;
    unsafe {
        if IsWindow(parent) == 0 || !child_belongs_to(child, parent, pid) {
            return Err("候选作者窗口身份或父子关系已变化，拒绝显示".into());
        }
        let (width, height) = client_size(parent)?;
        if SetWindowPos(
            child,
            HWND_TOP,
            0,
            0,
            width as i32,
            height as i32,
            SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        ) == 0
        {
            return Err(win32_error("无法显示候选作者子窗口"));
        }
    }
    Ok(())
}

/// 已确认当前作者退出后，将焦点交还持续存在的主工作台；不提升后台窗口，不触碰候选子窗口。
pub(crate) fn restore_host_focus(world: &mut World, parent: usize) -> Result<(), String> {
    let parent = parent as HWND;
    let focused = unsafe {
        if !window_belongs_to(parent, std::process::id()) {
            return Err("返回开始页的主窗口身份已失效".into());
        }
        let root = GetAncestor(parent, GA_ROOT);
        if !should_restore_host_focus(
            IsWindowVisible(parent) != 0,
            !root.is_null() && GetForegroundWindow() == root && IsIconic(root) == 0,
        ) {
            return Ok(()); // 用户已切到其他应用时保留其焦点，之后走正常窗口激活事件。
        }
        if GetFocus() != parent {
            SetFocus(parent); // 当前Winit线程的自有窗口，不跨进程SendMessage或AttachThreadInput。
        }
        GetFocus() == parent
    };
    if !focused {
        return Err(win32_error("开始页未能接回主窗口输入焦点"));
    }
    publish_host_focus(world);
    Ok(())
}

fn should_restore_host_focus(visible: bool, foreground: bool) -> bool {
    visible && foreground
}

fn publish_host_focus(world: &mut World) {
    let mut windows = world.query_filtered::<(Entity, &mut Window), With<PrimaryWindow>>();
    let Ok((entity, mut window)) = windows.single_mut(world) else { return; };
    window.focused = true;
    // 子窗口销毁可能没有父窗口WM_SETFOCUS边沿；同步原UI门，避免必须Alt+Tab才恢复点击。
    world.write_message(WindowFocused { window: entity, focused: true });
}

fn child_style(style: u32) -> u32 {
    (style & !(WS_OVERLAPPEDWINDOW | WS_POPUP)) | WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN
}

fn child_extended_style(style: u32) -> u32 {
    // 子窗口不单独进入任务栏/Alt+Tab；不使用NOACTIVATE，正常编辑仍能取得键盘焦点。
    (style & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW
}

unsafe fn set_style(window: HWND, index: WINDOW_LONG_PTR_INDEX, value: u32) -> Result<(), String> {
    unsafe {
        SetLastError(0);
        let previous = SetWindowLongPtrW(window, index, value as isize);
        if previous == 0 && GetLastError() != 0 {
            return Err(win32_error("无法设置作者子窗口样式"));
        }
    }
    Ok(())
}

unsafe fn enforce_child_style(window: HWND) -> Result<bool, String> {
    unsafe {
        let style = GetWindowLongPtrW(window, GWL_STYLE) as u32;
        let ex_style = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
        let expected_style = child_style(style);
        let expected_ex_style = child_extended_style(ex_style);
        if style == expected_style && ex_style == expected_ex_style {
            return Ok(false);
        }
        set_style(window, GWL_STYLE, expected_style)?;
        set_style(window, GWL_EXSTYLE, expected_ex_style)?;
        Ok(true)
    }
}

unsafe fn client_size(parent: HWND) -> Result<(u32, u32), String> {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetClientRect(parent, &mut rect) } == 0 {
        return Err(win32_error("无法读取工作台客户区尺寸"));
    }
    Ok((
        (rect.right - rect.left).max(1) as u32,
        (rect.bottom - rect.top).max(1) as u32,
    ))
}

unsafe fn fills_client(parent: HWND, child: HWND, size: (u32, u32)) -> bool {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(child, &mut rect) } == 0 {
        return false;
    }
    let mut origin = POINT {
        x: rect.left,
        y: rect.top,
    };
    (unsafe { ScreenToClient(parent, &mut origin) != 0 })
        && origin.x == 0
        && origin.y == 0
        && rect.right - rect.left == size.0 as i32
        && rect.bottom - rect.top == size.1 as i32
}

fn win32_error(context: &str) -> String {
    format!("{context}：{}", std::io::Error::last_os_error())
}

fn sync_embedded_window(
    // RawHandleWrapper可发送不代表窗口线程可迁移；所有原生写操作固定在Winit线程。
    _main_thread: NonSendMarker,
    mut state: ResMut<EmbedState>,
    mut status: ResMut<EmbeddedAuthorStatus>,
    mut windows: Query<(Entity, &mut Window, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
    fonts: Option<Res<Assets<Font>>>,
    close: Option<Res<crate::native_project_close::NativeProjectCloseState>>,
    mut focused: MessageWriter<WindowFocused>,
    mut exit: MessageWriter<AppExit>,
) {
    if state.failed {
        return;
    }
    let result = sync_inner(
        &mut state,
        &mut status,
        &mut windows,
        fonts.as_deref(),
        close.as_ref().is_some_and(|state| state.active()),
        &mut focused,
    );
    if let Err(error) = result {
        error!("{error}");
        status.error = Some(error);
        state.failed = true;
        // 仅嵌入身份/宿主丢失或初始化失败走此路径，不能把隐藏候选留成永久子进程。
        exit.write(AppExit::error());
    }
}

fn sync_inner(
    state: &mut EmbedState,
    status: &mut EmbeddedAuthorStatus,
    windows: &mut Query<(Entity, &mut Window, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
    fonts: Option<&Assets<Font>>,
    close_active: bool,
    focused: &mut MessageWriter<WindowFocused>,
) -> Result<(), String> {
    if let Some(error) = status.error.as_ref() {
        return Err(error.clone());
    }
    let target = state.target.ok_or("缺少工作台父窗口身份")?;
    if target.pid == std::process::id() || !parent_is_valid(target.parent, target.pid) {
        return Err("工作台父窗口已经退出或PID身份不符，停止作者实例".into());
    }
    let Ok((entity, mut window, raw)) = windows.single_mut() else {
        return check_preparation_timeout(state, "作者主窗口尚未创建");
    };
    let Some(hwnd) = raw.and_then(primary_hwnd) else {
        return check_preparation_timeout(state, "作者主窗口尚未取得Win32句柄");
    };
    let child = hwnd as HWND;
    let parent = target.parent as HWND;
    if !status.attached {
        unsafe {
            if !window_belongs_to(child, std::process::id()) {
                return Err("作者窗口不属于本进程，拒绝嵌入".into());
            }
            if IsWindowVisible(child) != 0 {
                return Err("作者候选窗口在嵌入前已经显示；应以visible=false创建".into());
            }
            if AreDpiAwarenessContextsEqual(
                GetWindowDpiAwarenessContext(parent),
                GetWindowDpiAwarenessContext(child),
            ) == 0
            {
                return Err("工作台与作者窗口的DPI模式不同，拒绝跨进程嵌入".into());
            }
            enforce_child_style(child)?;
            SetLastError(0);
            let previous = SetParent(child, parent);
            if previous.is_null() && GetLastError() != 0 || GetParent(child) != parent {
                return Err(win32_error("无法将作者窗口挂入工作台"));
            }
        }
        status.hwnd = Some(hwnd);
        status.attached = true;
        info!(
            "作者窗口已嵌入同一主窗口：parent={},child={hwnd}",
            target.parent
        );
    }
    if status.hwnd != Some(hwnd) {
        return Err(format!(
            "作者窗口句柄已被替换，停止当前实例：expected={:?},actual={hwnd},parent={}",
            status.hwnd, target.parent
        ));
    }

    // 稳定帧仅做固定次数的原生读取；Winit确实改样式或父尺寸变化时才重新排布。
    // GetParent对顶层窗口和子窗口采用不同语义。Last若刚清掉WS_CHILD，必须先恢复样式再判父关系，
    // 否则原生父句柄未变也会被GetParent的NULL误判为脱离；真实句柄/归属错误仍立即拒绝。
    let before_style = unsafe { GetWindowLongPtrW(child, GWL_STYLE) } as u32;
    let before_parent = unsafe { GetParent(child) };
    let style_changed = unsafe { enforce_child_style(child)? };
    let actual_parent = unsafe { GetParent(child) };
    if actual_parent != parent {
        return Err(format!(
            "作者窗口父关系不符，停止当前实例：child={hwnd},expected_parent={},before_parent={},actual_parent={},before_style={before_style:#x},after_style={:#x},parent_slot={}",
            target.parent,
            before_parent as usize,
            actual_parent as usize,
            unsafe { GetWindowLongPtrW(child, GWL_STYLE) } as u32,
            unsafe { GetWindowLongPtrW(child, GWLP_HWNDPARENT) } as usize,
        ));
    }
    if before_parent != parent {
        info!(
            "已在校正子窗口样式后确认原父关系：child={hwnd},parent={},before_parent={},before_style={before_style:#x},after_style={:#x}",
            target.parent,
            before_parent as usize,
            unsafe { GetWindowLongPtrW(child, GWL_STYLE) } as u32,
        );
    }
    sync_native_size(state, parent, child, style_changed)?;
    // 禁止在这里再写Window.resolution或DPI：本帧PostUpdate已计算UI裁剪和Camera视口。
    // 父窗口若在帧中途还原，late只对齐原生子窗口，新的渲染尺寸留给下一First原子发布。

    state.frames = state.frames.saturating_add(1);
    if !status.ready {
        // 默认中文字体已在插件装配时登记，不再等待外置字体路径的 AssetServer 加载状态。
        let font_ready = fonts.is_some_and(|fonts| fonts.contains(bevy::asset::AssetId::<Font>::default()));
        status.ready = state.frames >= 3 && font_ready;
        if !status.ready {
            check_preparation_timeout(state, "作者工作台字体尚未就绪")?;
        }
    }
    if status.ready && !state.candidate && !status.activated {
        activate_child(target.parent, std::process::id(), hwnd)?;
    }
    let visible = unsafe { IsWindowVisible(child) } != 0;
    if visible && !status.activated {
        status.activated = true;
        // 原生显示由父提交，随后把可见状态交还Bevy；下帧Winit样式回写仍经本阶段校正。
        window.visible = true;
    }
    let root = unsafe { GetAncestor(parent, GA_ROOT) };
    let root_foreground =
        unsafe { !root.is_null() && GetForegroundWindow() == root && IsIconic(root) == 0 };
    let actual_focus = unsafe { GetFocus() };
    let author_has_focus = actual_focus == child
        || unsafe { !actual_focus.is_null() && IsChild(child, actual_focus) != 0 };
    // 最小化期间Winit可能停帧，不能只依赖“曾观察到后台→前台”的边沿。
    // 恢复时系统若把焦点留在已核验宿主，主动归还可见作者；工程视口或其他子窗口持有焦点时不抢夺。
    let host_has_focus = actual_focus == parent || actual_focus == root;
    if should_restore_focus(
        status.activated && visible,
        root_foreground,
        state.focus_settled,
        state.root_was_foreground,
        state.play_return_requested,
        close_active && !author_has_focus,
        host_has_focus,
    ) {
        // 只在实际前台已属于本工作台时，恢复本线程的作者键盘焦点。Alt+Tab回工作台、
        // 关闭确认必须由作者窗口接收输入：系统X/游戏退役可能把焦点留在父宿主或失效窗口。
        // 仅在本工作台已处前台且确认层实际失焦时取回，不提升后台窗口，也不反复抢焦点。
        unsafe { SetFocus(child) };
    }
    state.focus_settled |= status.activated;
    state.root_was_foreground = root_foreground;
    state.play_return_requested = false;
    let native_focused = unsafe {
        let actual = GetFocus();
        status.activated
            && visible
            && root_foreground
            && (actual == child || !actual.is_null() && IsChild(child, actual) != 0)
    };
    if window.focused != native_focused {
        window.focused = native_focused;
    }
    if state.native_focused != native_focused {
        state.native_focused = native_focused;
        focused.write(WindowFocused {
            window: entity,
            focused: native_focused,
        });
    }
    Ok(())
}

fn sync_native_size(
    state: &mut EmbedState,
    parent: HWND,
    child: HWND,
    style_changed: bool,
) -> Result<(), String> {
    let size = unsafe { client_size(parent)? };
    // Winit仍持有创建前的位置缓存。窗口事件若把它改回顶层坐标，比较真实父相对矩形后局部纠正，
    // 不能只比较上一帧尺寸，否则同尺寸的漂移会一直留在工作台里。
    if state.last_size != Some(size)
        || style_changed
        || !unsafe { fills_client(parent, child, size) }
    {
        if unsafe {
            SetWindowPos(
                child,
                std::ptr::null_mut(),
                0,
                0,
                size.0 as i32,
                size.1 as i32,
                SWP_NOACTIVATE | SWP_NOZORDER | if style_changed { SWP_FRAMECHANGED } else { 0 },
            )
        } == 0
        {
            return Err(win32_error("无法同步作者子窗口尺寸"));
        }
        state.last_size = Some(size);
    }
    Ok(())
}

fn should_restore_focus(
    active_visible: bool,
    root_foreground: bool,
    settled: bool,
    was_foreground: bool,
    play_return: bool,
    modal_needs_focus: bool,
    host_has_focus: bool,
) -> bool {
    active_visible
        && root_foreground
        && (!settled || !was_foreground || play_return || modal_needs_focus || host_has_focus)
}

fn check_preparation_timeout(state: &EmbedState, detail: &str) -> Result<(), String> {
    if state.started.elapsed() >= PREPARATION_TIMEOUT {
        Err(format!("嵌入作者工作台准备超时：{detail}"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "render_size_tests.rs"]
mod render_size_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_author_returns_foreground_host_focus_and_ui_event() {
        assert!(should_restore_host_focus(true, true));
        assert!(!should_restore_host_focus(true, false), "后台关闭不能抢占其他应用焦点");
        assert!(!should_restore_host_focus(false, true), "隐藏主窗口不能取得焦点");
        let mut world = World::new();
        world.init_resource::<Messages<WindowFocused>>();
        let parent = world.spawn((Window { focused: false, ..default() }, PrimaryWindow)).id();
        let other = world.spawn(Window { focused: false, ..default() }).id();
        publish_host_focus(&mut world);
        assert!(world.get::<Window>(parent).unwrap().focused);
        assert!(!world.get::<Window>(other).unwrap().focused);
        let messages = world.resource::<Messages<WindowFocused>>();
        let mut cursor = messages.get_cursor();
        let events: Vec<_> = cursor.read(messages).collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].window, parent);
        assert!(events[0].focused, "原窗口焦点事件须同步UI输入门");
    }

    #[test]
    fn native_child_identity_survives_style_rewrite_but_not_real_reparenting() {
        struct HiddenWindow(HWND);
        impl HiddenWindow {
            fn new(parent: Option<HWND>) -> Self {
                let class: Vec<_> = "STATIC".encode_utf16().chain(Some(0)).collect();
                let window = unsafe {
                    CreateWindowExW(
                        0,
                        class.as_ptr(),
                        std::ptr::null(),
                        if parent.is_some() { WS_CHILD } else { 0 },
                        0,
                        0,
                        16,
                        16,
                        parent.unwrap_or(std::ptr::null_mut()),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null(),
                    )
                };
                assert!(!window.is_null(), "应能创建本测试拥有的隐藏窗口");
                Self(window)
            }
        }
        impl Drop for HiddenWindow {
            fn drop(&mut self) {
                unsafe { DestroyWindow(self.0) };
            }
        }

        // 不显示、不聚焦任何窗口；模拟真实父链不变而Winit暂时清除WS_CHILD的竞态。
        let first = HiddenWindow::new(None);
        let second = HiddenWindow::new(None);
        let child = HiddenWindow::new(Some(first.0));
        let pid = std::process::id();
        unsafe { set_style(child.0, GWL_STYLE, 0) }.unwrap();
        assert!(unsafe { child_belongs_to(child.0, first.0, pid) });
        assert_eq!(find_child(first.0 as usize, pid), Some(child.0 as usize));
        assert!(!unsafe { child_belongs_to(child.0, first.0, pid.wrapping_add(1)) });

        // 真正换父后必须拒绝原工程的查询与显示，不按历史句柄或样式恢复来托底。
        unsafe { SetParent(child.0, second.0) };
        assert!(!unsafe { child_belongs_to(child.0, first.0, pid) });
        assert_eq!(find_child(first.0 as usize, pid), None);
        assert!(activate_child(first.0 as usize, pid, child.0 as usize).is_err());
        assert!(unsafe { child_belongs_to(child.0, second.0, pid) });
        assert_eq!(find_child(second.0 as usize, pid), Some(child.0 as usize));
    }

    #[test]
    fn background_play_completion_cannot_activate_editor_or_hidden_candidate() {
        assert!(!should_restore_focus(true, false, true, false, true, false, false));
        assert!(!should_restore_focus(false, true, false, false, true, false, false));
        assert!(!should_restore_focus(true, true, true, true, false, false, false));
        assert!(should_restore_focus(true, true, true, false, false, false, false));
        assert!(should_restore_focus(true, true, true, true, true, false, false));
    }

    #[test]
    fn minimized_author_recovers_host_focus_even_if_no_background_frame_was_observed() {
        assert!(should_restore_focus(true, true, true, true, false, false, true),
            "最小化停帧后前台历史仍为true，宿主焦点仍须归还作者，不依赖打开关闭弹框");
        assert!(!should_restore_focus(true, false, true, true, false, false, true),
            "用户已切到其他应用时不得由残留宿主焦点激活编辑器");
        assert!(!should_restore_focus(false, true, true, true, false, false, true),
            "隐藏候选作者不能截获宿主输入");
        assert!(!should_restore_focus(true, true, true, true, false, false, false),
            "作者或工程视口等其他子窗口已获焦时保留现有输入所有权");
    }

    #[test]
    fn close_prompt_recovers_lost_native_focus_without_activating_background_or_candidates() {
        assert!(should_restore_focus(true, true, true, true, false, true, false),
            "系统X或运行实例退出后，已显示的关闭确认必须取回作者输入焦点");
        assert!(!should_restore_focus(true, false, true, true, false, true, false),
            "用户已切到其他应用时不能抢回焦点");
        assert!(!should_restore_focus(false, true, true, true, false, true, false),
            "隐藏候选不能因关闭意图获得输入");
        assert!(!should_restore_focus(true, true, true, true, false, false, false),
            "确认层已有作者焦点时不能重复设置");
    }

    #[test]
    fn embed_identity_is_optional_but_must_be_complete_and_nonzero() {
        assert_eq!(parse_target(None, None).unwrap(), None);
        assert_eq!(
            parse_target(Some("1234".into()), Some("5678".into())).unwrap(),
            Some(EmbedTarget {
                parent: 1234,
                pid: 5678
            })
        );
        for (hwnd, pid) in [
            (Some("1234".into()), None),
            (None, Some("5678".into())),
            (Some("0".into()), Some("5678".into())),
            (Some("1234".into()), Some("0".into())),
            (Some("not-a-handle".into()), Some("5678".into())),
        ] {
            assert!(parse_target(hwnd, pid).is_err());
        }
    }

    #[test]
    fn embedded_styles_keep_visibility_but_remove_independent_window_chrome() {
        let original = WS_OVERLAPPEDWINDOW | WS_POPUP | WS_VISIBLE | WS_DISABLED;
        let embedded = child_style(original);
        assert_eq!(embedded & (WS_OVERLAPPEDWINDOW | WS_POPUP), 0);
        assert_eq!(
            embedded & (WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN),
            WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN
        );
        assert_ne!(embedded & WS_VISIBLE, 0);
        assert_ne!(embedded & WS_DISABLED, 0);
        assert_eq!(child_style(embedded), embedded);
        let extended = child_extended_style(WS_EX_APPWINDOW | WS_EX_ACCEPTFILES);
        assert_eq!(extended & WS_EX_APPWINDOW, 0);
        assert_ne!(extended & WS_EX_TOOLWINDOW, 0);
        assert_ne!(extended & WS_EX_ACCEPTFILES, 0);
        assert_eq!(child_extended_style(extended), extended);
    }
}
