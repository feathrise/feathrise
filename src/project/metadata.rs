//! 当前工程资料的后台读取和保存。工程身份、原始字节及图标文件归属都在发布前复核。

mod descriptor;
mod icon;

use super::{Project, ProjectManifest};
use crate::{project_identity::{self, ProjectIdentity}, startup_store::replace_project_file};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{path::{Path, PathBuf}, sync::Arc};

pub(crate) use icon::{preview_icon, saved_icon_path};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Metadata {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct MetadataSnapshot {
    pub metadata: Metadata,
    identity: ProjectIdentity,
    file_name: &'static str,
    raw: Arc<[u8]>,
}

pub(crate) struct Edit {
    pub name: String,
    pub description: String,
    pub icon: IconEdit,
}

#[derive(Clone)]
pub(crate) enum IconEdit {
    Keep,
    Remove,
    Import(PathBuf),
}

pub(crate) fn read(project: &Project) -> Result<MetadataSnapshot, String> {
    let identity = project_identity::identity(project)?;
    let file_name = super::descriptor_name(&project.root)?;
    let path = project_identity::checked_path(&project.root, Path::new(file_name), false)?;
    let raw = super::read_descriptor_bytes(&path)?;
    if digest(&raw) != identity.descriptor_sha256 || project_identity::identity(project)? != identity {
        return Err("读取期间工程清单已变化，请重新载入工程设置".into());
    }
    let metadata = super::parse_descriptor(file_name, &raw)?;
    Ok(MetadataSnapshot { metadata, identity, file_name, raw: raw.into() })
}

pub(crate) fn save(
    project: &Project,
    before: &MetadataSnapshot,
    edit: Edit,
) -> Result<(Project, MetadataSnapshot), String> {
    super::valid_text("工程名称", &edit.name, 256)?;
    if edit.description.chars().any(|value| value.is_control() && !matches!(value, '\n' | '\r' | '\t')) {
        return Err("工程简介包含无效的控制字符".into());
    }
    ensure_current(project, before)?;
    let mut metadata = Metadata { name: edit.name, description: edit.description, icon: before.metadata.icon.clone() };
    let imported = match edit.icon {
        IconEdit::Keep => None,
        IconEdit::Remove => { metadata.icon = None; None }
        IconEdit::Import(path) => {
            let imported = icon::Import::prepare(project, &path)?;
            metadata.icon = Some(imported.relative.clone());
            Some(imported)
        }
    };
    let raw = if metadata == before.metadata {
        before.raw.clone()
    } else {
        let bytes = descriptor::update(before, &metadata)?;
        if bytes.len() > super::MANIFEST_LIMIT {
            return Err("保存后的工程清单超过 64 KiB 上限".into());
        }
        let _: ProjectManifest = super::parse_descriptor(before.file_name, &bytes)?;
        if super::parse_descriptor::<Metadata>(before.file_name, &bytes)? != metadata {
            return Err("工程资料写入后的清单校验不一致".into());
        }
        Arc::from(bytes)
    };
    if let Some(imported) = imported {
        // 先发布按内容摘要命名的新图，再发布引用；原清单仍可继续使用原图标。
        imported.publish(project, || ensure_current(project, before))?;
    }
    if raw != before.raw {
        let path = project.root.join(before.file_name);
        replace_project_file(&path, &raw, true, || ensure_current(project, before))?;
    }
    let current = project.refresh_for_session()?;
    let after = read(&current)?;
    if after.raw != raw || after.identity.cargo_sha256 != before.identity.cargo_sha256 {
        return Err("保存后工程清单或 Cargo 入口再次变化，请重新载入工程设置".into());
    }
    Ok((current, after))
}

fn ensure_current(project: &Project, before: &MetadataSnapshot) -> Result<(), String> {
    if super::descriptor_name(&project.root)? != before.file_name || project_identity::identity(project)? != before.identity {
        return Err("工程清单或 Cargo 入口已变化，拒绝覆盖，请重新载入工程设置".into());
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests;
