//! 后台冻结工程清单和Cargo原文字节身份，拒绝跨工程或外部修改后的迟到保存。
use crate::project::{self, Project};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};
const DESCRIPTOR_LIMIT: usize = 64 * 1024;
const CARGO_LIMIT: usize = 1024 * 1024;
const DEPTH_LIMIT: usize = 16;
/// 除已解析字段外还冻结原字节摘要，未知JSON字段的外部修改同样会使旧扫描/保存失效。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectIdentity {
    pub project_id: String,
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub descriptor_sha256: [u8; 32],
    pub cargo_sha256: [u8; 32],
}

pub(crate) fn identity(project: &Project) -> Result<ProjectIdentity, String> {
    if project::open(&project.root)? != *project {
        return Err("工程清单已变化，请重新打开工程".into());
    }
    let descriptor = checked_path(&project.root, Path::new("editor-project.json"), false)?;
    let cargo_relative = project
        .manifest
        .strip_prefix(&project.root)
        .map_err(|_| "Cargo入口不属于当前工程")?;
    let cargo = checked_path(&project.root, cargo_relative, false)?;
    if cargo != project.manifest {
        return Err("Cargo入口归属已变化".into());
    }
    let descriptor_bytes = read_bounded(&descriptor, DESCRIPTOR_LIMIT, "工程清单")?;
    let cargo_bytes = read_bounded(&cargo, CARGO_LIMIT, "Cargo清单")?;
    if project::open(&project.root)? != *project
        || read_bounded(&descriptor, DESCRIPTOR_LIMIT, "工程清单")? != descriptor_bytes
        || read_bounded(&cargo, CARGO_LIMIT, "Cargo清单")? != cargo_bytes
    {
        return Err("读取期间工程清单已变化".into());
    }
    Ok(ProjectIdentity {
        project_id: project.id.clone(),
        root: project.root.clone(),
        manifest: cargo,
        descriptor_sha256: digest(&descriptor_bytes),
        cargo_sha256: digest(&cargo_bytes),
    })
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn read_bounded(path: &Path, limit: usize, label: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(limit.min(4096));
    File::open(path)
        .map_err(|e| format!("{label}不可读：{e}"))?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{label}读取失败：{e}"))?;
    if bytes.len() > limit {
        return Err(format!("{label}超过{limit}字节上限"));
    }
    Ok(bytes)
}

fn reject_link(metadata: &fs::Metadata) -> Result<(), String> {
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.file_type().is_symlink() || reparse {
        return Err("资产路径不能是符号链接或junction".into());
    }
    Ok(())
}

fn checked_path(root: &Path, relative: &Path, directory: bool) -> Result<PathBuf, String> {
    let mut full = root.to_owned();
    if relative.components().count() > DEPTH_LIMIT {
        return Err("资产路径超过16层".into());
    }
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("资产路径不能跨目录或使用绝对路径".into());
        };
        full.push(name);
        let metadata =
            fs::symlink_metadata(&full).map_err(|e| format!("资产路径不存在或不可访问：{e}"))?;
        reject_link(&metadata)?;
    }
    let metadata =
        fs::symlink_metadata(&full).map_err(|e| format!("资产路径不存在或不可访问：{e}"))?;
    reject_link(&metadata)?;
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err("资产路径类型不符合要求".into());
    }
    let actual = full
        .canonicalize()
        .map_err(|e| format!("资产真实路径无效：{e}"))?;
    if actual != full || !actual.starts_with(root) {
        return Err("资产真实路径逃出登记根或已变化".into());
    }
    Ok(actual)
}
