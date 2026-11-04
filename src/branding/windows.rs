//! 为真实Winit窗口设置exe中同一组多尺寸图标，覆盖标题栏、任务栏与Alt+Tab。
use bevy::{
    ecs::system::NonSendMarker,
    prelude::*,
    window::{RawHandleWrapper, WindowScaleFactorChanged},
};
use windows_sys::Win32::{
    System::LibraryLoader::GetModuleHandleW,
    UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
};

pub(super) fn apply_window_icons(
    // Win32窗口操作必须在Winit主线程执行；不在ECS工作线程同步SendMessage。
    _main_thread: NonSendMarker,
    windows: Query<(Entity, Ref<RawHandleWrapper>)>,
    mut scale_changes: MessageReader<WindowScaleFactorChanged>,
) {
    let changed: Vec<_> = scale_changes.read().map(|event| event.window).collect();
    for (entity, raw) in &windows {
        if !raw.is_added() && !changed.contains(&entity) {
            continue;
        }
        let raw_window_handle::RawWindowHandle::Win32(handle) = raw.get_window_handle() else {
            continue;
        };
        let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        // LR_SHARED图标由当前模块持有，使用期间不销毁；DPI切换时重新选择合适分辨率。
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let dpi = GetDpiForWindow(hwnd).max(96);
            let mut adopted = 0;
            for (kind, logical_size) in [(ICON_SMALL, 16), (ICON_BIG, 32)] {
                let size = (logical_size * dpi / 96) as i32;
                let icon = LoadImageW(
                    module,
                    1usize as *const u16,
                    IMAGE_ICON,
                    size,
                    size,
                    LR_SHARED,
                );
                if icon.is_null() {
                    error!(
                        "无法加载Feathrise窗口图标，Win32={}",
                        windows_sys::Win32::Foundation::GetLastError()
                    );
                    continue;
                }
                SendMessageW(hwnd, WM_SETICON, kind as usize, icon as isize);
                if SendMessageW(hwnd, WM_GETICON, kind as usize, 0) != icon as isize {
                    error!("Feathrise窗口未采用图标：kind={kind}");
                } else {
                    adopted += 1;
                }
            }
            if adopted == 2 {
                info!("Feathrise窗口大小图标均已采用，DPI={dpi}");
            }
        }
    }
}
