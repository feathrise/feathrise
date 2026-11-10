//! 原路径只借用根应用提供的场景工厂；业务回显继续留在原调度。
use feathrise_editor_ui::UiInteraction;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use crate::ui::theme as colors;
use super::super::{resources::DocumentState, types::ViewMode};
use super::{components::*, types::supported};
pub(super) fn sync(
    mut commands: Commands,
    documents: Res<DocumentState>,
    mut buttons: Query<(
        Entity,
        &JsonAction,
        &UiInteraction,
        &mut Node,
        &mut BackgroundColor,
        Has<InteractionDisabled>,
    )>,
    mut labels: Query<(&mut Text, &mut TextColor), With<DiagnosticLabel>>,
) {
    let doc = documents
        .current()
        .filter(|doc| documents.visible && supported(&doc.path));
    let checked = doc.and_then(|doc| documents.json.checked(doc));
    let error = checked.and_then(|checked| checked.error.as_ref());
    let label = if doc.is_none() {
        String::new() // 没有JSON文档时，不在底栏生成虚假的检查进度。
    } else if let Some(error) = error {
        error.summary() // 全文进入底栏；正文保留独立“定位错误”动作。
    } else if checked.is_some() {
        "JSON语法正确".into()
    } else {
        "正在检查JSON语法…".into()
    };
    for (mut text, mut color) in &mut labels {
        if text.as_str() != label {
            **text = label.clone();
        }
        color.set_if_neq(TextColor(if error.is_some() {
            colors::TEXT_ACCENT
        } else {
            colors::TEXT_HINT
        }));
    }
    for (entity, action, interaction, mut node, mut background, disabled) in &mut buttons {
        let shown = doc.is_some_and(|doc| if *action == JsonAction::Locate {
            error.is_some() // 成功与检查进度不再保留一整行禁用的诊断卡。
        } else {
            doc.mode == ViewMode::Edit
        });
        let enabled = shown
            && documents.pending_load.is_none()
            && (documents.io.is_none() || documents.saving.is_some())
            && if *action == JsonAction::Locate {
                error.is_some()
            } else {
                documents.json.format.is_none() && documents.json.ready.is_none()
            };
        if node.display != if shown { Display::Flex } else { Display::None } {
            node.display = if shown { Display::Flex } else { Display::None };
        }
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        background.set_if_neq(BackgroundColor(
            if enabled && *interaction == UiInteraction::Hovered {
                colors::BUTTON_DEV_SMALL_HOVERED
            } else if enabled && *interaction == UiInteraction::Pressed {
                colors::BUTTON_DEV_SMALL_PRESSED
            } else {
                colors::SLOT_BG
            },
        ));
    }
}
