//! 原位迁移的播放器固定节点；所有按钮绑定当前完整文件身份。

use super::types::{AudioAction, AudioKey, SliderField};
use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub(super) struct AudioPanel;
#[derive(Component, Clone, Default)]
pub(super) struct AudioButton {
    pub action: AudioAction,
    pub key: Option<AudioKey>,
}
#[derive(Component, Clone, Default)]
pub(super) struct AudioTrack {
    pub field: SliderField,
    pub key: Option<AudioKey>,
}
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AudioFill(pub SliderField);
#[derive(Component, Clone, Copy, Default)]
pub(super) enum AudioLabel {
    #[default]
    Playback,
    Time,
    Metadata,
    Volume,
    Details,
    Button(AudioAction),
}
