//! 动作编辑器的固定槽位和冻结交互身份；翻页只改绑定，不重建输入实体。

use bevy::prelude::*;

pub(super) use feathrise_editor_animation_model::constants::{CHANNEL_SLOTS, KEY_SLOTS, RULER_TICK_WIDTH};

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationPanel;

#[derive(Component, Clone, Copy, Default)]
pub struct AnimationPoseToolbar;

#[derive(Component, Clone, Copy, Default)]
pub(super) enum AnimationWriteControls {
    #[default]
    Clip,
    History, // 撤销唯一动作后仍可重做或保存。
}

/// 动作来源的切换和模型绑定仍由当前原文件预览提供。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct AnimationDirectSourceControls;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnimationAction {
    CreateClip,
    ToggleEditing,
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

/// Bevy 加载 Scene0 时记录的作者节点索引，供同源动作定位实际网格实例。
#[derive(Component, Reflect)]
#[reflect(Component)]
pub(super) struct EmbeddedNodeIndex(pub usize);
