//! 跨卷移动逐项持有源和目标句柄，核验同一对象后再按源句柄删除。

use super::{Control, filesystem::{reject_link, reject_link_chain}, tree_manifest::{Entry, FileKey}};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED},
    Storage::FileSystem::{
        DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_GENERIC_READ, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FileDispositionInfo, FileIdInfo, GetFileInformationByHandleEx, SYNCHRONIZE, SetFileInformationByHandle,
    },
};

pub(super) fn file_key(file: &File, path: &Path) -> Result<FileKey, String> {
    let mut info = FILE_ID_INFO::default();
    // FileIdInfo 的卷序列号加 128 位 ID 属于此句柄，能识别原路径被同长度、同时间的文件替换。
    if unsafe { GetFileInformationByHandleEx(file.as_raw_handle(), FileIdInfo,
        (&raw mut info).cast(), std::mem::size_of::<FILE_ID_INFO>() as u32) } == 0
    {
        let error = io::Error::last_os_error();
        let reason = if matches!(error.raw_os_error().map(|code| code as u32),
            Some(ERROR_INVALID_FUNCTION | ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED))
        {
            "此卷或 Windows 版本不支持 FileIdInfo"
        } else { "无法从句柄取得文件 ID" };
        return Err(format!("{reason}，源保留 {}：{error}", path.display()));
    }
    let id = info.FileId.Identifier;
    if id == [0; 16] { return Err(format!("文件系统未返回有效文件 ID，源保留：{}", path.display())); }
    Ok(FileKey { volume: info.VolumeSerialNumber, id })
}

pub(super) fn lock_source(path: &Path, entry: &Entry, removing: bool) -> Result<(File, FileKey), String> {
    reject_link_chain(path)?;
    let mut options = OpenOptions::new();
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    if entry.directory {
        // 目录允许外部新建子项。最后仅删除空目录，原清单以外的内容仍留在源目录。
        options.access_mode(DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    } else {
        // 复制和清理阶段均先检查 DELETE 权限。持有此句柄期间拒绝外部写入和改名。
        options.access_mode(DELETE | FILE_GENERIC_READ).share_mode(FILE_SHARE_READ);
    }
    let file = options.open(path).map_err(|error| {
        format!("无法打开跨卷移动源「{}」，源可能被占用或缺少访问权限（{error}）", path.display())
    })?;
    let metadata = file.metadata().map_err(|error| format!("无法读取源句柄 {}：{error}", path.display()))?;
    reject_link(path, &metadata)?;
    let key = file_key(&file, path)?;
    // 移除子项本身可能改变目录的时间或大小。清理目录依靠原 ID 与空目录约束。
    if metadata.is_dir() != entry.directory
        || entry.file_key.as_ref().is_some_and(|expected| expected.volume != key.volume || expected.id != key.id)
        || !(entry.directory && removing) && !entry.matches(&metadata)
    {
        return Err(format!("源文件身份或内容已改变，跨卷移动停止：{}", path.display()));
    }
    Ok((file, key))
}

pub(super) fn verify_copy(
    source: &mut File, target: &Path, left: &mut [u8], right: &mut [u8], control: &Control<'_>,
) -> Result<File, String> {
    let size = left.len().min(right.len());
    if size == 0 { return Err("跨卷核验缺少缓冲区，源文件保留".into()); }
    control.check()?;
    reject_link_chain(target)?;
    // 读到的目标保持打开至源删除完成，校验后不会被另一个写入者替换或截断。
    let mut copied = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(target)
        .map_err(|error| format!("无法锁定已复制目标 {}：{error}", target.display()))?;
    let metadata = copied.metadata().map_err(|error| format!("无法读取目标句柄 {}：{error}", target.display()))?;
    reject_link(target, &metadata)?;
    source.rewind().map_err(|error| format!("无法重新读取源文件以核验目标 {}：{error}", target.display()))?;

    loop {
        control.check()?;
        let count = match source.read(&mut left[..size]) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("核验目标 {} 时读取源失败：{error}", target.display())),
        };
        if count == 0 {
            match copied.read(&mut right[..1]) {
                Ok(0) => return Ok(copied),
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(format!("无法读取复制目标 {}：{error}", target.display())),
            }
        }
        // 两端短读长度可能不同。以源本次读取长度为准，目标必须填满后才能逐字节比较。
        match copied.read_exact(&mut right[..count]) {
            Ok(()) if left[..count] == right[..count] => {},
            Ok(()) => break,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(format!("无法读取复制目标 {}：{error}", target.display())),
        }
    }
    Err(format!("复制目标长度或内容与源不一致，源文件保留：{}", target.display()))
}

pub(super) fn delete_locked(source: &File, path: &Path) -> Result<(), String> {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // 不重新解析路径。目录新增子项时，Windows 拒绝删除非空目录。
    if unsafe { SetFileInformationByHandle(source.as_raw_handle(), FileDispositionInfo,
        (&raw const disposition).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) } == 0
    {
        return Err(format!("无法删除已核验源，目录可能有新内容或文件被占用 {}：{}",
            path.display(), io::Error::last_os_error()));
    }
    Ok(())
}
