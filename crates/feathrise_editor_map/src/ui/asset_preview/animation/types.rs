//! 连续动作表、选区与姿势手势的轻量契约；显示槽不再决定可编辑的数据范围。

use bevy::prelude::*;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

/// 时间闭区间描述选区，框选十万个键也只保存最多八条可见轨道的范围。
#[derive(Clone, Default, Debug, PartialEq)]
pub(super) struct KeySelection(pub Vec<(usize, f32, f32)>);

impl KeySelection {
    pub fn contains(&self, channel: usize, time: f32) -> bool {
        self.0
            .iter()
            .any(|&(row, start, end)| row == channel && time >= start && time <= end)
    }
    pub fn single(channel: usize, time: f32) -> Self {
        Self(vec![(channel, time, time)])
    }
    pub fn toggle(&mut self, channel: usize, time: f32) {
        if self.contains(channel, time) {
            let old = std::mem::take(&mut self.0);
            for (row, start, end) in old {
                if row != channel || time < start || time > end {
                    self.0.push((row, start, end));
                } else {
                    if start < time {
                        self.0.push((row, start, time.next_down()));
                    }
                    if end > time {
                        self.0.push((row, time.next_up(), end));
                    }
                }
            }
        } else {
            self.0.push((channel, time, time));
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum PoseTool {
    #[default]
    Move,
    Rotate,
}

#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct AnimationGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct AnimationHighlightGizmos;

pub(super) struct PoseGesture {
    pub source: PathBuf,
    pub revision: u64,
    pub source_revision: u64,
    pub clip: usize,
    pub node: usize,
    pub before: Transform,
    pub previous: Option<(usize, Transform)>,
    pub parent: Mat4,
    pub origin: Vec3,
    pub start: Vec2,
    pub camera: crate::viewport::placement_move::MoveCamera,
    pub constraint: crate::authoring::mesh_edit::MoveConstraint,
    pub tool: PoseTool,
}

#[derive(Clone)]
pub(super) enum TimelineGestureKind {
    Scrub,
    Pan {
        start: f32,
    },
    Move {
        selection: KeySelection,
        delta: f32,
    },
    Box {
        additive: KeySelection,
        end_row: usize,
        end_time: f32,
    },
}

#[derive(Clone)]
pub(super) struct TimelineGesture {
    pub entity: Entity,
    pub source: PathBuf,
    pub binding: u64,
    pub cursor: Vec2,
    pub time: f32,
    pub row: usize,
    pub button: MouseButton,
    pub kind: TimelineGestureKind,
}

#[derive(Default)]
pub(super) struct AnimationView {
    pub visible: bool,
    pub binding: u64,
    pub status: String,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub busy: bool,
    pub editable: bool,
    pub clip_name: String,
    pub clip_index: usize,
    pub clip_count: usize,
    pub playing: bool,
    pub looping: bool,
    pub position: f32,
    pub duration: f32,
    pub fps: u32,
    pub speed: f32,
    pub selected_channel: Option<usize>,
    pub selected_key: Option<usize>,
    pub channel_count: usize,
    pub page: usize,
    pub rows: Vec<AnimationChannelView>,
    pub key_time: Option<f32>,
    pub key_values: Vec<f32>,
    pub interpolation: String,
    pub start: f32,
    pub span: f32,
    pub bone_name: String,
    pub selected_bone_only: bool,
    pub auto_key: bool,
    pub pose_tool: PoseTool,
    pub pose_dirty: bool,
    pub model_path: Option<String>,
    pub model_status: String,
    pub selected: KeySelection,
    pub move_delta: f32,
    pub box_selection: Option<(usize, usize, f32, f32)>,
}

pub(super) struct AnimationChannelView {
    pub index: usize,
    pub name: String,
    pub path: String,
    pub keys: Vec<AnimationKeyView>,
    pub selected: bool,
}

pub(super) struct AnimationKeyView {
    pub index: usize,
    pub time: f32,
    pub selected: bool,
    pub clustered: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct BindingIdentity {
    pub source: PathBuf,
    pub revision: u64,
    pub request: u64,
    pub model: String,
}

pub(super) struct SkeletonMapping {
    pub names: HashMap<String, usize>,
}

#[derive(Default)]
pub(super) struct ModelBank {
    pub root: Option<Entity>,
    pub ready: bool,
    pub scanned: bool,
    pub stack: Vec<(Entity, usize)>,
    pub entities: Vec<Entity>,
    pub nodes: HashMap<usize, Entity>,
    pub mesh_count: usize,
}

#[derive(Default)]
pub(super) struct RetiredModels {
    pub banks: Vec<ModelBank>,
    pub mappings: Vec<SkeletonMapping>,
}

#[derive(Clone)]
pub(super) struct Clipboard {
    pub source: PathBuf,
    /// 只持有轻量来源身份，避免为过期检测保留整份旧文档或二进制。
    pub source_identity: Arc<()>,
    pub origin: f32,
    pub keys: Vec<(usize, String, Vec<super::data::Keyframe>)>,
}

pub(super) enum Command {
    Copy {
        clip: usize,
        selection: KeySelection,
        source: PathBuf,
    },
    Move {
        clip: usize,
        selection: KeySelection,
        delta: f32,
    },
    Delete {
        clip: usize,
        selection: KeySelection,
    },
    Paste {
        clip: usize,
        at: f32,
        source: PathBuf,
        clipboard: Arc<Clipboard>,
    },
    Pose {
        clip: usize,
        time: f32,
        node: usize,
        transform: Transform,
    },
}
