//! 可移动导出包的身份标记与完整摘要清单；小标记不携带原源码和缓存的绝对路径。

use super::{files::{self, Plan}, paths};
use crate::{build_artifacts::{self, PublishedBundle}, build_process::BuildControl, project::Project};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::{Path, PathBuf}};

pub(super) const MARKER: &str = "feathrise-export.json";
pub(super) const MANIFEST: &str = "export-manifest.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct FileRecord {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Marker {
    pub format: u32,
    pub kind: String,
    pub project_id: String,
    pub package: String,
    pub executable: PathBuf,
    pub profile: String,
    pub export_id: String,
    pub manifest: String,
    pub manifest_sha256: String,
    pub runtime_manifest_sha256: String,
    pub source_identity: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportManifest {
    format: u32,
    files: Vec<FileRecord>,
    directories: Vec<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Verified {
    pub marker: Marker,
    pub marker_sha256: String,
}

fn digest(bytes: &[u8]) -> String { Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect() }

pub(super) fn write(project: &Project, bundle: &PublishedBundle, plan: &Plan, directory: &Path, control: &BuildControl) -> Result<Marker, String> {
    control.check()?;
    let manifest = ExportManifest { format: 1, files: plan.files.values().map(|file| file.record.clone()).collect(),
        directories: plan.directories.iter().cloned().collect() };
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > files::MANIFEST_LIMIT { return Err("完整导出清单超过32MiB，未发布导出包".into()); }
    files::write_new(&directory.join(MANIFEST), &bytes)?;
    let marker = Marker { format: 1, kind: "feathrise-export".into(), project_id: project.id.clone(), package: project.package.clone(),
        executable: plan.executable.clone(), profile: plan.profile.clone(), export_id: bevy::asset::uuid::Uuid::new_v4().to_string(),
        manifest: MANIFEST.into(), manifest_sha256: digest(&bytes), runtime_manifest_sha256: bundle.manifest_sha256.clone(),
        source_identity: plan.source_identity.clone() };
    let bytes = serde_json::to_vec_pretty(&marker).map_err(|error| error.to_string())?;
    if bytes.len() > 64 * 1024 { return Err("导出身份标记超过64KiB".into()); }
    // 标记最后写入：只有完整清单和文件全部齐备的目录才具备可运行身份。
    files::write_new(&directory.join(MARKER), &bytes)?;
    control.check()?;
    Ok(marker)
}

pub(super) fn verify(directory: &Path, project: &Project, control: &BuildControl, staging: bool) -> Result<Verified, String> {
    control.check()?;
    paths::checked(directory, true)?;
    let marker_bytes = files::read_bounded(&directory.join(MARKER), 64 * 1024, control)?;
    let marker: Marker = serde_json::from_slice(&marker_bytes).map_err(|error| format!("导出身份标记无效：{error}"))?;
    if marker.format != 1 || marker.kind != "feathrise-export" || marker.project_id != project.id
        || marker.package != project.package || marker.manifest != MANIFEST
        || bevy::asset::uuid::Uuid::parse_str(&marker.export_id).is_err() {
        return Err("目标目录不是当前工程的受支持完整导出包，原目录保留".into());
    }
    paths::relative(&marker.executable)?;
    if marker.executable.components().count() != 1 { return Err("导出程序必须位于包根目录".into()); }
    let bytes = files::read_bounded(&directory.join(MANIFEST), files::MANIFEST_LIMIT, control)?;
    if digest(&bytes) != marker.manifest_sha256 { return Err("导出文件清单摘要变化，原目录保留".into()); }
    let manifest: ExportManifest = serde_json::from_slice(&bytes).map_err(|error| format!("完整导出清单无效：{error}"))?;
    if manifest.format != 1 || manifest.files.is_empty() || manifest.files.len() > files::FILE_LIMIT || manifest.directories.len() > files::FILE_LIMIT {
        return Err("完整导出清单版本或容量无效".into());
    }
    let mut names = BTreeSet::new();
    let mut expected_files = BTreeSet::new();
    let mut expected_directories = BTreeSet::new();
    let mut bytes = 0_u64;
    for file in &manifest.files {
        control.check()?;
        paths::relative(&file.path)?;
        if file.path == Path::new(MARKER) || file.path == Path::new(MANIFEST)
            || file.path == Path::new(super::transaction::STAGE_OWNER)
            || !names.insert(file.path.to_string_lossy().to_ascii_lowercase()) {
            return Err("导出清单包含重复或保留文件名".into());
        }
        bytes = bytes.checked_add(file.bytes).filter(|size| *size <= files::BYTE_LIMIT).ok_or("导出清单大小超过64GiB")?;
        paths::checked(&directory.join(&file.path), false)?;
        if build_artifacts::hash_file(&directory.join(&file.path), control)? != (file.sha256.clone(), file.bytes) {
            return Err(format!("导出文件已变更，原目录保留：{}", file.path.display()));
        }
        expected_files.insert(file.path.clone());
    }
    if !expected_files.contains(&marker.executable) || !expected_files.contains(Path::new("editor-project.json")) {
        return Err("导出清单缺少程序入口或工程描述".into());
    }
    for directory in manifest.directories {
        paths::relative(&directory)?;
        if !names.insert(directory.to_string_lossy().to_ascii_lowercase()) { return Err("导出清单包含重复或冲突目录".into()); }
        expected_directories.insert(directory);
    }
    expected_files.insert(MARKER.into());
    expected_files.insert(MANIFEST.into());
    if staging { expected_files.insert(super::transaction::STAGE_OWNER.into()); }
    let actual = files::tree(directory, control)?;
    if actual.files != expected_files || actual.directories != expected_directories {
        return Err("导出目录含清单之外的文件或目录，拒绝覆盖并保留原内容".into());
    }
    Ok(Verified { marker, marker_sha256: digest(&marker_bytes) })
}
