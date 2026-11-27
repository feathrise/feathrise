//! 复制任务独占目标目录，源树按原版本复核，普通数据文件保持流式读写。

use super::{check_cancelled, content::CopyPaths};
use feathrise_editor_assets::glb_metadata;
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::SystemTime,
};

pub(super) fn ordinary(path: &Path) -> Result<Metadata, String> {
    glb_metadata::ordinary(path)
}

fn normalized_path_eq(left: &Path, right: &Path) -> bool {
    if left == right { return true; }
    cfg!(windows) && left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
}

pub(super) fn checked_directory(path: &Path) -> Result<Metadata, String> {
    let metadata = ordinary(path)?;
    let canonical = path.canonicalize()
        .map_err(|error| format!("无法定位场景目录 {}：{error}", path.display()))?;
    if !metadata.is_dir() || !normalized_path_eq(path, &canonical) {
        return Err(format!("场景目录不是未重定向的普通文件夹：{}", path.display()));
    }
    Ok(metadata)
}

#[derive(PartialEq, Eq)]
struct SourceStamp {
    directory: bool,
    length: u64,
    changed: Option<SystemTime>,
    created: Option<SystemTime>,
}

impl SourceStamp {
    fn read(metadata: &Metadata) -> Self {
        Self {
            directory: metadata.is_dir(),
            length: metadata.len(),
            changed: metadata.modified().ok(),
            created: metadata.created().ok(),
        }
    }
}

pub(super) struct CopyEntry {
    pub(super) source: String,
    pub(super) target: String,
    pub(super) directory: bool,
    pub(super) length: u64,
    root: bool,
    stamp: SourceStamp,
}

impl CopyEntry {
    pub(super) fn create_directory(&self, assets: &Path) -> Result<(), String> {
        if !self.root {
            let target = assets.join(&self.target);
            fs::create_dir(&target)
                .map_err(|error| format!("创建副本目录 {} 失败：{error}", target.display()))?;
        }
        Ok(())
    }

    pub(super) fn recheck(&self, assets: &Path) -> Result<(), String> {
        let path = assets.join(&self.source);
        let metadata = if self.directory { checked_directory(&path)? } else { ordinary(&path)? };
        if SourceStamp::read(&metadata) != self.stamp {
            return Err(format!("源场景文件在复制期间发生变化：{}", self.source));
        }
        Ok(())
    }
}

pub(super) fn collect_entries(paths: &CopyPaths, cancelled: &AtomicBool) -> Result<Vec<CopyEntry>, String> {
    let mut pending = paths.roots().iter().map(|mapping| (
        mapping.source.clone(), mapping.target.clone(), true,
    )).collect::<Vec<_>>();
    let mut entries = Vec::new();
    while let Some((source, target, root)) = pending.pop() {
        check_cancelled(cancelled)?;
        let absolute = paths.assets.join(&source);
        let metadata = ordinary(&absolute)?;
        let directory = metadata.is_dir();
        if directory {
            checked_directory(&absolute)?;
            let children = fs::read_dir(&absolute)
                .map_err(|error| format!("读取源目录 {} 失败：{error}", absolute.display()))?;
            for entry in children {
                check_cancelled(cancelled)?;
                let name = entry.map_err(|error| error.to_string())?.file_name()
                    .into_string().map_err(|_| format!("目录含有不是 UTF-8 的文件名：{}", absolute.display()))?;
                pending.push((format!("{source}/{name}"), format!("{target}/{name}"), false));
            }
        } else if root || !metadata.is_file() {
            return Err(format!("场景复制遇到非普通文件或目录：{}", absolute.display()));
        }
        entries.push(CopyEntry {
            source, target, directory, root,
            length: metadata.len(),
            stamp: SourceStamp::read(&metadata),
        });
    }
    Ok(entries)
}

pub(super) fn open_output(path: &Path) -> Result<File, String> {
    OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|error| format!("创建副本文件 {} 失败：{error}", path.display()))
}

pub(super) fn stream(
    mut source: impl Read,
    output: &mut File,
    bytes: u64,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let mut copied = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        check_cancelled(cancelled)?;
        let size = source.read(&mut buffer).map_err(|error| error.to_string())?;
        if size == 0 { break; }
        copied += size as u64;
        if copied > bytes { return Err("源场景文件在复制期间增长".into()); }
        output.write_all(&buffer[..size]).map_err(|error| error.to_string())?;
    }
    if copied != bytes { return Err("源场景文件在复制期间缩短".into()); }
    Ok(())
}

pub(super) struct OwnedFolders<'a> {
    root: &'a Path,
    created: Vec<(PathBuf, Option<SystemTime>)>,
}

impl<'a> OwnedFolders<'a> {
    pub(super) fn new(root: &'a Path) -> Self {
        Self { root, created: Vec::new() }
    }

    pub(super) fn create(&mut self, path: &Path) -> Result<(), String> {
        if path.parent() != Some(self.root) {
            return Err(format!("副本目录必须属于场景根：{}", path.display()));
        }
        // 不接管旧目录。副本正式可见前，只有本次成功独占创建的目录允许回收。
        fs::create_dir(path)
            .map_err(|error| format!("创建副本目录 {} 失败：{error}", path.display()))?;
        self.created.push((path.to_owned(), fs::metadata(path).and_then(|metadata| metadata.created()).ok()));
        Ok(())
    }

    pub(super) fn cleanup(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        for (path, created) in self.created.iter().rev() {
            let remove = || -> Result<(), String> {
                checked_directory(self.root)?;
                let metadata = checked_directory(path)?;
                if path.parent() != Some(self.root) || created.is_some_and(|time| metadata.created().ok() != Some(time)) {
                    return Err(format!("未登记目录已被替换，保留 {}", path.display()));
                }
                fs::remove_dir_all(path)
                    .map_err(|error| format!("移除本次副本目录 {} 失败：{error}", path.display()))
            };
            if let Err(error) = remove() { errors.push(error); }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("；")) }
    }
}
