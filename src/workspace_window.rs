//! 同窗口作者呈现的统一边界；Windows沿用原生子窗口，Unix合成原作者的离屏画面。

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;

#[cfg(unix)]
pub(crate) use crate::portable_author::{EmbedTarget, EmbeddedAuthorPlugin, EmbeddedAuthorStatus};

#[cfg(unix)]
pub(crate) fn primary_hwnd(raw: &bevy::window::RawHandleWrapper) -> Option<usize> {
    // Unix不把指针伪装成HWND；主宿主用此值传递X11对话框父窗，生命周期不再依赖它存在。
    match raw.get_window_handle() {
        raw_window_handle::RawWindowHandle::Xlib(handle) => Some(handle.window as usize),
        raw_window_handle::RawWindowHandle::Xcb(handle) => Some(handle.window.get() as usize),
        _ => None,
    }
}

#[cfg(unix)]
pub(crate) fn parent_is_valid(_parent: usize, pid: u32) -> bool {
    pid != 0 && unsafe { libc::kill(pid as libc::pid_t, 0) } == 0
}

#[cfg(unix)]
pub(crate) fn restore_host_focus(world: &mut bevy::prelude::World, _parent: usize) -> Result<(), String> {
    // 原生焦点始终属于主窗口；只撤掉已退役工程的合成图，不提升其他应用背后的窗口。
    crate::portable_author::host::clear(world);
    Ok(())
}
