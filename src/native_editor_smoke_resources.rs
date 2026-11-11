//! 显式原生窗口启动验收的有界状态；正常编辑会话不安装这些资源。

use bevy::prelude::*;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Resource)]
pub(super) struct SmokePlan {
    pub started: Instant,
    pub frames: u64,
    pub stable_frames: u16,
    pub scene_requested: bool,
    pub scene_error: Option<String>,
    pub preview_ready: bool,
    pub author_status: String,
    pub save_fixture: bool,
    pub save_requested: bool,
    pub save_error: Option<String>,
    pub save_proof: Option<feathrise_editor_scene::standalone::validation_fixture::SavedEditProof>,
    pub current_author_document: Option<serde_json::Value>,
    pub font: Handle<Font>,
    pub directory: PathBuf,
    pub project_root: PathBuf,
    pub asset_root: PathBuf,
    pub screenshot_requested: bool,
    pub capture_started: Option<Instant>,
    pub run_saved_play: bool,
    pub play_requested: bool,
    pub play_completed_before: u64,
    pub play_result: Option<Result<(), String>>,
    pub cancel_requested: bool,
    pub completion: Arc<Mutex<Option<Result<(), String>>>>,
    pub close_fixture: bool,
    pub recovery_ready: bool,
    pub f12_release_pending: bool,
    pub recovery_report:
        Option<Result<crate::native_play_recovery_validation::RecoveryReport, String>>,
}
