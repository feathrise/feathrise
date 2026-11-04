//! 独占暂存与目录发布；只替换同工程的完整旧包，失败时恢复旧包或保留明确备份。

use super::{files, manifest::{self, Verified}, paths};
use crate::{build_process::BuildControl, project::Project};
use std::{fs, path::{Path, PathBuf}};

pub(super) const STAGE_OWNER: &str = ".feathrise-export-stage-owner";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Previous { Empty, Package(Verified) }

pub(super) fn existing(directory: &Path, project: &Project, control: &BuildControl) -> Result<Option<Previous>, String> {
    control.check()?;
    match fs::symlink_metadata(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("导出目录不可访问：{error}")),
        Ok(_) => { paths::checked(directory, true)?; }
    }
    let mut entries = fs::read_dir(directory).map_err(|error| error.to_string())?;
    if entries.next().transpose().map_err(|error| error.to_string())?.is_none() { return Ok(Some(Previous::Empty)); }
    manifest::verify(directory, project, control, false).map(|package| Some(Previous::Package(package)))
        .map_err(|error| format!("目标目录已有内容，未覆盖：{error}"))
}

pub(super) struct Stage {
    pub path: PathBuf,
    parent: PathBuf,
    token: String,
    active: bool,
}

impl Stage {
    pub(super) fn create(destination: &Path) -> Result<Self, String> {
        let parent = paths::checked(destination.parent().ok_or("导出目录缺少父目录")?, true)?;
        let token = bevy::asset::uuid::Uuid::new_v4().to_string();
        let path = parent.join(format!(".feathrise-export-stage-{token}"));
        fs::create_dir(&path).map_err(|error| format!("无法创建独占导出暂存目录：{error}"))?;
        let guard = Self { path, parent, token, active: true };
        files::write_new(&guard.path.join(STAGE_OWNER), guard.token.as_bytes())?;
        Ok(guard)
    }

    pub(super) fn publish(&mut self, destination: &Path, previous: Option<&Previous>, project: &Project, control: &BuildControl) -> Result<Option<String>, String> {
        control.check()?;
        if paths::checked(destination.parent().ok_or("导出目录缺少父目录")?, true)? != self.parent
            || existing(destination, project, control)?.as_ref() != previous {
            return Err("导出目标在准备期间变化，旧内容保留".into());
        }
        self.check_owner()?;
        // 来源复核或旧包检查可能耗时；发布前再次验新包，不能沿用此前的成功回执。
        manifest::verify(&self.path, project, control, true)?;
        control.check()?;
        let backup = self.parent.join(format!(".feathrise-export-backup-{}", bevy::asset::uuid::Uuid::new_v4()));
        if backup.exists() { return Err("导出备份名称已占用".into()); }
        // 暂存归属文件不进入运行包；失败路径会恢复它，使守卫仍能确认只清理自己的目录。
        fs::remove_file(self.path.join(STAGE_OWNER)).map_err(|error| error.to_string())?;
        let result = (|| {
            control.check()?;
            if previous.is_some() { fs::rename(destination, &backup).map_err(|error| format!("旧导出仍被占用，已保留：{error}"))?; }
            if let Err(error) = control.check() {
                let restore = if previous.is_some() { fs::rename(&backup, destination) } else { Ok(()) };
                return Err(match restore {
                    Ok(()) => error,
                    Err(restore) => format!("{error}；旧导出保留在 {}，取消恢复失败：{restore}", backup.display()),
                });
            }
            if let Err(error) = fs::rename(&self.path, destination) {
                let restore = if previous.is_some() { fs::rename(&backup, destination) } else { Ok(()) };
                return Err(match restore {
                    Ok(()) => format!("新导出未发布，旧导出已恢复：{error}"),
                    Err(restore) => format!("新导出未发布：{error}；旧导出保留在 {}，恢复失败：{restore}", backup.display()),
                });
            }
            self.active = false;
            Ok(())
        })();
        if let Err(error) = result {
            if self.path.is_dir() { let _ = files::write_new(&self.path.join(STAGE_OWNER), self.token.as_bytes()); }
            return Err(error);
        }
        // 发布成功是不可拆分的完成点；旧包被外部占用仅保留备份并给出提示，不回滚成功的新包。
        if let Some(previous) = previous {
            if let Err(error) = remove_backup(&backup, &self.parent, previous, project, control) {
                return Ok(Some(format!("新导出已完成；旧备份暂时保留在 {}：{error}", crate::project::display_path(&backup))));
            }
        }
        Ok(None)
    }

    fn check_owner(&self) -> Result<(), String> {
        let actual = paths::checked(&self.path, true)?;
        let expected_name = format!(".feathrise-export-stage-{}", self.token);
        if actual.parent() != Some(self.parent.as_path()) || actual.file_name().is_none_or(|name| name != expected_name.as_str())
            || fs::read(actual.join(STAGE_OWNER)).map_err(|error| error.to_string())? != self.token.as_bytes() {
            return Err("导出暂存目录归属变化，拒绝发布或回收".into());
        }
        Ok(())
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if !self.active || self.check_owner().is_err() { return; }
        // 递归删除前核对绝对父目录、完整UUID名称和归属文件，并拒绝任何后加的junction。
        if files::tree(&self.path, &BuildControl::default()).is_ok() { let _ = fs::remove_dir_all(&self.path); }
    }
}

fn remove_backup(directory: &Path, parent: &Path, previous: &Previous, project: &Project, control: &BuildControl) -> Result<(), String> {
    let actual = paths::checked(directory, true)?;
    let name = actual.file_name().and_then(|name| name.to_str()).ok_or("备份名称无效")?;
    if actual.parent() != Some(parent)
        || name.strip_prefix(".feathrise-export-backup-").is_none_or(|suffix| bevy::asset::uuid::Uuid::parse_str(suffix).is_err()) {
        return Err("旧导出备份路径归属检查失败".into());
    }
    if existing(&actual, project, control)?.as_ref() != Some(previous) { return Err("旧导出备份内容已变化，保留目录".into()); }
    match previous {
        Previous::Empty => fs::remove_dir(actual).map_err(|error| error.to_string()),
        Previous::Package(_) => fs::remove_dir_all(actual).map_err(|error| format!("旧备份被占用，未删除：{error}")),
    }
}
