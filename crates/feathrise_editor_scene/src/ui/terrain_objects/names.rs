//! 对象名称复用官方中文输入；绑定身份切换时保留控件样式，确认后进入完整集合历史。

use feathrise_editor_ui::EditorButtonClick;
use bevy::{
    ecs::system::SystemParam,
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, TextEdit},
    ui::InteractionDisabled,
};

use super::{
    components::{Action, ActionButton, NameInput},
    resources::{NameInputBinding, TerrainObjectsEditorState},
    systems,
    types::Mutation,
};
use crate::{
    authoring::EditorAuthoringSession, resources::SceneEditorState,
    ui::text_input::EditorTextInputGuard,
};
use feathrise_editor_ui::{ActiveNumericInput, NumericInputSetValue, NumericInputValue};

#[derive(SystemParam)]
pub(super) struct NamePanelGate<'w> {
    sidebars: Option<Res<'w, crate::ui::sidebars::SceneEditorSidebarState>>,
    workspace: super::resources::PanelWorkspaceGate<'w>,
    selection: Option<Res<'w, crate::viewport::LayerCandidateState>>,
    layout: Option<Res<'w, crate::resources::SceneEditorLayoutState>>,
}

impl NamePanelGate<'_> {
    pub(super) fn visible(&self, terrain: &TerrainObjectsEditorState) -> bool {
        self.scene_visible() && self.selection.as_ref().is_none_or(|selection| {
            matches!(selection.selected.as_ref(), Some(crate::viewport::LayerTarget::TerrainObject(id))
                if terrain.selected.as_ref().is_some_and(|terrain| terrain.as_str() == id))
        })
    }
    pub(super) fn creation_visible(&self) -> bool {
        self.sidebars
            .as_ref()
            .is_none_or(|state| state.tool_page_visible(crate::ui::sidebars::EditorLeftSidebarTab::Meshes))
            && self.layout.as_ref().is_none_or(|layout| layout.inspector_shown())
            && self.workspace.creation_visible()
    }
    pub(super) fn field_visible(&self, field: super::components::Field, state: &TerrainObjectsEditorState) -> bool {
        if field.is_creation() { self.creation_visible() } else { self.visible(state) }
    }
    pub(super) fn action_visible(&self, action: Action, state: &TerrainObjectsEditorState) -> bool {
        if action.creation_action() {
            self.creation_visible()
        } else if action.scene_list_action() { self.tree_visible() } else { self.visible(state) }
    }
    pub(super) fn tree_visible(&self) -> bool {
        self.sidebars.as_ref().is_none_or(|state| state.scene_tree_visible())
            && self.workspace.visible()
            && self.layout.as_ref().is_none_or(|layout| layout.content_shown())
    }
    pub(super) fn scene_visible(&self) -> bool {
        self.sidebars.as_ref().is_none_or(|state| {
            state.right == crate::ui::sidebars::EditorRightSidebarTab::Scene
        }) && self.workspace.visible()
            && self.layout.as_ref().is_none_or(|layout| layout.inspector_shown())
    }
}

/// 中文框接管键盘前通过专业数值框的公开消息收口旧焦点；下一帧恢复名称焦点，避免被公共清理器收走。
pub(super) fn focus(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    state: Res<TerrainObjectsEditorState>,
    numeric: Res<ActiveNumericInput>,
    values: Query<&NumericInputValue>,
    fields: Query<(), With<NameInput>>,
    mut requests: MessageWriter<NumericInputSetValue>,
    mut input_focus: ResMut<InputFocus>,
    mut binding: ResMut<NameInputBinding>,
    gate: NamePanelGate,
) {
    let valid = gate.visible(&state)
        && systems::enabled(&editor, &session, &state)
        && systems::selected(&session, &state)
            .is_some_and(|object| !state.locked.contains(&object.id))
        && binding.target.as_ref().is_some_and(|(map, id)| {
            session
                .draft
                .as_ref()
                .is_some_and(|draft| &draft.scene_id == map)
                && state.selected.as_ref() == Some(id)
        });
    if !valid {
        binding.pending_focus = None;
        return;
    }
    if let Some((entity, map, id)) = binding.pending_focus.as_ref()
        && binding
            .target
            .as_ref()
            .is_some_and(|target| target == &(map.clone(), id.clone()))
        && numeric.entity.is_none()
        && fields.contains(*entity)
    {
        input_focus.set(*entity, FocusCause::Navigated);
        binding.pending_focus = None;
    }
    if let Some(entity) = input_focus.get().filter(|entity| fields.contains(*entity))
        && let Some(number) = numeric.entity
        && let Ok(value) = values.get(number)
    {
        requests.write(NumericInputSetValue {
            entity: number,
            value: value.0,
            cancel_edit: true,
        });
        let (map, id) = binding.target.as_ref().expect("已核对名称绑定");
        binding.pending_focus = Some((entity, map.clone(), id.clone()));
    }
}

pub(super) fn input(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<TerrainObjectsEditorState>,
    binding: Res<NameInputBinding>,
    fields: Query<&EditableText, With<NameInput>>,
    buttons: Query<(&EditorButtonClick, &ActionButton), Changed<EditorButtonClick>>,
    guard: Option<Res<EditorTextInputGuard>>,
    gate: NamePanelGate,
) {
    if !gate.visible(&state)
        || !systems::enabled(&editor, &session, &state)
        || !buttons
            .iter()
            .any(|(pressed, action)| pressed.is_clicked() && action.0 == Action::Rename)
    {
        return;
    }
    let Some(object) = systems::selected(&session, &state) else {
        return;
    };
    let Some(map) = session.draft.as_ref().map(|draft| &draft.scene_id) else {
        return;
    };
    if binding
        .target
        .as_ref()
        .is_none_or(|(bound_scene, id)| bound_scene != map || id != &object.id)
    {
        return; // 列表刚切换而表现绑定尚未更新时，旧名称绝不能提交给新对象。
    }
    let Ok(text) = fields.single() else {
        return;
    };
    if guard.as_ref().is_some_and(|guard| guard.block_shortcuts)
        || text.is_composing()
        || text.pending_paste.is_some()
        || !text.pending_edits.is_empty()
    {
        editor.status = "请先确认网格名称的输入法候选".into();
        return;
    }
    let mutation = Mutation::Rename(object.id.clone(), text.value().to_string());
    systems::start(&mut session, &mut editor, &mut state, mutation);
}

pub(super) fn sync(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    state: Res<TerrainObjectsEditorState>,
    mut binding: ResMut<NameInputBinding>,
    mut focus: ResMut<InputFocus>,
    mut fields: Query<(Entity, &mut EditableText, Has<InteractionDisabled>), With<NameInput>>,
    gate: NamePanelGate,
) {
    let object = systems::selected(&session, &state);
    let target = object
        .zip(session.draft.as_ref())
        .map(|(object, draft)| (draft.scene_id.clone(), object.id.clone()));
    let editable = gate.visible(&state)
        && systems::enabled(&editor, &session, &state)
        && object.is_some_and(|object| !state.locked.contains(&object.id));
    if !editable || binding.target != target {
        binding.pending_focus = None;
    }
    for (entity, mut text, disabled) in &mut fields {
        if !editable && focus.get() == Some(entity) {
            focus.clear();
            if text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()
            {
                let committed = text.value().to_string();
                text.clear();
                text.editor.set_text(&committed);
                text.queue_edit(TextEdit::TextEnd(false));
            } // 隐藏字段释放输入法候选，已确认但尚未应用的名称仍在本对象缓冲中保留。
        }
        if editable && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
        if !editable && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
        let value = object.map_or("", |object| object.display_name.as_str());
        if binding.target != target || binding.observed_name != value {
            if binding.target != target && focus.get() == Some(entity) {
                focus.clear();
            }
            // 原位替换保留官方已同步字体与排版代次，避免中文和光标在重建后消失。
            text.clear();
            text.editor.set_text(value);
            text.queue_edit(TextEdit::TextEnd(false));
        }
    }
    if binding.target != target {
        binding.target = target;
    }
    let name = object.map_or("", |object| object.display_name.as_str());
    if binding.observed_name != name {
        binding.observed_name = name.into();
    }
}
