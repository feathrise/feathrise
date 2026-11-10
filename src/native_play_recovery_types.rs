//! 失败恢复报告与冻结作者证据的普通契约；不承载保存或恢复流程。

use bevy::prelude::Entity;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RecoveryReport {
    pub schema: u32,
    pub scope: &'static str,
    pub complete: bool,
    pub failure_observed: bool,
    pub failed_attempt_started: u64,
    pub failed_attempt_completed: u64,
    pub failed_attempt_error: Option<String>,
    pub descriptor_restored: bool,
    pub same_world_marker: bool,
    pub original_save_proof_preserved: bool,
    pub scene_history_metadata_preserved: bool,
    pub surface_session_preserved: bool,
    pub marker_entity: String,
    pub owner_marker_checked: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SurfaceSession {
    pub tab: u64,
    pub descriptor: feathrise_scene_content::SurfacePresetDescriptor,
    pub baseline: feathrise_scene_content::SurfacePresetDescriptor,
    pub section: feathrise_editor_surface_model::types::SurfaceSection,
    pub close_confirmation: bool,
}

pub(super) struct AuthorSnapshot {
    pub marker: Entity,
    pub saved: Value,
    pub authoring: Value,
    pub surface: SurfaceSession,
    pub scene: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Idle,
    SelectSurface,
    OpenSurface,
    WaitSurface,
    Injecting,
    WaitFailure,
    Restoring,
    WaitScenePreview,
    Ready,
    Failed,
}
