//! 人形关节点、选择器及受检后台结果的内部合同。

use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task},
};

use super::backend::{AnimationAsset, Inspection, PreparedRig, PublishedRig};
use crate::viewport::placement_move::MoveCamera;

pub(super) const PICKER_ROWS: usize = 5;
pub(super) const LANDMARKS: [(&str, &str); 17] = [
    ("head", "rig.point.head"),
    ("neck", "rig.point.neck"),
    ("hips", "rig.point.hips"),
    ("left_shoulder", "rig.point.left_shoulder"),
    ("left_elbow", "rig.point.left_elbow"),
    ("left_wrist", "rig.point.left_wrist"),
    ("left_hip", "rig.point.left_hip"),
    ("left_knee", "rig.point.left_knee"),
    ("left_ankle", "rig.point.left_ankle"),
    ("left_toe", "rig.point.left_toe"),
    ("right_shoulder", "rig.point.right_shoulder"),
    ("right_elbow", "rig.point.right_elbow"),
    ("right_wrist", "rig.point.right_wrist"),
    ("right_hip", "rig.point.right_hip"),
    ("right_knee", "rig.point.right_knee"),
    ("right_ankle", "rig.point.right_ankle"),
    ("right_toe", "rig.point.right_toe"),
];

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RigAction {
    Launch,
    Close,
    Retry,
    ResetMarks,
    Bind,
    PickFile,
    PickClip,
    Choose(usize),
    Previous,
    Next,
    Preview,
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct RigPanel;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct RigChoices;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct RigMarkerTools;

#[derive(Component, Clone, Copy)]
pub(super) enum RigText {
    Launch,
    Close,
    Notice,
    Warning,
    SelectedMarker,
    File,
    Clip,
    Choice(usize),
    Page,
}

#[derive(Component, Clone, Copy)]
pub(crate) struct RigLandmarkDot(pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourceKey {
    pub root: PathBuf,
    pub path: String,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Unskinned,
    Skinned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Picker {
    File,
    Clip,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) enum Notice {
    #[default]
    Inspecting,
    Cataloguing,
    Review,
    Binding,
    Publishing,
    Saved {
        joints: usize,
        vertices: usize,
    },
    ChooseAnimation,
    NoReference,
    NoClips,
    Cancelled,
    Failed,
}

pub(super) enum Completion {
    Inspection(Result<(Inspection, Vec<usize>), String>),
    Catalog(Result<Vec<AnimationAsset>, String>),
    Binding(Result<PreparedRig, String>),
    Publish {
        result: Result<PublishedRig, String>,
        joints: usize,
        vertices: usize,
    },
    Disposed,
}

pub(super) struct Work {
    pub source: Option<SourceKey>,
    pub cancelled: Arc<AtomicBool>,
    pub task: Task<Completion>,
}

impl Work {
    pub(super) fn start(
        source: Option<SourceKey>,
        operation: impl FnOnce(&AtomicBool) -> Completion + Send + 'static,
    ) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = cancelled.clone();
        Self {
            source,
            cancelled,
            task: IoTaskPool::get().spawn(async move { operation(&token) }),
        }
    }
}

pub(super) struct LandmarkDrag {
    pub index: usize,
    pub before: Vec3,
    pub start: Vec3,
    pub normal: Dir3,
    pub camera: MoveCamera,
}
