//! 单槽后台加载、编辑、保存及身份检查；后台计算结束前继续显示上一完整姿态。

use super::super::{AssetPreviewWorkspace, DocumentState, types::AssetPreviewState};
use super::{context::AnimationContext, data::AnimationDocument, resources::*};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool, futures::check_ready},
};
use std::sync::Arc;

pub(super) fn drive(
    context: AnimationContext,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    documents: Res<DocumentState>,
    time: Res<Time<Real>>,
    mut state: ResMut<AnimationEditorState>,
) {
    let desired = preview
        .selected_path
        .as_ref()
        .filter(|path| {
            std::path::Path::new(path).extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("glb") || ext.eq_ignore_ascii_case("gltf")
            }) && preview.external_preview.is_none()
        })
        .map(|path| documents.asset_root().join(path));
    if state.active != desired {
        state.active = desired;
        state.playing = false;
        state.drag = None;
        state.gesture = None;
        state.pending = None;
        state.failed = None;
        state.status.clear();
        state.model_status.clear();
        state.changed();
        state.source_revision = state.source_revision.wrapping_add(1).max(1);
        let stamp = state.revision;
        if let Some(session) = state.current_mut() {
            session.last_used = stamp;
            if !session.dirty() {
                session.source_changed = true;
            }
        }
    }
    let generation = state
        .active
        .clone()
        .zip(preview.model.as_ref().map(|model| model.generation));
    if state.model_generation != generation {
        if let Some((path, _)) = &generation {
            // 相同路径的热更新不能覆盖脏草稿；干净会话稍后沿唯一IO槽重新受检读取。
            if state
                .model_generation
                .as_ref()
                .is_some_and(|(old, _)| old == path)
                && let Some(session) = state.sessions.get_mut(path)
            {
                session.source_changed = true;
            }
        }
        state.model_generation = generation;
    }
    state.visible = state.active.is_some() && context.available(&preview, &media);
    if !state.visible {
        state.playing = false;
        state.drag = None;
        state.gesture = None;
    }

    // 不丢弃旧任务句柄：即使切换文件，旧工作真正结束前仍占唯一并发槽。
    let completed = state
        .work
        .as_mut()
        .and_then(|work| check_ready(&mut work.task));
    if let Some(completed) = completed {
        let work = state.work.take().unwrap();
        let changes_view = !matches!(&completed, TaskResult::Released);
        let is_current = state.active.as_ref() == Some(&work.path);
        let current_generation = state
            .model_generation
            .as_ref()
            .filter(|(path, _)| path == &work.path)
            .map(|(_, generation)| *generation);
        let stale_source = is_current && current_generation != work.generation;
        // 同一路径第二次热更新可先于上一轮后台结果到达；保留新标记并沿唯一槽重验最新代。
        // 切到其他文件时仍按原 path 更新其停放会话，不能把正常后台完成误当成串源。
        let completed =
            if stale_source && matches!(&completed, TaskResult::Unchanged | TaskResult::Load(_)) {
                if let Some(session) = state.sessions.get_mut(&work.path) {
                    session.source_changed = true;
                }
                state.failed = None;
                state.status = "动画来源再次更新，正在读取最新版本".to_owned();
                debug_assert!(state.retired.is_none());
                state.retired = Some(Retired::Result(completed));
                None
            } else {
                Some(completed)
            };
        if let Some(completed) = completed {
            match completed {
                TaskResult::Copied(result) => match result {
                    Ok(clipboard) => {
                        if let Some(old) = state.clipboard.replace(clipboard) {
                            state.retired = Some(Retired::Clipboard(old));
                        }
                        state.status = "已复制选中关键帧；移动播放头后粘贴".into();
                    }
                    Err(error) => state.status = error,
                },
                TaskResult::Released => {}
                TaskResult::Unchanged => {
                    if let Some(session) = state.sessions.get_mut(&work.path) {
                        session.source_changed = false;
                    }
                    if is_current {
                        state.status = "磁盘动作未变化，已保留原草稿与历史".to_owned();
                    }
                }
                TaskResult::Load(result) => match result {
                    Ok(mut session) => {
                        session.last_used = state.revision;
                        if let Some(previous) = state.sessions.insert(work.path, session) {
                            debug_assert!(state.retired.is_none());
                            state.retired = Some(Retired::Session(previous));
                        }
                        if is_current {
                            state.status = "已载入动画；关键帧修改先保留为草稿".to_owned();
                            state.source_revision = state.source_revision.wrapping_add(1).max(1);
                        }
                    }
                    Err(error) => {
                        if is_current {
                            state.failed = Some(work.path);
                            state.status = error;
                        }
                    }
                },
                TaskResult::Edit {
                    document,
                    action,
                    key,
                    target,
                    selection,
                } => match document {
                    Ok((document, metadata)) => {
                        let document = Arc::new(document);
                        let retired = if let Some(session) = state.sessions.get_mut(&work.path) {
                            let previous = std::mem::replace(&mut session.document, document);
                            let previous_metadata =
                                std::mem::replace(&mut session.metadata, metadata);
                            let mut retired_history = Vec::new();
                            match action {
                                PendingAction::Edit(edit) => {
                                    if session
                                        .saved_cursor
                                        .is_some_and(|saved| saved > session.cursor)
                                    {
                                        session.saved_cursor = None;
                                    }
                                    retired_history = session.history.split_off(session.cursor);
                                    session.history.push(edit);
                                    session.cursor += 1;
                                }
                                PendingAction::Undo => session.cursor -= 1,
                                PendingAction::Redo => session.cursor += 1,
                                PendingAction::Save
                                | PendingAction::Reload
                                | PendingAction::Command(_) => unreachable!(),
                            }
                            session.clip = target.0;
                            session.channel = Some(target.1);
                            session.page = target.1 / super::components::CHANNEL_SLOTS;
                            session.key = key;
                            session.pose = None;
                            session.selected = selection.unwrap_or_default();
                            session.selected_node = session.document.clips[target.0]
                                .channels
                                .get(target.1)
                                .map(|c| c.node);
                            if session.bone_filter {
                                session.page = 0;
                            }
                            session.position = key
                                .and_then(|index| {
                                    session.document.clips[target.0].channels[target.1]
                                        .keys
                                        .get(index)
                                })
                                .map_or(
                                    session
                                        .position
                                        .min(session.metadata.durations[session.clip]),
                                    |key| key.time,
                                );
                            Retired::Document {
                                document: previous,
                                metadata: Some(previous_metadata),
                                history: retired_history,
                            }
                        } else {
                            Retired::Document {
                                document,
                                metadata: Some(metadata),
                                history: Vec::new(),
                            }
                        };
                        debug_assert!(state.retired.is_none());
                        state.retired = Some(retired);
                        if is_current {
                            state.status = "动画草稿已更新".to_owned();
                        }
                    }
                    Err(error) => {
                        if is_current {
                            state.status = error;
                        }
                    }
                },
                TaskResult::Save(result) => {
                    state.project_save_receipt = Some((work.path.clone(), result.as_ref().map(|_| ()).map_err(Clone::clone)));
                    match result {
                    Ok(document) => {
                        let document = Arc::new(document);
                        let previous = if let Some(session) = state.sessions.get_mut(&work.path) {
                            let previous = std::mem::replace(&mut session.document, document);
                            session.saved_cursor = Some(session.cursor);
                            session.source_changed = stale_source;
                            previous
                        } else {
                            document
                        };
                        debug_assert!(state.retired.is_none());
                        state.retired = Some(Retired::Document {
                            document: previous,
                            metadata: None,
                            history: Vec::new(),
                        });
                        if is_current {
                            state.status =
                                "动画已原子保存到原GLB；重新加载可复用修改后的动作".to_owned();
                        }
                    }
                    Err(error) => {
                        if is_current {
                            state.status = format!("保存失败，草稿保留：{error}");
                        }
                    }
                    }
                },
            }
        }
        if is_current && changes_view {
            state.changed();
        }
    }
    if state.work.is_none() && state.retired.is_some() {
        // 释放占用原作者工作槽，不 detach GC；旧结果完整归还前不再启动下一份大文档工作。
        let retired = state.retired.take().expect("已确认单份待释放所有权");
        let path = state.active.clone().unwrap_or_default();
        let generation = state
            .model_generation
            .as_ref()
            .filter(|(source, _)| source == &path)
            .map(|(_, generation)| *generation);
        state.work = Some(Work {
            path,
            generation,
            saving: false,
            task: AsyncComputeTaskPool::get().spawn(async move {
                retired.release();
                TaskResult::Released
            }),
        });
    } else if state.work.is_none() {
        if let Some(path) = state.active.clone().filter(|path| {
            !state.sessions.contains_key(path) && state.failed.as_ref() != Some(path)
        }) {
            // 与中央媒体32标签保持相同的文档缓存预算；只淘汰已保存会话，脏草稿绝不挤掉。
            let retired = if state.sessions.len() >= 32 {
                let oldest = state
                    .sessions
                    .iter()
                    .filter(|(_, session)| !session.dirty())
                    .min_by_key(|(_, session)| session.last_used)
                    .map(|(path, _)| path.clone());
                if let Some(oldest) = oldest {
                    state.sessions.remove(&oldest)
                } else {
                    state.failed = Some(path);
                    state.status =
                        "已有32份未保存动画草稿，请先保存其中一份再打开其他动画".to_owned();
                    return;
                }
            } else {
                None
            };
            state.status = "正在后台读取GLB动作与骨骼通道".to_owned();
            let source = path.clone();
            let generation = state
                .model_generation
                .as_ref()
                .filter(|(current, _)| current == &path)
                .map(|(_, generation)| *generation);
            state.work = Some(Work {
                path,
                generation,
                saving: false,
                task: IoTaskPool::get().spawn(async move {
                    drop(retired); // 大曲线和历史的最后引用也在单一后台槽释放。
                    TaskResult::Load(AnimationDocument::load(&source).map(Session::new))
                }),
            });
        } else if let Some(action) = state.pending.take() {
            launch(&mut state, action);
        } else if state.failed != state.active
            && state
                .current()
                .is_some_and(|session| session.source_changed && !session.dirty())
        {
            launch(&mut state, PendingAction::Reload);
        }
    }
    if state.visible && state.playing && !state.busy() {
        let delta = time.delta_secs().min(0.1) * state.speed;
        let looping = state.looping;
        let mut ended = false;
        if let Some(session) = state.current_mut() {
            let duration = session
                .metadata
                .durations
                .get(session.clip)
                .copied()
                .unwrap_or(0.0);
            if duration <= 0.0 {
                ended = true;
            } else {
                let position = session.position + delta;
                session.position = if looping {
                    position.rem_euclid(duration)
                } else {
                    position.min(duration)
                };
                ended = !looping && position >= duration;
            }
        }
        if ended {
            state.playing = false;
        }
    }
}

fn launch(state: &mut AnimationEditorState, action: PendingAction) {
    let Some(path) = state.active.clone() else {
        return;
    };
    let Some(session) = state.current() else {
        return;
    };
    let snapshot = session.document.clone();
    let generation = state
        .model_generation
        .as_ref()
        .filter(|(current, _)| current == &path)
        .map(|(_, generation)| *generation);
    if let PendingAction::Command(command) = action {
        state.playing = false;
        state.work = Some(Work {
            path,
            generation,
            saving: false,
            task: AsyncComputeTaskPool::get().spawn(async move {
                if let super::editing::Command::Copy {
                    clip,
                    selection,
                    source,
                } = command
                {
                    return TaskResult::Copied(super::editing::copy(
                        &snapshot, clip, &selection, source,
                    ));
                }
                if snapshot.read_only {
                    return TaskResult::Copied(Err("该动画为只读".into()));
                }
                let selection = if let super::editing::Command::Move {
                    selection, delta, ..
                } = &command
                {
                    Some(super::types::KeySelection(
                        selection
                            .0
                            .iter()
                            .map(|&(c, a, b)| (c, a + delta, b + delta))
                            .collect(),
                    ))
                } else {
                    None
                };
                match super::editing::build(&snapshot, command) {
                    Ok(edit) => {
                        let mut document = (*snapshot).clone();
                        let result = edit.apply(&mut document, true).map(|_| {
                            let metadata = Metadata::new(&document);
                            (document, metadata)
                        });
                        let target = edit.target();
                        TaskResult::Edit {
                            document: result,
                            action: PendingAction::Edit(edit),
                            key: None,
                            target,
                            selection,
                        }
                    }
                    Err(error) => TaskResult::Copied(Err(error)),
                }
            }),
        });
        return;
    }
    if matches!(action, PendingAction::Reload) {
        if session.dirty() {
            return;
        }
        state.failed = None;
        state.status = "正在后台重新载入动作".to_owned();
        let source = path.clone();
        state.work = Some(Work {
            path,
            generation,
            saving: false,
            task: IoTaskPool::get().spawn(async move {
                match snapshot.disk_unchanged(&source) {
                    Ok(true) => TaskResult::Unchanged,
                    Ok(false) => {
                        TaskResult::Load(AnimationDocument::load(&source).map(Session::new))
                    }
                    Err(error) => TaskResult::Load(Err(error)),
                }
            }),
        });
        return;
    }
    if snapshot.read_only {
        return;
    }
    if matches!(action, PendingAction::Save) {
        if let Err(error) = launch_save(state, &path) {
            state.status = error;
        }
    } else {
        let edit = match &action {
            PendingAction::Edit(edit) => edit.clone(),
            PendingAction::Undo => {
                let Some(edit) = session
                    .cursor
                    .checked_sub(1)
                    .and_then(|i| session.history.get(i))
                else {
                    return;
                };
                edit.clone()
            }
            PendingAction::Redo => {
                let Some(edit) = session.history.get(session.cursor) else {
                    return;
                };
                edit.clone()
            }
            PendingAction::Save | PendingAction::Reload | PendingAction::Command(_) => {
                unreachable!()
            }
        };
        let forward = !matches!(action, PendingAction::Undo);
        state.playing = false;
        state.work = Some(Work {
            path,
            generation,
            saving: false,
            task: AsyncComputeTaskPool::get().spawn(async move {
                let mut action = action;
                let mut document = (*snapshot).clone(); // 大曲线复制发生在后台；原草稿在失败时仍完整可用。
                let result = edit.apply(&mut document, forward);
                let key = result.as_ref().ok().copied().flatten();
                // 历史记录保存实际归一化值及完整样条切线，重做不依赖事后曲线重采样。
                if let (
                    PendingAction::Edit(Edit::Key {
                        after: Some(after), ..
                    }),
                    Some(index),
                ) = (&mut action, key)
                {
                    let (clip, channel) = edit.target();
                    *after = document.clips[clip].channels[channel].keys[index].clone();
                }
                let document = result.map(|_| {
                    let metadata = Metadata::new(&document);
                    (document, metadata)
                });
                TaskResult::Edit {
                    document,
                    action,
                    key,
                    target: edit.target(),
                    selection: None,
                }
            }),
        });
    }
}

/// 单个源GLB的唯一保存启动分支；原动画按钮与工程保存全部共同使用。
pub(super) fn launch_save(state: &mut AnimationEditorState, path: &std::path::Path) -> Result<(), String> {
    if state.busy() { return Err("动画任务尚未完成，请等待后再保存".into()); }
    let session = state.sessions.get(path).ok_or("动画源会话已变化")?;
    if session.pose.is_some() { return Err("请先插入或重置未提交姿态，再保存动画".into()); }
    let snapshot = session.document.clone();
    if snapshot.read_only { return Err("该动画源为只读，草稿已保留".into()); }
    let generation = state.model_generation.as_ref()
        .filter(|(current, _)| current == path).map(|(_, generation)| *generation);
    let path = path.to_path_buf();
    let source = path.clone();
    state.project_save_receipt = None;
    state.status = "正在后台校验磁盘版本并保存动画".to_owned();
    state.work = Some(Work {
        path, generation, saving: true,
        task: IoTaskPool::get().spawn(async move { TaskResult::Save(snapshot.save_rebased(&source)) }),
    });
    Ok(())
}
