//! 固定网格行的高亮、图标及点击身份；每帧只访问十行和三十个开关。
use super::{
    components::*, names::NamePanelGate, resources::TerrainObjectsEditorState, systems,
    types::RowTarget,
};
use crate::{
    authoring::EditorAuthoringSession,
    resources::MapEditorState,
    ui::{map_sidebar::tree_row_colors, theme as colors},
};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

pub(super) fn sync(
    mut commands: Commands,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    state: Res<TerrainObjectsEditorState>,
    gate: NamePanelGate,
    server: Res<AssetServer>,
    mut images: Local<Option<[Handle<Image>; 6]>>,
    mut rows: Query<
        (&Row, &Interaction, &mut BackgroundColor, &mut BorderColor),
        Without<RowAction>,
    >,
    mut buttons: Query<
        (
            Entity,
            &ActionButton,
            &mut RowAction,
            Option<&mut feathrise_editor_ui::EditorButtonClick>,
            &Interaction,
            Has<InteractionDisabled>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Without<Row>,
    >,
    mut icons: Query<(&RowIcon, &mut ImageNode)>,
) {
    let images = images.get_or_insert_with(|| {
        [
            "enabled", "disabled", "visible", "hidden", "locked", "unlocked",
        ]
        .map(|name| server.load(format!("icons/editor/mesh_{name}.png")))
    });
    let draft = session.draft.as_ref();
    let objects = draft
        .and_then(|draft| draft.manifest.terrains.as_deref())
        .unwrap_or_else(|| state.legacy.as_deref().map_or(&[], std::slice::from_ref));
    let usable = systems::enabled(&editor, &session, &state) && gate.scene_visible();
    for (row, interaction, mut background, mut border) in &mut rows {
        let selected = objects
            .get(state.list_start + row.0)
            .is_some_and(|object| state.selected.as_ref() == Some(&object.id));
        let (fill, outline) = tree_row_colors(selected, false, *interaction);
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(outline);
    }
    for (entity, action, mut row, click, interaction, disabled, mut background, mut border) in &mut buttons
    {
        let index = state.list_start + row.slot;
        let object = objects.get(index);
        let target = draft.zip(object).map(|(draft, object)| RowTarget {
            map: draft.map_id.clone(),
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
        let active = object.is_some_and(|object| match action.0 {
            Action::Enabled => object.enabled,
            Action::Visible => !state.hidden.contains(&object.id),
            Action::Lock => state.locked.contains(&object.id),
            _ => false,
        });
        let fill = if available && *interaction == Interaction::Pressed {
            colors::SELECT_BG_PRESSED
        } else if available && *interaction == Interaction::Hovered {
            colors::SELECT_BG_HOVER
        } else if active {
            colors::SELECT_BG
        } else {
            colors::BUTTON_DEV_SMALL_NORMAL
        };
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(if active {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        }));
    }
    for (icon, mut image) in &mut icons {
        let object = objects.get(state.list_start + icon.slot);
        let index = match icon.action {
            Action::Enabled => {
                if object.is_some_and(|object| object.enabled) {
                    0
                } else {
                    1
                }
            }
            Action::Visible => {
                if object.is_some_and(|object| !state.hidden.contains(&object.id)) {
                    2
                } else {
                    3
                }
            }
            _ => {
                if object.is_some_and(|object| state.locked.contains(&object.id)) {
                    4
                } else {
                    5
                }
            }
        };
        if image.image != images[index] {
            image.image = images[index].clone();
        }
        let color = if index == 1 || index == 3 {
            colors::TEXT_MUTED
        } else {
            colors::TEXT_PRIMARY
        };
        if image.color != color {
            image.color = color;
        }
    }
}
