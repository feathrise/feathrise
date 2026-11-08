//! PBR 地表左栏连续列表、摘要与局部按钮视觉同步。

use feathrise_editor_ui::InteractionButton;
use bevy::prelude::*;
use feathrise_editor_ui::colors;
use feathrise_map_content::SurfacePresetRegistry;

use super::components::SurfaceButtonKind;
use super::types::{
    SurfacePresetAction, SurfacePresetButton, SurfacePresetDynamicListRoot,
    SurfacePresetEditorState,
};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_core::{PresetWorkspaceState, PresetWorkspaceTab};
use feathrise_editor_surface_model::system_params::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_panel(selected: Option<&str>, params: PanelParams) {
    let PanelParams {
        mut commands,
        asset_server,
        registry,
        state,
        mut cache,
        hosts,
        roots,
        mut texts,
    } = params;
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
                        SurfacePresetDynamicListRoot,
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
                                SurfacePresetButton(SurfacePresetAction::SelectPreset(index)),
                                Node {
                                    height: Val::Px(26.0),
                                    flex_basis: Val::Percent(32.0),
                                    flex_grow: 1.0,
                                    min_width: Val::Px(0.0), // 三列随真实侧栏宽度收缩，不让固定最小宽度挤出窄栏。
                                    max_width: Val::Percent(33.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    padding: UiRect::horizontal(Val::Px(3.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(3.0)),
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
                                TextLayout {
                                    linebreak: LineBreak::NoWrap,
                                    ..default()
                                },
                                Node {
                                    max_width: percent(100),
                                    min_width: px(0),
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                            ));
                        }
                    });
            });
        }
    }
    if let Ok(mut title) = texts.p0().single_mut() {
        **title = registry.as_deref().map_or_else(
            || "地表预设 · 等待目录".to_owned(),
            |registry| {
                format!(
                    "地表预设 {} 项 · 连续列表（不分页）",
                    registry.presets().len()
                )
            },
        );
    }
    if let Ok(mut summary) = texts.p1().single_mut() {
        **summary = selected
            .and_then(|id| registry.as_deref()?.resolve(id))
            .map_or_else(
                || "当前预设：尚未选择".to_owned(),
                |preset| {
                    format!(
                        "当前：{} [{}] · 平铺{:.2}m · 粗糙{:.2}",
                        preset.label, preset.id, preset.tile_meters, preset.perceptual_roughness
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

/// 页头只在文档/草稿修订或忙碌边沿改变时查找活动草稿，稳定帧不扫描标签。
pub(crate) fn sync_document_badge(workspace: &PresetWorkspaceState, params: BadgeParams) {
    let BadgeParams {
        state,
        mut seen,
        mut badges,
    } = params;
    let signature = (state.ui_revision, workspace.ui_revision, state.busy());
    if *seen == Some(signature) {
        return;
    }
    let Ok((mut text, mut color)) = badges.single_mut() else {
        return; // 正文尚未构造时不消费修订，下一帧继续同步。
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

pub(crate) fn sync_button_styles(
    selected: Option<&str>,
    workspace: &PresetWorkspaceState,
    params: StylesParams,
) {
    let StylesParams {
        state,
        registry,
        mut cache,
        mut buttons,
    } = params;
    let selected = selected.map(str::to_owned);
    let full_sync = cache.state_revision != state.ui_revision
        || cache.workspace_revision != workspace.ui_revision
        || cache.selected_id != selected;
    if full_sync {
        cache.state_revision = state.ui_revision;
        cache.workspace_revision = workspace.ui_revision;
        cache.selected_id = selected.clone();
        for (interaction, button, kind, mut background, mut border) in &mut buttons.p1() {
            apply_style(
                interaction,
                button.0,
                kind.copied().unwrap_or_default(),
                &mut background,
                &mut border,
                &state,
                &workspace,
                selected.as_deref(),
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
                selected.as_deref(),
                registry.as_deref(),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_style(
    interaction: &Interaction,
    action: SurfacePresetAction,
    kind: SurfaceButtonKind,
    background: &mut BackgroundColor,
    border: &mut BorderColor,
    state: &SurfacePresetEditorState,
    workspace: &PresetWorkspaceState,
    selected: Option<&str>,
    registry: Option<&SurfacePresetRegistry>,
) {
    let active = match action {
        SurfacePresetAction::SelectPreset(index) => registry
            .and_then(|registry| registry.presets().get(index))
            .is_some_and(|preset| selected == Some(preset.id.as_str())),
        SurfacePresetAction::DeleteSelected => state.delete_confirmation_id.is_some(),
        SurfacePresetAction::ActivateTab(id) => {
            workspace.active_tab == PresetWorkspaceTab::Surface(id)
        }
        SurfacePresetAction::CloseTab(id) => state
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| tab.close_confirmation),
        SurfacePresetAction::Focus(field) => state.active_field == Some(field),
        SurfacePresetAction::Section(section) => {
            let PresetWorkspaceTab::Surface(id) = workspace.active_tab else {
                return;
            };
            state
                .tabs
                .iter()
                .find(|tab| tab.id == id)
                .is_some_and(|tab| tab.section == section)
        }
        SurfacePresetAction::ToggleNormalY => state
            .active_draft(workspace.active_tab)
            .is_some_and(|draft| draft.descriptor.normal_y_flip),
        SurfacePresetAction::CycleProjection => state
            .active_draft(workspace.active_tab)
            .is_some_and(|draft| {
                draft.descriptor.projection == feathrise_map_content::SurfaceProjection::Triplanar
            }),
        _ => false,
    };
    if kind == SurfaceButtonKind::Primary {
        background.0 = if *interaction == Interaction::None {
            colors::SELECT_BG
        } else {
            colors::SELECT_BG_HOVER
        };
        *border = BorderColor::all(colors::COPPER_BORDER);
        return; // 悬停结束后主保存仍保持强调，不退化为普通按钮。
    }
    if kind == SurfaceButtonKind::Navigation {
        background.0 = match interaction {
            Interaction::Pressed | Interaction::Hovered => colors::SELECT_BG_HOVER,
            Interaction::None if active => colors::SELECT_BG,
            Interaction::None => Color::NONE,
        };
        *border = BorderColor::all(if active {
            colors::COPPER_BORDER
        } else {
            Color::NONE
        });
        return; // 仅保留节点原有底部边线，不给每个分区套完整铜框。
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

#[cfg(test)]
mod workspace_tab_style_tests {
    use super::*;
    use bevy::ecs::world::CommandQueue;
    use feathrise_editor_ui::workspace_tabs::{WorkspaceTabButton, spawn_preset_tab};

    fn sync_styles(workspace: Res<PresetWorkspaceState>, params: StylesParams) {
        sync_button_styles(None, &workspace, params);
    }

    #[test]
    fn business_sync_preserves_workspace_tab_fill_and_danger_close() {
        let mut app = App::new();
        app.init_resource::<SurfacePresetEditorState>()
            .init_resource::<PresetWorkspaceState>()
            .add_systems(Update, sync_styles);
        let mut queue = CommandQueue::default();
        Commands::new(&mut queue, app.world()).spawn(Node::default()).with_children(|root| {
            for (id, active, confirm) in [(1, true, false), (2, false, false), (3, true, true)] {
                spawn_preset_tab(root, &Handle::default(), "地表·草地*",
                    SurfacePresetButton(SurfacePresetAction::ActivateTab(id)),
                    SurfacePresetButton(SurfacePresetAction::CloseTab(id)), active, confirm);
            }
        });
        queue.apply(app.world_mut());
        let expected: Vec<_> = app.world_mut()
            .query_filtered::<(Entity, &BackgroundColor, &BorderColor), With<WorkspaceTabButton>>()
            .iter(app.world()).map(|(entity, fill, border)| (entity, *fill, *border)).collect();
        assert_eq!(expected.len(), 6);
        let sentinel = BackgroundColor(Color::srgb(1.0, 0.0, 1.0));
        let ordinary = app.world_mut().spawn((InteractionButton, Node::default(),
            SurfacePresetButton(SurfacePresetAction::Save), sentinel,
            BorderColor::all(Color::NONE))).id();
        // 先触发真实全量同步，再走 Changed<Interaction> 快路；普通保存按钮证明系统确实执行。
        for (index, interaction) in [Interaction::None, Interaction::Hovered, Interaction::Pressed, Interaction::None].into_iter().enumerate() {
            if index == 0 || index == 3 {
                app.world_mut().resource_mut::<PresetWorkspaceState>().touch_ui();
            }
            app.world_mut().entity_mut(ordinary).insert((interaction, sentinel));
            for &(entity, _, _) in &expected {
                app.world_mut().entity_mut(entity).insert(interaction);
            }
            app.update();
            assert_ne!(app.world().get::<BackgroundColor>(ordinary), Some(&sentinel));
            for &(entity, fill, border) in &expected {
                assert_eq!(app.world().get::<BackgroundColor>(entity), Some(&fill));
                assert_eq!(app.world().get::<BorderColor>(entity), Some(&border));
                assert_eq!(app.world().get::<Node>(entity).unwrap().border, UiRect::default());
            }
        }
        assert!(expected.iter().any(|(_, fill, _)| fill.0 == colors::DANGER_BG));
    }
}
