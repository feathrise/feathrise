//! Windows主工作台的窗口记忆。启动时恢复原生位置，正常退出时交给宿主的个人偏好写入槽。

use crate::{user_preferences::WindowPlacement, workspace_window::primary_hwnd};
use bevy::{
    ecs::system::NonSendMarker,
    prelude::*,
    window::{PrimaryWindow, RawHandleWrapper, WindowMode},
};
use std::{io, mem::size_of, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, RECT},
    Graphics::Gdi::{
        CombineRgn, CreateRectRgnIndirect, DeleteObject, EnumDisplayMonitors, GetMonitorInfoW,
        GetRgnBox, HDC, HMONITOR, HRGN, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        NULLREGION, RGN_DIFF,
    },
    UI::HiDpi::GetDpiForWindow,
    UI::WindowsAndMessaging::{
        GetWindowPlacement, GetWindowRect, SW_HIDE, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED,
        SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPlacement, SetWindowPos, WINDOWPLACEMENT,
        WPF_RESTORETOMAXIMIZED,
    },
};

#[derive(Resource)]
pub(super) struct MainWindowPlacement {
    saved: Option<WindowPlacement>,
    exit_save_started: bool,
}

pub(crate) fn install(app: &mut App, saved: Option<WindowPlacement>) {
    app.insert_resource(MainWindowPlacement {
        saved,
        exit_save_started: false,
    })
    .add_systems(Startup, restore);
}

fn restore(
    _thread: NonSendMarker,
    mut placement: ResMut<MainWindowPlacement>,
    mut windows: Query<(&mut Window, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
) {
    let Some(saved) = placement.saved.take() else {
        return;
    };
    let Ok((mut window, raw)) = windows.single_mut() else {
        return;
    };
    // PreStartup已装好自绘框架。先在隐藏的HWND上定位，再让Winit显示，避免显示旧位置或普通尺寸的过渡帧。
    window.visible = true;
    let Some(hwnd) = raw.and_then(primary_hwnd) else {
        return;
    };
    let hwnd = hwnd as HWND;
    let [left, top, right, bottom] = saved.normal_rect;
    let native = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        showCmd: SW_HIDE as u32,
        rcNormalPosition: RECT {
            left,
            top,
            right,
            bottom,
        },
        ..Default::default()
    };
    let previous_dpi = unsafe { GetDpiForWindow(hwnd) };
    if unsafe { SetWindowPlacement(hwnd, &native) } == 0 {
        warn!("无法恢复主窗口位置：{}", io::Error::last_os_error());
        return;
    }
    // 隐藏窗口可能从系统默认屏跨到另一种DPI。等Winit处理这次缩放后，再应用已记录的物理尺寸。
    if unsafe { GetDpiForWindow(hwnd) != previous_dpi && SetWindowPlacement(hwnd, &native) == 0 } {
        warn!("无法恢复跨DPI的主窗口位置：{}", io::Error::last_os_error());
    }
    // SetWindowPlacement先处理完全离屏的旧坐标。这里只使用实际屏幕矩形，补齐部分离屏和过大窗口。
    if let Err(error) = fit_to_work_area(hwnd) {
        warn!("无法将主窗口移回当前工作区：{error}");
    }
    if saved.maximized {
        window.set_maximized(true);
    }
}

pub(crate) fn capture_on_exit(world: &mut World) -> Option<WindowPlacement> {
    let mut state = world.get_resource_mut::<MainWindowPlacement>()?;
    // 退出阶段只尝试一次。后台失败会保留原文件，退出门不会反复排同一写入。
    if std::mem::replace(&mut state.exit_save_started, true) {
        return None;
    }
    let mut windows = world.query_filtered::<(&Window, &RawHandleWrapper), With<PrimaryWindow>>();
    let (window, raw) = windows.single(world).ok()?;
    if window.mode != WindowMode::Windowed {
        return None;
    }
    let hwnd = primary_hwnd(raw)? as HWND;
    match read(hwnd) {
        Ok(placement) => Some(placement),
        Err(error) => {
            warn!("无法读取退出前的主窗口位置：{error}");
            None
        }
    }
}

pub(crate) fn cancel_exit(world: &mut World) {
    if let Some(mut placement) = world.get_resource_mut::<MainWindowPlacement>() {
        placement.exit_save_started = false;
    }
}

pub(super) fn read(hwnd: HWND) -> io::Result<WindowPlacement> {
    let mut native = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    if unsafe { GetWindowPlacement(hwnd, &mut native) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let RECT {
        left,
        top,
        right,
        bottom,
    } = native.rcNormalPosition;
    let placement = WindowPlacement {
        normal_rect: [left, top, right, bottom],
        // 最小化只影响当前会话。如果系统记录了从最大化进入最小化，重开仍恢复最大化。
        maximized: native.showCmd == SW_SHOWMAXIMIZED as u32
            || (native.showCmd == SW_SHOWMINIMIZED as u32
                && native.flags & WPF_RESTORETOMAXIMIZED != 0),
    };
    if !placement.is_valid() {
        return Err(io::Error::other("系统返回了无效的主窗口还原矩形"));
    }
    Ok(placement)
}

pub(super) fn work_area(hwnd: HWND) -> io::Result<RECT> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    monitor_work_area(monitor)
}

fn monitor_work_area(monitor: HMONITOR) -> io::Result<RECT> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.rcWork.right <= info.rcWork.left || info.rcWork.bottom <= info.rcWork.top {
        return Err(io::Error::other("当前显示器没有有效工作区"));
    }
    Ok(info.rcWork)
}

pub(super) fn outer_rect(hwnd: HWND) -> io::Result<RECT> {
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(rect)
}

pub(super) fn fit_to_work_area(hwnd: HWND) -> io::Result<()> {
    let rect = outer_rect(hwnd)?;
    let work = work_area(hwnd)?;
    let width = (rect.right - rect.left).min(work.right - work.left);
    let height = (rect.bottom - rect.top).min(work.bottom - work.top);
    let left = rect.left.clamp(work.left, work.right - width);
    let top = rect.top.clamp(work.top, work.bottom - height);
    if (left, top, left + width, top + height) == (rect.left, rect.top, rect.right, rect.bottom)
        || covered_by_work_areas(&rect)
    {
        return Ok(());
    }
    // 系统刚切换显示器时不抢焦点。坐标来自GetWindowRect，和WINDOWPLACEMENT的工作区坐标分开使用。
    if unsafe {
        SetWindowPos(
            hwnd,
            null_mut(),
            left,
            top,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn covered_by_work_areas(rect: &RECT) -> bool {
    // 有意横跨两块屏幕的窗口也可以完整可见。只有实际存在未被工作区覆盖的部分才收回单屏。
    unsafe extern "system" fn subtract_monitor(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> i32 {
        let Ok(work) = monitor_work_area(monitor) else {
            return 1;
        };
        let work_region = unsafe { CreateRectRgnIndirect(&work) };
        if work_region.is_null() {
            return 1;
        }
        let remaining = data as HRGN;
        let result = unsafe { CombineRgn(remaining, remaining, work_region, RGN_DIFF) };
        unsafe { DeleteObject(work_region) };
        i32::from(result != NULLREGION)
    }

    let remaining = unsafe { CreateRectRgnIndirect(rect) };
    if remaining.is_null() {
        return false;
    }
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            rect,
            Some(subtract_monitor),
            remaining as LPARAM,
        );
        let covered = GetRgnBox(remaining, &mut RECT::default()) == NULLREGION;
        DeleteObject(remaining);
        covered
    }
}
