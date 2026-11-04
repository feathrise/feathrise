//! 独立核对导出完整清单与每个普通文件的真实字节；不复用待验的导出器验证函数。

use super::read_json;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, fs, io::{Read, Write}, path::{Component, Path, PathBuf}};

const ENTRY_LIMIT: usize = 100_000;
const BYTE_LIMIT: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Identity { pub bytes: u64, pub sha256: String }

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct Inventory {
    pub files: BTreeMap<PathBuf, Identity>,
    pub directories: BTreeSet<PathBuf>,
}

pub(super) fn relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.components().any(|part| !matches!(part, Component::Normal(_))) {
        return Err(format!("验收清单路径不是包内普通相对路径：{}", path.display()));
    }
    Ok(())
}

fn ordinary(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    let mut redirected = metadata.file_type().is_symlink();
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        redirected |= metadata.file_attributes() & 0x400 != 0;
    }
    if redirected || (!metadata.is_dir() && !metadata.is_file()) {
        return Err(format!("自有验收不跟随链接或特殊文件：{}", path.display()));
    }
    Ok(metadata)
}

pub(super) fn hash(path: &Path) -> Result<Identity, String> {
    if !ordinary(path)?.is_file() { return Err("摘要来源不是普通文件".into()); }
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 { break; }
        bytes = bytes.checked_add(count as u64).filter(|bytes| *bytes <= BYTE_LIMIT).ok_or("验收文件超过64GiB")?;
        digest.update(&buffer[..count]);
    }
    Ok(Identity { bytes, sha256: digest.finalize().iter().map(|byte|format!("{byte:02x}")).collect() })
}

fn inventory(root: &Path) -> Result<Inventory, String> {
    if !ordinary(root)?.is_dir() { return Err("包根不是普通目录".into()); }
    let mut found = Inventory { files: BTreeMap::new(), directories: BTreeSet::new() };
    let mut pending = vec![PathBuf::new()];
    let mut total = 0_u64;
    while let Some(relative) = pending.pop() {
        for entry in fs::read_dir(root.join(relative)).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|error| error.to_string())?.to_path_buf();
            let metadata = ordinary(&path)?;
            if metadata.is_dir() {
                found.directories.insert(relative.clone());
                pending.push(relative);
            } else {
                let identity = hash(&path)?;
                total = total.checked_add(identity.bytes).filter(|bytes| *bytes <= BYTE_LIMIT).ok_or("验收包超过64GiB")?;
                found.files.insert(relative, identity);
            }
            if found.files.len() > ENTRY_LIMIT || found.directories.len() > ENTRY_LIMIT {
                return Err("验收包超过100000项，未截断检查".into());
            }
        }
    }
    Ok(found)
}

/// 文件身份、目录集合和小标记同时满足才返回；成功回执本身不能代替磁盘核验。
pub(super) fn verify(root: &Path, project_id: &str, package: &str) -> Result<(Inventory, Value), String> {
    let marker = read_json(&root.join("feathrise-export.json"), 64 * 1024)?;
    if marker["format"] != 1 || marker["kind"] != "feathrise-export" || marker["project_id"] != project_id
        || marker["package"] != package || marker["manifest"] != "export-manifest.json" {
        return Err("独立导出标记未匹配本轮工程身份".into());
    }
    let manifest_path = root.join("export-manifest.json");
    if marker["manifest_sha256"] != hash(&manifest_path)?.sha256 { return Err("完整清单SHA-256不匹配小标记".into()); }
    let manifest = read_json(&manifest_path, 32 * 1024 * 1024)?;
    if manifest["format"] != 1 { return Err("独立核对不支持此导出清单版本".into()); }
    let files = manifest["files"].as_array().filter(|files| !files.is_empty() && files.len() <= ENTRY_LIMIT)
        .ok_or("导出清单文件集合为空或超预算")?;
    let mut expected = Inventory { files: BTreeMap::new(), directories: BTreeSet::new() };
    for file in files {
        let path = PathBuf::from(file["path"].as_str().ok_or("清单缺少相对文件名")?);
        relative(&path)?;
        if matches!(path.to_str(), Some("feathrise-export.json" | "export-manifest.json")) {
            return Err("数据清单不得递归纳入自己或小标记".into());
        }
        let identity = Identity {
            bytes: file["bytes"].as_u64().ok_or("清单缺少字节数")?,
            sha256: file["sha256"].as_str().ok_or("清单缺少SHA-256")?.into(),
        };
        if expected.files.insert(path, identity).is_some() { return Err("清单包含重复文件".into()); }
    }
    for directory in manifest["directories"].as_array().filter(|items| items.len() <= ENTRY_LIMIT).ok_or("缺少完整目录集合")? {
        let path = PathBuf::from(directory.as_str().ok_or("清单目录名无效")?);
        relative(&path)?;
        if !expected.directories.insert(path) { return Err("清单包含重复目录".into()); }
    }
    let executable = PathBuf::from(marker["executable"].as_str().ok_or("导出标记缺少入口")?);
    relative(&executable)?;
    if executable.components().count() != 1 || !expected.files.contains_key(&executable)
        || !expected.files.contains_key(Path::new("editor-project.json")) {
        return Err("完整清单缺少包根程序或工程描述".into());
    }
    for name in ["feathrise-export.json", "export-manifest.json"] { expected.files.insert(name.into(), hash(&root.join(name))?); }
    let actual = inventory(root)?;
    if actual != expected { return Err("实际包的完整路径、空目录或文件字节与清单不一致".into()); }
    Ok((actual, marker))
}

/// 目标由本轮create_dir独占，完整搬运包含空目录；搬运后重新枚举并逐文件核对。
pub(super) fn copy_to(source: &Path, destination: &Path, expected: &Inventory) -> Result<(), String> {
    fs::create_dir(destination).map_err(|error| error.to_string())?;
    for directory in &expected.directories { fs::create_dir_all(destination.join(directory)).map_err(|error| error.to_string())?; }
    let mut buffer = [0_u8; 64 * 1024];
    for path in expected.files.keys() {
        let mut input = fs::File::open(source.join(path)).map_err(|error| error.to_string())?;
        let mut output = fs::OpenOptions::new().write(true).create_new(true).open(destination.join(path)).map_err(|error| error.to_string())?;
        loop {
            let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 { break; }
            output.write_all(&buffer[..count]).map_err(|error| error.to_string())?;
        }
    }
    if &inventory(destination)? != expected { return Err("工程外独立副本与完整导出包不同".into()); }
    Ok(())
}

pub(super) fn summary(inventory: &Inventory) -> Value {
    json!({"file_count":inventory.files.len(),"directory_count":inventory.directories.len(),
        "bytes":inventory.files.values().map(|file|file.bytes).sum::<u64>(),"inventory":inventory})
}
