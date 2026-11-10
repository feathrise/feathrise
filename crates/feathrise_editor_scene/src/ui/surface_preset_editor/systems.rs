//! 地表作者的薄宿主：原阶段、唯一工作区、消息游标和延迟命令仍由场景调度拥有。
use crate::{
    authoring::{EditorAuthoringSession, SurfaceBrushStroke, SurfaceLayerEditTask},
    resources::{SceneBrowserState, SceneEditorState},
    ui::vegetation_preset_editor::{VegetationPresetEditorState, edit_shared_workspace},
};
use bevy::prelude::*;
use feathrise_editor_core::{
    EditorStatusKind, EditorStatusOperation, EditorStatusTarget, PresetWorkspaceTab,
};
use feathrise_editor_surface_model::{SurfaceEditorExtension, SurfaceEditorHost, system_params::*};
use feathrise_scene_content::{SurfacePresetDescriptor, SurfacePresetRegistry};

struct Host<'w, 's> {
    editor: ResMut<'w, SceneEditorState>,
    session: Option<&'s EditorAuthoringSession>,
    surface_busy: bool,
    fallback: PresetWorkspaceTab,
}
// 读宿主事实不标记 Changed；只有原本写状态或选择的动作才解引用写入。
impl SurfaceEditorHost for Host<'_, '_> {
    fn is_open(&self) -> bool {
        self.editor.open
    }
    fn surface_busy(&self) -> bool {
        self.surface_busy
    }
    fn fallback_tab(&self) -> PresetWorkspaceTab {
        self.fallback
    }
    fn selected_asset(&self) -> Option<&str> {
        self.editor.selected_asset.as_deref()
    }
    fn selected_preset(&self) -> Option<&str> {
        self.editor.surface_selected_preset_id.as_deref()
    }
    fn set_selected_preset(&mut self, value: Option<String>) {
        self.editor.surface_selected_preset_id = value;
    }
    fn set_status(&mut self, value: String) {
        self.editor.status = value;
    }
    fn select_preset(
        &mut self,
        preset: &SurfacePresetDescriptor,
        registry: &SurfacePresetRegistry,
    ) -> Result<String, String> {
        let session = self
            .session
            .ok_or_else(|| "场景作者会话尚未就绪".to_owned())?;
        super::selection::select_preset(&mut self.editor, session, preset, registry)
    }
    fn draft_references(&self, names: &std::collections::BTreeSet<&str>) -> bool {
        self.session
            .and_then(|s| s.draft.as_ref())
            .and_then(|d| d.surface.as_ref())
            .is_some_and(|surface| {
                surface
                    .layers
                    .iter()
                    .any(|layer| names.contains(layer.preset_id.as_str()))
            })
    }
    fn record_operation_result(
        &mut self,
        operation: EditorStatusOperation,
        target: &EditorStatusTarget,
        kind: EditorStatusKind,
    ) {
        self.editor.record_operation_result(operation, target, kind);
    }
    fn record_operation_feedback(
        &mut self,
        operation: EditorStatusOperation,
        target: &EditorStatusTarget,
        kind: EditorStatusKind,
        detail: std::sync::Arc<str>,
    ) {
        self.editor
            .record_operation_feedback(operation, target, kind, detail);
    }
}

pub(super) fn handle_buttons(
    extension: Option<Res<SurfaceEditorExtension>>,
    editor: ResMut<SceneEditorState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    session: ResMut<EditorAuthoringSession>,
    surface_work: (Res<SurfaceBrushStroke>, Res<SurfaceLayerEditTask>),
    params: ButtonsParams,
) {
    let Some(extension) = extension else {
        return;
    };
    let fallback = workspace
        .tabs
        .last()
        .map_or(PresetWorkspaceTab::Scene, |tab| {
            PresetWorkspaceTab::Vegetation(tab.id)
        });
    let mut host = Host {
        editor,
        session: Some(&session),
        surface_busy: surface_work.0.active || surface_work.1.is_pending(),
        fallback,
    };
    edit_shared_workspace(&mut workspace, |workspace| {
        (extension.buttons)(&mut host, workspace, params)
    });
}
pub(super) fn handle_text_input(
    extension: Option<Res<SurfaceEditorExtension>>,
    editor: Res<SceneEditorState>,
    browser: Option<Res<SceneBrowserState>>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    params: TextParams,
) {
    let Some(extension) = extension else {
        return;
    };
    let modal = editor.new_scene_dialog_open || browser.as_ref().is_some_and(|b| b.dialog.is_some());
    edit_shared_workspace(&mut workspace, |workspace| {
        (extension.text)(editor.open, modal, workspace, params)
    });
}
pub(super) fn poll_catalog_write(
    extension: Option<Res<SurfaceEditorExtension>>,
    editor: ResMut<SceneEditorState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    params: PersistenceParams,
) {
    let Some(extension) = extension else {
        return;
    };
    let mut host = Host {
        editor,
        session: None,
        surface_busy: false,
        fallback: PresetWorkspaceTab::Scene,
    };
    edit_shared_workspace(&mut workspace, |workspace| {
        (extension.persistence)(&mut host, workspace, params)
    });
}
pub(super) fn sync_panel(
    extension: Option<Res<SurfaceEditorExtension>>,
    editor: Res<SceneEditorState>,
    params: PanelParams,
) {
    if let Some(extension) = extension {
        (extension.panel)(editor.surface_selected_preset_id.as_deref(), params);
    }
}
pub(super) fn sync_button_styles(
    extension: Option<Res<SurfaceEditorExtension>>,
    editor: Res<SceneEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    params: StylesParams,
) {
    if let Some(extension) = extension {
        (extension.styles)(
            editor.surface_selected_preset_id.as_deref(),
            &workspace.workspace,
            params,
        );
    }
}
pub(super) fn sync_document_badge(
    extension: Option<Res<SurfaceEditorExtension>>,
    workspace: Res<VegetationPresetEditorState>,
    params: BadgeParams,
) {
    if let Some(extension) = extension {
        (extension.badge)(&workspace.workspace, params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::SystemState;

    #[test]
    fn host_reads_preserve_change_detection_and_writes_keep_the_original_resource() {
        let mut world = World::new();
        world.init_resource::<SceneEditorState>();
        let mut system = SystemState::<ResMut<SceneEditorState>>::new(&mut world);
        world.clear_trackers();
        {
            let host = Host {
                editor: system.get_mut(&mut world).unwrap(),
                session: None,
                surface_busy: false,
                fallback: PresetWorkspaceTab::Scene,
            };
            assert!(!host.is_open());
            assert!(host.selected_asset().is_none());
            assert!(host.selected_preset().is_none());
            assert!(!host.draft_references(&default()));
        }
        assert!(
            !world
                .get_resource_ref::<SceneEditorState>()
                .unwrap()
                .is_changed()
        );
        {
            let mut host = Host {
                editor: system.get_mut(&mut world).unwrap(),
                session: None,
                surface_busy: false,
                fallback: PresetWorkspaceTab::Scene,
            };
            host.set_status("地表宿主回执".into());
        }
        let editor = world.get_resource_ref::<SceneEditorState>().unwrap();
        assert!(editor.is_changed());
        assert_eq!(editor.status, "地表宿主回执");
    }
}
