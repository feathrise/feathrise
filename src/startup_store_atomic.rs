//! 场景文档的同目录临时写与原子发布；已有文件使用ReplaceFileW，首存使用不覆盖的原子移动。

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(super) fn replace(
    path: &Path,
    bytes: &[u8],
    existed: bool,
    verify: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let parent = path.parent().ok_or("场景文件没有父目录")?;
    let mut candidate = None;
    for _ in 0..16 {
        let temporary = parent.join(format!(
            ".scene-layout-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => {
                candidate = Some((temporary, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(format!("无法准备场景临时文件：{error}")),
        }
    }
    let (temporary, mut file) = candidate.ok_or("无法分配场景临时文件")?;
    let result = (|| {
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("场景临时文件写入失败：{e}"))?;
        drop(file);
        verify()?;
        if existed {
            atomic_replace(&temporary, path)
        } else {
            atomic_create(&temporary, path)
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn atomic_create(temporary: &Path, path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let temporary: Vec<_> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    // 同目录移动不使用REPLACE_EXISTING或COPY_ALLOWED；目标若被外部创建就失败，不能覆盖。
    if unsafe { MoveFileExW(temporary.as_ptr(), path.as_ptr(), 0) } == 0 {
        return Err(format!(
            "场景首次原子发布失败（目标可能已存在）：{}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_create(temporary: &Path, path: &Path) -> Result<(), String> {
    fs::hard_link(temporary, path).map_err(|e| format!("场景首次原子发布失败：{e}"))?;
    fs::remove_file(temporary).map_err(|e| format!("场景已保存，但清理临时链接失败：{e}"))
}

#[cfg(windows)]
fn atomic_replace(temporary: &Path, path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let temporary: Vec<_> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        ReplaceFileW(
            path.as_ptr(),
            temporary.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(format!(
            "原子替换场景失败：{}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(temporary: &Path, path: &Path) -> Result<(), String> {
    fs::rename(temporary, path).map_err(|e| format!("原子替换场景失败：{e}"))
}
