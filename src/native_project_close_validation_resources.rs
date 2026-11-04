//! 显式自有工程关闭验收的有限状态；正常作者会话不安装。

use bevy::prelude::*;
use bevy::tasks::Task;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Idle,
    LoadingDocument,
    CleanPrompt,
    CleanCapturing,
    CleanCancelled,
    CancelPrompt,
    Capturing,
    Cancelled,
    InjectingConflict,
    FailurePrompt,
    FailedSave,
    RestoringConflict,
    FinalSave,
    Complete,
}

/// 原App返回后才由外层落盘；最后一帧只交付有界证据，不撤销已发布的AppExit。
#[derive(Clone)]
pub(crate) struct ValidationReport {
    pub directory: PathBuf,
    pub proof: Arc<Mutex<Option<serde_json::Value>>>,
}

#[derive(Resource)]
pub(super) struct Fixture {
    pub phase: Phase,
    pub report: ValidationReport,
    pub started: Instant,
    pub project: crate::project::Project,
    pub asset_root: PathBuf,
    pub original: Arc<Vec<u8>>,
    pub draft: Option<serde_json::Value>,
    pub marker: Option<Entity>,
    pub io: Option<Task<Result<(), String>>>,
    pub pulse: Option<Entity>,
    pub capture: Arc<Mutex<Option<Result<(), String>>>>,
    pub cancel_verified: bool,
    pub failure_verified: bool,
    pub clean_prompt_started: Option<Instant>,
    pub clean_cancel_verified: bool,
}
