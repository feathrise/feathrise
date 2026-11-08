//! 水体预设左栏连续列表、摘要、状态和局部按钮视觉同步。

use feathrise_editor_ui::InteractionButton;
use bevy::prelude::*;
use feathrise_map_content::WaterPresetRegistry;
use crate::ui::theme as colors;

use super::components::{WaterButtonKind, WaterDocumentBadge};
use super::types::{
    WaterPresetAction, WaterPresetButton, WaterPresetDynamicListRoot, WaterPresetEditorState,
    WaterPresetList, WaterPresetListTitle, WaterPresetStatusText, WaterPresetSummaryText,
};
use crate::resources::MapEditorState;
use crate::ui::scene::FONT_PATH;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};

#[derive(Default)]
pub(super) struct WaterListCache {
    initialized: bool,
    revision: u64,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_panel(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    registry: Option<Res<WaterPresetRegistry>>,
    editor: Res<MapEditorState>,
    state: Res<WaterPresetEditorState>,
    mut cache: Local<WaterListCache>,
    hosts: Query<Entity, With<WaterPresetList>>,
    roots: Query<Entity, With<WaterPresetDynamicListRoot>>,
    mut texts: ParamSet<(
        Query<&mut Text, With<WaterPresetListTitle>>,
        Query<&mut Text, With<WaterPresetSummaryText>>,
        Query<&mut Text, With<WaterPresetStatusText>>,
    )>,
) {
    let revision = registry.as_deref().map_or(0, |registry| registry.revision);
    if !cache.initialized || cache.revision != revision {
        cache.initialized = true;
        cache.revision = revision;
        for root in &roots {
            commands.entity(root).despawn();
        }
        if let (Ok(host), Some(registry)) = (hosts.single(), registry.as_deref()) {
            let font = feathrise_editor_core::editor_font(&asset_server, FONT_PATH);
            commands.entity(host).with_children(|parent| {
                parent
                    .spawn((
                        WaterPresetDynamicListRoot,
                        Node {
                            width: Val::Percent(100.0),
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(3.0),
                            row_gap: Val::Px(3.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ))
                    .with_children(|list| {
                        for (index, preset) in registry.presets().iter().enumerate() {
                            list.spawn((
                                InteractionButton,
                                WaterPresetButton(WaterPresetAction::SelectPreset(index)),
                                Node {
                                    height: Val::Px(26.0),
                                    flex_basis: Val::Percent(32.0),
                                    flex_grow: 1.0,
                                    min_width: Val::Px(0.0),
                                    max_width: Val::Percent(33.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    padding: UiRect::horizontal(Val::Px(3.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
                                    overflow: Overflow::clip_x(), // 三列固定在窄栏内，完整名称与ID由可换行摘要显示。
                                    ..default()
                                },
                                BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                                BorderColor::all(colors::COPPER_BORDER_WEAK),
                            ))
                            .with_child((
                                Text::new(preset.label.clone()),
                                TextFont {
                                    font: font.clone().into(),
                                    font_size: 9.4.into(),
                                    ..default()
                                },
                                TextColor(colors::TEXT_PRIMARY),
                                TextLayout { linebreak: LineBreak::NoWrap, ..default() },
                                Node { min_width: px(0), max_width: percent(100), ..default() },
                            ));
                        }
                    });
            });
        }
    }
    if let Ok(mut title) = texts.p0().single_mut() {
        **title = registry.as_deref().map_or_else(
            || "水体预设 · 等待目录".to_owned(),
            |registry| {
                format!(
                    "水体预设 {} 项 · 连续列表（不分页）",
                    registry.presets().len()
                )
            },
        );
    }
    if let Ok(mut summary) = texts.p1().single_mut() {
        **summary = editor
            .water_selected_preset_id
            .as_deref()
            .and_then(|id| registry.as_deref()?.resolve(id))
            .map_or_else(
                || "当前预设：尚未选择".to_owned(),
                |preset| {
                    let normals = preset
                        .micro_normals
                        .iter()
                        .filter(|normal| normal.texture_path.is_some() && normal.strength > 0.0)
                        .count();
                    format!(
                        "当前：{} [{}] · 总波幅{:.3}m · 微法线{normals}/2",
                        preset.label,
                        preset.id,
                        preset.maximum_vertical_displacement()
                    )
                },
            );
    }
    for mut status in &mut texts.p2() {
        if status.as_str() != state.status {
            **status = state.status.clone();
        }
    }
}

/// 正文换代与忙碌边沿才更新文档状态，稳定帧不扫描水体标签。
pub(super) fn sync_document_badge(
    state: Res<WaterPresetEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    mut seen: Local<Option<(u64, u64, bool)>>,
    mut badges: Query<(&mut Text, &mut TextColor), With<WaterDocumentBadge>>,
) {
    let signature = (state.ui_revision, workspace.ui_revision, state.busy());
    if *seen == Some(signature) {
        return;
    }
    let Ok((mut text, mut color)) = badges.single_mut() else {
        return; // 正文尚未提交时不消费修订，下一帧继续同步。
    };
    *seen = Some(signature);
    if let Some(draft) = state.active_draft(workspace.active_tab) {
        let label = super::workspace::draft_badge(draft, state.busy());
        if text.as_str() != label {
            text.0 = label.to_owned();
        }
        color.0 = if draft.dirty() {
            colors::TEXT_ACCENT
        } else {
            colors::TEXT_MUTED
        };
    }
}

#[derive(Default)]
pub(super) struct WaterStyleCache {
    state_revision: u64,
    workspace_revision: u64,
    selected_id: Option<String>,
}

type WaterButtonStyle<'a> = (
    &'a Interaction,
    &'a WaterPresetButton,
    Option<&'a WaterButtonKind>,
    &'a mut BackgroundColor,
    &'a mut BorderColor,
);

pub(super) fn sync_button_styles(
    state: Res<WaterPresetEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    editor: Res<MapEditorState>,
    registry: Option<Res<WaterPresetRegistry>>,
    mut cache: Local<WaterStyleCache>,
    mut buttons: ParamSet<(
        Query<WaterButtonStyle<'_>, (Changed<Interaction>, Without<feathrise_editor_ui::workspace_tabs::WorkspaceTabButton>)>,
        Query<WaterButtonStyle<'_>, Without<feathrise_editor_ui::workspace_tabs::WorkspaceTabButton>>,
    )>,
) {
    let selected = editor.water_selected_preset_id.clone();
    let full_sync = cache.state_revision != state.ui_revision
        || cache.workspace_revision != workspace.ui_revision
        || cache.selected_id != selected;
    if full_sync {
        cache.state_revision = state.ui_revision;
        cache.workspace_revision = workspace.ui_revision;
        cache.selected_id = selected;
        for (interaction, button, kind, mut background, mut border) in &mut buttons.p1() {
            apply_style(
                interaction,
                button.0,
                kind.copied().unwrap_or_default(),
                &mut background,
                &mut border,
                &state,
                &workspace,
                &editor,
                registry.as_deref(),
            );
        }
    } else {
        for (interaction, button, kind, mut background, mut border) in &mut buttons.p0() {
            apply_style(
                interaction,
                button.0,
                kind.copied().unwrap_or_default(),
                &mut background,
                &mut border,
                &state,
                &workspace,
                &editor,
                registry.as_deref(),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_style(
    interaction: &Interaction,
    action: WaterPresetAction,
    kind: WaterButtonKind,
    background: &mut BackgroundColor,
    border: &mut BorderColor,
    state: &WaterPresetEditorState,
    workspace: &VegetationPresetEditorState,
    editor: &MapEditorState,
    registry: Option<&WaterPresetRegistry>,
) {
    let active = match action {
        WaterPresetAction::SelectPreset(index) => registry
            .and_then(|registry| registry.presets().get(index))
            .is_some_and(|preset| {
                editor.water_selected_preset_id.as_deref() == Some(preset.id.as_str())
            }),
        WaterPresetAction::DeleteSelected => state.delete_confirmation_id.is_some(),
        WaterPresetAction::ActivateTab(id) => workspace.active_tab == PresetWorkspaceTab::Water(id),
        WaterPresetAction::CloseTab(id) => state
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| tab.close_confirmation),
        WaterPresetAction::Focus(field) => state.active_field == Some(field),
        WaterPresetAction::Section(section) => {
            let PresetWorkspaceTab::Water(id) = workspace.active_tab else {
                return;
            };
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .is_some_and(|tab| tab.section == section)
        }
        _ => false,
    };
    if kind == WaterButtonKind::Primary {
        background.0 = if *interaction == Interaction::None {
            colors::SELECT_BG
        } else {
            colors::SELECT_BG_HOVER
        };
        *border = BorderColor::all(colors::COPPER_BORDER);
        return; // 主保存在悬停结束后仍保持强调。
    }
    if kind == WaterButtonKind::Navigation {
        background.0 = match interaction {
            Interaction::Pressed | Interaction::Hovered => colors::SELECT_BG_HOVER,
            Interaction::None if active => colors::SELECT_BG,
            Interaction::None => Color::NONE,
        };
        *border = BorderColor::all(if active { colors::COPPER_BORDER } else { Color::NONE });
        return; // 分区仅显示底线，不给页签恢复旧的完整铜框。
    }
    background.0 = match interaction {
        Interaction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
        Interaction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
        Interaction::None if active => colors::BUTTON_DEV_SMALL_PRESSED,
        Interaction::None => colors::BUTTON_DEV_SMALL_NORMAL,
    };
    *border = BorderColor::all(if active {
        colors::COPPER_BORDER
    } else {
        colors::COPPER_BORDER_WEAK
    });
}
