//! 建筑文档状态与主保存/顶部导航/字段选中样式；稳定帧只处理真实变化。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, types::*, workspace::draft_badge};
use crate::ui::{theme as colors, vegetation_preset_editor::VegetationPresetEditorState};
use bevy::prelude::*;

pub(super) fn sync_document_badge(
    state: Res<BuildingPresetEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    mut seen: Local<Option<(Entity, u64, Option<BuildingPresetField>, u8)>>,
    mut badges: Query<(Entity, &mut Text, &mut TextColor), With<BuildingDocumentBadge>>,
) {
    let Ok((entity, mut text, mut color)) = badges.single_mut() else {
        return;
    };
    let phase = u8::from(state.write_task.is_some()) | (u8::from(state.adoption.is_some()) << 1);
    let signature = (entity, state.ui_revision, state.active_field, phase);
    if *seen == Some(signature) {
        return;
    }
    let Some(draft) = state.active_draft(workspace.active_tab) else {
        return;
    };
    *seen = Some(signature);
    let label = draft_badge(draft, &state);
    if text.as_str() != label {
        **text = label.to_owned();
    }
    let next = if draft.dirty()
        || state.active_field.is_some()
        || matches!(draft.mode, BuildingDraftMode::Create)
    {
        colors::TEXT_ACCENT
    } else {
        colors::TEXT_MUTED
    };
    if color.0 != next {
        color.0 = next;
    }
}

/// 只在修订/字段拥有者变化时重算选择；普通稳定帧由Changed查询处理悬停。
pub(super) fn sync_button_styles(
    state: Res<BuildingPresetEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    mut cache: Local<BuildingWorkbenchStyleCache>,
    mut buttons: ParamSet<(
        Query<
            (
                &UiInteraction,
                &BuildingPresetButton,
                &BuildingButtonKind,
                &mut BackgroundColor,
                &mut BorderColor,
            ),
            Or<(Changed<UiInteraction>, Added<BuildingButtonKind>)>,
        >,
        Query<(
            &UiInteraction,
            &BuildingPresetButton,
            &BuildingButtonKind,
            &mut BackgroundColor,
            &mut BorderColor,
        )>,
    )>,
) {
    let changed = !cache.initialized
        || cache.ui_revision != state.ui_revision
        || cache.workspace_revision != workspace.ui_revision
        || cache.active_field != state.active_field;
    if !changed && buttons.p0().is_empty() {
        return;
    }
    let Some(draft) = state.active_draft(workspace.active_tab) else {
        return;
    };
    if changed {
        for (interaction, action, kind, mut background, mut border) in &mut buttons.p1() {
            style(
                *interaction,
                action.0,
                *kind,
                draft,
                state.active_field,
                &mut background,
                &mut border,
            );
        }
        cache.initialized = true;
        cache.ui_revision = state.ui_revision;
        cache.workspace_revision = workspace.ui_revision;
        cache.active_field = state.active_field;
    } else {
        for (interaction, action, kind, mut background, mut border) in &mut buttons.p0() {
            style(
                *interaction,
                action.0,
                *kind,
                draft,
                state.active_field,
                &mut background,
                &mut border,
            );
        }
    }
}

fn style(
    interaction: UiInteraction,
    action: BuildingPresetAction,
    kind: BuildingButtonKind,
    draft: &BuildingDraft,
    field: Option<BuildingPresetField>,
    background: &mut BackgroundColor,
    border: &mut BorderColor,
) {
    let active = match action {
        BuildingPresetAction::SelectSection(section) => draft.section == section,
        BuildingPresetAction::SelectConnection(index) => {
            draft
                .selected_connection
                .min(draft.descriptor.connections.len().saturating_sub(1))
                == index
        }
        BuildingPresetAction::Focus(value) => field == Some(value),
        _ => false,
    };
    let next_background = match (kind, interaction) {
        (BuildingButtonKind::Field, _) => colors::SLOT_BG,
        (BuildingButtonKind::Navigation, UiInteraction::None) if !active => Color::NONE,
        (BuildingButtonKind::Navigation | BuildingButtonKind::Primary, UiInteraction::None) => {
            colors::SELECT_BG
        }
        (_, UiInteraction::Pressed) => colors::BUTTON_DEV_SMALL_PRESSED,
        (_, UiInteraction::Hovered) => colors::BUTTON_DEV_SMALL_HOVERED,
        (_, UiInteraction::None) if active => colors::SELECT_BG,
        (_, UiInteraction::None) => colors::BUTTON_DEV_SMALL_NORMAL,
    };
    let next_border = BorderColor::all(if active || kind == BuildingButtonKind::Primary {
        colors::COPPER_BORDER
    } else if kind == BuildingButtonKind::Navigation {
        Color::NONE
    } else {
        colors::COPPER_BORDER_WEAK
    });
    if background.0 != next_background {
        background.0 = next_background;
    }
    if *border != next_border {
        *border = next_border;
    }
}
