//! 音频试听的轻量文件身份、固定控件动作与唯一后台槽回执。

use super::source::PcmData;
use bevy::{prelude::*, tasks::Task};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AudioKey {
    pub root: PathBuf,
    pub path: String,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AudioAction {
    #[default]
    Toggle,
    Stop,
    Reload,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SliderField {
    #[default]
    Position,
    Volume,
}

pub(super) struct SliderDrag {
    pub entity: Entity,
    pub key: AudioKey,
    pub field: SliderField,
}

pub(super) struct AudioLoadResult {
    pub key: Option<AudioKey>, // None仅归还旧PCM，不触发文件采用。
    pub data: Result<Arc<PcmData>, String>,
}

pub(super) struct AudioLoadSlot {
    pub cancel: Arc<AtomicBool>,
    pub task: Task<AudioLoadResult>,
}
