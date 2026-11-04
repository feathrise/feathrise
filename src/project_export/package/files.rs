//! 导出文件集合与流式复制；所有枚举、摘要和回读都只在唯一后台导出任务中执行。

use super::{manifest::FileRecord, paths};
use crate::{build_artifacts::{self, PublishedBundle}, build_process::BuildControl, project::Project};
use serde::Deserialize;
use std::{collections::{BTreeMap, BTreeSet}, fs::{self, File, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, sync::mpsc::SyncSender};

// 边界是明确拒绝门而非静默截断；单个64KiB缓冲复用，清单内存随有界路径集合增长。
pub(super) const FILE_LIMIT: usize = 100_000;
pub(super) const BYTE_LIMIT: u64 = 64 * 1024 * 1024 * 1024;
pub(super) const MANIFEST_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourceFile {
    pub source: PathBuf,
    pub record: FileRecord,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Plan {
    pub files: BTreeMap<PathBuf, SourceFile>,
    pub directories: BTreeSet<PathBuf>,
    pub bytes: u64,
    pub runtime_manifest_sha256: String,
    pub executable: PathBuf,
    pub profile: String,
    pub source_identity: String,
    names: BTreeMap<String, bool>,
}

#[derive(Deserialize)]
struct RuntimeManifest {
    format: u32,
    metadata: serde_json::Value,
    executable: PathBuf,
    files: Vec<FileRecord>,
}

impl Plan {
    pub(super) fn collect(project: &Project, bundle: &PublishedBundle, control: &BuildControl) -> Result<Self, String> {
        control.check()?;
        if project.context.format != 2 {
            return Err("独立导出要求v2工程清单与saved-world v1运行入口；旧工程需显式迁移，导出不会改写原清单".into());
        }
        build_artifacts::verify(bundle, control)?;
        let bytes = read_bounded(&bundle.directory.join("runtime-manifest.json"), 128 * 1024, control)?;
        let manifest: RuntimeManifest = serde_json::from_slice(&bytes).map_err(|error| format!("运行版本清单无效：{error}"))?;
        if manifest.format != 1 || manifest.files.is_empty() || manifest.files.len() > 256
            || bundle.directory.join(&manifest.executable) != bundle.executable
            || manifest.metadata["project_id"].as_str() != Some(project.id.as_str())
            || manifest.metadata["project_root"].as_str().map(Path::new) != Some(project.root.as_path()) {
            return Err("运行版本清单入口与本次完整构建不一致".into());
        }
        let source_identity = manifest.metadata["export_source_identity"].as_str()
            .filter(|identity| !identity.is_empty() && identity.len() <= 256).ok_or("运行版本尚未绑定本次导出的完整源码身份")?.to_owned();
        let mut plan = Self { files: BTreeMap::new(), directories: BTreeSet::new(), bytes: 0,
            runtime_manifest_sha256: bundle.manifest_sha256.clone(), executable: manifest.executable,
            profile: manifest.metadata["profile"].as_str().unwrap_or("unknown").to_owned(), source_identity, names: BTreeMap::new() };
        for record in manifest.files {
            paths::relative(&record.path)?;
            let source = bundle.directory.join(&record.path);
            plan.add_file(source, record.path.clone(), control)?;
            if plan.files.get(&record.path).is_none_or(|file| file.record != record) {
                return Err("本次运行文件与已验证构建清单不一致".into());
            }
        }
        plan.add_file(project.root.join("editor-project.json"), "editor-project.json".into(), control)?;
        let selection = project.root.join("scene.layout.json");
        match fs::symlink_metadata(&selection) {
            Ok(_) => {
                let bytes = read_bounded(&selection, crate::startup_document::MAX_BYTES as u64, control)?;
                let document = crate::startup_document::parse(&bytes, &project.id)?;
                // 启动选择必须与独立运行入口使用同一校验；空工程允许无选择，已选地图必须可加载。
                if let Some(id) = feathrise_project::selection(&document.extra)? {
                    let context = feathrise_runtime::ProjectContext {
                        root: project.root.clone(), id: project.id.clone(), name: project.name.clone(),
                        asset_roots: project.context.asset_roots.clone(),
                    };
                    feathrise_runtime::validate_saved_map_with_check(&context, id, || control.check())?;
                }
                plan.add_file(selection, "scene.layout.json".into(), control)?;
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(format!("已保存启动场景不可读：{error}")),
        }
        for root in &project.context.asset_roots {
            let checked = paths::checked(root, true)?;
            if !paths::contains(&project.root, &checked) { return Err("资产根不属于当前工程".into()); }
            let relative = root.strip_prefix(&project.root).map_err(|_| "资产根不属于工程")?;
            paths::relative(relative)?;
            let tree = tree(root, control)?;
            plan.add_directory(relative.to_owned())?;
            for directory in tree.directories { plan.add_directory(relative.join(directory))?; }
            for file in tree.files { plan.add_file(root.join(&file), relative.join(file), control)?; }
        }
        if !plan.files.contains_key(&plan.executable) { return Err("导出包缺少构建回执中的运行入口".into()); }
        Ok(plan)
    }

    fn add_directory(&mut self, path: PathBuf) -> Result<(), String> {
        paths::relative(&path)?;
        let mut current = PathBuf::new();
        for component in path.components() {
            current.push(component);
            let key = current.to_string_lossy().to_ascii_lowercase();
            if self.names.get(&key) == Some(&false) { return Err("导出文件与目录名称冲突".into()); }
            self.names.insert(key, true);
            self.directories.insert(current.clone());
        }
        if self.directories.len() > FILE_LIMIT { return Err("导出目录超过100000项，未截断内容".into()); }
        Ok(())
    }

    fn add_file(&mut self, source: PathBuf, relative: PathBuf, control: &BuildControl) -> Result<(), String> {
        paths::relative(&relative)?;
        if relative == Path::new(super::manifest::MARKER) || relative == Path::new(super::manifest::MANIFEST)
            || relative == Path::new(super::transaction::STAGE_OWNER) {
            return Err("导出来源占用了包清单保留名称".into());
        }
        let source = paths::checked(&source, false)?;
        let (sha256, bytes) = build_artifacts::hash_file(&source, control)?;
        let record = FileRecord { path: relative.clone(), bytes, sha256 };
        if let Some(previous) = self.files.get(&relative) {
            if previous.source == source && previous.record == record { return Ok(()); }
            return Err("多个导出来源使用了同一相对文件名".into());
        }
        let key = relative.to_string_lossy().to_ascii_lowercase();
        if self.names.contains_key(&key) {
            return Err("导出来源存在Windows大小写或文件目录冲突".into());
        }
        if self.files.len() == FILE_LIMIT { return Err("导出文件超过100000项，未截断内容".into()); }
        self.bytes = self.bytes.checked_add(bytes).filter(|bytes| *bytes <= BYTE_LIMIT).ok_or("导出总大小超过64GiB，未截断内容")?;
        if let Some(parent) = relative.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            let mut current = PathBuf::new();
            for component in parent.components() { current.push(component); self.add_directory(current.clone())?; }
        }
        self.names.insert(key, false);
        self.files.insert(relative, SourceFile { source, record });
        Ok(())
    }

    pub(super) fn copy(&self, directory: &Path, control: &BuildControl, progress: &SyncSender<String>) -> Result<(), String> {
        for relative in &self.directories {
            control.check()?;
            let mut current = paths::checked(directory, true)?;
            for component in relative.components() {
                current.push(component);
                match fs::create_dir(&current) {
                    Ok(()) => (),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                    Err(error) => return Err(format!("无法创建导出目录：{error}")),
                }
                paths::checked(&current, true)?;
            }
        }
        for (index, file) in self.files.values().enumerate() {
            control.check()?;
            let source = paths::checked(&file.source, false)?;
            if source != file.source { return Err("导出来源真实路径已变化".into()); }
            let destination = directory.join(&file.record.path);
            copy_file(&source, &destination, file.record.bytes, control)?;
            if build_artifacts::hash_file(&destination, control)? != (file.record.sha256.clone(), file.record.bytes)
                || build_artifacts::hash_file(&source, control)? != (file.record.sha256.clone(), file.record.bytes) {
                return Err(format!("文件在导出期间变化，旧导出保留：{}", file.record.path.display()));
            }
            if index % 16 == 0 || index + 1 == self.files.len() {
                let _ = progress.try_send(format!("正在复制文件：{}/{}", index + 1, self.files.len()));
            }
        }
        Ok(())
    }

    pub(super) fn verify_sources(&self, project: &Project, bundle: &PublishedBundle, target: &Path, control: &BuildControl) -> Result<(), String> {
        if crate::project::open(&project.root)? != *project || Self::collect(project, bundle, control)? != *self {
            return Err("导出期间已保存工程、资产或构建身份发生变化；未发布混合版本".into());
        }
        // 导出可从未安装实时监听的入口调用；发布前仍独立复核完整源码与工具链身份。
        if crate::code_iteration::export_source_identity(project, target, control)? != self.source_identity {
            return Err("复制资产期间工程源码已变化，原输出保留，请重新导出".into());
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Tree {
    pub files: BTreeSet<PathBuf>,
    pub directories: BTreeSet<PathBuf>,
}

/// 保留空目录，并检查每一级链接；目录内容不通过 read_dir().collect() 无界载入。
pub(super) fn tree(root: &Path, control: &BuildControl) -> Result<Tree, String> {
    paths::checked(root, true)?;
    let mut result = Tree { files: BTreeSet::new(), directories: BTreeSet::new() };
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        control.check()?;
        let directory = root.join(&relative);
        paths::checked(&directory, true)?;
        for entry in fs::read_dir(directory).map_err(|error| format!("导出目录读取失败：{error}"))? {
            control.check()?;
            let entry = entry.map_err(|error| error.to_string())?;
            let relative = relative.join(entry.file_name());
            paths::relative(&relative)?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
            if !paths::ordinary(&metadata) { return Err(format!("导出不能复制链接或junction：{}", entry.path().display())); }
            if metadata.is_dir() { result.directories.insert(relative.clone()); pending.push(relative); }
            else if metadata.is_file() { result.files.insert(relative); }
            else { return Err("导出只支持普通文件与目录".into()); }
            if result.files.len() > FILE_LIMIT + 3 || result.directories.len() > FILE_LIMIT { return Err("导出目录条目超过100000项".into()); }
        }
    }
    Ok(result)
}

pub(super) fn read_bounded(path: &Path, limit: u64, control: &BuildControl) -> Result<Vec<u8>, String> {
    paths::checked(path, false)?;
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > limit { return Err(format!("导出清单超过{}字节预算", limit)); }
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        control.check()?;
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 { break; }
        if bytes.len() as u64 + count as u64 > limit { return Err("导出清单读取期间超过容量预算".into()); }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(bytes)
}

pub(super) fn copy_file(source: &Path, destination: &Path, expected_bytes: u64, control: &BuildControl) -> Result<(), String> {
    paths::checked(destination.parent().ok_or("导出文件缺少父目录")?, true)?;
    let mut source = File::open(source).map_err(|error| format!("导出来源不可读：{error}"))?;
    let mut destination = OpenOptions::new().write(true).create_new(true).open(destination).map_err(|error| format!("导出文件无法独占创建：{error}"))?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut copied = 0_u64;
    loop {
        control.check()?;
        let count = source.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 { break; }
        // 冻结长度同时是本次复制的写入预算；来源增长时不能等到完整回读才拒收，避免暂存先写爆磁盘。
        copied = copied.checked_add(count as u64).filter(|bytes| *bytes <= expected_bytes)
            .ok_or("导出来源在复制期间增长，已停止写入并保留原输出")?;
        destination.write_all(&buffer[..count]).map_err(|error| error.to_string())?;
    }
    if copied != expected_bytes { return Err("导出来源在复制期间缩短，原输出保留".into()); }
    destination.sync_all().map_err(|error| error.to_string())
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    paths::checked(path.parent().ok_or("导出清单缺少父目录")?, true)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path).map_err(|error| format!("导出清单无法独占创建：{error}"))?;
    file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|error| error.to_string())
}
