//! 有限地形工具验收的阶段与只读证据；不定义作者数据或第二套笔刷算法。

use bevy::prelude::*;
use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Domain {
    Height,
    Surface,
    Blade,
}

impl Domain {
    pub(super) fn center(self) -> Vec3 {
        match self {
            Self::Height => Vec3::new(-8., 0., -8.),
            Self::Surface => Vec3::new(-8., 0., 8.),
            Self::Blade => Vec3::new(-16., 0., 8.),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    Idle,
    Focus,
    WaitFocus,
    Prepare,
    WaitPreset,
    SavePreset,
    WaitPresetSave,
    ClosePreset,
    WaitPresetClose,
    CreateLayer,
    WaitLayer,
    Before,
    Hover,
    Paint,
    Release,
    After,
    Undo,
    WaitUndo,
    Undone,
    Redo,
    WaitRedo,
    Redone,
    Restore,
    Complete,
    Failed,
}

/// 完整源内容的摘要在唯一后台槽计算，主线程只比较固定大小的摘要和统计。
#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct Fingerprint {
    pub sha256: String,
    pub bytes: usize,
    pub nonzero_samples: usize,
    pub files: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct StrokeProof {
    pub tool: &'static str,
    pub center: [f32; 3],
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub history_kind: String,
    pub changed_samples: usize,
    pub history_entries: usize,
    pub before: Fingerprint,
    pub after: Fingerprint,
    pub undo_matches_before: bool,
    pub redo_matches_after: bool,
    pub live_preview_ready: bool,
}

/// 只读记录原视口参数；验收结束后恢复，不改变作者历史。
pub(super) type View = (crate::types::EditorView, bool, Vec3, f32, f32, f32);

pub(super) struct Clock {
    pub start: Instant,
    pub phase: Instant,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            phase: Instant::now(),
        }
    }
}
