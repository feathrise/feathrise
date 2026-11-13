//! 浮窗与固定文件栏复用真实分类、GLB 元数据缓存和目录监听；全桌面仍只有两个目录 IO 槽。

use super::{
    browser::{
        BrowserSessions,
        resources::{BrowserScan, BrowserSession, BrowserSnapshot},
    },
    resources::FileManagerState,
    types::FileEntry,
};
use crate::ui::asset_shortcuts::AssetVisited;
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
};
use feathrise_editor_project_browser_model::{
    DirectoryFilters, DirectoryOperations, DirectoryQuery, DirectoryWorker, FolderScope,
};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

pub(super) fn start(
    mut state: ResMut<FileManagerState>,
    mut browsers: ResMut<BrowserSessions>,
    operations: Option<Res<DirectoryOperations>>,
    settings: Option<Res<feathrise_editor_core::EditorSettings>>,
    host: Option<Res<crate::workbench::WorkbenchModel>>,
) {
    let Some(operations) = operations else {
        return;
    };
    browsers.asset_root = match host.as_ref() {
        Some(host) => host.asset_root.clone(), // 宿主显式没有工程时，不能浏览编辑器自身同名资产。
        None => Some(feathrise_editor_core::workspace_asset_root()),
    };
    let stale: Vec<_> = browsers
        .sessions
        .keys()
        .copied()
        .filter(|id| !state.windows.iter().any(|window| window.id == *id))
        .collect();
    for id in stale {
        if let Some(session) = browsers.sessions.remove(&id) {
            IoTaskPool::get()
                .spawn(async move {
                    drop(session);
                })
                .detach();
        }
    }
    let view = settings.as_ref().map_or(
        feathrise_editor_core::AssetBrowserViewMode::List,
        |settings| settings.asset_browser_view_mode,
    );
    for window in &mut state.windows {
        let session = browsers
            .sessions
            .entry(window.id)
            .or_insert_with(|| BrowserSession::new(view));
        session.worker.ensure(*operations);
        let revealing = session.reveal.as_ref().is_some_and(|(revision, _)| *revision == window.revision);
        if session.worker.take_changed() && !revealing {
            window.refresh();
        }
    }
    // 关闭/换代的任务也占原槽直到真正完成；丢句柄不能冒充取消磁盘 IO。
    for _ in 0..2_usize.saturating_sub(browsers.tasks.len()) {
        let Some(window) = state.windows.iter_mut().find(|window| {
            window.revision != window.scanned_revision
                && !browsers.tasks.iter().any(|(id, _)| *id == window.id)
        }) else {
            break;
        };
        let id = window.id;
        let revision = window.revision;
        window.scanned_revision = revision;
        let requested = window.requested_path.clone();
        let selection = window.selection.clone(); // 选择上限4096；完整交集留在后台，不在采用帧扫描大目录。
        let session = browsers.sessions.get_mut(&id).expect("已为每个窗口建立会话");
        if session.reveal.as_ref().is_some_and(|(ticket, _)| *ticket != revision) {
            session.reveal = None; // 用户已导航或修改筛选，旧定位不参与新快照。
        }
        let reveal = session.reveal.as_ref().map(|(_, path)| path.clone());
        session.worker.request_refresh();
        let worker = session.worker.clone();
        let filters = DirectoryFilters {
            kind: session.kind,
            filename: window.filter.clone(),
        };
        let context = state.context;
        let project = state.project.clone();
        let asset_root = browsers.asset_root.clone();
        browsers.tasks.push((
            id,
            IoTaskPool::get().spawn(async move {
                let result = directory(
                    &requested, project, asset_root, worker, context, filters, selection, reveal.as_deref(),
                );
                BrowserScan {
                    window: id,
                    revision,
                    context,
                    requested,
                    result,
                }
            }),
        ));
    }
}

pub(super) fn poll(
    mut state: ResMut<FileManagerState>,
    mut browsers: ResMut<BrowserSessions>,
    mut visits: Option<ResMut<Messages<AssetVisited>>>,
) {
    let mut index = 0;
    while index < browsers.tasks.len() {
        let Some(result) = check_ready(&mut browsers.tasks[index].1) else {
            index += 1;
            continue;
        };
        drop(browsers.tasks.swap_remove(index));
        if result.context != state.context
            || !state.active(result.window).is_some_and(|window| {
                window.revision == result.revision && window.requested_path == result.requested
            })
        {
            IoTaskPool::get()
                .spawn(async move {
                    drop(result);
                })
                .detach();
            continue;
        }
        let focused = state.focused == Some(result.window);
        let visible = !state.suspended;
        let window = state.active_mut(result.window).unwrap();
        window.loading = false;
        let session = browsers.sessions.get_mut(&result.window);
        match result.result {
            Ok(snapshot) => {
                let Some(session) = session else { continue; };
                session.reveal = None;
                // 只记录真正进入并成功采用的资产子目录，目录刷新与单文件定位不会生成重复目录访问。
                if visible && !window.minimized && (session.applied == 0 || window.path != result.requested)
                    && snapshot.reveal_index.is_none()
                    && let (Some(path), Some(visits)) = (snapshot.asset_folder, visits.as_mut())
                {
                    visits.write(AssetVisited { path, is_directory: true });
                }
                window.path = result.requested.clone();
                window.selection = snapshot.selection;
                window.first = snapshot.reveal_index.unwrap_or(0); // 表现层将起点夹在当前可见容量以内。
                window.anchor = snapshot.reveal_index;
                window.error = None;
                session.root = snapshot.root;
                session.assets = snapshot.assets;
                session.applied = result.revision;
                let old_files = std::mem::replace(&mut window.entries, snapshot.files);
                let old_entries = std::mem::replace(&mut session.entries, snapshot.entries);
                let old_animations =
                    std::mem::replace(&mut session.animations, snapshot.animations);
                if focused && !window.minimized && let Some(index) = snapshot.reveal_index {
                    browsers.revealed = Some((result.window, result.revision, index));
                }
                IoTaskPool::get()
                    .spawn(async move {
                        drop((old_files, old_entries, old_animations));
                    })
                    .detach();
                state.allow_directory(result.requested);
            }
            Err(error) => {
                if let Some(session) = session { session.reveal = None; }
                // 读取错误属于当前目录请求，即使没有浏览会话也必须报告；保留上一个完整目录和选择。
                window.error = Some(error);
            }
        }
    }
}

fn directory(
    path: &PathBuf,
    project: Option<PathBuf>,
    asset_root: Option<PathBuf>,
    worker: DirectoryWorker,
    context: u64,
    filters: DirectoryFilters,
    selection: BTreeSet<PathBuf>,
    reveal: Option<&str>,
) -> Result<BrowserSnapshot, String> {
    if !path.is_absolute() {
        return Err("目录地址必须为绝对路径".into());
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("无法打开 {}：{error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err("请直接打开真实目录，文件窗口不跟随目录符号链接".into());
    }
    let requested = path.canonicalize().map_err(|error| error.to_string())?;
    let asset_root = asset_root.and_then(|root| root.canonicalize().ok());
    let project = project.and_then(|root| root.canonicalize().ok());
    let assets = asset_root
        .as_ref()
        .is_some_and(|root| requested.starts_with(root));
    if reveal.is_some() && !assets {
        return Err("定位目标所在目录不属于当前工程资产根".into());
    }
    let root = if assets {
        asset_root.clone().unwrap()
    } else {
        project
            .filter(|root| requested.starts_with(root))
            .unwrap_or_else(|| requested.clone())
    };
    let folder = requested
        .strip_prefix(&root)
        .map_err(|error| error.to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    let snapshot = worker.scan(
        root.clone(),
        folder.clone(),
        DirectoryQuery {
            context,
            scope: if assets {
                FolderScope::Assets
            } else {
                FolderScope::ProjectFiles
            },
            asset_root: asset_root.as_deref(),
            filters: &filters,
            reveal_file: reveal,
        },
    )?;
    // 完整排序的行号只在 IO 线程寻找一次。查不到时不发布新目录或假选择。
    let reveal_index = reveal.map(|path| {
        snapshot.entries.iter().position(|entry| !entry.is_directory && entry.asset_path == path)
            .ok_or_else(|| format!("资产定位失败：找不到 assets/{path}"))
    }).transpose()?;
    let files: Arc<[FileEntry]> = snapshot
        .entries
        .iter()
        .map(|entry| {
            let full = crate::ui::external_files::platform_path(&root.join(&entry.asset_path));
            let bytes = std::fs::metadata(&full).map_or(0, |metadata| metadata.len());
            FileEntry {
                path: full,
                name: entry.display_name.clone(),
                directory: entry.is_directory,
                bytes,
            }
        })
        .collect::<Vec<_>>()
        .into();
    let selection = if let Some(index) = reveal_index {
        BTreeSet::from([files[index].path.clone()])
    } else {
        let existing: std::collections::HashSet<_> = files.iter().map(|entry| &entry.path).collect();
        selection.into_iter().map(|path| crate::ui::external_files::platform_path(&path))
            .filter(|path| existing.contains(path)).collect()
    };
    Ok(BrowserSnapshot {
        root,
        assets,
        asset_folder: (assets && !folder.is_empty()).then_some(folder),
        files,
        entries: snapshot.entries.into(),
        animations: snapshot.animations,
        selection,
        reveal_index,
    })
}
