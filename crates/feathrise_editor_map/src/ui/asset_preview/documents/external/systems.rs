//! 外部内容检查复用加载/保存的单槽；结果采用须匹配根、文件代次、磁盘基线和内容修订。

use super::{resources::ExternalCheckState, types::*};
use super::super::{io, resources::DocumentState, types::IoResult};
use crate::resources::MapEditorState;
use bevy::{prelude::*, tasks::IoTaskPool, window::PrimaryWindow};

pub(in super::super) fn request_check(
    editor: Res<MapEditorState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut state: ResMut<DocumentState>,
) {
    let open = editor.open && !editor.close_requested;
    let focused = windows.single().is_ok_and(|window| window.focused);
    let previous = &state.external;
    if (previous.was_open, previous.was_focused) == (open, focused) {
        return; // 稳定帧不标记资源变化，不扫描磁盘或重绑全文。
    }
    let requested = open && focused && (!previous.was_open || !previous.was_focused);
    let check: &mut ExternalCheckState = &mut state.external;
    check.available = open && focused;
    check.was_open = open;
    check.was_focused = focused;
    check.requested |= requested; // 失焦期间多次打开/返回只合并为一个意图。
}

pub(in super::super) fn drive(state: &mut DocumentState, input_busy: bool) {
    if !state.external.available || state.io.is_some() || super::super::json::save_busy(state) {
        return;
    }
    if !input_busy && let Some(reply) = state.external.ready.take() {
        adopt(state, reply);
    }
    if !state.external.requested || state.external.ready.is_some() || state.io.is_some()
        || state.pending_load.is_some() || super::super::json::save_busy(state)
    {
        return; // 加载、显式保存和 JSON 保存前校验优先；旧任务始终占槽到完成。
    }
    state.external.requested = false;
    let snapshots: Vec<_> = state.documents.iter().map(|doc| CheckSnapshot {
        key: doc.source_key(), revision: doc.revision, disk: doc.disk.clone(),
    }).collect(); // 会话上限8份；只复制短身份，不在帧内读取或复制全文。
    if snapshots.is_empty() {
        return;
    }
    let roots = state.source_roots();
    state.io = Some(IoTaskPool::get().spawn(async move {
        IoResult::Checked(check_sources(roots, snapshots))
    }));
}

#[cfg(test)]
pub(super) fn check(root: std::path::PathBuf, snapshots: Vec<CheckSnapshot>) -> CheckReply {
    check_sources(super::super::source::SourceRoots { assets: root, project: None }, snapshots)
}

fn check_sources(roots: super::super::source::SourceRoots, snapshots: Vec<CheckSnapshot>) -> CheckReply {
    let documents = snapshots.into_iter().map(|snapshot| {
        let result = io::load_source(&roots, &snapshot.key.path).and_then(|(disk, contents)| {
            if disk.path != snapshot.disk.path {
                Err("文件链接目标已变化，内置内容已保留；请确认后重新载入".into())
            } else {
                Ok((disk, contents))
            }
        });
        CheckedDocument { snapshot, result }
    }).collect(); // 每份沿用256 KiB上限，目录外链接、删除和无效编码都返回错误。
    CheckReply { root: roots.assets, documents }
}

pub(in super::super) fn adopt(state: &mut DocumentState, reply: CheckReply) {
    if reply.root != state.root {
        return;
    }
    for checked in reply.documents {
        let snapshot = checked.snapshot;
        let Some(index) = state.documents.iter().position(|doc| doc.source_key() == snapshot.key
            && doc.disk == snapshot.disk) else { continue; };
        if state.documents[index].revision != snapshot.revision {
            state.external.requested = true;
            continue; // 检查期间编辑、撤销或保存不能被旧结果覆盖；稍后核对新快照。
        }
        match checked.result {
            Ok((disk, contents)) if disk == snapshot.disk => {
                let doc = &mut state.documents[index];
                if doc.external.observed.is_some() {
                    doc.external = default();
                    doc.status = "磁盘内容已与载入版本一致".into();
                }
                drop(contents);
            }
            Ok((disk, contents)) if !state.documents[index].dirty() => {
                state.load_ticket = state.load_ticket.max(state.documents.iter().map(|doc| doc.generation).max().unwrap_or(0)).wrapping_add(1);
                // IO核对期间可能又选择了待加载文件；更新其票据保留最新显式加载意图。
                if let Some(request) = &mut state.pending_load {
                    request.ticket = state.load_ticket;
                }
                let doc = &mut state.documents[index];
                doc.generation = state.load_ticket;
                doc.disk = disk;
                doc.pages = contents.pages;
                doc.source = contents.text;
                doc.revision = 0;
                doc.saved_revision = 0;
                doc.history = default();
                doc.preview = std::sync::Arc::new(Vec::new());
                doc.preview_revision = None;
                doc.preview_clipped = false;
                doc.confirm_reload = false;
                doc.external = default();
                doc.status = "已自动更新外部修改 · 当前文件没有未保存草稿".into();
                // 模式与预览视区继续保留；新 SourceKey 使旧原生输入与后台派生结果失效。
            }
            result => {
                let (identity, status) = match result {
                    Ok((disk, _)) => (ExternalIdentity::Changed(disk),
                        "磁盘文件已被外部修改；内置草稿已保留，请选择保留草稿或重新载入".into()),
                    Err(error) => (ExternalIdentity::Unavailable(error.clone()), error),
                };
                let doc = &mut state.documents[index];
                if doc.external.observed.as_ref() != Some(&identity) {
                    doc.external.observed = Some(identity);
                    doc.external.warning = true;
                    doc.status = status;
                } // “保留草稿”只抑制同一外部修订；新的外部修改仍会再次提示。
            }
        }
    }
}
