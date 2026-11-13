//! 后台受检文件系统操作；完整预检后逐顶层提交，复制先写本轮暂存目录，发布时禁止覆盖。

use super::{Control, MAX_SOURCES, recycle};
use feathrise_editor_project_browser_model::file_operations::*;
use std::{collections::HashSet, fs::{self, File, Metadata, OpenOptions}, io::{Read, Write}, path::{Component, Path, PathBuf}};

const MAX_TREE_ENTRIES: usize = 100_000;
const MAX_PATH_BYTES: usize = 32 * 1024 * 1024;
const BUFFER_SIZE: usize = 256 * 1024;

#[derive(Clone)]
pub(super) struct Entry { relative: PathBuf, metadata: Metadata }

/// Windows给文件路径附加空相对段会产生尾随分隔符；根条目必须直接使用原路径。
fn member(root: &Path, relative: &Path) -> PathBuf { if relative.as_os_str().is_empty() { root.to_owned() } else { root.join(relative) } }

pub(super) fn execute(request: &FileOperationRequest, control: &Control<'_>, receipt: &mut FileOperationReceipt) -> Result<(), String> {
    control.check()?;
    let mut roots = Vec::new();
    for path in &request.allowed_roots {
        absolute(path)?;
        // 授权目录可能已被外部工具移走，不能让一个失效历史地址毒化其他窗口。
        // 这里只保留当前可访问目录；每个实际源和目标仍须通过checked命中有效根，绝不扩大授权边界。
        if let Ok(root) = path.canonicalize() {
            if root.is_dir() && !roots.contains(&root) { roots.push(root); }
        }
    }
    if roots.is_empty() { return Err("已打开的目录均已失效或无法访问，请重新打开一个有效目录；未操作任何文件".into()); }
    match &request.operation {
        FileOperation::Copy { sources, destination, conflict } | FileOperation::Move { sources, destination, conflict } => {
            let destination = checked(destination, &roots, true)?;
            if !destination.is_dir() { return Err("粘贴目标必须是文件夹".into()); }
            let sources = sources_checked(sources, &roots)?;
            let moving = matches!(request.operation, FileOperation::Move { .. });
            // 整批先核对路径；不能复制一半之后才发现后面条目越界或移入自身后代。
            for source in &sources {
                if moving { protect_root(source, &roots)?; }
                if source.is_dir() && destination.starts_with(source) {
                    return Err(format!("不能把文件夹放入自身或子文件夹：{}", source.display()));
                }
            }
            for (index, source) in sources.iter().enumerate() {
                control.check()?;
                control.update(Some(source), 0, 0);
                let file_name = source.file_name().ok_or("源路径没有文件名")?;
                let requested = destination.join(file_name);
                if moving && requested == *source { receipt.skipped.push(source.clone()); continue; }
                let target = target_path(&requested, *conflict)?;
                let Some(target) = target else { receipt.skipped.push(source.clone()); continue; };
                let manifest = tree(source, control)?;
                if moving && move_without_replace(source, &target).is_ok() {
                    receipt.completed.push(FileOperationChange { source: Some(source.clone()), destination: Some(target), source_retained: false });
                    control.update(None, 1, 0);
                    continue;
                }
                // 只有跨卷/受限移动才锁住源；同卷原子rename和普通复制不增加文件句柄预算。
                let mut source_locks = if moving { Some(SourceLocks::acquire(source, &manifest)?) } else { None };
                // 跨卷移动复用受检复制；失败时保留源，不能先删源再尝试复制。
                let stage_container = reserve_stage(&destination, control.id(), index)?;
                let stage = stage_container.join("content");
                let result = copy_tree(source, &stage, &manifest, control)
                    .and_then(|()| { control.check()?; verify_unchanged(source, &manifest)?; move_without_replace(&stage, &target) });
                if let Err(error) = result {
                    cleanup_stage(&stage_container);
                    return Err(error);
                }
                let _ = fs::remove_dir(&stage_container);
                if moving {
                    // 文件发布后再次逐字节核验；并发改写则保留两份，绝不把复制成功当成源可删除的证明。
                    if let Err(error) = verify_copied(source, &target, &manifest, control)
                        .and_then(|()| source_locks.as_mut().expect("移动持有源身份锁").remove(&manifest, control)) {
                        receipt.completed.push(FileOperationChange { source: Some(source.clone()), destination: Some(target), source_retained: true });
                        return Err(format!("目标已复制，源文件保留或部分保留：{error}"));
                    }
                }
                receipt.completed.push(FileOperationChange { source: Some(source.clone()), destination: Some(target), source_retained: !moving });
                control.update(None, 1, 0);
            }
        }
        FileOperation::Delete { sources } => {
            let sources = sources_checked(sources, &roots)?;
            for source in &sources { protect_root(source, &roots)?; tree(source, control)?; }
            for source in sources {
                control.check()?;
                control.update(Some(&source), 0, 0);
                recycle::delete(&source)?;
                receipt.completed.push(FileOperationChange { source: Some(source), destination: None, source_retained: false });
                control.update(None, 1, 0);
            }
        }
        FileOperation::CreateFolder { parent, name } => {
            let parent = checked(parent, &roots, true)?;
            validate_name(name)?;
            control.check()?;
            let destination = parent.join(name);
            fs::create_dir(&destination).map_err(|error| format!("无法新建文件夹 {}：{error}", destination.display()))?;
            receipt.completed.push(FileOperationChange { source: None, destination: Some(destination), source_retained: false });
            control.update(None, 1, 0);
        }
        FileOperation::CreateFile { parent, name, kind } => {
            let parent = checked(parent, &roots, true)?;
            validate_name(name)?;
            let destination = parent.join(name);
            target_path(&destination, ConflictPolicy::Fail)?;
            control.check()?;
            reject_link_chain(&parent)?;
            let stage_container = reserve_stage(&parent, control.id(), 0)?;
            let stage = stage_container.join("content");
            let bytes = new_file_bytes(*kind);
            // 即使GLB很小也先完整写入私有暂存并关闭句柄，再无覆盖发布；失败不会留下正式半文件。
            let result = (|| {
                let mut output = OpenOptions::new().create_new(true).write(true).open(&stage)
                    .map_err(|error| format!("不能创建暂存文件：{error}"))?;
                output.write_all(&bytes).and_then(|()| output.sync_all())
                    .map_err(|error| format!("不能写入新文件：{error}"))?;
                drop(output);
                control.check()?;
                reject_link_chain(&parent)?;
                move_without_replace(&stage, &destination)
            })();
            if let Err(error) = result { cleanup_stage(&stage_container); return Err(error); }
            let _ = fs::remove_dir(&stage_container);
            control.update(Some(&destination), 1, bytes.len() as u64);
            receipt.completed.push(FileOperationChange { source: None, destination: Some(destination), source_retained: false });
        }
        FileOperation::Rename { source, name } => {
            let source = checked(source, &roots, false)?;
            protect_root(&source, &roots)?;
            validate_name(name)?;
            let destination = source.parent().ok_or("不能重命名磁盘根")?.join(name);
            if destination == source { receipt.skipped.push(source); return Ok(()); }
            control.check()?;
            rename_checked(&source, &destination, control.id())?;
            receipt.completed.push(FileOperationChange { source: Some(source), destination: Some(destination), source_retained: false });
            control.update(None, 1, 0);
        }
    }
    Ok(())
}

fn new_file_bytes(kind: NewFileKind) -> Vec<u8> {
    match kind {
        // 空字节串本身是合法UTF-8；不写BOM或默认正文，作者打开后可直接输入。
        NewFileKind::Text | NewFileKind::Markdown => Vec::new(),
        NewFileKind::Glb => {
            // GLB 2允许只有JSON块；提供可独立打开的空默认场景，不制造无用途的BIN块。
            let json = br#"{"asset":{"version":"2.0","generator":"Feathrise"},"scene":0,"scenes":[{"nodes":[]}]}"#;
            let padded_length = (json.len() + 3) & !3;
            let mut bytes = Vec::with_capacity(20 + padded_length);
            bytes.extend_from_slice(b"glTF");
            bytes.extend_from_slice(&2_u32.to_le_bytes());
            bytes.extend_from_slice(&((20 + padded_length) as u32).to_le_bytes());
            bytes.extend_from_slice(&(padded_length as u32).to_le_bytes());
            bytes.extend_from_slice(b"JSON");
            bytes.extend_from_slice(json);
            bytes.resize(20 + padded_length, b' ');
            bytes
        }
    }
}

fn absolute(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir)) {
        Err(format!("文件操作必须使用已定位的绝对路径：{}", path.display()))
    } else { Ok(()) }
}

fn checked(path: &Path, roots: &[PathBuf], require_directory: bool) -> Result<PathBuf, String> {
    absolute(path)?;
    let resolved = path.canonicalize().map_err(|error| format!("文件路径不可用 {}：{error}", path.display()))?;
    let root = roots.iter().filter(|root| resolved.starts_with(root)).max_by_key(|root| root.components().count())
        .ok_or_else(|| format!("文件超出已打开目录：{}", path.display()))?;
    // 逐级检查原请求路径，canonicalize不能掩盖指向另一目录的符号链接和Windows联接。
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|error| error.to_string())?;
        reject_link(ancestor, &metadata)?;
        if ancestor.canonicalize().ok().as_ref() == Some(root) { break; }
    }
    if require_directory && !resolved.is_dir() { return Err(format!("目标不是文件夹：{}", resolved.display())); }
    Ok(resolved)
}

fn sources_checked(sources: &[PathBuf], roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    if sources.is_empty() || sources.len() > MAX_SOURCES { return Err("文件选择数量超出单次任务上限".into()); }
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for source in sources {
        let source = checked(source, roots, false)?;
        if seen.insert(source.clone()) { normalized.push(source); }
    }
    let parents = normalized.clone();
    normalized.retain(|path| !parents.iter().any(|parent| parent != path && path.starts_with(parent)));
    Ok(normalized)
}

fn protect_root(path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if roots.iter().any(|root| root == path || root.starts_with(path)) {
        Err("不能移动、重命名或删除已打开的目录根；请选择其中的文件或子文件夹".into())
    } else { Ok(()) }
}

fn validate_name(name: &str) -> Result<(), String> {
    let stem = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if name.trim().is_empty() || name.len() > 240 || matches!(name, "." | "..") || name.ends_with([' ', '.'])
        || name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && matches!(stem.as_bytes()[3], b'1'..=b'9') {
        return Err("名称不能包含路径、系统保留名称、末尾空格或非法字符".into());
    }
    Ok(())
}

fn reject_link(path: &Path, metadata: &Metadata) -> Result<(), String> {
    #[cfg(windows)]
    let reparse = { use std::os::windows::fs::MetadataExt; metadata.file_attributes() & 0x400 != 0 };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.file_type().is_symlink() || reparse { return Err(format!("文件整理暂不跟随符号链接或目录联接：{}", path.display())); }
    if !metadata.is_file() && !metadata.is_dir() { return Err(format!("不支持的特殊文件：{}", path.display())); }
    Ok(())
}

fn reject_link_chain(path: &Path) -> Result<(), String> {
    // 预检之后外部工具仍可能改动目录；重新检查中间层，不能只检查叶子文件而跟随新联接。
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|error| format!("路径已经改变 {}：{error}", ancestor.display()))?;
        reject_link(ancestor, &metadata)?;
    }
    Ok(())
}

fn tree(source: &Path, control: &Control<'_>) -> Result<Vec<Entry>, String> {
    let mut stack = vec![PathBuf::new()];
    let mut entries = Vec::new();
    let mut path_bytes = 0;
    while let Some(relative) = stack.pop() {
        control.check()?;
        let path = member(source, &relative);
        reject_link_chain(&path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
        reject_link(&path, &metadata)?;
        path_bytes += relative.as_os_str().len();
        if entries.len() >= MAX_TREE_ENTRIES || path_bytes > MAX_PATH_BYTES || relative.components().count() > 128 {
            return Err("文件夹超过单次整理预算（10万条目、128层或32MiB路径），请分批整理".into());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                let child = relative.join(entry.file_name());
                path_bytes += child.as_os_str().len();
                if path_bytes > MAX_PATH_BYTES { return Err("文件夹路径超过32MiB预算，请分批整理".into()); }
                stack.push(child);
                if entries.len() + stack.len() > MAX_TREE_ENTRIES { return Err("文件夹条目超过10万，请分批整理".into()); }
            }
        }
        entries.push(Entry { relative, metadata });
    }
    Ok(entries)
}

fn target_path(requested: &Path, policy: ConflictPolicy) -> Result<Option<PathBuf>, String> {
    if !requested.try_exists().map_err(|error| error.to_string())? { return Ok(Some(requested.to_owned())); }
    match policy {
        ConflictPolicy::Fail => Err(format!("目标已存在，请选择保留两者或跳过：{}", requested.display())),
        ConflictPolicy::Skip => Ok(None),
        ConflictPolicy::KeepBoth => {
            let parent = requested.parent().ok_or("目标缺少父目录")?;
            let stem = requested.file_stem().unwrap_or_default().to_string_lossy();
            let extension = requested.extension().map(|extension| format!(".{}", extension.to_string_lossy())).unwrap_or_default();
            for suffix in 2..=10_000 {
                let path = parent.join(format!("{stem} ({suffix}){extension}"));
                if !path.try_exists().map_err(|error| error.to_string())? { return Ok(Some(path)); }
            }
            Err("同名副本过多，请先更换名称".into())
        }
    }
}

fn reserve_stage(parent: &Path, id: u64, index: usize) -> Result<PathBuf, String> {
    for attempt in 0..100 {
        let stage = parent.join(format!(".feathrise-copy-{}-{id}-{index}-{attempt}", std::process::id()));
        match fs::create_dir(&stage) {
            Ok(()) => return Ok(stage),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("不能创建复制暂存目录：{error}")),
        }
    }
    Err("复制暂存名称已被占用，请稍后重试".into())
}

fn copy_tree(source: &Path, destination: &Path, manifest: &[Entry], control: &Control<'_>) -> Result<(), String> {
    for entry in manifest {
        control.check()?;
        let from = member(source, &entry.relative);
        let to = member(destination, &entry.relative);
        reject_link_chain(&from)?;
        let actual = fs::symlink_metadata(&from).map_err(|error| error.to_string())?;
        reject_link(&from, &actual)?;
        if !unchanged(&actual, &entry.metadata) { return Err(format!("复制期间源文件已改变：{}", from.display())); }
        if entry.metadata.is_dir() {
            fs::create_dir(&to).map_err(|error| format!("不能创建暂存目录 {}：{error}", to.display()))?;
        } else {
            let mut input = File::open(&from).map_err(|error| format!("不能读取 {}：{error}", from.display()))?;
            let mut output = OpenOptions::new().create_new(true).write(true).open(&to).map_err(|error| format!("不能创建暂存文件 {}：{error}", to.display()))?;
            let mut buffer = vec![0; BUFFER_SIZE];
            loop {
                control.check()?;
                let length = input.read(&mut buffer).map_err(|error| error.to_string())?;
                if length == 0 { break; }
                output.write_all(&buffer[..length]).map_err(|error| error.to_string())?;
                control.update(Some(&from), 0, length as u64);
            }
            output.sync_all().map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn unchanged(actual: &Metadata, original: &Metadata) -> bool {
    actual.is_dir() == original.is_dir() && actual.len() == original.len() && actual.modified().ok() == original.modified().ok()
}

fn verify_unchanged(source: &Path, manifest: &[Entry]) -> Result<(), String> {
    for entry in manifest {
        let path = member(source, &entry.relative);
        reject_link_chain(&path)?;
        let actual = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        reject_link(&path, &actual)?;
        if !unchanged(&actual, &entry.metadata) { return Err(format!("复制期间源内容已改变，源文件保留：{}", path.display())); }
    }
    Ok(())
}

fn verify_copied(source: &Path, target: &Path, manifest: &[Entry], control: &Control<'_>) -> Result<(), String> {
    verify_unchanged(source, manifest)?;
    let mut left = vec![0; BUFFER_SIZE];
    let mut right = vec![0; BUFFER_SIZE];
    for entry in manifest.iter().filter(|entry| entry.metadata.is_file()) {
        control.check()?;
        let mut original = File::open(member(source, &entry.relative)).map_err(|error| error.to_string())?;
        let mut copied = File::open(member(target, &entry.relative)).map_err(|error| error.to_string())?;
        loop {
            control.check()?;
            let a = original.read(&mut left).map_err(|error| error.to_string())?;
            let b = copied.read(&mut right).map_err(|error| error.to_string())?;
            if a != b || left[..a] != right[..b] { return Err("目标逐字节校验失败".into()); }
            if a == 0 { break; }
        }
    }
    verify_unchanged(source, manifest)
}

#[cfg(windows)]
struct SourceLocks { handles: Vec<Option<File>> }

#[cfg(windows)]
impl SourceLocks {
    fn acquire(source: &Path, manifest: &[Entry]) -> Result<Self, String> {
        use std::os::windows::fs::OpenOptionsExt;
        if manifest.len() > 4096 { return Err("跨卷移动超过4096条目，请分批移动或先复制；源文件未删除".into()); }
        let mut handles = Vec::with_capacity(manifest.len());
        for entry in manifest {
            let path = member(source, &entry.relative);
            reject_link_chain(&path)?;
            let mut options = OpenOptions::new();
            if entry.metadata.is_dir() {
                // 目录只禁止重命名/替换，仍允许外部工具增加新文件；新增成员会让最终空目录删除失败并保留。
                options.access_mode(0x0001_0000 | 0x0010_0000 | 0x80).share_mode(1 | 2).custom_flags(0x0200_0000);
            } else {
                // 持有READ+DELETE权限，同时禁止其他WRITE/DELETE句柄；既不误删后来替换的文件，也不丢外部写入。
                options.access_mode(0x8000_0000 | 0x0001_0000).share_mode(1);
            }
            let file = options.open(&path).map_err(|error| format!("源正在被写入、替换或占用，移动已停止，源保留 {}：{error}", path.display()))?;
            if !unchanged(&file.metadata().map_err(|error| error.to_string())?, &entry.metadata) {
                return Err(format!("源已被外部修改，移动已停止：{}", path.display()));
            }
            handles.push(Some(file));
        }
        Ok(Self { handles })
    }

    fn remove(&mut self, manifest: &[Entry], control: &Control<'_>) -> Result<(), String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{SetFileInformationByHandle, FileDispositionInfo, FILE_DISPOSITION_INFO};
        for index in (0..manifest.len()).rev() {
            control.check()?;
            let Some(file) = self.handles[index].take() else { continue; };
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // 按锁定的真实句柄删除，不重新解析源路径；文件与目录在drop时真正释放。
            if unsafe { SetFileInformationByHandle(file.as_raw_handle(), FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) } == 0 {
                return Err(format!("源目录有新内容或文件被占用，未完成源保留：{}", std::io::Error::last_os_error()));
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
struct SourceLocks;

#[cfg(not(windows))]
impl SourceLocks {
    fn acquire(_source: &Path, _manifest: &[Entry]) -> Result<Self, String> {
        Err("当前平台尚未支持可阻止并发写入的跨卷移动，请复制后另行回收源文件".into())
    }
    fn remove(&mut self, _manifest: &[Entry], _control: &Control<'_>) -> Result<(), String> { unreachable!("非Windows不会取得跨卷源锁") }
}

fn rename_checked(source: &Path, destination: &Path, id: u64) -> Result<(), String> {
    #[cfg(windows)]
    if destination.canonicalize().ok().as_deref() == Some(source) {
        // Windows同名大小写调整经同目录暂存跳转；两次MoveFileW仍均禁止覆盖，不使用可覆盖rename。
        let container = reserve_stage(source.parent().ok_or("源缺少父目录")?, id, 0)?;
        let temporary = container.join("content");
        if let Err(error) = move_without_replace(source, &temporary) { let _ = fs::remove_dir(&container); return Err(error); }
        if let Err(error) = move_without_replace(&temporary, destination) {
            if move_without_replace(&temporary, source).is_ok() { let _ = fs::remove_dir(&container); }
            return Err(format!("名称被并发占用，已尝试还原；剩余暂存如有保留在 {}：{error}", temporary.display()));
        }
        let _ = fs::remove_dir(&container);
        return Ok(());
    }
    let _ = id;
    move_without_replace(source, destination)
}

fn cleanup_stage(path: &Path) {
    // 只清理本任务create_new创建的私有暂存路径；从不对用户源或目标执行递归清理。
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if reject_link(path, &metadata).is_ok() {
            if metadata.is_dir() { let _ = fs::remove_dir_all(path); } else { let _ = fs::remove_file(path); }
        }
    }
}

#[cfg(windows)]
fn move_without_replace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let source_wide: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<_> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
    // MoveFileW既可同卷移动目录，也保证同名目标出现时失败，不会覆盖竞态中新建的文件。
    if unsafe { windows_sys::Win32::Storage::FileSystem::MoveFileW(source_wide.as_ptr(), destination_wide.as_ptr()) } == 0 {
        Err(format!("无法移动 {} 至 {}：{}", source.display(), destination.display(), std::io::Error::last_os_error()))
    } else { Ok(()) }
}

#[cfg(unix)]
fn move_without_replace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| "源路径包含空字符")?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| "目标路径包含空字符")?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let result = unsafe { libc::renameat2(libc::AT_FDCWD, source.as_ptr(), libc::AT_FDCWD, destination.as_ptr(), libc::RENAME_NOREPLACE) };
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let result = unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios")))]
    return Err("当前平台尚未提供不会覆盖目标的文件移动接口".into());
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios"))]
    if result == 0 { Ok(()) } else { Err(std::io::Error::last_os_error().to_string()) }
}

#[cfg(not(any(windows, unix)))]
fn move_without_replace(_source: &Path, _destination: &Path) -> Result<(), String> { Err("当前平台尚未支持本地文件整理".into()) }

#[cfg(all(test, windows))]
mod lock_tests {
    use super::*;
    use super::super::{Control, Job, Shared, tests::Fixture};
    use std::sync::{Arc, atomic::AtomicBool};

    fn job(fixture: &Fixture) -> Job {
        Job { id: 9001, request: fixture.request(FileOperation::Move { sources: vec![fixture.path("source")],
            destination: fixture.path("destination"), conflict: ConflictPolicy::Fail }), cancel: Arc::new(AtomicBool::new(false)) }
    }

    #[test]
    fn cross_volume_source_locks_reject_external_writers_and_delete_the_exact_verified_files() {
        let fixture = Fixture::new();
        fs::write(fixture.path("source/file.txt"), b"locked source content").unwrap();
        let job = job(&fixture);
        let shared = Shared::default();
        let control = Control { shared: &shared, job: &job };
        let manifest = tree(&fixture.path("source"), &control).unwrap();
        let mut locks = SourceLocks::acquire(&fixture.path("source"), &manifest).unwrap();
        assert!(OpenOptions::new().write(true).open(fixture.path("source/file.txt")).is_err(), "普通外部作者写入应被源锁拒绝");
        assert!(fs::rename(fixture.path("source/file.txt"), fixture.path("source/replaced.txt")).is_err(), "源不可在复制期间被替换");
        let copied = fixture.path("destination/copied");
        copy_tree(&fixture.path("source"), &copied, &manifest, &control).unwrap();
        verify_copied(&fixture.path("source"), &copied, &manifest, &control).unwrap();
        locks.remove(&manifest, &control).unwrap();
        assert!(!fixture.path("source").exists());
        assert_eq!(fs::read(copied.join("file.txt")).unwrap(), b"locked source content");
    }

    #[test]
    fn already_open_external_writer_prevents_cross_volume_source_deletion() {
        let fixture = Fixture::new();
        fs::write(fixture.path("source/file.txt"), b"author keeps writing").unwrap();
        let job = job(&fixture);
        let shared = Shared::default();
        let control = Control { shared: &shared, job: &job };
        let manifest = tree(&fixture.path("source"), &control).unwrap();
        let mut author = OpenOptions::new().write(true).append(true).open(fixture.path("source/file.txt")).unwrap();
        assert!(SourceLocks::acquire(&fixture.path("source"), &manifest).is_err());
        author.write_all(b" new bytes").unwrap();
        drop(author);
        assert_eq!(fs::read(fixture.path("source/file.txt")).unwrap(), b"author keeps writing new bytes");
        assert!(fixture.path("source").exists());
    }

    #[test]
    fn external_new_directory_member_is_preserved_when_cross_volume_cleanup_finishes() {
        let fixture = Fixture::new();
        fs::write(fixture.path("source/file.txt"), b"copied content").unwrap();
        let job = job(&fixture);
        let shared = Shared::default();
        let control = Control { shared: &shared, job: &job };
        let manifest = tree(&fixture.path("source"), &control).unwrap();
        let mut locks = SourceLocks::acquire(&fixture.path("source"), &manifest).unwrap();
        let copied = fixture.path("destination/copied");
        copy_tree(&fixture.path("source"), &copied, &manifest, &control).unwrap();
        verify_copied(&fixture.path("source"), &copied, &manifest, &control).unwrap();
        fs::write(fixture.path("source/new-author-file.txt"), b"must survive").unwrap();
        assert!(locks.remove(&manifest, &control).is_err(), "新文件不能被清单外递归删除");
        drop(locks);
        assert_eq!(fs::read(fixture.path("source/new-author-file.txt")).unwrap(), b"must survive");
        assert_eq!(fs::read(copied.join("file.txt")).unwrap(), b"copied content");
    }
}
