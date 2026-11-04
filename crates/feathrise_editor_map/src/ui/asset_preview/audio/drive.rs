//! 单槽加载、完整PCM采用和唯一输出生命周期；旧任务完成前继续占槽。

use super::super::{
    AssetPreviewWorkspace, DocumentState,
    types::{AssetPreviewContent, AssetPreviewPhase, AssetPreviewState},
};
use super::{
    context::AudioContext,
    io,
    resources::AudioPreviewState,
    source::{PlaybackCursor, PreviewAudio},
    types::*,
};
use bevy::{
    audio::{AudioSinkPlayback, GlobalVolume, Volume},
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

fn phase(preview: &mut AssetPreviewState, phase: AssetPreviewPhase, message: &str) {
    if let AssetPreviewContent::Audio { phase: current } = &mut preview.content {
        *current = phase;
    }
    if preview.status != message {
        preview.status = message.to_owned();
    }
}

fn release_output(
    state: &mut AudioPreviewState,
    commands: &mut Commands,
    assets: &mut Assets<PreviewAudio>,
    sinks: &mut Query<&mut AudioSink>,
) {
    if let Some(entity) = state.output.take() {
        if let Ok(sink) = sinks.get_mut(entity) {
            sink.stop();
        }
        commands.entity(entity).try_despawn();
    }
    if let Some(handle) = state.handle.take() {
        if let Some(audio) = assets.remove(handle.id()) {
            if state
                .data
                .as_ref()
                .is_none_or(|data| !Arc::ptr_eq(data, &audio.data))
            {
                retire(state, audio.data);
            } // 当前仍持有同一PCM时只递减Arc；不能每次EOF重建都堆重复退役引用。
        }
    }
    state.cursor = None;
    state.waiting = None;
    state.applied_volume = -1.0;
}

fn retire(state: &mut AudioPreviewState, data: Arc<super::source::PcmData>) {
    if !state.retired.iter().any(|entry| Arc::ptr_eq(entry, &data)) {
        state.retired.push(data);
    }
}

fn save_position(state: &AudioPreviewState, media: &mut AssetPreviewWorkspace) {
    let Some(key) = &state.key else { return };
    let position = state
        .cursor
        .as_ref()
        .zip(state.data.as_ref())
        .map_or(state.position, |(cursor, data)| cursor.position(data));
    for tab in media.expanded.iter_mut().chain(media.parked.iter_mut()) {
        if tab.path == key.path {
            tab.audio_position = position;
        }
    } // 固定32份轻量身份；侧栏选中别的文件不能覆盖中央位置。
}

pub(super) fn drive(
    mut commands: Commands,
    context: AudioContext,
    documents: Res<DocumentState>,
    mut preview: ResMut<AssetPreviewState>,
    mut media: ResMut<AssetPreviewWorkspace>,
    mut state: ResMut<AudioPreviewState>,
    mut assets: ResMut<Assets<PreviewAudio>>,
    mut sinks: Query<&mut AudioSink>,
    global: Option<Res<GlobalVolume>>,
) {
    let now = Instant::now();
    let root_changed = state
        .bookmark_root
        .as_ref()
        .is_some_and(|root| root != documents.asset_root());
    if root_changed {
        if let Some(tab) = media.expanded.as_mut() {
            tab.audio_position = 0.0;
        }
        for tab in &mut media.parked {
            tab.audio_position = 0.0;
        }
    }
    if state.bookmark_root.as_deref() != Some(documents.asset_root()) {
        state.bookmark_root = Some(documents.asset_root().to_path_buf());
    }
    let wanted = if context.editor.open
        && !context.editor.close_requested
        && matches!(preview.content, AssetPreviewContent::Audio { .. })
    {
        preview.selected_path.as_deref()
    } else {
        None
    };
    let changed = state.retry
        || match (&state.key, wanted) {
            (Some(key), Some(path)) => key.path != path || key.root != documents.asset_root(),
            (None, None) => false,
            _ => true,
        };
    if changed {
        if !root_changed {
            save_position(&state, &mut media);
        }
        if let Some(slot) = &state.task {
            slot.cancel.store(true, Ordering::Relaxed);
        }
        release_output(&mut state, &mut commands, &mut assets, &mut sinks);
        if let Some(data) = state.data.take() {
            retire(&mut state, data);
        }
        state.generation = state.generation.wrapping_add(1).max(1);
        state.position = media
            .tabs()
            .find(|tab| Some(tab.path.as_str()) == wanted)
            .map_or(0.0, |tab| tab.audio_position);
        state.key = wanted.map(|path| AudioKey {
            root: documents.asset_root().to_path_buf(),
            path: path.to_owned(),
            generation: state.generation,
        });
        state.pending = state.key.clone();
        state.playing = false;
        state.retry = false;
        state.rebuild = false;
        state.owner = None;
        state.drag = None;
        if wanted.is_some() {
            phase(
                &mut preview,
                AssetPreviewPhase::Loading,
                "正在后台读取并解码音频…",
            );
        }
    }
    if let Some(result) = state
        .task
        .as_mut()
        .and_then(|slot| check_ready(&mut slot.task))
    {
        state.task = None;
        if result.key.is_some() && result.key == state.key {
            match result.data {
                Ok(data) => {
                    state.position = state.position.clamp(0.0, data.duration());
                    let description = format!(
                        "{}声道 · {}Hz · {:.1}秒 · 点击播放试听",
                        data.channels.get(),
                        data.sample_rate.get(),
                        data.duration()
                    );
                    state.data = Some(data);
                    phase(&mut preview, AssetPreviewPhase::Ready, &description);
                }
                Err(error) => phase(&mut preview, AssetPreviewPhase::Failed, &error),
            }
        } else if let Ok(data) = result.data {
            retire(&mut state, data);
        }
    }
    if state.task.is_none() && (state.pending.is_some() || !state.retired.is_empty()) {
        let key = state.pending.take();
        let retired = std::mem::take(&mut state.retired);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        state.task = Some(AudioLoadSlot {
            cancel,
            task: IoTaskPool::get().spawn(async move {
                drop(retired); // 回收和下一次读取复用唯一槽，没有独立的无限退役队列。
                let data = match &key {
                    Some(key) => io::load(&key.root, &key.path, worker_cancel).await,
                    None => Err(String::new()),
                };
                AudioLoadResult { key, data }
            }),
        });
    }
    let owner = context.owner(&preview, &media);
    if owner != state.owner {
        state.playing = false; // 离开、重进或更换宿主均不自动发声。
        state.owner = owner;
    }
    if owner.is_none() {
        state.playing = false;
    }
    if state.rebuild {
        release_output(&mut state, &mut commands, &mut assets, &mut sinks);
        state.rebuild = false;
    }
    let volume = (state.volume
        * global
            .as_ref()
            .map_or(1.0, |value| value.volume.to_linear()))
    .clamp(0.0, 1.0);
    if state.playing
        && state.output.is_none()
        && let Some(data) = state.data.clone()
    {
        let cursor = PlaybackCursor::new(&data, state.position);
        let handle = assets.add(PreviewAudio {
            data,
            cursor: cursor.clone(),
        });
        let output = commands
            .spawn((
                AudioPlayer(handle.clone()),
                PlaybackSettings::ONCE
                    .paused()
                    .with_volume(Volume::Linear(state.volume)),
            ))
            .id();
        state.cursor = Some(cursor);
        state.handle = Some(handle);
        state.output = Some(output);
        state.waiting = Some(now);
    }
    if let Some(entity) = state.output {
        if let Ok(mut sink) = sinks.get_mut(entity) {
            state.waiting = None;
            if state.applied_volume != volume {
                sink.set_volume(Volume::Linear(volume));
                state.applied_volume = volume;
            }
            if sink.empty() {
                state.playing = false;
            } else if state.playing {
                if sink.is_paused() {
                    sink.play();
                }
            } else if !sink.is_paused() {
                sink.pause();
            }
        } else if state
            .waiting
            .is_some_and(|start| now.duration_since(start) > Duration::from_secs(2))
        {
            release_output(&mut state, &mut commands, &mut assets, &mut sinks);
            state.playing = false;
            phase(
                &mut preview,
                AssetPreviewPhase::Failed,
                "音频输出不可用，请检查系统输出设备后重新载入",
            );
        }
    }
    if now >= state.next_feedback {
        if let (Some(cursor), Some(data)) = (&state.cursor, &state.data) {
            state.position = cursor.position(data);
        }
        save_position(&state, &mut media);
        state.next_feedback = now + Duration::from_millis(100);
    }
}
