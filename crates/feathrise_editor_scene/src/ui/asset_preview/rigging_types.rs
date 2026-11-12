//! 人形关节点、选择器及受检后台结果的内部合同。

use std::{
    future::Future,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task},
};

use feathrise_editor_glb_model::rigging::{AnimationAsset, Inspection, Landmarks, PreparedRig, PublishedRig};
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
    ChooseProjectMotion,
    Close,
    Retry,
    SwitchBackend,
    ToggleMirror,
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
    Title,
    Close,
    Backend,
    Mirror,
    Notice,
    Warning,
    SelectedMarker,
    Reference,
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
    Inspection(Result<(Inspection, bool), String>),
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
        Self::start_async(source, |token| async move { operation(&token) })
    }

    pub(super) fn start_async<F>(
        source: Option<SourceKey>,
        operation: impl FnOnce(Arc<AtomicBool>) -> F + Send + 'static,
    ) -> Self
    where
        F: Future<Output = Completion> + Send + 'static,
    {
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = Arc::clone(&cancelled);
        Self {
            source,
            cancelled,
            task: IoTaskPool::get().spawn(async move { operation(token).await }),
        }
    }
}

/// 站姿的稳定左右中线，来源是初次检测的两肩，不随被编辑的点旋转或漂移。
#[derive(Clone, Copy)]
pub(super) struct RigFrame {
    center: Vec3,
    left: Dir3,
}

impl RigFrame {
    pub(super) fn from_landmarks(points: &Landmarks) -> Option<Self> {
        let left = Vec3::from_array(*points.get("left_shoulder")?);
        let right = Vec3::from_array(*points.get("right_shoulder")?);
        Some(Self { center: left.midpoint(right), left: Dir3::new((left - right).with_y(0.0)).ok()? })
    }

    pub(super) fn reflect(self, point: Vec3) -> Vec3 {
        point - *self.left * (2.0 * (point - self.center).dot(*self.left))
    }

    pub(super) fn yaw(self) -> f32 {
        let front = self.left.cross(Vec3::Y);
        front.x.atan2(front.z)
    }
}

pub(super) struct MirrorDrag {
    pub index: usize,
    pub before: Vec3,
    pub frame: RigFrame,
}

pub(super) struct LandmarkDrag {
    pub index: usize,
    pub before: Vec3,
    pub start: Vec3,
    pub normal: Dir3,
    pub camera: MoveCamera,
    pub snap: Option<(feathrise_editor_core::EditorTranslationSnap, Vec3, Vec3)>,
    pub mirror: Option<MirrorDrag>,
}
