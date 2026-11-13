//! 可恢复删除的平台适配；Windows使用Shell回收事务，其余平台明确报错，不隐式永久删除。

use std::path::Path;

#[cfg(windows)]
pub(super) fn delete(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{core::PCWSTR, Win32::{System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED},
        UI::Shell::{FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
            FOF_ALLOWUNDO, FOF_NOERRORUI, FOF_SILENT, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR, FOF_WANTNUKEWARNING,
            FOFX_ADDUNDORECORD, FOFX_RECYCLEONDELETE, FOFX_EARLYFAILURE}}};
    struct Apartment;
    impl Drop for Apartment { fn drop(&mut self) { unsafe { CoUninitialize(); } } }
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(|error| format!("无法初始化系统回收站：{error}"))?;
        let _apartment = Apartment;
        let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("系统回收站不可用，文件保留：{error}"))?;
        // 明确请求回收站和系统撤销记录；保留系统对不可恢复删除的确认，禁止静默永久删除的回退。
        operation.SetOperationFlags(FOF_ALLOWUNDO | FOF_NOERRORUI | FOF_SILENT | FOF_NOCONFIRMMKDIR | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING
            | FOFX_ADDUNDORECORD | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE)
            .map_err(|error| error.to_string())?;
        // Shell解析器使用普通盘符路径；canonicalize的\\?\前缀是文件IO约定，不能直接交给Shell命名空间。
        let display = path.to_string_lossy();
        let shell_path = if let Some(unc) = display.strip_prefix("\\\\?\\UNC\\") { format!("\\\\{unc}") }
            else { display.strip_prefix("\\\\?\\").unwrap_or(&display).to_owned() };
        let wide: Vec<_> = std::ffi::OsStr::new(&shell_path).encode_wide().chain(Some(0)).collect();
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).map_err(|error| error.to_string())?;
        operation.DeleteItem(&item, None).map_err(|error| error.to_string())?;
        operation.PerformOperations().map_err(|error| format!("回收站删除失败，未完成文件保留：{error}"))?;
        if operation.GetAnyOperationsAborted().map_err(|error| error.to_string())?.as_bool() {
            return Err("回收站操作已取消，未完成文件保留".into());
        }
    }
    if path.exists() { return Err("系统回收站未移走源文件，请查看系统提示".into()); }
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn delete(_path: &Path) -> Result<(), String> {
    Err("当前平台尚未接入可恢复回收站，文件保留；不会改成永久删除".into())
}
