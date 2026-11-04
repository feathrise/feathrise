//! 非阻塞轮询及最多8份草稿原子采用；完整预检之后才改正文，失败不产生半批次。

use super::super::super::{
    constants::MAX_DOCUMENTS,
    history::EditContext,
    resources::DocumentState,
    types::{Document, ViewMode},
};
use super::super::{
    control::{SearchGate, busy},
    resources::AssetSearchState,
    types::DraftStamp,
};
use super::{resources::AssetReplaceState, types::*, worker};
use crate::resources::MapEditorState;
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
    text::EditableText,
};
use std::{collections::HashSet, sync::Arc};

pub(super) fn plan_current(plan: &ReplacePlan, documents: &DocumentState) -> bool {
    !plan.search.references && plan.root == documents.root
        && plan.files.iter().all(|file| {
            let path = &plan.search.files[file.index].path;
            match (
                &file.draft,
                documents.documents.iter().find(|doc| doc.path == *path),
            ) {
                (Some(stamp), Some(doc)) => {
                    doc.path == stamp.key.path
                        && doc.generation == stamp.key.generation
                        && doc.revision == stamp.revision
                        && doc.disk == file.disk
                }
                (None, None) => true,
                _ => false,
            }
        })
}

pub(super) fn capacity(
    plan: &ReplacePlan,
    content: &[AppliedFile],
    documents: &DocumentState,
) -> Result<(), String> {
    // 尚未读完的中央标签也占槽；容量不足不淘汰干净文档，不部分载入或应用。
    let mut paths: HashSet<&str> = documents
        .documents
        .iter()
        .map(|doc| doc.path.as_str())
        .collect();
    paths.extend(documents.tabs().map(|tab| tab.path.as_str()));
    paths.extend(
        content
            .iter()
            .map(|entry| plan.search.files[entry.index].path.as_str()),
    );
    if paths.len() > MAX_DOCUMENTS {
        Err("8份文档会话容量不足；请先保存并关闭其他文件，再更新预览，所有草稿保持".into())
    } else {
        Ok(())
    }
}

pub(super) fn adopt(
    plan: &ReplacePlan,
    content: Vec<AppliedFile>,
    documents: &mut DocumentState,
) -> Result<Option<BatchHistory>, (String, Vec<AppliedFile>)> {
    let error =
        if documents.io.is_some() || documents.pending_load.is_some() || documents.saving.is_some()
        {
            Some("文件任务尚未结束，请稍后更新预览；所有草稿保持".to_owned())
        } else if !plan_current(plan, documents) {
            Some("文档在预览后已变化，请更新预览；所有草稿保持".to_owned())
        } else {
            capacity(plan, &content, documents).err()
        };
    if let Some(error) = error {
        return Err((error, content));
    }
    if content.is_empty() {
        return Ok(None);
    }
    let mut stamps = Vec::new();
    for entry in content {
        let file = plan
            .files
            .iter()
            .find(|file| file.index == entry.index)
            .expect("后台只返回已预检文件");
        let path = &plan.search.files[entry.index].path;
        let index =
            if let Some(index) = documents.documents.iter().position(|doc| doc.path == *path) {
                index
            } else {
                documents.load_ticket = documents.load_ticket.wrapping_add(1).max(1);
                documents.documents.push(Document {
                    path: path.clone(),
                    format: crate::asset_catalog::document_format(path).expect("共享文本白名单"),
                    generation: documents.load_ticket,
                    disk: file.disk.clone(),
                    pages: file.pages.clone(),
                    source: file.source.clone(),
                    revision: 0,
                    saved_revision: 0,
                    history: default(),
                    preview: Arc::new(Vec::new()),
                    preview_revision: None,
                    preview_clipped: false,
                    mode: ViewMode::Edit,
                    preview_scroll: Vec2::ZERO,
                    confirm_reload: false,
                    external: default(),
                    status: String::new(),
                });
                documents.documents.len() - 1
            };
        let doc = &mut documents.documents[index];
        let count = entry.content.count;
        doc.replace_content(
            entry.content.source,
            entry.content.pages,
            EditContext {
                before: file.selection,
                after: entry.content.selection,
                ..default()
            },
        );
        doc.status = if plan.search.limited {
            format!("跨文件替换已列出的{count}处，其余保持；草稿未保存，可在原文档撤销")
        } else {
            format!("跨文件替换{count}处；草稿未保存，可在原文档撤销")
        };
        stamps.push(DraftStamp {
            key: doc.source_key(),
            revision: doc.revision,
        });
    }
    Ok(Some(BatchHistory {
        stamps,
        undone: false,
    }))
}

pub(super) fn travel(
    batch: &mut BatchHistory,
    documents: &mut DocumentState,
    redo: bool,
) -> Result<Vec<super::super::super::history::Selection>, String> {
    if batch.undone != redo || documents.io.is_some() || documents.pending_load.is_some() {
        return Err("当前批次没有可执行的历史，或文件任务尚未结束".into());
    }
    let valid = batch.stamps.iter().all(|stamp| {
        documents.documents.iter().any(|doc| {
            doc.path == stamp.key.path
                && doc.generation == stamp.key.generation
                && doc.revision == stamp.revision
                && if redo {
                    doc.history.can_redo()
                } else {
                    doc.history.can_undo()
                }
        })
    });
    if !valid {
        return Err(
            "这批文件已有后续编辑或重新载入，整批历史已停止；可在各文件使用原撤销/重做".into(),
        );
    }
    let mut selections = Vec::new();
    for stamp in &mut batch.stamps {
        let doc = documents
            .documents
            .iter_mut()
            .find(|doc| doc.path == stamp.key.path && doc.generation == stamp.key.generation)
            .unwrap();
        selections.push(doc.travel(redo).unwrap());
        stamp.revision = doc.revision;
    }
    batch.undone = !redo;
    Ok(selections)
}

fn drafts_valid(drafts: &[DraftInput], documents: &DocumentState) -> bool {
    drafts.iter().all(|draft| {
        documents.documents.iter().any(|doc| {
            doc.path == draft.stamp.key.path
                && doc.generation == draft.stamp.key.generation
                && doc.revision == draft.stamp.revision
                && doc.disk == draft.disk
        })
    })
}

pub(super) fn drive(
    editor: Res<MapEditorState>,
    gate: SearchGate,
    search: Res<AssetSearchState>,
    mut documents: ResMut<DocumentState>,
    mut state: ResMut<AssetReplaceState>,
    inputs: Query<&EditableText>,
) {
    let blocked = search.reference_target.is_some() || gate.blocked(&editor) || inputs.iter().any(busy) || !state.preview_visible();
    let stale = state
        .pending
        .as_ref()
        .or(state.running.as_ref())
        .filter(|request| request.ticket == state.ticket)
        .is_some_and(|request| match &request.work {
            Work::Preview {
                search: result,
                drafts,
                ..
            } => {
                result.ticket != search.ticket
                    || result.root != documents.root
                    || !drafts_valid(drafts, &documents)
            }
            Work::Apply { plan, .. } => {
                plan.search_ticket != search.ticket || !plan_current(plan, &documents)
            }
        })
        || (!state.applied
            && state.current().is_some_and(|plan| {
                plan.search_ticket != search.ticket || !plan_current(plan, &documents)
            }));
    if (blocked || stale)
        && (state.pending.is_some()
            || state.current().is_some() && !state.applied
            || state
                .running
                .as_ref()
                .is_some_and(|request| request.ticket == state.ticket))
    {
        state.cancel(if stale {
            "搜索或草稿已变化，请重新搜索并更新预览；草稿保持"
        } else {
            "替换已取消，请更新预览；草稿保持"
        });
    }
    if let Some(reply) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        state.retired_request = state.running.take();
        if reply.ticket != state.ticket || blocked || stale {
            state.retired = Some(reply.outcome);
        } else {
            match reply.outcome {
                Outcome::Preview(Ok(plan)) => {
                    state.hint = format!(
                        "已预览{}个文件{}；选择匹配后可替换单处，应用只改草稿",
                        plan.files.len(),
                        if plan.search.limited {
                            "，仅处理已列出匹配，其余保持"
                        } else {
                            ""
                        }
                    );
                    state.plan = Some(plan);
                    state.reset_diff = true;
                }
                Outcome::Preview(Err(error)) => state.hint = error,
                Outcome::Apply {
                    plan,
                    content: Ok(content),
                } => {
                    let hits: usize = content.iter().map(|file| file.content.count).sum();
                    let selections: Vec<_> =
                        content.iter().map(|file| file.content.selection).collect();
                    match adopt(&plan, content, &mut documents) {
                        Ok(batch) => {
                            state.applied = true;
                            state.hint = if let Some(batch) = batch {
                                let count = batch.stamps.len();
                                state.restores =
                                    batch.stamps.iter().cloned().zip(selections).collect();
                                state.batch = Some(batch);
                                format!(
                                    "已替换{count}个文件、{hits}处{}；可保存本批、整批撤销或打开文件继续编辑",
                                    if plan.search.limited {
                                        "，仅处理已列出匹配，其余保持"
                                    } else {
                                        ""
                                    }
                                )
                            } else {
                                "内容已相同；未改动草稿或历史".into()
                            };
                        }
                        Err((error, content)) => {
                            state.hint = error;
                            state.retired = Some(Outcome::Apply {
                                plan,
                                content: Ok(content),
                            });
                        }
                    }
                }
                Outcome::Apply {
                    content: Err(error),
                    ..
                } => state.hint = error,
            }
        }
    }
    if state.task.is_none()
        && !blocked
        && let Some(request) = state.pending.take()
    {
        let retired = state.retired.take();
        let old_request = state.retired_request.take();
        let old_plan = if matches!(&request.work, Work::Preview { .. }) {
            state.plan.take()
        } else {
            None
        };
        state.running = Some(request.clone());
        state.task =
            Some(IoTaskPool::get().spawn(worker::run(request, retired, old_plan, old_request)));
    }
}
