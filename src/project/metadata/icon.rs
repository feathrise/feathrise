//! 已保存图标的工程归属及内容寻址导入。重复导入复用现有原始文件。

use crate::{project::Project, project_identity::checked_path, startup_store::replace_project_file};
use std::{fs::{self, File}, io::Read, path::{Path, PathBuf}};

const DIRECTORY: &str = "project-branding";

pub(crate) fn preview_icon(path: &Path) -> Result<bevy::prelude::Image, String> {
    feathrise_editor_scene::project_icon::decode(path).map(|icon| icon.preview())
}

pub(crate) fn saved_icon_path(project: &Project, relative: &Path) -> Result<PathBuf, String> {
    // 先核对登记资产根，再核对根下的图标，兼容接近路径层级上限的正式资产目录。
    for root in &project.context.asset_roots {
        if let Ok(asset_root) = root.strip_prefix(&project.root)
            && let Ok(path) = relative.strip_prefix(asset_root)
        {
            let root = checked_path(&project.root, asset_root, true)?;
            return checked_path(&root, path, false);
        }
    }
    checked_path(&project.root, relative, false)
}

pub(super) struct Import {
    pub(super) relative: PathBuf,
    bytes: Vec<u8>,
}

impl Import {
    pub(super) fn prepare(project: &Project, selected: &Path) -> Result<Self, String> {
        let icon = feathrise_editor_scene::project_icon::decode(selected)?;
        let hash: String = super::digest(&icon.bytes).iter().map(|byte| format!("{byte:02x}")).collect();
        let relative = first_asset_root(project)?.join(DIRECTORY).join(format!("{hash}.{}", icon.extension));
        Ok(Self { relative, bytes: icon.bytes })
    }

    pub(super) fn publish(self, project: &Project, verify: impl Fn() -> Result<(), String>) -> Result<(), String> {
        verify()?;
        let assets = first_asset_root(project)?;
        let assets = super::super::context::ensure_directory(&project.root, assets)?;
        let directory = super::super::context::ensure_directory(&assets, Path::new(DIRECTORY))?;
        let name = self.relative.file_name().ok_or("图标缺少文件名")?;
        let path = directory.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => return self.verify_existing(project),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(format!("无法检查工程图标：{error}")),
        }
        replace_project_file(&path, &self.bytes, false, || {
            verify()?;
            checked_path(&assets, Path::new(DIRECTORY), true)?;
            Ok(())
        }).map_err(|error| format!("发布工程图标失败：{error}"))
    }

    fn verify_existing(&self, project: &Project) -> Result<(), String> {
        let path = saved_icon_path(project, &self.relative)?;
        let mut file = File::open(path).map_err(|error| format!("无法读取同摘要图标：{error}"))?;
        let mut buffer = [0_u8; 8192];
        for bytes in self.bytes.chunks(buffer.len()) {
            let read = &mut buffer[..bytes.len()];
            file.read_exact(read).map_err(|error| format!("无法读取同摘要图标：{error}"))?;
            if read != bytes {
                return Err("同摘要图标路径已有不同内容".into());
            }
        }
        if file.read(&mut buffer[..1]).map_err(|error| format!("无法读取同摘要图标：{error}"))? != 0 {
            return Err("同摘要图标路径已有不同内容".into());
        }
        Ok(())
    }
}

fn first_asset_root(project: &Project) -> Result<&Path, String> {
    project.context.asset_roots.first().and_then(|root| root.strip_prefix(&project.root).ok())
        .ok_or_else(|| "工程没有可写的登记资产根".into())
}
