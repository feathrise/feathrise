//! 唯一音频输出和有界加载槽；后台标签只在媒体工作区保留位置。

use super::{
    source::{PcmData, PlaybackCursor, PreviewAudio},
    types::*,
};
use bevy::prelude::*;
use std::{sync::Arc, time::Instant};

#[derive(Resource)]
pub(crate) struct AudioPreviewState {
    pub(super) key: Option<AudioKey>,
    pub(super) bookmark_root: Option<std::path::PathBuf>, // F12关闭后仍保留根身份，防止同名新根借用旧标签位置。
    pub(super) generation: u64,
    pub(super) pending: Option<AudioKey>,
    pub(super) task: Option<AudioLoadSlot>,
    pub(super) retired: Vec<Arc<PcmData>>, // 至多旧活动值和唯一迟到值，归还在同一IO槽。
    pub(super) data: Option<Arc<PcmData>>,
    pub(super) cursor: Option<Arc<PlaybackCursor>>,
    pub(super) handle: Option<Handle<PreviewAudio>>,
    pub(super) output: Option<Entity>,
    pub(super) waiting: Option<Instant>,
    pub(super) playing: bool,
    pub(super) position: f64,
    pub(super) volume: f32,
    pub(super) applied_volume: f32,
    pub(super) rebuild: bool,
    pub(super) retry: bool,
    pub(super) owner: Option<Option<u64>>, // Some(None)侧栏，Some(Some(id))中央，None隐藏。
    pub(super) next_feedback: Instant,
    pub(super) drag: Option<SliderDrag>,
    pub(super) mouse_owned: bool,
    pub(super) wheel_owned: bool,
}

impl Default for AudioPreviewState {
    fn default() -> Self {
        Self {
            key: None,
            bookmark_root: None,
            generation: 0,
            pending: None,
            task: None,
            retired: Vec::new(),
            data: None,
            cursor: None,
            handle: None,
            output: None,
            waiting: None,
            playing: false,
            position: 0.0,
            volume: 0.75,
            applied_volume: -1.0,
            rebuild: false,
            retry: false,
            owner: None,
            next_feedback: Instant::now(),
            drag: None,
            mouse_owned: false,
            wheel_owned: false,
        }
    }
}

impl AudioPreviewState {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.mouse_owned || self.drag.is_some()
    }
    pub(crate) fn owns_wheel(&self) -> bool {
        self.wheel_owned || self.owns_mouse()
    }
    pub(super) fn seek(&mut self, position: f64) {
        let Some(data) = &self.data else { return };
        if !position.is_finite() {
            return;
        }
        self.position = position.clamp(0.0, data.duration());
        if let Some(cursor) = &self.cursor {
            self.rebuild |= cursor.finished(); // EOF后的Source不再被音频线程轮询，显式重建轻量光标。
            cursor.seek(data, self.position);
        }
    }
}
