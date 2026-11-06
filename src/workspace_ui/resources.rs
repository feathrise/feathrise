//! 工程页快照和表单现场；模型更新不覆盖正在输入的名称或目录。

use super::{
    Page, RecentEntry, WorkspaceField, WorkspaceProject,
    types::{ProbeButton, ProbeField},
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// 关于持有浮层输入租约。许可与概览在同一个面板内切换。
#[derive(Resource, Default)]
pub(crate) struct AboutState {
    pub(super) open: bool,
    pub(super) captured: bool,
    pub(super) license_page: Option<usize>,
}

impl AboutState {
    pub(crate) fn captures_input(&self) -> bool { self.captured }
}

/// 显式验收只拥有本轮无工程窗口和独占取证目录，普通构建不安装自动输入。
#[cfg(feature = "validation")]
#[derive(Resource)]
pub(super) struct AboutValidation {
    pub directory: std::path::PathBuf,
    pub started: std::time::Instant,
    pub step: u8,
    pub frames: u32,
    pub issued: bool,
    pub pressing: Option<Entity>,
    pub fields: Vec<(String, String)>,
    pub proofs: Vec<serde_json::Value>,
    pub commands: bevy::ecs::message::MessageCursor<super::WorkspaceCommand>,
}

/// 无工程插件管理专项只保存驱动与异步证据回执，不替代生产管理器的任何成功状态。
#[cfg(feature = "validation")]
#[derive(Resource)]
pub(super) struct PluginHostValidation {
    pub directory: std::path::PathBuf,
    pub global: std::path::PathBuf,
    pub started: std::time::Instant,
    pub step: u8,
    pub frames: u32,
    pub mouse_phase: u8,
    pub target: Option<Entity>,
    pub click_seen: bool,
    pub key_phase: u8,
    pub issued: bool,
    pub start_fields: Vec<(String, String)>,
    pub form_fields: Vec<(String, String)>,
    pub proofs: Vec<serde_json::Value>,
    pub captures: Vec<serde_json::Value>,
    pub capture: Option<async_channel::Receiver<Result<serde_json::Value, String>>>,
    pub report: Option<async_channel::Receiver<Result<(), String>>>,
    pub failure: Option<String>,
    pub open_commands: u32,
    pub commands: bevy::ecs::message::MessageCursor<super::WorkspaceCommand>,
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct WorkspaceModel {
    pub project: Option<WorkspaceProject>,
    pub default_directory: String,
    pub recent: Vec<RecentEntry>,
    pub busy: bool,
    pub cancellable: bool,
    pub status: String,
    pub revision: u64,
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct WorkspaceProbe {
    pub page: Page,
    pub project: Option<WorkspaceProject>,
    pub status: String,
    pub buttons: Vec<ProbeButton>,
    pub fields: Vec<ProbeField>,
}

#[derive(Resource)]
pub(super) struct Presentation {
    pub page: Page,
    pub start_open: bool,
    pub new_open: bool,
    pub open_open: bool,
    pub menu: bool,
    pub menu_releasing: bool,
    pub name: String,
    pub game_view: feathrise_runtime::GameView,
    pub parent: String,
    pub open_path: String,
    pub default_directory: String,
    pub applied_default: Option<String>,
    pub project: Option<WorkspaceProject>,
    pub recent: Vec<RecentEntry>,
    pub recent_revision: u64,
    pub native_tab_revision: u64,
    pub error: String,
    pub fills: Vec<(WorkspaceField, String)>,
}

impl Default for Presentation {
    fn default() -> Self {
        Self {
            page: Page::Start,
            start_open: true,
            new_open: false,
            open_open: false,
            menu: false,
            menu_releasing: false,
            name: "新工程".into(),
            game_view: Default::default(),
            parent: String::new(),
            open_path: String::new(),
            default_directory: String::new(),
            applied_default: None,
            project: None,
            recent: Vec::new(),
            recent_revision: 0,
            native_tab_revision: 0,
            error: String::new(),
            fills: Vec::new(),
        }
    }
}
