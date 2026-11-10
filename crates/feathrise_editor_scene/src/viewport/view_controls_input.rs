//! 观察控件可在元素选择期间使用，但必须让位于模态、外部菜单和已取得所有权的手势。
use crate::{
    authoring::{
        EditorAuthoringSession, TerrainExternalChangeState, mesh_edit::MeshEditExternalOwners,
    },
    resources::{
        SceneBrowserDragState, SceneBrowserState, SceneDeletionTask, SceneEditorLayoutState,
        SceneEditorSettingsState, SceneEditorState,
    },
    ui::{
        courtyard::{CourtyardUiState, CourtyardViewportMode},
        placement_transform::PlacementTransformInputState,
        text_input::{EditorTextInput, EditorTextInputGuard},
        vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
    },
};
use bevy::{ecs::system::SystemParam, input_focus::InputFocus, prelude::*, window::PrimaryWindow};
use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub};

#[derive(SystemParam)]
pub(crate) struct ViewControlsInput<'w, 's> {
    external: MeshEditExternalOwners<'w>,
    workspace: Option<Res<'w, VegetationPresetEditorState>>,
    settings: Option<Res<'w, SceneEditorSettingsState>>,
    folders: Option<Res<'w, SceneBrowserState>>,
    scene_drag: Option<Res<'w, SceneBrowserDragState>>,
    conflict: Option<Res<'w, TerrainExternalChangeState>>,
    deletion: Option<Res<'w, SceneDeletionTask>>,
    courtyard: Option<Res<'w, CourtyardUiState>>,
    transform: Option<Res<'w, PlacementTransformInputState>>,
    layout: Option<Res<'w, SceneEditorLayoutState>>,
    session: Option<Res<'w, EditorAuthoringSession>>,
    numeric: Option<Res<'w, ActiveNumericInput>>,
    scrub: Option<Res<'w, ActiveNumericInputScrub>>,
    text: Option<Res<'w, EditorTextInputGuard>>,
    focus: Option<Res<'w, InputFocus>>,
    fields: Query<'w, 's, (), With<EditorTextInput>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Resource, Default)]
    struct Available(bool);
    fn check(
        input: ViewControlsInput,
        editor: Res<SceneEditorState>,
        mut available: ResMut<Available>,
    ) {
        available.0 = input.available(&editor);
    }
    #[test]
    fn modal_numeric_focus_and_window_focus_block_view_controls_without_losing_the_editor_session()
    {
        let mut app = App::new();
        app.insert_resource(SceneEditorState {
            open: true,
            ..default()
        })
        .init_resource::<Available>()
        .init_resource::<SceneEditorSettingsState>()
        .init_resource::<ActiveNumericInput>()
        .add_systems(Update, check);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.update();
        assert!(app.world().resource::<Available>().0);
        app.world_mut()
            .resource_mut::<SceneEditorSettingsState>()
            .open = true;
        app.update();
        assert!(!app.world().resource::<Available>().0);
        app.world_mut()
            .resource_mut::<SceneEditorSettingsState>()
            .open = false;
        app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(Entity::PLACEHOLDER);
        app.update();
        assert!(!app.world().resource::<Available>().0);
        app.world_mut().resource_mut::<ActiveNumericInput>().entity = None;
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert!(!app.world().resource::<Available>().0);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert!(app.world().resource::<Available>().0);
        assert!(app.world().resource::<SceneEditorState>().open);
    }
}
impl ViewControlsInput<'_, '_> {
    pub(super) fn available(&self, editor: &SceneEditorState) -> bool {
        editor.open
            && !editor.close_requested
            && !editor.new_scene_dialog_open
            && !editor.suppress_world_pick_this_frame
            && !editor.scene_transition_in_progress
            && editor.pending_scene_transition.is_none()
            && editor.scene_switch_confirmation.is_none()
            && !self.external.owns_mouse()
            && self
                .workspace
                .as_ref()
                .is_none_or(|s| s.active_tab == PresetWorkspaceTab::Scene)
            && self
                .settings
                .as_ref()
                .is_none_or(|s| !s.open && !s.owns_mouse_this_frame)
            && self.folders.as_ref().is_none_or(|s| s.dialog.is_none())
            && self
                .scene_drag
                .as_ref()
                .is_none_or(|s| !s.blocks_other_mouse())
            && self.conflict.as_ref().is_none_or(|s| !s.modal_open())
            && self.deletion.as_ref().is_none_or(|s| !s.busy())
            && self
                .courtyard
                .as_ref()
                .is_none_or(|s| s.mode == CourtyardViewportMode::Idle)
            && self
                .transform
                .as_ref()
                .is_none_or(|s| !s.relocation_armed())
            && self
                .layout
                .as_ref()
                .is_none_or(|s| s.drag.is_none() && !s.owns_mouse_this_frame)
            && self.session.as_ref().is_none_or(|s| !s.saving)
            && self.numeric.as_ref().is_none_or(|s| s.entity.is_none())
            && self.scrub.as_ref().is_none_or(|s| s.entity.is_none())
            && self.text.as_ref().is_none_or(|s| !s.block_shortcuts)
            && !self
                .focus
                .as_ref()
                .and_then(|s| s.get())
                .is_some_and(|entity| self.fields.contains(entity))
            && self.windows.single().map_or(true, |window| window.focused)
    }
}
