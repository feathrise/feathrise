//! 普通作者验收的状态、当前范围与动作身份；历史RPG报告不作为本轮依据。

use bevy::prelude::Entity;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Domain {
    Surface,
}

impl Domain {
    pub(super) const ALL: [Self; 1] = [Self::Surface];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Surface => "地表",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PresetProof {
    pub(super) domain: Domain,
    pub(super) stable_id: String,
    pub(super) name_before: String,
    pub(super) name_after: String,
    pub(super) expected_name: Option<String>,
    pub(super) catalog_hash_before: String,
    pub(super) catalog_hash_after: String,
    pub(super) author_revision_before: u64,
    pub(super) author_revision_after: u64,
    pub(super) original_text_field_used: bool,
    pub(super) original_save_button_used: bool,
    pub(super) write_confirmed: bool,
    pub(super) tab_closed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AuthoringReport {
    pub(super) schema: u32,
    pub(super) scope: &'static str,
    pub(super) mode: &'static str,
    pub(super) project_root: PathBuf,
    pub(super) asset_root: PathBuf,
    pub(crate) complete: bool,
    pub(crate) error: Option<String>,
    pub(super) presets: Vec<PresetProof>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Phase {
    Waiting,
    Select,
    Open,
    WaitOpen,
    Text,
    WaitBuffer,
    WaitCommit,
    Save,
    WaitSave,
    Close,
    WaitClose,
    Complete,
    Failed,
}

pub(crate) struct Pulse {
    pub(super) entity: Entity,
    pub(super) synthetic: bool,
}

pub(super) struct CatalogFact {
    pub id: String,
    pub name: String,
    pub hash: String,
}
pub(super) struct TabFact {
    pub id: u64,
    pub name: String,
    pub hash: String,
    pub revision: u64,
    pub dirty: bool,
    pub writing: bool,
}
