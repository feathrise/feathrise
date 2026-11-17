//! 所有导航记录 IO 均在单一后台槽内完成；完整结果按根与修订采用。

use super::{constants::*, helpers::asset_path, types::*};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

fn state_path(project: &Path, directory: &str) -> Result<PathBuf, String> {
    feathrise_scene_content::authoring_io::safe_content_path(
        project,
        &format!("{directory}/{STATE_FILE}"),
    )
}

fn decode(bytes: &[u8]) -> Result<Snapshot, String> {
    let saved: SavedNavigation =
        serde_json::from_slice(bytes).map_err(|error| format!("导航记录格式错误：{error}"))?;
    if saved.version != 1 {
        return Err("导航记录版本不兼容，已保留原文件".into());
    }
    let mut snapshot = Snapshot::default();
    for (targets, limit, entries) in [
        (saved.favorites, FAVORITES, &mut snapshot.favorites),
        (saved.recent, RECENT, &mut snapshot.recent),
    ] {
        if targets.len() > limit {
            return Err("导航记录项数超过个人导航预算".into());
        }
        for target in targets {
            asset_path(&target)?;
            if !entries.iter().any(|entry| entry.target == target) {
                entries.push(ShortcutEntry {
                    target,
                    available: false,
                });
            }
        }
    }
    Ok(snapshot)
}

fn load(root: &Path) -> Result<Snapshot, String> {
    let project = crate::ui::external_files::checked_project(root)?;
    // 新位置缺席时兼容旧工作树根目录的个人记录。正常保存才写入新位置。
    for directory in [PREFERENCES_DIRECTORY, ".local"] {
        let path = state_path(&project, directory)?;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("读取导航记录失败：{error}")),
        };
        let mut bytes = Vec::new();
        file.take(STATE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > STATE_BYTES {
            return Err("导航记录超过 128 KiB，已保留原文件".into());
        }
        return decode(&bytes);
    }
    Ok(Snapshot::default())
}

fn check(root: &Path, snapshot: &mut Snapshot) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("资产根目录不可用：{error}"))?;
    for entry in snapshot
        .favorites
        .iter_mut()
        .chain(snapshot.recent.iter_mut())
    {
        let relative = asset_path(&entry.target)?;
        entry.available = root
            .join(relative)
            .canonicalize()
            .ok()
            .filter(|path| path.starts_with(&root))
            .and_then(|path| std::fs::metadata(path).ok())
            .is_some_and(|metadata| {
                if entry.target.is_directory {
                    metadata.is_dir()
                } else {
                    metadata.is_file()
                }
            });
    } // 最多84次路径校验，不递归目录、不读取资产正文，失效记录仍可显式移除。
    Ok(())
}

fn save(root: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let saved = SavedNavigation {
        version: 1,
        favorites: snapshot
            .favorites
            .iter()
            .map(|entry| entry.target.clone())
            .collect(),
        recent: snapshot
            .recent
            .iter()
            .map(|entry| entry.target.clone())
            .collect(),
    };
    let mut bytes = serde_json::to_vec_pretty(&saved).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > STATE_BYTES {
        return Err("导航记录超过保存预算".into());
    }
    bytes.push(b'\n');
    let project = crate::ui::external_files::checked_project(root)?;
    let path = state_path(&project, PREFERENCES_DIRECTORY)?;
    let directory = path.parent().ok_or("个人导航文件缺少父目录")?;
    std::fs::create_dir_all(directory).map_err(|error| format!("创建个人导航目录失败：{error}"))?;
    // 创建后再次核对路径。沿用同目录原子替换，失败保留现有记录。
    crate::authoring::atomic_write(&state_path(&project, PREFERENCES_DIRECTORY)?, &bytes)
}

pub(super) fn run(job: Job) -> Completion {
    let result = (|| {
        let mut snapshot = if job.kind == JobKind::Load {
            load(&job.root)?
        } else {
            job.snapshot
        };
        check(&job.root, &mut snapshot)?;
        if job.kind == JobKind::Save {
            save(&job.root, &snapshot)?;
        }
        Ok(snapshot)
    })();
    Completion {
        root: job.root,
        revision: job.revision,
        kind: job.kind,
        result,
    }
}

#[cfg(test)]
pub(super) fn decode_for_tests(bytes: &[u8]) -> Result<Snapshot, String> {
    decode(bytes)
}
