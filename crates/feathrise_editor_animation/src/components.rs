//! 动作编辑器的固定槽位和冻结交互身份；翻页只改绑定，不重建输入实体。

use bevy::prelude::*;

pub(super) use feathrise_editor_animation_model::constants::{CHANNEL_SLOTS, KEY_SLOTS, RULER_TICK_WIDTH};

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationPanel;

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationPoseToolbar;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct AnimationWriteControls; // 就地播放只读，复用控件但收起作者写入口。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnimationAction {
    PreviousClip,
    NextClip,
    FirstFrame,
    PreviousFrame,
    PreviousKey,
    #[default]
    TogglePlayback,
    NextFrame,
    NextKey,
    LastFrame,
    ToggleLoop,
    Save,
    Reload,
    Undo,
    Redo,
    ToggleFps,
    CycleSpeed,
    PreviousChannelPage,
    NextChannelPage,
    InsertKey,
    DeleteKey,
    CycleInterpolation,
    FitTimeline,
    ZoomIn,
    ZoomOut,
    ToggleBoneFilter,
    PreviousBone,
    NextBone,
    MovePose,
    RotatePose,
    InsertPose,
    ToggleAutoKey,
    ResetPose,
    BindSelectedModel,
    UnbindModel,
    CopyKeys,
    PasteKeys,
}

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationButton {
    pub action: AnimationAction,
    pub binding: u64,
}

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationTrack {
    pub binding: u64,
    /// None为帧尺；有行号时支持关键帧拾取、拖动和跨行框选。
    pub row: Option<usize>,
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct AnimationSelectionBox(pub usize);

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationChannelButton {
    pub slot: usize,
    pub binding: u64,
    pub index: Option<usize>,
}

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationKeyButton {
    pub row: usize,
    pub slot: usize,
    pub binding: u64,
    pub channel: Option<usize>,
    pub index: Option<usize>,
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct AnimationRow(pub usize);

#[derive(Component, Clone, Copy, Default)]
pub(super) struct AnimationPlayhead;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct AnimationRulerTick(pub usize);

#[derive(Component, Clone, Copy, Default)]
pub(super) enum AnimationLabel {
    #[default]
    Clip,
    ClipParameters,
    Time,
    Pages,
    Status,
    Bone,
    Model,
    Hint,
    Channel(usize),
    Tick(usize),
    Button(AnimationAction),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnimationNumericField {
    #[default]
    Frame,
    Value(usize),
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct AnimationNumericSlot(pub AnimationNumericField);

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationNumericInput {
    pub field: AnimationNumericField,
    pub binding: u64,
}

#[derive(Component)]
pub(super) struct BoundModel(pub usize);
