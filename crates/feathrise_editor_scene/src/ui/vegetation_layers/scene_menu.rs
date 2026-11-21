//! 统一树植被命令只作用于冻结的作者层，参与与锁定复用作者事务，眼睛只写编辑器会话。

use crate::{
    authoring::{EditorAuthoringSession, VegetationBrushStroke},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    types::EditorAction,
    ui::{
        scene_context_menu::{MenuContext, SceneContextMenuState, SceneMenuAction, SceneObjectKey, SceneObjectRow},
        scene_layers::VegetationRowToggle,
        scene_sidebar::TreeRowToggle,
    },
};
use bevy::prelude::*;
use feathrise_editor_ui::EditorButtonClick;

enum LayerAction {
    Authoring(EditorAction),
    Visibility,
}

pub(in crate::ui) fn input(
    menu: Res<SceneContextMenuState>,
    mut authoring: ParamSet<(MenuContext, ResMut<EditorAuthoringSession>)>,
    mut editor: ResMut<SceneEditorState>,
    stroke: Res<VegetationBrushStroke>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    buttons: Query<(&EditorButtonClick, &VegetationRowToggle, &ChildOf), Changed<EditorButtonClick>>,
    rows: Query<&SceneObjectRow>,
    mut allocation: Option<ResMut<super::VegetationBlankLayerAllocation>>,
) {
    if stroke.active || guard.blocked() { return; }
    let button = buttons.iter().find(|(click, ..)| click.is_clicked());
    let inline = if let Some((_, toggle, parent)) = button
        && !menu.owns_mouse() && authoring.p0().enabled(&editor)
    {
        rows.get(parent.parent()).ok().and_then(|row| row.0.as_ref()).and_then(|target| {
            let action = match toggle.0 {
                TreeRowToggle::Participation if !target.locked => LayerAction::Authoring(EditorAction::VegetationToggleEnabled),
                TreeRowToggle::Participation => return None,
                TreeRowToggle::Visibility => LayerAction::Visibility,
                TreeRowToggle::Lock => LayerAction::Authoring(EditorAction::VegetationToggleLocked),
            };
            Some((target, action))
        })
    } else { None };
    let menu_action = menu.ready.as_ref().and_then(|request| {
        if !request.target.allows(request.action) { return None; }
        let action = match request.action {
            SceneMenuAction::Copy => LayerAction::Authoring(EditorAction::VegetationDuplicateLayer),
            SceneMenuAction::Delete => LayerAction::Authoring(EditorAction::VegetationDeleteSelected),
            SceneMenuAction::Visibility => LayerAction::Visibility,
            SceneMenuAction::Lock => LayerAction::Authoring(EditorAction::VegetationToggleLocked),
            _ => return None,
        };
        Some((&request.target, action))
    });
    let Some((target, action)) = menu_action.or(inline) else { return; };
    let mut session = authoring.p1();
    if !target.current(&editor, &session) {
        return;
    }
    let SceneObjectKey::Vegetation {
        index,
        output: false,
        id,
    } = &target.key
    else {
        return;
    };
    let LayerAction::Authoring(action) = action else {
        let draft = session.draft.as_ref().unwrap();
        let terrain = draft.terrain_documents.active.as_ref().map(|canvas| &canvas.definition.id);
        let hidden = editor.vegetation_visibility.toggle(&target.scene_id, terrain, id);
        editor.status = format!("植被图层“{}”已在编辑器{}",
            target.name, if hidden { "隐藏" } else { "显示" });
        if terrain.is_some() && draft.vegetation.as_ref().and_then(|asset| asset.authoring.as_ref())
            .is_some_and(|authoring| {
                let output = &authoring.layers[*index].output_layer_id;
                authoring.layers.iter().any(|layer| &layer.id != id && &layer.output_layer_id == output)
            })
        {
            editor.status.push_str("；同一输出已有的旧三维样本共用来源，维持原显示");
        }
        return;
    };
    editor.selected_vegetation_layer = *index;
    if allocation.as_ref().is_some_and(|state| state.busy()) {
        editor.status = "植被作者层事务仍在后台完成，请稍候".into();
        return;
    }
    if let Some(allocation) = allocation.as_deref_mut() {
        match super::allocation::request_model_layer(action, &mut editor, &session, allocation) {
            Ok(true) => return,
            Err(error) => { editor.status = error; return; }
            Ok(false) => {}
        }
    }
    match super::operations::apply_vegetation_authoring_action(
        action,
        &mut editor,
        &mut session,
        None,
    ) {
        Ok(outcome) => editor.status = outcome.status,
        Err(error) => editor.status = error,
    }
}
