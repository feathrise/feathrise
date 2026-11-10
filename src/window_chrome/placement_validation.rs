//! 真实主窗口的保存、重开与显示区域回退验收。只对标记过的无工程夹具发送窗口操作。

use super::{placement, resources::ChromeParts};
use crate::{
    native_plugins::management::packages::global_plugins_root,
    native_project_close::{NativeProjectCloseState, confirm_workspace_validation_close},
    workspace_ui::WorkspaceModel,
    workspace_window::primary_hwnd,
};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    window::{PrimaryWindow, RawHandleWrapper},
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    ptr::null_mut,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromWindow},
    UI::WindowsAndMessaging::{
        GetClientRect, GetSystemMetrics, IsIconic, IsWindowVisible, IsZoomed, PostMessageW,
        SC_MAXIMIZE, SM_CXVIRTUALSCREEN, SM_XVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOZORDER,
        SetWindowPos, WM_CLOSE, WM_DISPLAYCHANGE, WM_SYSCOMMAND,
    },
};

#[derive(Resource)]
pub(crate) struct Plan {
    report_file: PathBuf,
    screenshot: PathBuf,
    started: Instant,
    positioned_at: Option<Instant>,
    offscreen_rect: Option<[i32; 4]>,
    evidence: Option<Value>,
    capture_attempts: u8,
    closing: bool,
    exercise: bool,
    maximized: bool,
    changed_display: bool,
    live: bool,
}

pub(crate) fn prepare(has_project: bool) -> Result<Option<Plan>, String> {
    let Some(root) =
        std::env::var_os("FEATHRISE_WINDOW_PLACEMENT_VALIDATION_DIR").map(PathBuf::from)
    else {
        return Ok(None);
    };
    if has_project || !root.is_absolute() {
        return Err("窗口记忆验收只接受无工程工作台和绝对夹具目录".into());
    }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    if fs::read(root.join(".placement-owner")).ok().as_deref()
        != Some(b"feathrise-window-placement-v1")
    {
        return Err("窗口记忆验收缺少本轮夹具归属标记".into());
    }
    let preferences = std::env::var_os("FEATHRISE_PREFERENCES_FILE").map(PathBuf::from);
    let isolated = preferences.as_ref().is_some_and(|path| {
        path.is_absolute()
            && path.file_name() == Some(std::ffi::OsStr::new("isolated-preferences.json"))
            && path.parent().and_then(|parent| parent.canonicalize().ok()) == Some(root.clone())
    });
    if !isolated
        || global_plugins_root()?.canonicalize().ok() != Some(root.join("isolated-global-plugins"))
    {
        return Err("窗口记忆验收必须在夹具内隔离个人偏好和全局插件".into());
    }
    let phase = std::env::var("FEATHRISE_WINDOW_PLACEMENT_VALIDATION_PHASE").unwrap_or_default();
    let (exercise, maximized, changed_display, live) = match phase.as_str() {
        "normal-save" => (true, false, false, false),
        "normal-reopen" => (false, false, false, false),
        "maximized-save" => (true, true, false, false),
        "maximized-reopen" => (false, true, false, false),
        "display-reopen" => (false, false, true, false),
        "display-live" => (true, false, true, true),
        _ => return Err("窗口记忆验收阶段无效".into()),
    };
    // 各阶段允许共享同轮偏好，但拒绝覆盖任何旧的输出。
    let report_file = root.join(format!("{phase}.json"));
    let screenshot = root.join(format!("{phase}.png"));
    if screenshot.exists() {
        return Err("窗口记忆验收拒绝复用截图路径".into());
    }
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&report_file)
        .map_err(|error| format!("窗口记忆验收报告路径必须独占：{error}"))?;
    Ok(Some(Plan {
        report_file,
        screenshot,
        started: Instant::now(),
        positioned_at: None,
        offscreen_rect: None,
        evidence: None,
        capture_attempts: 0,
        closing: false,
        exercise,
        maximized,
        changed_display,
        live,
    }))
}

pub(crate) fn drive(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<Plan>() else {
        return;
    };
    if let Err(error) = step(world, &mut plan) {
        error!("窗口记忆验收失败：{error}");
        world.write_message(AppExit::error());
    } else {
        world.insert_resource(plan);
    }
}

fn step(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    if plan.started.elapsed() > Duration::from_secs(90) {
        return Err("等待真实主窗口、截图或退出确认超时".into());
    }
    if world.resource::<WorkspaceModel>().project.is_some() {
        return Err("窗口记忆验收不能操作已打开的工程".into());
    }
    if plan.closing {
        if world.resource::<NativeProjectCloseState>().active() {
            world
                .run_system_once(confirm_workspace_validation_close)
                .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    if world.resource::<ChromeParts>().0.is_none() {
        return Ok(());
    }
    let mut windows = world.query_filtered::<&RawHandleWrapper, With<PrimaryWindow>>();
    let Some(hwnd) = windows
        .single(world)
        .ok()
        .and_then(primary_hwnd)
        .map(|hwnd| hwnd as HWND)
    else {
        return Ok(());
    };
    if unsafe { IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 } {
        return Ok(());
    }
    let native = |error: io::Error| error.to_string();
    if let Some(evidence) = &plan.evidence {
        if plan.screenshot.metadata().is_ok_and(|file| file.len() > 0) {
            // 截图回读后走实际WM_CLOSE和原确认。下一进程才能读取已完成写入的偏好。
            let mut report = OpenOptions::new()
                .write(true)
                .open(&plan.report_file)
                .map_err(native)?;
            let bytes = serde_json::to_vec_pretty(evidence).map_err(|error| error.to_string())?;
            report.write_all(&bytes).map_err(native)?;
            post(hwnd, WM_CLOSE, 0)?;
            plan.closing = true;
        }
        return Ok(());
    }

    let work = placement::work_area(hwnd).map_err(native)?;
    let Some(positioned_at) = plan.positioned_at else {
        if plan.exercise {
            let width = (work.right - work.left - 80).clamp(320, 1080);
            let height = (work.bottom - work.top - 90).clamp(240, 700);
            let left = if plan.live {
                unsafe {
                    GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN) + 48
                }
            } else {
                work.left + 40
            };
            let top = work.top + 45;
            if unsafe {
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    left,
                    top,
                    width,
                    height,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                )
            } == 0
            {
                return Err(io::Error::last_os_error().to_string());
            }
            if plan.maximized {
                post(hwnd, WM_SYSCOMMAND, SC_MAXIMIZE as usize)?;
            }
            if plan.live {
                // 用真正离开工作区的窗口和原生消息触发生产回退，不修改系统显示设置。
                let offscreen = placement::outer_rect(hwnd).map_err(native)?;
                if !unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL) }.is_null() {
                    return Err("未能把本轮夹具窗口移到工作区以外".into());
                }
                plan.offscreen_rect = Some(coords(offscreen));
                post(hwnd, WM_DISPLAYCHANGE, 32)?;
            }
        }
        plan.positioned_at = Some(Instant::now());
        return Ok(());
    };
    if positioned_at.elapsed() < Duration::from_millis(350)
        || (unsafe { IsZoomed(hwnd) != 0 }) != plan.maximized
    {
        return Ok(());
    }
    let outer = placement::outer_rect(hwnd).map_err(native)?;
    if plan.changed_display
        && (outer.left < work.left
            || outer.top < work.top
            || outer.right > work.right
            || outer.bottom > work.bottom)
    {
        return Err(format!(
            "显示变化后的主窗口未回到工作区：{:?} / {:?}",
            coords(outer),
            coords(work)
        ));
    }
    let mut client = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if plan.capture_attempts >= 16 {
        return Err("16次GPU回读内未捕获到已绘制的工作台".into());
    }
    plan.capture_attempts += 1;
    plan.evidence = Some(json!({
        "pid": std::process::id(),
        "outer_rect": coords(outer),
        "work_area": coords(work),
        "client_size": [client.right, client.bottom],
        "placement": placement::read(hwnd).map_err(native)?,
        "display_notification_simulated": plan.live,
        "offscreen_before_notification": plan.offscreen_rect,
        "capture_attempts": plan.capture_attempts,
    }));
    let mut save = save_to_disk(plan.screenshot.clone());
    let expected_size = [client.right as u32, client.bottom as u32];
    world.spawn(Screenshot::primary_window()).observe(
        move |capture: On<ScreenshotCaptured>, mut plan: ResMut<Plan>| {
            let image = &capture.image;
            // 冷启动的首批回读可能只有背景色。等待真实绘制，且截图尺寸必须与原生客户区一致。
            if [image.width(), image.height()] == expected_size && has_workbench_pixels(image) {
                save(capture);
            } else {
                plan.evidence = None;
                plan.positioned_at = Some(Instant::now());
            }
        },
    );
    Ok(())
}

fn has_workbench_pixels(image: &Image) -> bool {
    // 隔离夹具使用默认深色工作台。只在左上区域定量采样，确认标题或菜单的亮像素已绘制。
    (0..image.height().min(900)).step_by(8).any(|y| {
        (0..image.width().min(1440)).step_by(8).any(|x| {
            image.get_color_at(x, y).ok().is_some_and(|color| {
                let color = color.to_srgba();
                color.red.max(color.green).max(color.blue) > 0.5
            })
        })
    })
}

fn post(hwnd: HWND, message: u32, value: usize) -> Result<(), String> {
    if unsafe { PostMessageW(hwnd, message, value, 0) } == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}

fn coords(rect: RECT) -> [i32; 4] {
    [rect.left, rect.top, rect.right, rect.bottom]
}
