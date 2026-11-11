//! 混合场景/文件夹整批移动：后台去重与完整校验，通过身份门后复用唯一受检写入链。

use std::{collections::BTreeSet, sync::Arc};

use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool, futures::check_ready},
    window::PrimaryWindow,
};
use feathrise_scene_content::SceneContentRegistry;

use crate::{
    authoring::workspace_asset_root,
    constants::{SCENE_FOLDER_ASSET_PATH, SCENE_FOLDER_MAX_DEPTH},
    resources::{SceneBrowserIndexState, SceneBrowserState, SceneEditorState, SceneFolderPersistence},
    types::{
        SceneBrowserDropTarget, SceneBrowserEntry, SceneBrowserIndexSignature, SceneBrowserItem,
        SceneFolderBatchMove, SceneFolderBatchPhase, SceneFolderDocument, SceneFolderFileSnapshot,
        SceneFolderWriteFailure,
    },
};

use super::{
    helpers::{
        decode_folder_document, encode_folder_document, expand_folder_ancestors, folder_by_id,
    },
    persistence::write_snapshot_if_unchanged,
};

/// 悬停只检查目标祖先链，不逐帧扫描选集；同名、深度和两源互撞在后台完整校验。
pub(super) fn target_allowed(
    document: &SceneFolderDocument,
    selected: &BTreeSet<SceneBrowserItem>,
    target: &SceneBrowserDropTarget,
) -> Result<(), String> {
    let mut parent = match target {
        SceneBrowserDropTarget::Folder(id) => Some(id.as_str()),
        SceneBrowserDropTarget::Root => None,
    };
    for _ in 0..=SCENE_FOLDER_MAX_DEPTH {
        let Some(id) = parent else {
            return Ok(());
        };
        if selected.contains(&SceneBrowserItem::Folder(id.to_owned())) {
            return Err("不能把所选文件夹移入自身或子文件夹".into());
        }
        parent = folder_by_id(document, id)
            .ok_or("目标文件夹已不存在")?
            .parent_id
            .as_deref();
    }
    Err("目标文件夹层级超过上限".into())
}

/// 在完整深度优先树上去掉已选父目录覆盖的后代，再一次修改并验收整份文档。
pub(super) fn prepare_batch(
    mut document: SceneFolderDocument,
    entries: &[SceneBrowserEntry],
    selected: &BTreeSet<SceneBrowserItem>,
    target: &SceneBrowserDropTarget,
) -> Result<Option<SceneFolderFileSnapshot>, String> {
    if selected.is_empty() || selected.contains(&SceneBrowserItem::Root) {
        return Err("请先选择可移动的场景或文件夹".into());
    }
    target_allowed(&document, selected, target)?;
    let parent = match target {
        SceneBrowserDropTarget::Folder(id) => Some(id.as_str()),
        SceneBrowserDropTarget::Root => None,
    };
    let mut covered_depth = None;
    let mut found = BTreeSet::new();
    let mut roots = Vec::new();
    for entry in entries {
        if covered_depth.is_some_and(|depth| entry.depth <= depth) {
            covered_depth = None;
        }
        if !selected.contains(&entry.item) {
            continue;
        }
        found.insert(entry.item.clone());
        if covered_depth.is_none() {
            roots.push(&entry.item);
            if matches!(entry.item, SceneBrowserItem::Folder(_)) {
                covered_depth = Some(entry.depth);
            }
        }
    }
    if found.len() != selected.len() {
        return Err("部分所选场景或文件夹已不在当前目录中，请重新选择".into());
    }
    let mut changed = false;
    for item in roots {
        match item {
            SceneBrowserItem::Folder(id) => {
                let folder = document
                    .folders
                    .iter_mut()
                    .find(|folder| &folder.id == id)
                    .ok_or("待移动的文件夹已不存在")?;
                if folder.parent_id.as_deref() != parent {
                    folder.parent_id = parent.map(str::to_owned);
                    changed = true;
                }
            }
            SceneBrowserItem::Scene(id) => {
                if document.scene_folders.get(id.as_str()).map(String::as_str) != parent {
                    if let Some(parent) = parent {
                        document
                            .scene_folders
                            .insert(id.as_str().to_owned(), parent.to_owned());
                    } else {
                        document.scene_folders.remove(id.as_str());
                    }
                    changed = true;
                }
            }
            SceneBrowserItem::Root => unreachable!(),
        }
    }
    if !changed {
        return Ok(None);
    }
    // 统一最终校验能发现来自不同父级的同名目录，任何失败都不会发布部分修改。
    let source_bytes = encode_folder_document(&document)?;
    Ok(Some(SceneFolderFileSnapshot {
        document,
        source_bytes,
    }))
}

pub(super) fn begin_batch(
    browser: &SceneBrowserState,
    index: &SceneBrowserIndexState,
    persistence: &mut SceneFolderPersistence,
    selected: Arc<BTreeSet<SceneBrowserItem>>,
    target: SceneBrowserDropTarget,
    catalog_revision: u64,
) -> Result<(), String> {
    let source = SceneBrowserIndexSignature {
        folder_revision: browser.document_revision,
        catalog_revision,
    };
    if persistence.busy() || index.applied_signature.as_ref() != Some(&source) {
        return Err("场景目录正在整理或保存，请等待完成后重新拖动".into());
    }
    if let Some(error) = browser.write_blocked.as_deref() {
        return Err(error.to_owned());
    }
    let expected_bytes = browser
        .source_bytes
        .as_ref()
        .map(|bytes| Arc::new(bytes.clone())); // 沿用256KiB源字节预算，只冻结一份后浅共享给准备任务。
    let bytes = expected_bytes.clone();
    let entries = Arc::clone(&index.full_entries);
    let destination = target.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let document = bytes
            .as_deref()
            .map(|bytes| decode_folder_document(bytes))
            .transpose()?
            .unwrap_or_default();
        prepare_batch(document, &entries, &selected, &destination)
    });
    persistence.ticket = persistence.ticket.wrapping_add(1);
    persistence.batch = Some(SceneFolderBatchMove {
        ticket: persistence.ticket,
        source,
        target,
        expected_bytes,
        asset_root: workspace_asset_root(),
        cancelled: false,
        phase: SceneFolderBatchPhase::Preparing(task),
    });
    Ok(())
}

/// 准备和IO串行共用一个事务；主线程只非阻塞轮询并原子采用完整结果。
pub(crate) fn poll_scene_folder_batch(
    mut persistence: ResMut<SceneFolderPersistence>,
    mut browser: ResMut<SceneBrowserState>,
    mut editor: ResMut<SceneEditorState>,
    registry: Option<Res<SceneContentRegistry>>,
    asset_server: Res<AssetServer>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut context: crate::ui::scene_sidebar::SceneSidebarInputContext,
    blockers: (
        Option<Res<crate::resources::SceneEditorSettingsState>>,
        Option<Res<crate::authoring::TerrainExternalChangeState>>,
        Option<Res<crate::resources::SceneEditorNumericScrubMouseGuard>>,
        Option<Res<feathrise_editor_ui::ActiveNumericInputScrub>>,
    ),
    drag: Option<Res<crate::resources::SceneBrowserDragState>>,
) {
    let Some(mut batch) = persistence.batch.take() else {
        return;
    };
    let source_current = batch.ticket == persistence.ticket
        && batch.source.folder_revision == browser.document_revision
        && batch.source.catalog_revision
            == registry.as_ref().map_or(0, |registry| registry.revision);
    match &mut batch.phase {
        SceneFolderBatchPhase::Preparing(task) => {
            let mouse_blocked = blockers.2.as_ref().is_some_and(|guard| guard.blocked());
            let own_release = drag.as_ref().is_some_and(|drag| {
                drag.blocks_other_mouse() && !drag.active && drag.press.is_none()
            }) && !blockers
                .3
                .as_ref()
                .is_some_and(|scrub| scrub.entity.is_some());
            batch.cancelled |= !source_current
                || !editor.open
                || editor.close_requested
                || !context.visible()
                || browser.dialog.is_some()
                || editor.new_scene_dialog_open
                || editor.scene_transition_in_progress
                || editor.pending_scene_transition.is_some()
                || editor.scene_switch_confirmation.is_some()
                || blockers
                    .0
                    .as_ref()
                    .is_some_and(|settings| settings.open || settings.owns_mouse_this_frame)
                || blockers
                    .1
                    .as_ref()
                    .is_some_and(|conflict| conflict.modal_open())
                || (mouse_blocked && !own_release)
                || windows.single().map_or(true, |window| !window.focused);
            if mouse_blocked && own_release && !batch.cancelled {
                persistence.batch = Some(batch);
                return; // 共享门包含本次拖拽自己的release/和弦保护；等静默帧，不能把合法释放误判为外部冲突。
            }
            let Some(result) = check_ready(task) else {
                persistence.batch = Some(batch); // 取消也要等原任务真实结束，禁止后台槽堆积。
                return;
            };
            if batch.cancelled {
                editor.status = "目录上下文已变化，已取消批量整理".into();
            } else {
                match result {
                    Ok(Some(snapshot)) => {
                        let root = batch.asset_root.clone();
                        let bytes = batch.expected_bytes.take();
                        batch.phase =
                            SceneFolderBatchPhase::Saving(IoTaskPool::get().spawn(async move {
                                write_snapshot_if_unchanged(
                                    &root,
                                    bytes.map(|bytes| (*bytes).clone()),
                                    snapshot,
                                )
                            }));
                        persistence.batch = Some(batch);
                        editor.status = "正在保存整批场景归类…".into();
                        return;
                    }
                    Ok(None) => editor.status = "所选对象已经属于该虚拟目录".into(),
                    Err(error) => editor.status = format!("批量整理已取消：{error}"),
                }
            }
        }
        SceneFolderBatchPhase::Saving(task) => {
            let Some(result) = check_ready(task) else {
                persistence.batch = Some(batch);
                return;
            };
            if !source_current {
                browser.write_blocked = Some("批量保存结果已过期，正在重新加载".into());
                editor.status = browser.write_blocked.clone().unwrap();
                asset_server.reload(SCENE_FOLDER_ASSET_PATH);
            } else {
                match result {
                    Ok(snapshot) => {
                        browser.document = snapshot.document;
                        browser.source_bytes = Some(snapshot.source_bytes);
                        browser.initial_load_resolved = true;
                        browser.document_revision = browser.document_revision.wrapping_add(1);
                        let parent = match &batch.target {
                            SceneBrowserDropTarget::Folder(id) => Some(id.as_str()),
                            SceneBrowserDropTarget::Root => None,
                        };
                        let SceneBrowserState {
                            document,
                            expanded_folder_ids,
                            ..
                        } = &mut *browser;
                        if expand_folder_ancestors(document, parent, expanded_folder_ids) {
                            browser.expansion_revision = browser.expansion_revision.wrapping_add(1);
                        }
                        browser.last_error = None;
                        editor.status = "场景归类已整批保存".into();
                        asset_server.reload(SCENE_FOLDER_ASSET_PATH);
                    }
                    Err(error) => {
                        if matches!(error, SceneFolderWriteFailure::ConcurrentChange(_)) {
                            browser.write_blocked = Some("归类文件被外部修改，正在重新加载".into());
                            asset_server.reload(SCENE_FOLDER_ASSET_PATH);
                        }
                        editor.status = format!("批量整理保存失败：{error}");
                        browser.last_error = Some(editor.status.clone());
                    }
                }
            }
        }
    }
    if let Some(feedback) = context.feedback.as_deref_mut() {
        feedback.record(&editor, &browser, registry.as_deref(), false);
    }
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
