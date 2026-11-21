//! 统一树植被命令只作用于冻结的作者层，参与与锁定复用作者事务，眼睛只写编辑器会话。

use crate::{
    authoring::{EditorAuthoringSession, VegetationBrushStroke, terrain_documents::{TerrainDocumentTask, TreeVegetationAction}},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    types::EditorAction,
    ui::{
        scene_context_menu::{MenuContext, SceneContextMenuState, SceneMenuAction, SceneObjectKey, SceneObjectRow},
        scene_layers::{VegetationCatalogAssets, VegetationRowToggle, request_cold_vegetation, vegetation_source},
        scene_sidebar::TreeRowToggle,
        terrain_objects::TerrainObjectsEditorState,
        water_feature_list::WaterSourceSelectionGuard,
    },
    viewport::LayerCandidateState,
};
use bevy::prelude::*;
use feathrise_editor_ui::EditorButtonClick;

enum LayerAction {
    Authoring(EditorAction),
    Visibility,
}

pub(in crate::ui) fn input(
    menu: Res<SceneContextMenuState>,
    mut authoring: ParamSet<(
        MenuContext,
        (WaterSourceSelectionGuard, Res<EditorAuthoringSession>),
        (ResMut<EditorAuthoringSession>, Option<ResMut<TerrainObjectsEditorState>>,
            Option<ResMut<TerrainDocumentTask>>, Option<ResMut<super::VegetationBlankLayerAllocation>>),
    )>,
    mut editor: ResMut<SceneEditorState>,
    stroke: Res<VegetationBrushStroke>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    buttons: Query<(&EditorButtonClick, &VegetationRowToggle, &ChildOf), Changed<EditorButtonClick>>,
    rows: Query<&SceneObjectRow>,
    catalog: VegetationCatalogAssets,
    candidates: Option<Res<LayerCandidateState>>,
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
    let deferred = authoring.p2().2.as_deref_mut()
        .and_then(|documents| documents.vegetation_ready.take_if(|intent| !intent.action.selects()))
        .filter(|intent| intent.selection_ticket == candidates.as_deref().map(LayerCandidateState::selection_ticket));
    let deferred_action = deferred.as_ref().and_then(|intent| {
        let action = match intent.action {
            TreeVegetationAction::Author(edit) => LayerAction::Authoring(edit),
            TreeVegetationAction::Visibility => LayerAction::Visibility,
            _ => return None,
        };
        Some((&intent.target, action))
    });
    let Some((target, action)) = menu_action.or(inline).or(deferred_action) else { return; };
    let SceneObjectKey::Vegetation {
        index,
        output: false,
        id,
        terrain,
        cold,
    } = &target.key
    else {
        return;
    };
    if cold.is_some() {
        let action = match action {
            LayerAction::Authoring(edit) => TreeVegetationAction::Author(edit),
            LayerAction::Visibility => TreeVegetationAction::Visibility,
        };
        let (session, _, mut documents, _) = authoring.p2();
        if let Some(documents) = documents.as_deref_mut() {
            request_cold_vegetation(target.clone(), &mut editor, &session, &catalog, documents,
                candidates.as_deref(), action);
        }
        return;
    }
    let (action, changed_canvas) = {
        let (source_guard, session) = authoring.p1();
        if !target.current(&editor, &session) { return; }
        let draft = session.draft.as_ref().unwrap();
        let LayerAction::Authoring(action) = action else {
            // 眼睛是编辑器状态，按冻结来源更新即可，不抢走当前画布和选中对象。
            let hidden = editor.vegetation_visibility.toggle(&target.scene_id, terrain.as_ref(), id);
            editor.status = format!("植被图层“{}”已在编辑器{}",
                target.name, if hidden { "隐藏" } else { "显示" });
            if terrain.is_some() && vegetation_source(draft, terrain.as_ref()).and_then(|asset| asset.authoring.as_ref())
                .is_some_and(|authoring| {
                    let output = &authoring.layers[*index].output_layer_id;
                    authoring.layers.iter().any(|layer| &layer.id != id && &layer.output_layer_id == output)
                })
            {
                editor.status.push_str("；同一输出已有的旧三维样本共用来源，维持原显示");
            }
            return;
        };
        if target.locked && action != EditorAction::VegetationToggleLocked { return; }
        let busy = source_guard.busy(&session);
        let before = session.revision;
        let (mut session, mut terrains, mut documents, allocation) = authoring.p2();
        if allocation.as_ref().is_some_and(|state| state.busy()) {
            editor.status = "植被作者层事务仍在后台完成，请稍候".into();
            return;
        }
        // 参与和锁定复用正规的作者命令与预览编译，写侧先恢复被点中的完整来源。
        if let Err(error) = crate::ui::water_feature_list::focus_author_source(&mut editor, &mut session,
            terrain.as_ref(), busy, terrains.as_deref_mut(), documents.as_deref_mut())
        {
            editor.status = error;
            return;
        }
        (action, before != session.revision)
    };
    if changed_canvas { authoring.p1().0.retarget_terrain_canvas(); }
    let (mut session, _, _, mut allocation) = authoring.p2();
    editor.selected_vegetation_layer = *index;
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
