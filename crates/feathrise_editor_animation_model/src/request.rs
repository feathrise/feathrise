//! 工程动作请求、源身份与作者会话待选片段。

use bevy::prelude::*;
use feathrise_editor_assets::files::safe_asset_path;
use feathrise_editor_core::PresetWorkspaceTab;
use std::path::{Path, PathBuf};

use super::{media::{AssetPreviewState, AssetPreviewWorkspace, AssetPreviewModelTarget}, resources::AnimationEditorState};

#[derive(Message, Clone)]
pub struct PreviewRigAnimation {
    pub model_path: String,
    pub animation_path: String,
    pub clip_index: usize,
    pub host_path: String,
    pub host_generation: u64,
    pub host_tab: u64,
    pub asset_root: PathBuf,
    pub recheck_source: bool,
}

impl PreviewRigAnimation {
    pub fn matches(
        &self,
        preview: &AssetPreviewState,
        media: &AssetPreviewWorkspace,
        tab: PresetWorkspaceTab,
        asset_root: &Path,
    ) -> bool {
        self.asset_root == asset_root
            && media.active(tab)
            && media.expanded.as_ref().is_some_and(|entry| {
                entry.id == self.host_tab && entry.path == self.host_path
            })
            && preview.selected_path.as_deref() == Some(self.host_path.as_str())
            && preview.external_preview.is_none()
            && preview.model.as_ref().is_some_and(|model| {
                model.generation == self.host_generation && model.target == AssetPreviewModelTarget::Scene0
            })
    }

    pub fn same_assets(&self, other: &Self) -> bool {
        self.host_tab == other.host_tab
            && self.host_path == other.host_path
            && self.host_generation == other.host_generation
            && self.asset_root == other.asset_root
            && self.model_path == other.model_path
            && self.animation_path == other.animation_path
    }
}

pub struct RigPreviewRequest {
    pub ticket: u64,
    pub source: PathBuf,
    pub request: PreviewRigAnimation,
    pub failed: bool,
}

pub fn checked_clip_path(root: &Path, path: &str) -> Result<PathBuf, String> {
    let path = safe_asset_path(root, path)?;
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
    {
        return Err("动作预览需要工程资产中的 GLB 文件".into());
    }
    Ok(path)
}

pub struct DirectoryPick {
    pub root: PathBuf,
    pub source: PathBuf,
    pub tab: u64,
    pub clip_index: usize,
    pub recheck_source: bool,
}

impl AnimationEditorState {
    pub fn queue_directory_pick(
        &mut self, root: PathBuf, source: PathBuf, tab: u64, clip_index: usize,
    ) {
        self.stop_rig_preview();
        let recheck_source = self.sessions.contains_key(&source)
            || self.work.as_ref().is_some_and(|work| work.path == source);
        if self.failed.as_ref() == Some(&source) {
            self.failed = None;
        }
        self.directory_pick = Some(DirectoryPick { root, source, tab, clip_index, recheck_source });
    }

    pub fn cancel_directory_pick(&mut self) {
        self.directory_pick = None;
    }
}
