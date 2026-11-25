//! 后台受检文件系统操作；完整预检后逐顶层提交，复制先写本轮暂存目录，发布时禁止覆盖。

use super::{Control, recycle, tree_manifest::{Entry, Manifest, ManifestWriter}};
use feathrise_editor_project_browser_model::file_operations::*;
use std::{collections::HashSet, fs::{self, File, Metadata, OpenOptions}, io::{self, Read, Write}, path::{Component, Path, PathBuf}};

const BUFFER_SIZE: usize = 256 * 1024;

/// Windows给文件路径附加空相对段会产生尾随分隔符；根条目必须直接使用原路径。
fn member(root: &Path, relative: &Path) -> PathBuf { if relative.as_os_str().is_empty() { root.to_owned() } else { root.join(relative) } }

pub(super) fn execute(
    request: &FileOperationRequest, control: &Control<'_>, receipt: &mut FileOperationReceipt,
    current: &mut Option<PathBuf>,
) -> Result<(), String> {
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
    if let Some(references) = &request.references {
        return super::references::execute(request, references, &roots, control, receipt, current);
    }
    execute_plain(request, &roots, control, receipt, current, None)
}

pub(super) fn execute_plain(
    request: &FileOperationRequest, roots: &[PathBuf], control: &Control<'_>, receipt: &mut FileOperationReceipt,
    current: &mut Option<PathBuf>, approved: Option<&ReferencePreview>,
) -> Result<(), String> {
    match &request.operation {
        FileOperation::Copy { sources, destination, conflict } | FileOperation::Move { sources, destination, conflict } => {
            let destination = checked(destination, &roots, true)?;
            if !destination.is_dir() { return Err("粘贴目标必须是文件夹".into()); }
            let sources = sources_checked(sources, &roots, control, &mut receipt.skipped, current)?;
            let moving = matches!(request.operation, FileOperation::Move { .. });
            // 整批先核对路径；不能复制一半之后才发现后面条目越界或移入自身后代。
            for (source, _) in &sources {
                control.check()?;
                *current = Some(source.clone());
                if moving { protect_root(source, &roots)?; }
                if source.is_dir() && destination.starts_with(source) {
                    return Err(format!("不能把文件夹放入自身或子文件夹：{}", source.display()));
                }
            }
            for (index, (source, stamp)) in sources.iter().enumerate() {
                let stamp = approved.and_then(|plan| plan.source_stamps.get(source)).unwrap_or(stamp);
                control.check()?;
                *current = Some(source.clone());
                control.update(Some(source), 0, 0);
                let file_name = source.file_name().ok_or("源路径没有文件名")?;
                let requested = destination.join(file_name);
                if moving && requested == *source { receipt.skipped.push(source.clone()); continue; }
                let target = target_path(&requested, *conflict)?;
                if let Some(approved) = approved && approved.moves.get(source) != target.as_ref() {
                    return Err(format!("目标名称在确认后改变，请重新预览：{}", requested.display()));
                }
                let Some(target) = target else { receipt.skipped.push(source.clone()); continue; };
                if moving {
                    walk_tree(source, *stamp, control, |_, _, _| Ok(()))?;
                    control.check()?;
                    let metadata = tree_metadata(source)?;
                    verify_source_stamp(source, *stamp, &metadata)?;
                    reject_link_chain(&destination)?;
                    match system_move_without_replace(source, &target, metadata.is_file()) {
                        Ok(()) => {
                            receipt.completed.push(FileOperationChange { source: Some(source.clone()), destination: Some(target), source_retained: false });
                            control.update(None, 1, 0);
                            continue;
                        }
                        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
                            #[cfg(not(windows))]
                            return Err(cross_volume_unsupported());
                        }
                        Err(error) => return Err(move_error(source, &target, error)),
                    }
                }
                reject_link_chain(&destination)?;
                let stage_container = reserve_stage(&destination, control.id(), index)?;
                let stage = stage_container.join("content");
                let manifest_path = stage_container.join("source-manifest");
                let published = (|| {
                    let mut manifest = copy_tree(source, *stamp, &stage, &manifest_path, moving, control)?;
                    // 先核对完整暂存结果，再发布目标。跨卷移动发布前完成整树逐字节核验。
                    if moving { verify_cross_volume(source, &stage, &mut manifest, control, false)?; }
                    else { verify_unchanged(source, &mut manifest, control)?; }
                    control.check()?;
                    reject_link_chain(&stage)?;
                    move_without_replace(&stage, &target)?;
                    Ok::<_, String>(manifest)
                })();
                let mut manifest = match published {
                    Ok(manifest) => manifest,
                    Err(error) => { cleanup_stage(&stage_container); return Err(error); }
                };
                // 发布后逐项重新持有源和目标，源内容发生变化的条目留在原位。
                let removal = if moving { verify_cross_volume(source, &target, &mut manifest, control, true) } else { Ok(()) };
                drop(manifest);
                let _ = fs::remove_file(&manifest_path);
                let _ = fs::remove_dir(&stage_container);
                receipt.completed.push(FileOperationChange { source: Some(source.clone()), destination: Some(target), source_retained: !moving || removal.is_err() });
                removal.map_err(|error| format!("目标已复制，源文件保留或部分保留：{error}"))?;
                control.update(None, 1, 0);
            }
        }
        FileOperation::Delete { sources } => {
            let sources = sources_checked(sources, &roots, control, &mut receipt.skipped, current)?;
            for (source, stamp) in &sources {
                control.check()?;
                *current = Some(source.clone());
                protect_root(source, &roots)?;
                walk_tree(source, *stamp, control, |_, _, _| Ok(()))?;
            }
            for (source, stamp) in sources {
                control.check()?;
                *current = Some(source.clone());
                control.update(Some(&source), 0, 0);
                verify_source_stamp(&source, stamp, &tree_metadata(&source)?)?;
                recycle::delete(&source)?;
                receipt.completed.push(FileOperationChange { source: Some(source), destination: None, source_retained: false });
                control.update(None, 1, 0);
            }
        }
        FileOperation::CreateFolder { parent, name } => {
            *current = Some(parent.join(name));
            let parent = checked(parent, &roots, true)?;
            validate_name(name)?;
            control.check()?;
            let destination = parent.join(name);
            fs::create_dir(&destination).map_err(|error| format!("无法新建文件夹 {}：{error}", destination.display()))?;
            receipt.completed.push(FileOperationChange { source: None, destination: Some(destination), source_retained: false });
            control.update(None, 1, 0);
        }
        FileOperation::CreateFile { parent, name, .. } | FileOperation::CreateSceneGlb { parent, name } => {
            *current = Some(parent.join(name));
            let parent = checked(parent, &roots, true)?;
            validate_name(name)?;
            let (parent, kind) = if let FileOperation::CreateFile { kind, .. } = &request.operation {
                (parent, *kind)
            } else {
                let models = parent.join("models");
                *current = Some(models.join(name));
                control.check()?;
                // 只创建直属资产目录，再走原目录与联接校验，和文件窗口共用无覆盖发布。
                match fs::create_dir(&models) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(format!("无法建立场景模型目录：{error}")),
                }
                (checked(&models, &roots, true)?, NewFileKind::Glb)
            };
            let destination = parent.join(name);
            target_path(&destination, ConflictPolicy::Fail)?;
            control.check()?;
            reject_link_chain(&parent)?;
            let stage_container = reserve_stage(&parent, control.id(), 0)?;
            let stage = stage_container.join("content");
            let bytes = new_file_bytes(kind);
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
        FileOperation::Rename { source, stamp, name } => {
            *current = Some(source.clone());
            let (source, metadata) = checked_entry(source, &roots)?;
            let stamp = approved.and_then(|plan| plan.source_stamps.get(&source)).unwrap_or(stamp);
            verify_source_stamp(&source, *stamp, &metadata)?;
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

pub(super) fn checked(path: &Path, roots: &[PathBuf], require_directory: bool) -> Result<PathBuf, String> {
    let (path, metadata) = checked_entry(path, roots)?;
    if require_directory && !metadata.is_dir() { return Err(format!("目标不是文件夹：{}", path.display())); }
    Ok(path)
}

pub(super) fn checked_entry(path: &Path, roots: &[PathBuf]) -> Result<(PathBuf, Metadata), String> {
    absolute(path)?;
    let resolved = path.canonicalize().map_err(|error| format!("文件路径不可用 {}：{error}", path.display()))?;
    let root = roots.iter().filter(|root| resolved.starts_with(root)).max_by_key(|root| root.components().count())
        .ok_or_else(|| format!("文件超出已打开目录：{}", path.display()))?;
    // 逐级检查原请求路径，canonicalize不能掩盖指向另一目录的符号链接和Windows联接。
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    reject_link(path, &metadata)?;
    if &resolved != root {
        for ancestor in path.ancestors().skip(1) {
            let metadata = fs::symlink_metadata(ancestor).map_err(|error| error.to_string())?;
            reject_link(ancestor, &metadata)?;
            if ancestor.canonicalize().ok().as_ref() == Some(root) { break; }
        }
    }
    Ok((resolved, metadata))
}

pub(super) fn verify_source_stamp(path: &Path, stamp: FileEntryStamp, metadata: &Metadata) -> Result<(), String> {
    if stamp.matches(metadata) { return Ok(()); }
    Err(format!("选中后文件已变化，请刷新目录并重新选择：{}", path.display()))
}

pub(super) fn sources_checked(
    sources: &FileSources, roots: &[PathBuf], control: &Control<'_>,
    skipped: &mut Vec<PathBuf>, current: &mut Option<PathBuf>,
) -> Result<Vec<(PathBuf, FileEntryStamp)>, String> {
    if sources.is_empty() { return Err("请先选择文件或文件夹".into()); }
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    // 冻结选区按需展开到后台。整批受检后再发布，父子去重按深度查找。
    for (source, stamp) in sources.iter_with_stamp() {
        control.check()?;
        *current = Some(source.to_path_buf());
        let (source, metadata) = checked_entry(source, roots)?;
        verify_source_stamp(&source, stamp, &metadata)?; // 原地址在排队期间仍可被替换；先验证整批再写磁盘。
        if seen.insert(source.clone()) { normalized.push((source, stamp)); }
    }
    normalized.retain(|(path, _)| {
        let covered = path.ancestors().skip(1).any(|parent| seen.contains(parent));
        if covered { skipped.push(path.clone()); }
        !covered
    });
    Ok(normalized)
}

fn protect_root(path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if roots.iter().any(|root| root == path || root.starts_with(path)) {
        Err("不能移动、重命名或删除已打开的目录根；请选择其中的文件或子文件夹".into())
    } else { Ok(()) }
}

pub(super) fn validate_name(name: &str) -> Result<(), String> {
    let stem = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if name.trim().is_empty() || name.len() > 240 || matches!(name, "." | "..") || name.ends_with([' ', '.'])
        || name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && matches!(stem.as_bytes()[3], b'1'..=b'9') {
        return Err("名称不能包含路径、系统保留名称、末尾空格或非法字符".into());
    }
    Ok(())
}

pub(super) fn reject_link(path: &Path, metadata: &Metadata) -> Result<(), String> {
    #[cfg(windows)]
    let reparse = { use std::os::windows::fs::MetadataExt; metadata.file_attributes() & 0x400 != 0 };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.file_type().is_symlink() || reparse { return Err(format!("文件整理暂不跟随符号链接或目录联接：{}", path.display())); }
    if !metadata.is_file() && !metadata.is_dir() { return Err(format!("不支持的特殊文件：{}", path.display())); }
    Ok(())
}

pub(super) fn reject_link_chain(path: &Path) -> Result<(), String> {
    // 预检之后外部工具仍可能改动目录；重新检查中间层，不能只检查叶子文件而跟随新联接。
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|error| format!("路径已经改变 {}：{error}", ancestor.display()))?;
        reject_link(ancestor, &metadata)?;
    }
    Ok(())
}

fn tree_metadata(path: &Path) -> Result<Metadata, String> {
    if let Some(parent) = path.parent() { reject_link_chain(parent)?; }
    let metadata = fs::symlink_metadata(path).map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    reject_link(path, &metadata)?;
    Ok(metadata)
}

/// 同卷移动和回收只需核验目录，逐层保留正在枚举的迭代器，不把兄弟条目积在内存。
fn walk_tree(
    source: &Path,
    stamp: FileEntryStamp,
    control: &Control<'_>,
    mut visit: impl FnMut(&Path, &Path, &Metadata) -> Result<(), String>,
) -> Result<(), String> {
    let mut directories = Vec::<fs::ReadDir>::new();
    let mut relative = PathBuf::new();
    loop {
        control.check()?;
        let path = member(source, &relative);
        let metadata = tree_metadata(&path)?;
        if relative.as_os_str().is_empty() { verify_source_stamp(source, stamp, &metadata)?; }
        visit(&path, &relative, &metadata)?;
        if metadata.is_dir() {
            directories.push(fs::read_dir(&path).map_err(|error| format!("无法枚举 {}：{error}", path.display()))?);
        } else {
            relative.pop();
        }
        loop {
            control.check()?;
            let Some(directory) = directories.last_mut() else { return Ok(()); };
            if let Some(entry) = directory.next() {
                relative.push(entry.map_err(|error| error.to_string())?.file_name());
                break;
            }
            directories.pop();
            relative.pop();
        }
    }
}

fn target_path(requested: &Path, policy: ConflictPolicy) -> Result<Option<PathBuf>, String> {
    planned_target(requested, policy, &HashSet::new())
}

/// 预览复用实际发布的同名策略；同一批中的早先目标也占用名称。
pub(super) fn planned_target(requested: &Path, policy: ConflictPolicy, reserved: &HashSet<PathBuf>) -> Result<Option<PathBuf>, String> {
    let occupied = |path: &Path| -> Result<bool, String> {
        Ok(reserved.contains(&path_key(path)) || path.try_exists().map_err(|error| error.to_string())?)
    };
    if !occupied(requested)? { return Ok(Some(requested.to_owned())); }
    match policy {
        ConflictPolicy::Fail => Err(format!("目标已存在，请选择保留两者或跳过：{}", requested.display())),
        ConflictPolicy::Skip => Ok(None),
        ConflictPolicy::KeepBoth => {
            let parent = requested.parent().ok_or("目标缺少父目录")?;
            let stem = requested.file_stem().unwrap_or_default().to_string_lossy();
            let extension = requested.extension().map(|extension| format!(".{}", extension.to_string_lossy())).unwrap_or_default();
            for suffix in 2..=10_000 {
                let path = parent.join(format!("{stem} ({suffix}){extension}"));
                if !occupied(&path)? { return Ok(Some(path)); }
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

fn copy_tree(
    source: &Path,
    stamp: FileEntryStamp,
    destination: &Path,
    manifest_path: &Path,
    moving: bool,
    control: &Control<'_>,
) -> Result<Manifest, String> {
    let mut manifest = ManifestWriter::new(manifest_path.to_owned());
    let mut buffer = vec![0; BUFFER_SIZE];
    walk_tree(source, stamp, control, |from, relative, metadata| {
        let mut entry = Entry::capture(relative.to_owned(), metadata);
        let to = member(destination, relative);
        // 跨卷复制从已核对身份的源句柄读取，一次持有一个源，其他条目稍后重新验证。
        let locked = if moving { Some(lock_move_source(from, &mut entry)?) } else { None };
        if entry.directory {
            fs::create_dir(&to).map_err(|error| format!("不能创建暂存目录 {}：{error}", to.display()))?;
        } else {
            let mut input = match locked {
                Some(file) => file,
                None => open_copy_file(from, &entry)?,
            };
            let mut output = OpenOptions::new().create_new(true).write(true).open(&to)
                .map_err(|error| format!("不能创建暂存文件 {}：{error}", to.display()))?;
            loop {
                control.check()?;
                let length = input.read(&mut buffer).map_err(|error| error.to_string())?;
                if length == 0 { break; }
                output.write_all(&buffer[..length]).map_err(|error| error.to_string())?;
                control.update(Some(from), 0, length as u64);
            }
            output.sync_all().map_err(|error| error.to_string())?;
        }
        manifest.push(entry)
    })?;
    manifest.finish()
}

fn open_copy_file(path: &Path, entry: &Entry) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|error| format!("不能读取 {}：{error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    reject_link(path, &metadata)?;
    if !entry.matches(&metadata) { return Err(format!("复制期间源文件已改变：{}", path.display())); }
    Ok(file)
}

fn verify_unchanged(source: &Path, manifest: &mut Manifest, control: &Control<'_>) -> Result<(), String> {
    manifest.visit(false, |entry| {
        control.check()?;
        let path = member(source, &entry.relative);
        if !entry.matches(&tree_metadata(&path)?) {
            return Err(format!("复制期间源内容已改变，源文件保留：{}", path.display()));
        }
        Ok(())
    })
}

#[cfg(windows)]
fn lock_move_source(path: &Path, entry: &mut Entry) -> Result<File, String> {
    let (file, key) = super::cross_volume::lock_source(path, entry, false)?;
    entry.file_key = Some(key);
    Ok(file)
}

#[cfg(windows)]
fn verify_cross_volume(
    source: &Path,
    target: &Path,
    manifest: &mut Manifest,
    control: &Control<'_>,
    remove_source: bool,
) -> Result<(), String> {
    use super::cross_volume::{delete_locked, lock_source, verify_copy};
    let mut left = vec![0; BUFFER_SIZE];
    let mut right = vec![0; BUFFER_SIZE];
    // 清单父项先记录。反序清理只删除本轮已复制的项，目录只能在子项处理完毕后按句柄删空目录。
    manifest.visit(remove_source, |entry| {
        control.check()?;
        if entry.file_key.is_none() { return Err("跨卷清单缺少原文件身份，源文件保留".into()); }
        let from = member(source, &entry.relative);
        let to = member(target, &entry.relative);
        let (mut original, _) = lock_source(&from, entry, remove_source)?;
        let copied = if entry.directory {
            if !tree_metadata(&to)?.is_dir() { return Err(format!("目标目录已改变：{}", to.display())); }
            None
        } else {
            Some(verify_copy(&mut original, &to, &mut left, &mut right, control)?)
        };
        if remove_source {
            control.check()?;
            delete_locked(&original, &from)?;
            drop(original); // 先关闭已标记删除的源，再释放目标的读锁。
        }
        drop(copied);
        Ok(())
    })
}

#[cfg(not(windows))]
fn cross_volume_unsupported() -> String {
    "当前平台尚未支持可阻止并发写入的跨卷移动，请复制后另行回收源文件".into()
}

#[cfg(not(windows))]
fn lock_move_source(_path: &Path, _entry: &mut Entry) -> Result<File, String> { Err(cross_volume_unsupported()) }

#[cfg(not(windows))]
fn verify_cross_volume(
    _source: &Path, _target: &Path, _manifest: &mut Manifest, _control: &Control<'_>, _remove_source: bool,
) -> Result<(), String> { Err(cross_volume_unsupported()) }

pub(super) fn rename_checked(source: &Path, destination: &Path, id: u64) -> Result<(), String> {
    #[cfg(windows)]
    if destination.canonicalize().ok().as_deref() == Some(source) {
        // Windows仅改大小写仍经同目录暂存跳转，两次移动都禁止覆盖。
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

fn move_without_replace(source: &Path, destination: &Path) -> Result<(), String> {
    system_move_without_replace(source, destination, false).map_err(|error| move_error(source, destination, error))
}

fn move_error(source: &Path, destination: &Path, error: io::Error) -> String {
    format!("无法移动 {} 至 {}：{error}", source.display(), destination.display())
}

#[cfg(windows)]
fn system_move_without_replace(source: &Path, destination: &Path, native_file: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MoveFileW};
    let source_wide: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<_> = destination.as_os_str().encode_wide().chain(Some(0)).collect();
    // 单文件沿用原生跨卷语义。目录和暂存发布明确要求同卷，才能准确区分跨卷和权限错误。
    let result = unsafe {
        if native_file { MoveFileW(source_wide.as_ptr(), destination_wide.as_ptr()) }
        else { MoveFileExW(source_wide.as_ptr(), destination_wide.as_ptr(), 0) }
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else { Ok(()) }
}

#[cfg(unix)]
fn system_move_without_replace(source: &Path, destination: &Path, _native_file: bool) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source = CString::new(source.as_os_str().as_bytes()).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let result = unsafe { libc::renameat2(libc::AT_FDCWD, source.as_ptr(), libc::AT_FDCWD, destination.as_ptr(), libc::RENAME_NOREPLACE) };
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let result = unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios")))]
    return Err(io::Error::new(io::ErrorKind::Unsupported, "当前平台尚未提供不会覆盖目标的文件移动接口"));
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios"))]
    if result == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

#[cfg(not(any(windows, unix)))]
fn system_move_without_replace(_source: &Path, _destination: &Path, _native_file: bool) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "当前平台尚未支持本地文件整理"))
}

#[cfg(all(test, windows))]
mod lock_tests {
    use super::*;
    use super::super::{Control, Job, Shared, tests::Fixture};
    use std::sync::{Arc, atomic::AtomicBool};

    fn job(fixture: &Fixture) -> Job {
        Job { id: 9001, request: fixture.request(FileOperation::Move { sources: vec![fixture.path("source")].into(),
            destination: fixture.path("destination"), conflict: ConflictPolicy::Fail }), cancel: Arc::new(AtomicBool::new(false)) }
    }

    fn copy_fixture(fixture: &Fixture, control: &Control<'_>) -> Result<Manifest, String> {
        copy_tree(&fixture.path("source"), FileEntryStamp::Untracked, &fixture.path("destination/copied"),
            &fixture.path("destination/manifest"), true, control)
    }

    #[test]
    fn cross_volume_item_lock_rejects_external_writers_and_deletes_the_verified_files() {
        let fixture = Fixture::new();
        let source = fixture.path("source/file.txt");
        fs::write(&source, b"locked source content").unwrap();
        let job = job(&fixture);
        let shared = Shared::default();
        let control = Control { shared: &shared, job: &job };
        let mut entry = Entry::capture(PathBuf::from("file.txt"), &fs::metadata(&source).unwrap());
        let lock = lock_move_source(&source, &mut entry).unwrap();
        assert!(OpenOptions::new().write(true).open(&source).is_err(), "当前持锁项应拒绝外部写入");
        assert!(fs::rename(&source, fixture.path("source/replaced.txt")).is_err(), "当前持锁项应拒绝改名");
        drop(lock);
        let copied = fixture.path("destination/copied");
        let mut manifest = copy_fixture(&fixture, &control).unwrap();
        verify_cross_volume(&fixture.path("source"), &copied, &mut manifest, &control, false).unwrap();
        verify_cross_volume(&fixture.path("source"), &copied, &mut manifest, &control, true).unwrap();
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
        let mut author = OpenOptions::new().write(true).append(true).open(fixture.path("source/file.txt")).unwrap();
        assert!(copy_fixture(&fixture, &control).is_err());
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
        let copied = fixture.path("destination/copied");
        let mut manifest = copy_fixture(&fixture, &control).unwrap();
        verify_cross_volume(&fixture.path("source"), &copied, &mut manifest, &control, false).unwrap();
        fs::write(fixture.path("source/new-author-file.txt"), b"must survive").unwrap();
        assert!(verify_cross_volume(&fixture.path("source"), &copied, &mut manifest, &control, true).is_err(),
            "新文件不能被清单外递归删除");
        assert_eq!(fs::read(fixture.path("source/new-author-file.txt")).unwrap(), b"must survive");
        assert_eq!(fs::read(copied.join("file.txt")).unwrap(), b"copied content");
    }

    #[test]
    fn replacement_with_identical_bytes_and_time_is_preserved_during_cross_volume_cleanup() {
        let fixture = Fixture::new();
        let source = fixture.path("source/file.txt");
        let bytes = b"identical contents";
        fs::write(&source, bytes).unwrap();
        let original = fs::metadata(&source).unwrap();
        let job = job(&fixture);
        let shared = Shared::default();
        let control = Control { shared: &shared, job: &job };
        let copied = fixture.path("destination/copied");
        let mut manifest = copy_fixture(&fixture, &control).unwrap();
        verify_cross_volume(&fixture.path("source"), &copied, &mut manifest, &control, false).unwrap();
        let saved = fixture.path("source/original.txt");
        fs::rename(&source, &saved).unwrap();
        fs::write(&source, bytes).unwrap();
        OpenOptions::new().write(true).open(&source).unwrap()
            .set_times(fs::FileTimes::new().set_modified(original.modified().unwrap())).unwrap();
        assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), original.modified().unwrap());
        assert!(verify_cross_volume(&fixture.path("source"), &copied, &mut manifest, &control, true).is_err());
        assert_eq!(fs::read(&source).unwrap(), bytes);
        assert_eq!(fs::read(&saved).unwrap(), bytes);
        assert_eq!(fs::read(copied.join("file.txt")).unwrap(), bytes);
    }

    #[test]
    #[ignore = "需要通过 FEATHRISE_FILE_OPERATION_SECOND_VOLUME 指定另一个本地卷上的验收目录"]
    fn moves_file_and_folder_via_verified_worker_on_two_real_volumes() {
        let source = Fixture::new();
        let target_parent = PathBuf::from(std::env::var_os("FEATHRISE_FILE_OPERATION_SECOND_VOLUME")
            .expect("指定另一卷的真实验收目录"));
        assert!(target_parent.is_absolute() && target_parent.is_dir());
        let target = Fixture::under(target_parent);
        let loose = source.path("source/loose.txt");
        let folder = source.path("source/asset-folder");
        let nested = folder.join("子模型");
        fs::create_dir_all(&nested).unwrap();
        fs::write(&loose, b"loose asset").unwrap();
        fs::write(source.path("source/loose.txt:editor-validation"), b"file stream").unwrap();
        fs::write(nested.join("模型.txt"), b"nested asset").unwrap();
        let destination = target.path("destination");
        // 在两个真实卷上确认目录进入受检路径，同时覆盖系统移动单文件时的附加数据流。
        assert_eq!(system_move_without_replace(&folder.canonicalize().unwrap(),
            &destination.canonicalize().unwrap().join("asset-folder"), false).unwrap_err().kind(), io::ErrorKind::CrossesDevices);
        let mut job = job(&source);
        job.request.allowed_roots.push(target.path(""));
        job.request.operation = FileOperation::Move {
            sources: vec![loose.clone(), folder.clone()].into(),
            destination: destination.clone(),
            conflict: ConflictPolicy::Fail,
        };
        let shared = Shared::default();
        let control = Control { shared: &shared, job: &job };
        let mut receipt = FileOperationReceipt::new(job.id, 1, job.request.operation.kind());
        execute(&job.request, &control, &mut receipt, &mut None).unwrap();
        assert_eq!(receipt.completed.len(), 2);
        assert!(receipt.completed.iter().all(|change| !change.source_retained));
        assert!(!loose.exists() && !folder.exists());
        assert_eq!(fs::read(destination.join("loose.txt")).unwrap(), b"loose asset");
        assert_eq!(fs::read(destination.join("loose.txt:editor-validation")).unwrap(), b"file stream");
        assert_eq!(fs::read(destination.join("asset-folder/子模型/模型.txt")).unwrap(), b"nested asset");
        assert_eq!(fs::read_dir(destination).unwrap().count(), 2, "正式文件以外的暂存已清理");
    }
}
