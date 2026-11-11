//! 主线程只轮询一个后台任务；载入重放、保存回执与路径检查均按完整身份采用。

use super::{helpers, resources::AssetShortcuts, types::*, worker};
use crate::resources::SceneEditorState;
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
};

pub(super) fn adopt(state: &mut AssetShortcuts, completed: Completion) {
    if completed.root != state.root {
        return;
    } // 其他项目或旧夹具结果不能采用。
    match completed.result {
        Ok(mut snapshot) => {
            if completed.kind == JobKind::Load {
                let original = snapshot.clone(); // 最多84条，不复制完整资产目录。
                let mut error = None;
                for change in std::mem::take(&mut state.pending) {
                    if let Err(message) = helpers::apply(&mut snapshot, &change) {
                        error = Some(message);
                    }
                }
                state.loaded = true;
                state.save_blocked = false;
                state.revision = state.revision.wrapping_add(1);
                state.saved_revision = if snapshot == original {
                    state.revision
                } else {
                    completed.revision
                };
                state.snapshot = snapshot;
                state.feedback = error.unwrap_or_default();
            } else {
                if completed.kind == JobKind::Save {
                    state.saved_revision = completed.revision;
                }
                if completed.revision == state.revision {
                    state.snapshot = snapshot; // 新点击之后的旧检查不能覆盖收藏或最近访问顺序。
                    if !state.save_blocked {
                        state.feedback.clear();
                    } // 路径检查成功不能抹掉原记录损坏、未保存的提示。
                    state.failed_revision = None;
                }
            }
        }
        Err(error) => {
            if completed.kind == JobKind::Load {
                state.loaded = true;
                state.save_blocked = true;
                state.pending.clear();
                state.feedback = format!("记录加载失败，原文件已保留；本次改动暂未保存：{error}");
            } else if completed.revision == state.revision {
                state.failed_revision = Some(completed.revision);
                state.feedback = format!(
                    "导航记录未完成{}：{error}",
                    if completed.kind == JobKind::Save {
                        "保存"
                    } else {
                        "检查"
                    }
                );
            }
        }
    }
}

pub(super) fn drive(editor: Res<SceneEditorState>, mut state: ResMut<AssetShortcuts>) {
    let open = editor.open && !editor.close_requested;
    if open && !state.was_open && state.loaded {
        state.check_requested = true;
    }
    if open != state.was_open {
        state.was_open = open;
    }
    if state.task.is_some() {
        let task = state.task.as_mut().expect("唯一在途槽");
        let Some(completed) = check_ready(task) else {
            return;
        }; // 真正完成前不丢弃句柄或启动第二次 IO。
        state.task = None;
        adopt(&mut state, completed);
    }
    let kind = if !state.loaded {
        if !open {
            return;
        }
        JobKind::Load
    } else if !state.save_blocked
        && state.revision != state.saved_revision
        && state.failed_revision != Some(state.revision)
    {
        JobKind::Save // 即使F12已经关闭，也收口已授权的个人导航记录。
    } else if state.check_requested {
        JobKind::Check
    } else {
        return;
    };
    state.check_requested = false;
    let job = Job {
        root: state.root.clone(),
        revision: state.revision,
        kind,
        snapshot: if kind == JobKind::Load {
            Snapshot::default()
        } else {
            state.snapshot.clone()
        },
    };
    state.task = Some(IoTaskPool::get().spawn(async move { worker::run(job) }));
}
