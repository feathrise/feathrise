//! 绑骨只操作当前项目预览中的完整 Scene0，其他页面和模态沿用编辑器输入门。

use std::path::Path;

use bevy::{ecs::system::SystemParam, gltf::Gltf, prelude::*, window::PrimaryWindow};

use super::{
    super::{AssetPreviewWorkspace, DocumentState, types::*},
    types::*,
};
use crate::{
    resources::*,
    ui::{
        sidebars::{EditorRightSidebarTab, MapEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};

#[derive(Clone, Copy)]
pub(super) struct Candidate<'a> {
    pub root: &'a Path,
    pub path: &'a str,
    pub generation: u64,
    pub entry: Entry,
}

impl Candidate<'_> {
    pub fn key(self) -> SourceKey {
        SourceKey {
            root: self.root.to_path_buf(),
            path: self.path.to_owned(),
            generation: self.generation,
        }
    }

    pub fn same_file(self, key: &SourceKey) -> bool {
        self.root == key.root && self.path == key.path
    }

    pub fn same_generation(self, key: &SourceKey) -> bool {
        self.same_file(key) && self.generation == key.generation
    }
}

#[derive(SystemParam)]
pub(super) struct Context<'w, 's> {
    pub preview: Res<'w, AssetPreviewState>,
    documents: Res<'w, DocumentState>,
    media: Res<'w, AssetPreviewWorkspace>,
    workspace: Res<'w, VegetationPresetEditorState>,
    editor: Res<'w, MapEditorState>,
    sidebar: Res<'w, MapEditorSidebarState>,
    gltfs: Option<Res<'w, Assets<Gltf>>>,
    layout: Option<Res<'w, MapEditorLayoutState>>,
    settings: Option<Res<'w, MapEditorSettingsState>>,
    browser: Option<Res<'w, MapBrowserState>>,
    deletion: Option<Res<'w, MapDeletionTask>>,
    conflict: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
    map_drag: Option<Res<'w, MapBrowserDragState>>,
    numeric: Option<Res<'w, feathrise_editor_ui::ActiveNumericInputScrub>>,
    host_overlay: Option<Res<'w, feathrise_editor_core::EditorHostOverlay>>,
    pub mouse_guard: Option<Res<'w, MapEditorNumericScrubMouseGuard>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

impl Context<'_, '_> {
    pub(super) fn central(&self) -> bool {
        self.media.active(self.workspace.active_tab)
    }

    pub(super) fn candidate(&self) -> Option<Candidate<'_>> {
        let path = self.preview.selected_path.as_deref()?;
        let root = self.documents.asset_root();
        if root.as_os_str().is_empty()
            || !self.editor.open
            || self.editor.close_requested
            || self.editor.pending_map_transition.is_some()
            || self.preview.external_preview.is_some()
            || !matches!(
                self.preview.content,
                AssetPreviewContent::Model {
                    phase: AssetPreviewPhase::Ready,
                    ..
                }
            )
            || !Path::new(path)
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
        {
            return None;
        }
        // 中央活动标签的路径可以独立于侧栏选择；隐藏的模型不能继续绑骨或采用迟到结果。
        let visible = if self.central() {
            self.media
                .expanded
                .as_ref()
                .is_some_and(|tab| tab.path == path)
        } else {
            self.sidebar.right == EditorRightSidebarTab::Assets
                && self
                    .layout
                    .as_ref()
                    .is_none_or(|layout| layout.inspector_shown())
                && self.editor.selected_asset.as_deref() == Some(path)
        };
        if !visible {
            return None;
        }
        let model = self.preview.model.as_ref()?;
        if model.target != AssetPreviewModelTarget::Scene0 || model.scene_mesh_count == 0 {
            return None;
        }
        let gltf = self.gltfs.as_ref()?.get(&model.catalog.gltf)?;
        let entry = if gltf.skins.is_empty() {
            Entry::Unskinned
        } else {
            Entry::Skinned
        };
        Some(Candidate {
            root,
            path,
            generation: model.generation,
            entry,
        })
    }

    pub(super) fn can_input(&self) -> bool {
        !self.editor.new_map_dialog_open
            && self.editor.map_switch_confirmation.is_none()
            && self.settings.as_ref().is_none_or(|settings| !settings.open)
            && self
                .browser
                .as_ref()
                .is_none_or(|browser| browser.dialog.is_none())
            && self
                .deletion
                .as_ref()
                .is_none_or(|deletion| deletion.confirmation.is_none())
            && self
                .conflict
                .as_ref()
                .is_none_or(|conflict| !conflict.modal_open())
            && self
                .map_drag
                .as_ref()
                .is_none_or(|drag| !drag.blocks_other_mouse() && drag.press.is_none())
            && self
                .numeric
                .as_ref()
                .is_none_or(|numeric| numeric.entity.is_none())
            && self
                .host_overlay
                .as_ref()
                .is_none_or(|overlay| !overlay.active)
            && self.windows.single().is_ok_and(|window| window.focused)
    }
}
