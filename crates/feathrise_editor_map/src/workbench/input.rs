//! 原按钮与目录行产生意图，真正业务由独立宿主执行；本地保留原导航和布局响应。
use super::{EditorAction, WorkbenchIntent, WorkbenchModel};
use crate::{
    components::{MapEditorActionButton, MapEditorMapRow},
    resources::{MapBrowserIndexState, MapBrowserState, MapEditorState},
    types::MapBrowserItem,
};
use bevy::prelude::*;
use feathrise_editor_ui::EditorButtonClick;

pub(super) fn forward_actions(
    model: Res<WorkbenchModel>,
    index: Res<MapBrowserIndexState>,
    catalog: Res<super::resources::CatalogJob>,
    actions: Query<
        (
            &MapEditorActionButton,
            &EditorButtonClick,
            Option<&crate::ui::map_sidebar::MapSidebarActionBinding>,
        ),
        Changed<EditorButtonClick>,
    >,
    rows: Query<(&MapEditorMapRow, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut browser: ResMut<MapBrowserState>,
    mut editor: ResMut<MapEditorState>,
    mut intents: MessageWriter<WorkbenchIntent>,
    time: Res<Time<bevy::time::Real>>,
    mut last_click: Local<Option<crate::map_browser::MapBrowserClick>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if !model.visible
        || model.modal
        || model.busy
        || index.rebuilding
        || !index
            .applied_signature
            .as_ref()
            .is_some_and(|source| source.folder_revision == model.context)
    {
        *last_click = None;
        return;
    }
    for (row, click) in &rows {
        if click.is_clicked()
            && let Some(MapBrowserItem::Map(id)) = &row.item
        {
            browser.select_only(Some(MapBrowserItem::Map(id.clone())));
            editor.map_list_selection_id = Some(id.clone());
            let modified = [
                KeyCode::ControlLeft,
                KeyCode::ControlRight,
                KeyCode::ShiftLeft,
                KeyCode::ShiftRight,
            ]
            .into_iter()
            .any(|key| keys.pressed(key));
            if modified {
                *last_click = None;
            }
            if !modified
                && let Some(projection) = &index.applied_projection
                && crate::map_browser::MapBrowserClick::release(
                    &mut last_click,
                    id,
                    projection,
                    time.elapsed_secs_f64(),
                )
                && model
                    .enabled_actions
                    .contains(&EditorAction::EnterSelectedMap)
            {
                intents.write(WorkbenchIntent {
                    context: model.context,
                    action: EditorAction::EnterSelectedMap,
                    selected_map: Some(id.0.clone()),
                    selected_index: catalog.indices.get(&id.0).copied(),
                });
            }
        }
    }
    for (button, click, binding) in &actions {
        if click.is_clicked()
            && model.enabled_actions.contains(&button.0)
            && binding.is_none_or(|binding| binding.matches(browser.selected_item.as_ref()))
        {
            *last_click = None;
            intents.write(WorkbenchIntent {
                context: model.context,
                action: button.0,
                selected_map: editor.map_list_selection_id.as_ref().map(|id| id.0.clone()),
                selected_index: editor
                    .map_list_selection_id
                    .as_ref()
                    .and_then(|id| catalog.indices.get(&id.0).copied()),
            });
        }
    }
}
