//! 固定网格行的高亮、图标及点击身份；每帧只访问十行和三十个开关。
use feathrise_editor_ui::UiInteraction;
use super::{
    components::*, names::NamePanelGate, resources::TerrainObjectsEditorState, systems,
    types::RowTarget,
};
use crate::{
    authoring::EditorAuthoringSession,
    resources::SceneEditorState,
    ui::scene_sidebar::{
        TreeRowToggle, load_tree_toggle_images, sync_tree_toggle_icon, tree_row_colors,
        tree_toggle_colors,
    },
    viewport::{LayerCandidateState, LayerTarget},
};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

pub(super) fn tree_toggle(action: Action) -> TreeRowToggle {
    match action {
        Action::Enabled => TreeRowToggle::Participation,
        Action::Visible => TreeRowToggle::Visibility,
        _ => TreeRowToggle::Lock,
    }
}

pub(super) fn sync(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    state: Res<TerrainObjectsEditorState>,
    selection: Res<LayerCandidateState>,
    gate: NamePanelGate,
    server: Res<AssetServer>,
    mut images: Local<Option<[Handle<Image>; 6]>>,
    mut rows: Query<
        (&Row, &UiInteraction, &mut BackgroundColor, &mut BorderColor),
        Without<RowAction>,
    >,
    mut buttons: Query<
        (
            Entity,
            &ActionButton,
            &mut RowAction,
            Option<&mut feathrise_editor_ui::EditorButtonClick>,
            &UiInteraction,
            Has<InteractionDisabled>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Without<Row>,
    >,
    mut icons: Query<(&RowIcon, &mut ImageNode)>,
) {
    let images = images.get_or_insert_with(|| load_tree_toggle_images(&server));
    let draft = session.draft.as_ref();
    let objects = draft
        .and_then(|draft| draft.manifest.terrains.as_deref())
        .unwrap_or_else(|| state.legacy.as_deref().map_or(&[], std::slice::from_ref));
    let usable = systems::enabled(&editor, &session, &state) && gate.scene_visible();
    let active = |object: Option<&feathrise_scene_content::SceneTerrainObject>, toggle| object.is_some_and(|object| match toggle {
        TreeRowToggle::Participation => object.enabled,
        TreeRowToggle::Visibility => !state.hidden.contains(&object.id),
        TreeRowToggle::Lock => state.locked.contains(&object.id),
    });
    // 地形状态会记住作者画布。树行只高亮当前正式选择，切到摆放物时画布仍可复用。
    let selected_terrain = match selection.selected.as_ref() {
        Some(LayerTarget::TerrainObject(id)) => Some(id.as_str()),
        _ => None,
    };
    for (row, interaction, mut background, mut border) in &mut rows {
        let selected = objects
            .get(state.list_start + row.0)
            .is_some_and(|object| selected_terrain == Some(object.id.as_str()));
        let (fill, outline) = tree_row_colors(selected, false, *interaction);
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(outline);
    }
    for (entity, action, mut row, click, interaction, disabled, mut background, mut border) in &mut buttons
    {
        let index = state.list_start + row.slot;
        let object = objects.get(index);
        let target = draft.zip(object).map(|(draft, object)| RowTarget {
            scene_id: draft.scene_id.clone(),
            revision: session.revision,
            terrain: object.id.clone(),
            index,
        });
        if row.target != target {
            row.target = target;
            if let Some(mut click) = click { click.invalidate(); } // 复用行只能接受在同一已呈现对象上的完整手势。
        }
        let available =
            usable && super::presentation::action_available(action.0, object, &state);
        if available && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
        if !available && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
        let (fill, outline) = tree_toggle_colors(available, active(object, tree_toggle(action.0)), *interaction);
        background.set_if_neq(fill);
        border.set_if_neq(outline);
    }
    for (icon, mut image) in &mut icons {
        let object = objects.get(state.list_start + icon.slot);
        let toggle = tree_toggle(icon.action);
        sync_tree_toggle_icon(toggle, active(object, toggle), images, &mut image);
    }
}
