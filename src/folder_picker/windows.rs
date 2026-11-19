//! Windows原生文件／目录选择器的单在途STA任务，只取消本任务拥有的对话框。
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
};
use windows::{
    Win32::{Foundation::HWND, System::Com::*, UI::Shell::*},
    core::PCWSTR,
};
use windows_sys::Win32::{
    Foundation::{HWND as NativeHwnd, LPARAM},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::*,
};

pub(crate) struct FolderPicker {
    receiver: Receiver<Result<Option<String>, String>>,
    worker: Option<JoinHandle<()>>,
    owner: usize,
    thread: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
}

impl FolderPicker {
    pub fn start(owner: usize, initial: PathBuf, new: bool) -> Result<Self, String> {
        Self::start_named(owner, initial,
            if new { "选择新工程的父目录" } else { "选择已有工程目录" },
            if new { "选择父目录" } else { "选择工程目录" }, None)
    }
    /// 导出只选择目录；沿用同一受控STA、所属窗口取消和取消优先的完成轮询。
    pub(crate) fn start_export(owner: usize, initial: PathBuf) -> Result<Self, String> {
        Self::start_named(owner, initial, "选择工程导出目录", "选择输出目录", None)
    }
    /// 单文件插件仍复用同一STA和归属取消；平台／架构另由静态读取器校验。
    pub fn start_plugin(owner: usize, initial: PathBuf) -> Result<Self, String> {
        Self::start_named(owner, initial, "选择插件文件", "选择插件", Some(("Feathrise 插件 (*.dll)", "*.dll")))
    }
    pub(crate) fn start_blender(owner: usize, initial: PathBuf) -> Result<Self, String> {
        // 内嵌作者窗的系统对话框归顶层窗口，取消器也使用同一原生所有者。
        let owner = unsafe { GetAncestor(owner as NativeHwnd, GA_ROOT) };
        if owner.is_null() {
            return Err("当前窗口尚未准备好，请重新选择 Blender 程序。".into());
        }
        Self::start_named(owner as usize, initial, "选择 Blender 程序", "选择程序", Some(("可执行文件 (*.exe)", "*.exe")))
    }
    fn start_named(owner: usize, initial: PathBuf, title: &'static str, label: &'static str, filter: Option<(&'static str, &'static str)>) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread_id = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let tid = thread_id.clone();
        let cancelled = cancel.clone();
        let worker = thread::Builder::new()
            .name("new-editor-folder".into())
            .spawn(move || {
                tid.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
                let result = std::panic::catch_unwind(|| choose(owner, &initial, title, label, filter, &cancelled))
                    .unwrap_or_else(|_| Err("系统选择器异常，请重试。".into()));
                let _ = sender.send(result);
            })
            .map_err(|e| format!("无法启动系统选择器：{e}"))?;
        Ok(Self {
            receiver,
            worker: Some(worker),
            owner,
            thread: thread_id,
            cancel,
        })
    }
    pub fn poll(&mut self) -> Option<Result<Option<String>, String>> {
        if !self.worker.as_ref()?.is_finished() {
            return None;
        }
        let _ = self.worker.take()?.join();
        let result = self.receiver
                .try_recv()
                .unwrap_or_else(|_| Err("系统选择器已退出，未返回选择。".into()));
        // 退出确认可能晚于结果入队；取消必须优先，不能在收口时登记一次迟到安装。
        Some(if self.cancel.load(Ordering::Acquire) { Ok(None) } else { result })
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        let tid = self.thread.load(Ordering::Acquire);
        if tid != 0 {
            // 不跨线程直接调用COM；WM_CLOSE由目录选择器自己的模态消息循环处理。
            unsafe {
                EnumThreadWindows(tid, Some(close_owned_dialog), self.owner as LPARAM);
            }
        }
    }
}

unsafe extern "system" fn close_owned_dialog(
    hwnd: NativeHwnd,
    owner: LPARAM,
) -> windows_sys::core::BOOL {
    unsafe {
        if GetWindow(hwnd, GW_OWNER) as usize == owner as usize {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
    }
    1
}

fn choose(
    owner: usize,
    initial: &std::path::Path,
    title: &str,
    label: &str,
    filter: Option<(&str, &str)>,
    cancel: &AtomicBool,
) -> Result<Option<String>, String> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
            .ok()
            .map_err(|e| format!("系统选择器初始化失败：{e}"))?;
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _apartment = Apartment;
        let operation = || -> windows::core::Result<Option<String>> {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(
                dialog.GetOptions()?
                    | if filter.is_some() { FOS_FILEMUSTEXIST } else { FOS_PICKFOLDERS }
                    | FOS_FORCEFILESYSTEM
                    | FOS_PATHMUSTEXIST
                    | FOS_NOCHANGEDIR
                    | FOS_DONTADDTORECENT,
            )?;
            if let Some((filter_name, filter_pattern)) = filter {
                let filter_name = wide(filter_name);
                let filter_pattern = wide(filter_pattern);
                dialog.SetFileTypes(&[Common::COMDLG_FILTERSPEC {
                    pszName: PCWSTR(filter_name.as_ptr()),
                    pszSpec: PCWSTR(filter_pattern.as_ptr()),
                }])?;
            }
            let title = wide(title);
            dialog.SetTitle(PCWSTR(title.as_ptr()))?;
            let label = wide(label);
            dialog.SetOkButtonLabel(PCWSTR(label.as_ptr()))?;
            // 默认~/feathrise可能尚未创建；从最近存在的父目录浏览，避免系统退回无关的文档目录。
            // 目录检查留在本STA后台线程，不能放进工作台每帧输入处理。
            let initial = initial
                .ancestors()
                .find(|path| path.is_dir())
                .unwrap_or(initial);
            let initial = wide(&initial.to_string_lossy());
            if let Ok(folder) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(initial.as_ptr()), None)
            {
                dialog.SetFolder(&folder)?;
            }
            if cancel.load(Ordering::Acquire) {
                return Ok(None);
            }
            if let Err(error) = dialog.Show(Some(HWND(owner as _))) {
                if error.code().0 as u32 == 0x800704c7 {
                    return Ok(None);
                }
                return Err(error);
            }
            let path = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let result = path.to_string();
            CoTaskMemFree(Some(path.0.cast()));
            Ok(Some(result.map_err(|_| {
                windows::core::Error::from_hresult(windows::core::HRESULT(0x80070057u32 as i32))
            })?))
        };
        operation().map_err(|e| format!("系统选择器失败：{e}"))
    }
}

// 系统目录选择器只需要UTF16字符串，不依赖旧原型Win32工作台。
fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }
