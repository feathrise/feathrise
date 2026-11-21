//! 场景树命令先同步冻结对象的唯一正式选择；聚焦继续走原相机取景入口。

use super::{SceneContextMenuState, SceneMenuAction, SceneObjectKey};
use crate::{
    authoring::{EditorAuthoringSession, terrain_documents::TreeVegetationAction},
    resources::{EditorViewportSelectionState, SceneEditorState},
    ui::gltf_browser::EditorGltfBrowserState,
    viewport::{LayerCandidateState, LayerTarget},
};
use bevy::prelude::*;

pub(crate) fn select_target(
    mut menu: ResMut<SceneContextMenuState>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut browser: Option<ResMut<EditorGltfBrowserState>>,
    mut candidates: Option<ResMut<LayerCandidateState>>,
    mut terrains: Option<ResMut<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    mut subscenes: Option<ResMut<crate::ui::subscenes::SubsceneEditorState>>,
    mut multi: Option<ResMut<crate::viewport::multi_select::PlacementSelectionSet>>,
    mut viewport_selection: Option<ResMut<EditorViewportSelectionState>>,
    mut placement: Option<ResMut<crate::viewport::SelectedPlacementViewportState>>,
    mut layers: crate::ui::scene_layers::LayerSelectionContext,
    mut tree_selection: Option<ResMut<crate::ui::scene_layers::SceneLayerSelection>>,
) {
    let click = tree_selection.as_mut().and_then(|state| state.0.take());
    // 文件操作与植被的编辑器显隐可按稳定来源处理，保持当前选择和画布。
    let mut request = menu.ready.as_ref().filter(|request| !matches!(
        (&request.target.key, request.action),
        (SceneObjectKey::MainSceneGlb, _) | (_, SceneMenuAction::RevealGlb)
            | (SceneObjectKey::Vegetation { .. }, SceneMenuAction::Visibility)
    )).cloned();
    let deferred = layers.take_vegetation_selection(candidates.as_deref());
    let mut adopted = None;
    if request.is_none() && click.is_none() && let Some(intent) = deferred
        && intent.target.current(&editor, &session)
    {
        if intent.action == TreeVegetationAction::Focus {
            let focus = super::types::SceneObjectRequest { target: intent.target.clone(), action: SceneMenuAction::Focus, name: String::new() };
            menu.ready = Some(focus.clone());
            request = Some(focus);
        }
        adopted = Some(intent.target);
    }
    let Some(mut target) = request
        .as_ref().map(|request| &request.target)
        .or(click.as_ref()).or(adopted.as_ref()).cloned()
        .filter(|target| target.current(&editor, &session))
    else {
        return;
    };
    if matches!(target.key, SceneObjectKey::Vegetation { cold: Some(_), .. }) {
        if request.as_ref().is_none_or(|request| request.action == SceneMenuAction::Focus) {
            let action = if request.is_some() { TreeVegetationAction::Focus } else { TreeVegetationAction::Select };
            layers.request_vegetation(target, &mut editor, &session, candidates.as_deref(), action);
            menu.ready = None;
        }
        return; // 冷作者命令由植被事务系统处理，等待完整画布前维持原正式选择。
    }
    layers.cancel_vegetation(&mut session); // 任何明确换选都比旧的冷来源意图优先。
    let author_layer_selected = matches!(&target.key, SceneObjectKey::Water { .. } | SceneObjectKey::Vegetation { .. });
    if author_layer_selected && !crate::ui::scene_layers::select(
        &target,
        &mut editor,
        &mut session,
        terrains.as_deref_mut(),
        candidates.as_deref_mut(),
        viewport_selection.as_deref_mut(),
        &mut layers,
    ) {
        menu.ready = None; // 来源门拒绝后同时取消后续定位/结构命令，原正式选择和属性保持原样。
        return;
    } // 画布恢复可能因笔划、未保存高度或历史归属而失败，成功前不能清除旧选择。
    if let Some(multi) = multi.as_mut() {
        multi.clear();
    }
    if !matches!(target.key, SceneObjectKey::Vegetation { .. }) && let Some(selection) = viewport_selection.as_mut() {
        selection.clear_vegetation_selection();
    }
    if let Some(placement) = placement.as_mut() {
        placement.clear_click_sequence();
    }
    if let Some(browser) = browser.as_mut() {
        browser.selected_group = None;
        browser.selection = None;
        browser.selected_placement_id = None;
    }
    if !author_layer_selected && let Some(candidates) = candidates.as_mut() {
        candidates.clear_for_scene_tree();
    }
    match &target.key {
        SceneObjectKey::Water { .. } | SceneObjectKey::Vegetation { .. } => {},
        SceneObjectKey::Placement { id, mesh, .. } => {
            if let Some(browser) = browser.as_mut() {
                browser.selected_placement_id = Some(id.clone());
                browser.selection = mesh.clone();
            }
            if let Some(candidates) = candidates.as_mut() {
                candidates.selected = Some(LayerTarget::Placement(id.clone()));
            }
        }
        SceneObjectKey::Group { .. } => {
            if let Some(browser) = browser.as_mut() {
                browser.selected_group = Some(target.name.clone()); // 聚焦组沿用普通树行选中的属性入口。
            }
        }
        SceneObjectKey::Terrain { index, id } => {
            if let Some(state) = terrains.as_mut() {
                if state.selected.as_ref() != Some(id) {
                    state.reset_lod_selection();
                }
                state.scene_id = Some(target.scene_id.clone());
                state.selected = Some(id.clone());
                state.selected_index = Some(*index);
            }
            if let Some(candidates) = candidates.as_mut() {
                candidates.confirm_terrain(id, *index);
            }
        }
        SceneObjectKey::Subscene { id, .. } => {
            if let Some(state) = subscenes.as_mut() {
                state.scene_id = Some(target.scene_id.clone());
                state.selected = Some(id.clone());
            }
            if let Some(candidates) = candidates.as_mut() {
                candidates.restore_subscene(
                    id,
                    &session,
                    layers
                        .domain
                        .as_ref()
                        .map_or(0, |state| state.observed_generation),
                    layers
                        .modes
                        .as_ref()
                        .map_or_else(Default::default, |state| state.selection_filter(&editor)),
                );
            }
        }
        _ => { if !crate::ui::scene_layers::select(
            &target,
            &mut editor,
            &mut session,
            terrains.as_deref_mut(),
            candidates.as_deref_mut(),
            viewport_selection.as_deref_mut(),
            &mut layers,
        ) { return; } },
    }
    if author_layer_selected {
        if request.is_none() && let Some((first, _)) = menu.bypass_change_detection().last_tree_click.as_mut()
            && first.object == target
        {
            first.object.revision = session.revision; // 首击换入自己的作者画布后才能续接同一对象的双击。
        }
        target.revision = session.revision;
        if let Some(request) = menu.ready.as_mut() { request.target.revision = session.revision; }
    } // 仅已经验证并完成的来源切换续接当前菜单命令；其他旧修订仍由current拒绝。
    layers.finish_selection(&target.key);
    if request.as_ref().is_some_and(|request| request.action == SceneMenuAction::Focus) {
        use crate::types::EditorViewportFraming;
        let framing = match target.key {
            SceneObjectKey::Placement { .. } => EditorViewportFraming::TreePlacement,
            SceneObjectKey::Group { .. } | SceneObjectKey::Terrain { .. }
                | SceneObjectKey::Subscene { .. } => EditorViewportFraming::Selected,
            _ => return, // 其他类型沿用各领域的范围计算。
        };
        editor.viewport_framing_requested = Some(framing);
    }
}

#[cfg(test)]
#[path = "focus_tests.rs"]
mod tests;
