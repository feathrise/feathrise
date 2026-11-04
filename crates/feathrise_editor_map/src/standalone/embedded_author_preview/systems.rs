use super::{resources::*, source};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};
fn retire<T: Send + Sync + 'static>(world: &mut World, value: T) {
    world
        .resource_mut::<crate::authoring::AuthoringRetirement>()
        .push(value);
}
pub(super) fn advance(world: &mut World) {
    let mut state = world
        .remove_resource::<EmbeddedAuthorPreviewState>()
        .unwrap();
    let mut current = source::identity(world, &state);
    if current != state.identity {
        state.epoch = state.epoch.wrapping_add(1);
        if let Some(current) = current.as_mut() {
            current.epoch = state.epoch;
        }
        state.identity = current.clone();
        retire(
            world,
            (
                std::mem::take(&mut state.acknowledged),
                std::mem::take(&mut state.heights),
                state.offered.take(),
                state.ready.take(),
            ),
        );
        state.failed = None;
        state.error = None;
        state.cursor = 0;
        state.sub_map_pose.clear();
        state.terrain_pose.clear();
    }
    // 每帧只观察两个原生产资源。编码/ACK 背压期间仍保存取消、提交边沿，绝不重算手势。
    if let Some(identity) = &current {
        super::poses::observe(world, &mut state, &identity.map);
    }
    super::poses::retire_stale(world, &mut state);
    if let Some(done) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        if current.as_ref() == Some(&done.identity) {
            match done.result {
                Ok(ready) => {
                    state.offered = Some((done.key, done.version, done.height));
                    state.ready = Some(ready);
                    state.error = None;
                }
                Err(error) => {
                    state.failed = Some((done.key, done.version));
                    state.error = Some(error);
                    retire(world, done.height);
                }
            }
        } else {
            retire(world, done);
        }
    }
    if state
        .ready
        .as_ref()
        .is_some_and(|ready| ready.sequence == state.acknowledged_sequence)
    {
        if let Some((key, version, height)) = state.offered.take() {
            if let Key::Height(id) = &key
                && let Some(height) = height
            {
                retire(world, state.heights.insert(id.clone(), height));
            }
            state.acknowledged.insert(key, version);
        }
        retire(world, state.ready.take());
    }
    super::poses::acknowledge(world, &mut state);
    if let Some(identity) = current
        && state.task.is_none()
        && state.ready.is_none()
        && let Some((key, version, frozen)) = source::capture(world, &mut state, &identity.map)
    {
        if let Some(sequence) = state.sequence.checked_add(1) {
            state.sequence = sequence;
            state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
                source::encode_frozen(identity, sequence, key, version, frozen)
            }));
        } else {
            state.error = Some("作者预览序号已耗尽，请重新连接".into());
        }
    }
    state.pending = state.task.is_some() || state.ready.is_some();
    world.insert_resource(state);
}
