//! 绑骨只操作当前项目预览中的完整 Scene0，其他页面和模态沿用编辑器输入门。

use std::path::Path;

use bevy::{ecs::system::SystemParam, gltf::Gltf, prelude::*, window::PrimaryWindow};

use super::{
    super::{AssetPreviewWorkspace, DocumentState, types::*},
    resources::{RiggingState, Session},
    types::*,
};
use crate::{
    resources::*,
    ui::{
        sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};

#[derive(SystemParam)]
struct ReferenceContext<'w> {
    media: Option<Res<'w, AssetPreviewWorkspace>>,
    preview: Option<Res<'w, AssetPreviewState>>,
    documents: Option<Res<'w, DocumentState>>,
    workspace: Option<Res<'w, VegetationPresetEditorState>>,
}

impl ReferenceContext<'_> {
    fn matches(&self, session: &Session) -> bool {
        let (Some(media), Some(preview), Some(documents), Some(workspace)) = (
            self.media.as_deref(), self.preview.as_deref(), self.documents.as_deref(), self.workspace.as_deref(),
        ) else { return false };
        let source = &session.source;
        session.reference_mode()
            && media.active(workspace.active_tab)
            && media.expanded.as_ref().is_some_and(|tab| tab.path == source.path)
            && documents.asset_root() == source.root.as_path()
            && preview.selected_path.as_deref() == Some(source.path.as_str())
            && matches!(preview.content, AssetPreviewContent::Model { phase: AssetPreviewPhase::Ready, .. })
            && preview.model.as_ref().is_some_and(|model| {
                model.generation == source.generation && model.target == AssetPreviewModelTarget::Scene0
            })
    }
}

/// 左侧文件行进入绑骨选引用时，普通资产选择仍指向当前工作对象。
#[derive(SystemParam)]
pub(crate) struct RigReferenceInput<'w> {
    state: Option<ResMut<'w, RiggingState>>,
    context: ReferenceContext<'w>,
}

impl RigReferenceInput<'_> {
    pub(crate) fn select(&mut self, path: &str) -> bool {
        let Some(state) = self.state.as_deref_mut().filter(|state| state.panel_open) else { return false };
        let busy = state.work.is_some();
        let marker_drag = state.owns_mouse();
        let Some(session) = state.session.as_mut().filter(|session| self.context.matches(session)) else {
            return false;
        };
        // 检测期间只暂存点击。生成期间冻结引用，继续吞掉双击，避免打开文件中断绑定。
        if !marker_drag && (!busy || matches!(session.notice, Notice::Inspecting)) {
            session.choose_reference(path);
        }
        true
    }
}

#[derive(SystemParam)]
pub(crate) struct RigReferenceRows<'w> {
    state: Option<Res<'w, RiggingState>>,
    context: ReferenceContext<'w>,
}

impl RigReferenceRows<'_> {
    pub(crate) fn active(&self) -> bool {
        self.state.as_deref().filter(|state| state.panel_open)
            .and_then(|state| state.session.as_ref()).is_some_and(|session| self.context.matches(session))
    }

    pub(crate) fn selected<'a>(&'a self, ordinary: Option<&'a str>) -> Option<&'a str> {
        let session = self.state.as_deref().filter(|state| state.panel_open)
            .and_then(|state| state.session.as_ref()).filter(|session| self.context.matches(session));
        session.map_or(ordinary, |session| session.reference_path.as_deref())
    }
}

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
    editor: Res<'w, SceneEditorState>,
    sidebar: Res<'w, SceneEditorSidebarState>,
    gltfs: Option<Res<'w, Assets<Gltf>>>,
    layout: Option<Res<'w, SceneEditorLayoutState>>,
    settings: Option<Res<'w, SceneEditorSettingsState>>,
    browser: Option<Res<'w, SceneBrowserState>>,
    deletion: Option<Res<'w, SceneDeletionTask>>,
    conflict: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
    scene_drag: Option<Res<'w, SceneBrowserDragState>>,
    numeric: Option<Res<'w, feathrise_editor_ui::ActiveNumericInputScrub>>,
    host_overlay: Option<Res<'w, feathrise_editor_core::EditorHostOverlay>>,
    pub mouse_guard: Option<Res<'w, SceneEditorNumericScrubMouseGuard>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

impl Context<'_, '_> {
    pub(super) fn asset_root(&self) -> &Path {
        self.documents.asset_root()
    }

    pub(super) fn can_pick_project_animation(&self, file_window: bool) -> bool {
        self.editor.open
            && !self.editor.close_requested
            && self.editor.pending_scene_transition.is_none()
            && (file_window || self.sidebar.assets_browser_visible())
            && (file_window || self.layout.as_ref().is_none_or(|layout| layout.content_shown()))
            && (file_window || self.mouse_guard.as_ref().is_none_or(|guard| !guard.blocked()))
            && self.can_input()
    }

    pub(super) fn central(&self) -> bool {
        self.media.active(self.workspace.active_tab)
    }

    pub(super) fn active_tab_id(&self) -> Option<u64> {
        self.media.expanded.as_ref().filter(|_| self.central()).map(|tab| tab.id)
    }

    pub(super) fn rigging_sidebar_shown(&self) -> bool {
        self.sidebar.right == EditorRightSidebarTab::Rigging
            && self.inspector_shown()
    }

    pub(super) fn owns_properties(&self) -> bool {
        super::super::gltf_tree::owns_properties(
            &self.media,
            self.workspace.active_tab,
            self.editor.selected_asset.as_deref(),
        )
    }

    pub(super) fn motion_properties_shown(&self) -> bool {
        self.sidebar.right == EditorRightSidebarTab::Assets
            && self.inspector_shown()
            && self.owns_properties()
    }

    pub(super) fn inspector_shown(&self) -> bool {
        self.layout.as_ref().is_none_or(|layout| layout.inspector_shown())
    }

    pub(super) fn candidate(&self) -> Option<Candidate<'_>> {
        let path = self.preview.selected_path.as_deref()?;
        let root = self.documents.asset_root();
        if root.as_os_str().is_empty()
            || !self.editor.open
            || self.editor.close_requested
            || self.editor.pending_scene_transition.is_some()
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
                && self.inspector_shown()
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
        !self.editor.new_scene_dialog_open
            && self.editor.scene_switch_confirmation.is_none()
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
                .scene_drag
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
