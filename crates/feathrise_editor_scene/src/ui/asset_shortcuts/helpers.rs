//! 有界导航快照变更与严格项目相对路径门；不读取文件系统。

use super::{constants::*, resources::AssetShortcuts, types::*};

pub(super) fn target(path: &str, is_directory: bool) -> Result<ShortcutTarget, String> {
    let path = path.split('#').next().unwrap_or_default(); // 子资产访问归入所属文件。
    let result = ShortcutTarget {
        path: format!("assets/{path}"),
        is_directory,
    };
    asset_path(&result)?;
    Ok(result)
}

pub(super) fn asset_path(target: &ShortcutTarget) -> Result<&str, String> {
    if target.path.len() > PATH_BYTES {
        return Err("导航路径超过 1024 字节".into());
    }
    let path = target
        .path
        .strip_prefix("assets/")
        .ok_or("导航记录必须相对于当前工程的 assets/ 目录")?;
    let normalized = crate::asset_catalog::normalize_asset_reference(path)?;
    if normalized != path || path.contains('#') {
        return Err("导航记录不是规范文件路径".into());
    }
    Ok(path)
}

pub(super) fn apply(snapshot: &mut Snapshot, change: &Change) -> Result<bool, String> {
    match change {
        Change::Favorite(target, favorite) => {
            asset_path(target)?;
            if let Some(index) = snapshot
                .favorites
                .iter()
                .position(|entry| entry.target == *target)
            {
                if *favorite {
                    return Ok(false);
                }
                snapshot.favorites.remove(index);
            } else {
                if !*favorite {
                    return Ok(false);
                }
                if snapshot.favorites.len() == FAVORITES {
                    return Err("最多收藏 64 项，请先移除一项".into());
                }
                snapshot.favorites.push(ShortcutEntry {
                    target: target.clone(),
                    available: true,
                });
            }
        }
        Change::Visit(target) => {
            asset_path(target)?;
            if snapshot
                .recent
                .first()
                .is_some_and(|entry| entry.target == *target && entry.available)
            {
                return Ok(false);
            }
            snapshot.recent.retain(|entry| entry.target != *target);
            snapshot.recent.insert(
                0,
                ShortcutEntry {
                    target: target.clone(),
                    available: true,
                },
            );
            snapshot.recent.truncate(RECENT); // MRU 的二十项是个人历史预算，不截断真实资产目录。
        }
        Change::Remove(section, target) => {
            let list = match section {
                Section::Favorites => &mut snapshot.favorites,
                Section::Recent => &mut snapshot.recent,
            };
            let before = list.len();
            list.retain(|entry| entry.target != *target);
            return Ok(before != list.len());
        }
        Change::ClearRecent => {
            if snapshot.recent.is_empty() {
                return Ok(false);
            }
            snapshot.recent.clear();
        }
    }
    Ok(true)
}

pub(super) fn change(state: &mut AssetShortcuts, change: Change) -> Result<(), String> {
    if !state.loaded && state.pending.len() == PENDING_CHANGES {
        return Err("导航记录仍在载入，请稍后重试".into()); // 先检查队列预算，不能改完内存后丢掉载入重放。
    }
    let changed = apply(&mut state.snapshot, &change)?;
    if !state.loaded {
        state.pending.push(change);
    } // 包括当前缓存无变化的明确意图；磁盘基线可能已经收藏同一文件。
    if changed {
        state.revision = state.revision.wrapping_add(1);
        state.failed_revision = None; // 新的显式意图允许重试保存，稳定帧失败不反复写磁盘。
    }
    Ok(())
}
