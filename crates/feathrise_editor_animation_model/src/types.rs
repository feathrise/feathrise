//! 连续动作表、选区与姿势手势的轻量契约；显示槽不再决定可编辑的数据范围。

use bevy::prelude::*;
use std::{collections::{BTreeMap, HashMap, HashSet}, path::PathBuf, sync::Arc};

/// 加载 GLB 时记录真实 nodes 索引。场景树和动画都用它绑定同一份源节点。
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct EmbeddedNodeIndex(pub usize);

/// 源文件中的原片段顺序，只供编辑器读 WorldAsset，不在每个渲染实例重复存放。
#[derive(Component)]
#[component(clone_behavior = Ignore)]
pub struct EmbeddedSceneAnimations(pub Arc<[String]>);

/// 时间闭区间描述选区，框选十万个键也只保存最多八条可见轨道的范围。
#[derive(Clone, Default, Debug, PartialEq)]
pub struct KeySelection(pub Vec<(usize, f32, f32)>);

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
pub enum PoseTool {
    #[default]
    Move,
    Rotate,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum PoseProperty {
    Translation,
    Rotation,
    Scale,
}

impl PoseProperty {
    pub const ALL: [Self; 3] = [Self::Translation, Self::Rotation, Self::Scale];

    pub fn from_path(path: &str) -> Option<Self> {
        match path {
            "translation" => Some(Self::Translation),
            "rotation" => Some(Self::Rotation),
            "scale" => Some(Self::Scale),
            _ => None,
        }
    }

    pub fn path(self) -> &'static str {
        match self {
            Self::Translation => "translation",
            Self::Rotation => "rotation",
            Self::Scale => "scale",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BonePose {
    pub transform: Transform,
    properties: u8,
}

impl BonePose {
    pub fn contains(self, property: PoseProperty) -> bool {
        self.properties & property.bit() != 0
    }
}

/// 同一帧累积各骨局部变换。指针拖动只更新当前骨，换骨首次修改才合入已完成的草稿。
#[derive(Clone, Debug, Default)]
pub struct PoseDraft {
    settled: Arc<BTreeMap<usize, BonePose>>,
    active: Option<(usize, BonePose)>,
}

impl PartialEq for PoseDraft {
    fn eq(&self, other: &Self) -> bool {
        // 发布快照共享同一份已完成姿势，逐帧核对无需遍历整个骨骼集合。
        self.active == other.active && Arc::ptr_eq(&self.settled, &other.settled)
    }
}

impl PoseDraft {
    pub fn get(&self, node: usize) -> Option<BonePose> {
        self.active.filter(|(index, _)| *index == node).map(|(_, value)| value)
            .or_else(|| self.settled.get(&node).copied())
    }

    pub fn set(&mut self, node: usize, transform: Transform, property: PoseProperty) -> bool {
        let previous = self.get(node);
        let value = BonePose {
            transform,
            properties: previous.map_or(0, |pose| pose.properties) | property.bit(),
        };
        if previous == Some(value) {
            return false;
        }
        if let Some((previous_node, previous_pose)) = self.active.filter(|(index, _)| *index != node) {
            Arc::make_mut(&mut self.settled).insert(previous_node, previous_pose);
        }
        self.active = Some((node, value));
        true
    }

    pub fn len(&self) -> usize {
        self.settled.len() + usize::from(self.active.is_some_and(|(node, _)| !self.settled.contains_key(&node)))
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_none() && self.settled.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, BonePose)> + '_ {
        self.settled.iter()
            .filter(|(node, _)| self.active.is_none_or(|(index, _)| index != **node))
            .map(|(&node, &value)| (node, value))
            .chain(self.active)
    }
}

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct AnimationGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct AnimationHighlightGizmos;

pub struct PoseGesture {
    pub source: PathBuf,
    pub revision: u64,
    pub source_revision: u64,
    pub clip: usize,
    pub node: usize,
    pub before: Transform,
    pub previous: Option<PoseDraft>,
    pub parent: Mat4,
    pub origin: Vec3,
    pub start: Vec2,
    pub camera: crate::gizmo::GizmoCamera,
    pub constraint: crate::gizmo::MoveConstraint,
    pub tool: PoseTool,
}

#[derive(Clone)]
pub enum TimelineGestureKind {
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
pub struct TimelineGesture {
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
pub struct AnimationView {
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
    pub clip_source_index: usize,
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
    pub pose_bones: usize,
    pub model_path: Option<String>,
    pub model_status: String,
    pub selected: KeySelection,
    pub move_delta: f32,
    pub box_selection: Option<(usize, usize, f32, f32)>,
}

pub struct AnimationChannelView {
    pub index: usize,
    pub name: String,
    pub path: String,
    pub keys: Vec<AnimationKeyView>,
    pub selected: bool,
}

pub struct AnimationKeyView {
    pub index: usize,
    pub time: f32,
    pub selected: bool,
    pub clustered: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct BindingIdentity {
    pub source: PathBuf,
    pub revision: u64,
    pub request: u64,
    pub model: String,
    pub retarget: bool,
    /// 同一文件的动作直接写入本代 Scene0，热更新后不得复用旧节点索引。
    pub embedded_generation: Option<u64>,
}

pub struct SkeletonMapping {
    pub names: HashMap<String, usize>,
    pub retarget: Option<Arc<super::retarget::RigRetarget>>,
    pub embedded_nodes: HashSet<usize>,
}

#[derive(Default)]
pub struct ModelBank {
    pub root: Option<Entity>,
    pub ready: bool,
    pub scanned: bool,
    pub stack: Vec<(Entity, usize)>,
    pub entities: Vec<Entity>,
    pub nodes: HashMap<usize, Entity>,
    pub mesh_count: usize,
}

#[derive(Default)]
pub struct RetiredModels {
    pub banks: Vec<ModelBank>,
    pub mappings: Vec<SkeletonMapping>,
}

#[derive(Clone)]
pub struct Clipboard {
    pub source: PathBuf,
    /// 只持有轻量来源身份，避免为过期检测保留整份旧文档或二进制。
    pub source_identity: Arc<()>,
    pub origin: f32,
    pub keys: Vec<(usize, String, Vec<super::data::Keyframe>)>,
}

pub enum Command {
    ImportClips { source: Arc<super::data::AnimationDocument>, rows: Arc<[super::data::ContentSpan]>, adapt: bool },
    CreateClip,
    RenameClip { id: usize, name: String },
    DuplicateClip { id: usize },
    DeleteClip { id: usize },
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
        draft: PoseDraft,
    },
    CapturePose {
        clip: usize,
        time: f32,
        node: usize,
    },
}
