//! 每帧只冻结固定数量成员并轮询一个编码任务；中间更新可合并，起止边沿必须等待真实ACK。

use super::*;
use bevy::tasks::{AsyncComputeTaskPool, futures::check_ready};
use feathrise_scene_content::embedded_placement_preview::{
    PlacementPreviewCommand as Command, PlacementPreviewKind,
};

pub(super) fn advance(world: &mut World) {
    let mut state = world.remove_resource::<EmbeddedPlacementPreview>().unwrap();
    advance_inner(world, &mut state);
    state.pending = state.active.is_some()
        || state.finishing.is_some()
        || !state.queue.is_empty()
        || state.task.is_some()
        || state.ready.is_some();
    world.insert_resource(state);
}

fn advance_inner(world: &World, state: &mut EmbeddedPlacementPreview) {
    if state.observed_session != state.session
        || state.observed_generation != state.generation
        || state.observed_enabled != state.enabled
    {
        // 新会话/新场景先由宿主和SDK切代；旧投影绝不能在新World重放。后台槽仍占有到实际结束。
        state.observed_generation = state.generation;
        state.observed_session = state.session;
        state.observed_enabled = state.enabled;
        state.epoch = state.epoch.wrapping_add(1);
        state.active = None;
        state.finishing = None;
        state.queue.clear();
        state.ready = None;
        state.error = None;
    }
    if let Some((session, generation, epoch, result)) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        if state.enabled
            && session == state.session
            && generation == state.generation
            && epoch == state.epoch
        {
            match result {
                Ok(ready) => state.ready = Some(ready),
                Err(error) => state.error = Some(error),
            }
        }
    }
    if !state.enabled {
        return;
    }
    let current = match source::current(world) {
        Ok(current) => current,
        Err(error) => {
            state.error = Some(error);
            None
        }
    };
    if let Some(mut active) = state.active.take() {
        let same = current
            .as_ref()
            .is_some_and(|current| current.key == active.source.key);
        if same {
            active.source = current.as_ref().unwrap().clone();
        }
        if !same || active.source.finished {
            finish(world, state, active, same);
        } else {
            publish_active(state, &mut active);
            state.active = Some(active);
        }
    }
    advance_finishing(world, state);
    if state.active.is_none()
        && state.finishing.is_none()
        && let Some(source) = current.filter(|source| !source.finished)
    {
        state.next_gesture = state.next_gesture.wrapping_add(1).max(1);
        let mut active = Active {
            source,
            gesture: state.next_gesture,
            sequence: 0,
            ids: Vec::new(),
            begin_queued: false,
            last_transform: None,
        };
        publish_active(state, &mut active);
        state.active = Some(active);
    }
    encode_next(state);
}

fn publish_active(state: &mut EmbeddedPlacementPreview, active: &mut Active) {
    if !active.begin_queued {
        // 原多选工具硬门是256成员；此处仍按64项/帧复制ID，不在输入帧展开整个成员集。
        let end = (active.ids.len() + MEMBERS_PER_FRAME).min(active.source.members.len());
        for index in active.ids.len()..end {
            active.ids.push(active.source.members.id(index).to_owned());
        }
        if active.ids.len() != active.source.members.len() {
            return;
        }
        // 为当前手势最终End保留一个可靠槽；暂时拥塞只等待，绝不截断成员或丢弃取消。
        if state.queue.len() >= MAX_QUEUED_COMMANDS - 2 {
            state.error = Some("对象实时预览通道暂时拥塞，正在等待工程实例确认".into());
            return;
        }
        state.queue.push_back(Queued {
            scene_id: active.source.key.scene_id.0.clone(),
            command: Command::Begin {
                gesture: active.gesture,
                scene_id: active.source.key.scene_id.clone(),
                source_revision: active.source.key.revision,
                ids: std::mem::take(&mut active.ids),
                kind: active.source.kind,
            },
        });
        active.begin_queued = true;
        state.error = None;
    }
    if active.last_transform != Some(active.source.transform) {
        active.sequence = active.sequence.wrapping_add(1).max(1);
        let item = Queued {
            scene_id: active.source.key.scene_id.0.clone(),
            command: Command::Update {
                gesture: active.gesture,
                sequence: active.sequence,
                transform: active.source.transform,
            },
        };
        // 未交给后台或宿主的同一手势中间值只保留最新；在途包和可靠边沿保持原顺序。
        if state.queue.back().is_some_and(|last| matches!(last.command, Command::Update { gesture, .. } if gesture == active.gesture)) {
            *state.queue.back_mut().unwrap() = item;
        } else {
            state.queue.push_back(item);
        }
        active.last_transform = Some(active.source.transform);
    }
}

fn finish(world: &World, state: &mut EmbeddedPlacementPreview, mut active: Active, same: bool) {
    if !active.begin_queued {
        return;
    } // 还未发起远端手势便松手，无远端暂态需要撤回。
    active.sequence = active.sequence.wrapping_add(1).max(1);
    let revision = source::author_revision(world);
    let transform = match active.source.kind {
        PlacementPreviewKind::Absolute => source::authority(world, &active.source.key),
        PlacementPreviewKind::GroupDelta if same && active.source.committed => active.last_transform,
        PlacementPreviewKind::GroupDelta if same => Some(active.source.transform),
        PlacementPreviewKind::GroupDelta => None,
    };
    let cancelled = match active.source.kind {
        PlacementPreviewKind::Absolute => {
            transform == Some(active.source.original_transform) || transform.is_none()
        }
        PlacementPreviewKind::GroupDelta => {
            !same || !active.source.committed
        }
    };
    state.finishing = Some(Finishing {
        active,
        revision,
        transform,
        cancelled,
        members: Vec::new(),
    });
}

fn advance_finishing(world: &World, state: &mut EmbeddedPlacementPreview) {
    let Some(mut finishing) = state.finishing.take() else {
        return;
    };
    let revision = source::author_revision(world);
    if revision != finishing.revision {
        // 原工具收尾期间不接受新手势；外部作者接管则从同一新修订重新冻结，禁止拼接两代姿态。
        finishing.members.clear();
        finishing.revision = revision;
    }
    let end =
        (finishing.members.len() + MEMBERS_PER_FRAME).min(finishing.active.source.members.len());
    for index in finishing.members.len()..end {
        match source::final_member(world, &finishing.active.source, index) {
            Ok(member) => finishing.members.push(member),
            Err(_) => {
                // 删除/切换场景已经撤销成员拥有权；可靠取消只清临时投影，不能用旧初值覆盖新作者。
                finishing.members.clear();
                finishing.cancelled = true;
                finishing.transform = None;
                break;
            }
        }
    }
    if !finishing.members.is_empty()
        && finishing.members.len() < finishing.active.source.members.len()
    {
        state.finishing = Some(finishing);
        return;
    }
    state.queue.push_back(Queued {
        scene_id: finishing.active.source.key.scene_id.0.clone(),
        command: Command::End {
            gesture: finishing.active.gesture,
            sequence: finishing.active.sequence,
            source_revision: finishing.revision,
            transform: finishing.transform,
            cancelled: finishing.cancelled,
            final_members: finishing.members,
        },
    });
}

fn encode_next(state: &mut EmbeddedPlacementPreview) {
    if state.ready.is_some() || state.task.is_some() {
        return;
    }
    let Some(next) = state.queue.pop_front() else {
        return;
    };
    state.next_sequence = state.next_sequence.wrapping_add(1).max(1);
    let sequence = state.next_sequence;
    let generation = state.generation;
    let session = state.session;
    let epoch = state.epoch;
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let encoded = serde_json::to_vec(&next.command)
            .map_err(|error| format!("对象预览编码失败：{error}"))
            .map(|bytes| {
                Arc::new(EncodedPlacementPreview {
                    sequence,
                    session,
                    generation,
                    scene_id: next.scene_id,
                    bytes: Arc::new(bytes),
                })
            });
        (session, generation, epoch, encoded)
    }));
}
