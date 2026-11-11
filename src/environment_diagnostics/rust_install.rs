//! Windows Rust 安装提示：后台确认工具缺失，当前可见的编辑器窗口只显示一个原生对话框。

use bevy::{
    prelude::*,
    window::{PrimaryWindow, RawHandleWrapper},
};
use feathrise_editor_ui::localization::EditorLocale;
use std::{
    ptr::null,
    sync::atomic::{AtomicBool, Ordering},
    thread::{Builder, JoinHandle},
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Shell::ShellExecuteW,
        WindowsAndMessaging::{
            GA_ROOT, GetAncestor, IDYES, IsWindowVisible, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO,
            MessageBoxW, SW_SHOWNORMAL,
        },
    },
};

const INSTALL_URL: &str = "https://rust-lang.org/tools/install/";
static REQUESTED: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "validation")]
static DISMISSED: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "validation")]
pub(crate) fn dismissed_for_validation() -> bool {
    DISMISSED.load(Ordering::Acquire)
}

#[cfg(feature = "validation")]
pub(crate) fn dismiss_for_validation(owner: usize, locale: EditorLocale) -> Result<bool, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, GW_ENABLEDPOPUP, GetDlgItem, GetWindow, GetWindowTextW,
        GetWindowThreadProcessId, IDNO, PostMessageW, WM_COMMAND,
    };

    let owner = owner as HWND;
    let window = unsafe { GetWindow(owner, GW_ENABLEDPOPUP) };
    if window.is_null() || window == owner || unsafe { IsWindowVisible(window) } == 0 {
        return Ok(false);
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if pid != std::process::id() {
        return Ok(false);
    }
    let mut title = [0; 128];
    let length = unsafe { GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32) };
    if String::from_utf16_lossy(&title[..length as usize]) != locale.text("rust_install.title") {
        return Ok(false);
    }

    unsafe extern "system" fn find_url(control: HWND, found: isize) -> i32 {
        let mut text = [0; 1024];
        let length = unsafe { GetWindowTextW(control, text.as_mut_ptr(), text.len() as i32) };
        if String::from_utf16_lossy(&text[..length as usize]).contains(INSTALL_URL) {
            // SAFETY：同步枚举期间 found 指向调用方栈上的布尔值。
            unsafe { *(found as *mut bool) = true };
            return 0;
        }
        1
    }

    // 核对本进程真实对话框的可见文字，再走原生“否”按钮回执，避免验收打开浏览器。
    let mut found = false;
    unsafe { EnumChildWindows(window, Some(find_url), (&mut found as *mut bool) as isize) };
    let decline = unsafe { GetDlgItem(window, IDNO) };
    if !found || decline.is_null() {
        return Err("Rust 安装提示缺少官网链接或关闭选项".into());
    }
    if unsafe { PostMessageW(window, WM_COMMAND, IDNO as usize, decline as isize) } == 0 {
        return Err(format!(
            "无法关闭本轮 Rust 安装提示：{}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(true)
}

pub(crate) fn request() {
    // 缺失的是本进程的工具环境。构建、自动迭代和主动预检同时失败也只排一个提示。
    REQUESTED.store(true, Ordering::Release);
}

pub(crate) fn install(app: &mut App) {
    app.add_systems(Last, present);
}

fn present(
    mut dialog: Local<Option<JoinHandle<()>>>,
    windows: Query<&RawHandleWrapper, With<PrimaryWindow>>,
    close: Option<Res<crate::native_project_close::NativeProjectCloseState>>,
    locale: Option<Res<EditorLocale>>,
) {
    if let Some(thread) = dialog.as_ref() {
        // 已显示的提示覆盖同一时间的重复失败；关闭后仍允许下一次操作重新检查。
        REQUESTED.store(false, Ordering::Release);
        if thread.is_finished() {
            let _ = dialog.take().expect("安装对话框线程仍在槽中").join();
        }
        return;
    }
    if close.is_some_and(|close| close.active()) {
        REQUESTED.store(false, Ordering::Release);
        return;
    }
    if !REQUESTED.load(Ordering::Acquire) {
        return;
    }
    let Some(window) = windows
        .single()
        .ok()
        .and_then(crate::workspace_window::primary_hwnd)
    else {
        return;
    };
    // 内嵌作者窗口的拥有者是外层工作台。尚未呈现的候选作者不得弹窗。
    let window = window as HWND;
    let owner = unsafe { GetAncestor(window, GA_ROOT) };
    if owner.is_null() || unsafe { IsWindowVisible(window) } == 0 {
        return;
    }
    if !REQUESTED.swap(false, Ordering::AcqRel) {
        return;
    }
    let owner = owner as usize;
    let locale = locale.as_deref().copied().unwrap_or_default();
    // 等待用户点击只占用一个专用线程，作者帧和构建槽可以继续完成收口。
    match Builder::new()
        .name("feathrise-rust-install".into())
        .spawn(move || show(owner, locale))
    {
        Ok(thread) => *dialog = Some(thread),
        Err(error) => warn!("无法显示 Rust 安装提示：{error}"),
    }
}

fn show(owner: usize, locale: EditorLocale) {
    let owner = owner as HWND;
    let title = wide(locale.text("rust_install.title"));
    let body = wide(&format!(
        "{}\n\n{}\n{}",
        locale.text("rust_install.instructions"),
        locale.text("rust_install.open_page"),
        INSTALL_URL
    ));
    // 只有明确选择“是”才打开浏览器。链接同时显示在正文中，便于手动复制。
    let response = unsafe {
        MessageBoxW(
            owner,
            body.as_ptr(),
            title.as_ptr(),
            MB_ICONWARNING | MB_YESNO | MB_DEFBUTTON2,
        )
    };
    #[cfg(feature = "validation")]
    if response != 0 {
        DISMISSED.store(true, Ordering::Release);
    }
    if response == IDYES {
        let url = wide(INSTALL_URL);
        let opened =
            unsafe { ShellExecuteW(owner, null(), url.as_ptr(), null(), null(), SW_SHOWNORMAL) };
        if opened as isize <= 32 {
            warn!("无法打开 Rust 官方安装页：{INSTALL_URL}");
        }
    } else if response == 0 {
        warn!(
            "无法显示 Rust 安装提示：{}",
            std::io::Error::last_os_error()
        );
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
