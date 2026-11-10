//! 原路径只借用根应用提供的场景工厂；业务回显继续留在原调度。
use feathrise_editor_ui::UiInteraction;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use super::{super::resources::DocumentState, components::HistoryAction};
use crate::ui::theme as colors;
pub(super) fn sync(
    mut commands: Commands,
    documents: Res<DocumentState>,
    mut buttons: Query<(
        Entity,
        &HistoryAction,
        &UiInteraction,
        &mut Node,
        &mut BackgroundColor,
        Has<InteractionDisabled>,
    )>,
) {
    let doc = documents
        .current()
        .filter(|doc| documents.visible && doc.mode == super::super::types::ViewMode::Edit);
    for (entity, action, interaction, mut node, mut background, disabled) in &mut buttons {
        let enabled = doc.is_some_and(|doc| {
            if *action == HistoryAction::Undo {
                doc.history.can_undo()
            } else {
                doc.history.can_redo()
            }
        });
        node.set_if_neq(Node {
            display: if doc.is_some() {
                Display::Flex
            } else {
                Display::None
            },
            ..node.clone()
        });
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        background.set_if_neq(BackgroundColor(if !enabled {
            colors::SLOT_BG
        } else {
            match interaction {
                UiInteraction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
                UiInteraction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
                UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL,
            }
        }));
    }
}
