//! 16个候选文件与32个并列差异行按视区复用；稳定帧不扫描正文、不按匹配数创建实体。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::super::resources::AssetSearchState;
use super::{components::*, constants::*, resources::AssetReplaceState};
use crate::{
    components::MapEditorPresetTabScroll,
    resources::MapEditorState,
    ui::{
        scene::FONT_PATH,
        theme as colors,
        vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
    },
};
use bevy::{
    prelude::*,
    ui::{FocusPolicy, InteractionDisabled},
};

pub(super) fn setup(
    scenes: Res<feathrise_editor_documents_model::DocumentScenes>,
    mut commands: Commands,
    server: Res<AssetServer>,
    files: Query<Entity, Added<ReplaceFileContent>>,
    diffs: Query<Entity, Added<ReplaceDiffContent>>,
    tabs: Query<Entity, Added<MapEditorPresetTabScroll>>,
) {
    for host in &tabs {
        let tab = commands.spawn_scene(super::scene::tab(&scenes)).id();
        commands.entity(host).add_child(tab);
    }
    let font = feathrise_editor_core::editor_font(&server, FONT_PATH);
    for host in &files {
        commands.entity(host).with_children(|parent| {
            for slot in 0..FILE_SLOTS {
                parent
                    .spawn((
                        ReplaceFileRow(slot),
                        Node {
                            display: Display::None,
                            position_type: PositionType::Absolute,
                            width: percent(100),
                            height: px(FILE_HEIGHT),
                            ..default()
                        },
                    ))
                    .with_children(|row| {
                        for checked in [true, false] {
                            row.spawn((
                                InteractionButton,
                                ReplaceAction::Inactive,
                                if checked {
                                    ReplaceButtonSlot::Check(slot)
                                } else {
                                    ReplaceButtonSlot::File(slot)
                                },
                                Node {
                                    width: if checked { px(24) } else { Val::Auto },
                                    min_width: px(0),
                                    flex_grow: if checked { 0.0 } else { 1.0 },
                                    height: percent(100),
                                    padding: UiRect::horizontal(px(4)),
                                    overflow: Overflow::clip(),
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(colors::SLOT_BG),
                                BorderColor::all(colors::COPPER_BORDER_WEAK),
                            ))
                            .with_children(|button| {
                                button.spawn((
                                    if checked {
                                        ReplaceLabel::Check(slot)
                                    } else {
                                        ReplaceLabel::File(slot)
                                    },
                                    Text::new(""),
                                    FocusPolicy::Pass,
                                    TextLayout::no_wrap(),
                                    TextFont {
                                        font: font.clone().into(),
                                        font_size: px(ui_metrics::FONT_CONTROL).into(),
                                        ..default()
                                    },
                                    TextColor(colors::TEXT_PRIMARY),
                                ));
                            });
                        }
                    });
            }
        });
    }
    for host in &diffs {
        commands.entity(host).with_children(|parent| {
            for slot in 0..DIFF_SLOTS {
                parent
                    .spawn((
                        InteractionButton,
                        ReplaceDiffRow(slot),
                        ReplaceButtonSlot::Hit(slot),
                        ReplaceAction::Inactive,
                        Node {
                            display: Display::None,
                            position_type: PositionType::Absolute,
                            width: percent(100),
                            height: px(DIFF_HEIGHT),
                            overflow: Overflow::clip(),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(colors::INPUT_BG),
                        BorderColor::all(colors::COPPER_BORDER_WEAK),
                    ))
                    .with_children(|row| {
                        for after in [false, true] {
                            row.spawn((
                                ReplaceLabel::Diff(slot, after),
                                Text::new(""),
                                FocusPolicy::Pass,
                                TextLayout::no_wrap(),
                                Node {
                                    width: percent(50),
                                    min_width: px(0),
                                    padding: UiRect::horizontal(px(6)),
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                TextFont {
                                    font: font.clone().into(),
                                    font_size: px(ui_metrics::FONT_CONTROL).into(),
                                    ..default()
                                },
                                TextColor(if after {
                                    Color::srgb(0.65, 0.74, 0.52)
                                } else {
                                    colors::TEXT_PRIMARY
                                }),
                            ));
                        }
                    });
            }
        });
    }
}

pub(super) fn sync(
    mut commands: Commands,
    state: Res<AssetReplaceState>,
    search: Res<AssetSearchState>,
    editor: Res<MapEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    files_scroll: Query<&ScrollPosition, With<ReplaceFiles>>,
    diff_scroll: Query<&ScrollPosition, With<ReplaceDiff>>,
    mut nodes: Query<
        (
            &mut Node,
            Has<ReplacePanel>,
            Has<ReplaceTab>,
            Has<ReplaceFileContent>,
            Has<ReplaceDiffContent>,
            Option<&ReplaceFileRow>,
            Option<&ReplaceDiffRow>,
        ),
        Or<(
            With<ReplacePanel>,
            With<ReplaceTab>,
            With<ReplaceFileContent>,
            With<ReplaceDiffContent>,
            With<ReplaceFileRow>,
            With<ReplaceDiffRow>,
        )>,
    >,
    mut labels: Query<(&ReplaceLabel, &mut Text)>,
    mut buttons: Query<(
        Entity,
        &mut ReplaceAction,
        Option<&ReplaceButtonSlot>,
        &UiInteraction,
        &mut BackgroundColor,
        &mut BorderColor,
        Has<InteractionDisabled>,
    )>,
    changed: Query<(), (With<ReplaceAction>, Changed<UiInteraction>)>,
    mut previous: Local<Option<(usize, usize)>>,
) {
    let file_start = (files_scroll
        .single()
        .map_or(0.0, |scroll| scroll.y)
        .max(0.0)
        / FILE_HEIGHT)
        .floor() as usize;
    let diff_start = (diff_scroll.single().map_or(0.0, |scroll| scroll.y).max(0.0) / DIFF_HEIGHT)
        .floor() as usize;
    if !state.is_changed()
        && !search.is_changed()
        && !editor.is_changed()
        && !workspace.is_changed()
        && changed.is_empty()
        && *previous == Some((file_start, diff_start))
    {
        return;
    }
    *previous = Some((file_start, diff_start));
    let plan = state.current();
    let result = plan.map(|plan| plan.search.as_ref()).or_else(|| {
        search
            .current()
            .filter(|result| Some(result.ticket) == state.search_ticket)
    });
    let file = plan.and_then(|plan| plan.files.iter().find(|file| file.index == state.file));
    let limited = result.is_some_and(|result| result.limited);
    let file_count = result.map_or(0, |result| result.files.len());
    let diff_count = file.map_or(0, |file| file.after.len());
    let shown = state.preview_visible()
        && editor.open
        && !editor.close_requested
        && workspace.active_tab == PresetWorkspaceTab::Document;
    for (mut node, panel, tab, file_content, diff_content, file_row, diff_row) in &mut nodes {
        if panel {
            node.display = if shown { Display::Flex } else { Display::None };
        }
        if tab {
            node.display = if state.tab_open {
                Display::Flex
            } else {
                Display::None
            };
        }
        if file_content {
            node.height = px(file_count as f32 * FILE_HEIGHT);
        }
        if diff_content {
            node.height = px(diff_count as f32 * DIFF_HEIGHT);
        }
        if let Some(row) = file_row {
            let index = file_start + row.0;
            node.display = if index < file_count {
                Display::Flex
            } else {
                Display::None
            };
            node.top = px(index as f32 * FILE_HEIGHT);
        }
        if let Some(row) = diff_row {
            let index = diff_start + row.0;
            node.display = if index < diff_count {
                Display::Flex
            } else {
                Display::None
            };
            node.top = px(index as f32 * DIFF_HEIGHT);
        }
    }
    for (label, mut text) in &mut labels {
        let value = match label {
            ReplaceLabel::Status => state.hint.clone(),
            ReplaceLabel::Summary => format!(
                "跨文件替换预览{} · 已勾选{}个文件 · 替换为：{}{}",
                if limited {
                    " · 仅处理已列出匹配，未列出内容保持"
                } else {
                    ""
                },
                state.checked.len(),
                if state.replacement.is_empty() {
                    "（删除）".to_owned()
                } else {
                    state.replacement.chars().take(64).collect()
                },
                if state.replacement.chars().nth(64).is_some() {
                    "…"
                } else {
                    ""
                }
            ),
            ReplaceLabel::Path => result
                .and_then(|result| result.files.get(state.file))
                .map_or("选择文件并更新预览".into(), |file| {
                    format!(
                        "{}{}",
                        file.path,
                        if file.dirty {
                            " *（使用未保存草稿）"
                        } else {
                            ""
                        }
                    )
                }),
            ReplaceLabel::File(slot) => result
                .and_then(|result| result.files.get(file_start + *slot))
                .map_or(String::new(), |file| {
                    format!(
                        "{}{} · {}",
                        file.path,
                        if file.dirty { " *" } else { "" },
                        file.hits.len()
                    )
                }),
            ReplaceLabel::Check(slot) => if state.checked.contains(&(file_start + *slot)) {
                "✓"
            } else {
                "□"
            }
            .into(),
            ReplaceLabel::Diff(slot, after) => result
                .and_then(|result| result.files.get(state.file))
                .and_then(|group| {
                    let index = diff_start + *slot;
                    let hit = group.hits.get(index)?;
                    Some(format!(
                        "{}:{}  {}",
                        hit.line,
                        hit.column,
                        if *after {
                            file.and_then(|file| file.after.get(index))
                                .map_or("", String::as_str)
                        } else {
                            &hit.snippet
                        }
                    ))
                })
                .unwrap_or_default(),
            ReplaceLabel::Button(action) => match action {
                ReplaceAction::ApplyFile => {
                    if limited {
                        "替换当前列出项"
                    } else {
                        "替换当前文件"
                    }
                }
                ReplaceAction::ApplyChecked => {
                    if limited {
                        "替换选中列出项"
                    } else {
                        "替换选中文件"
                    }
                }
                _ => continue,
            }
            .into(),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for (entity, mut action, slot, interaction, mut background, mut border, disabled) in
        &mut buttons
    {
        if let Some(slot) = slot {
            *action = match slot {
                ReplaceButtonSlot::File(slot) => ReplaceAction::File {
                    ticket: state.preview_ticket,
                    index: file_start + slot,
                },
                ReplaceButtonSlot::Check(slot) => ReplaceAction::Check {
                    ticket: state.preview_ticket,
                    index: file_start + slot,
                },
                ReplaceButtonSlot::Hit(slot) => ReplaceAction::Hit {
                    ticket: state.preview_ticket,
                    index: diff_start + slot,
                },
            };
        }
        let selected = match *action {
            ReplaceAction::File { index, .. } => index == state.file,
            ReplaceAction::Check { index, .. } => state.checked.contains(&index),
            ReplaceAction::Hit { index, .. } => index == state.hit,
            ReplaceAction::Activate => shown,
            _ => false,
        };
        let enabled = match *action {
            ReplaceAction::ApplyHit | ReplaceAction::ApplyFile => {
                file.is_some() && !state.applied && state.task.is_none() && state.pending.is_none()
            }
            ReplaceAction::ApplyChecked => {
                plan.is_some() && !state.applied && state.task.is_none() && state.pending.is_none()
            }
            ReplaceAction::Undo => state.batch.as_ref().is_some_and(|batch| !batch.undone),
            ReplaceAction::Redo => state.batch.as_ref().is_some_and(|batch| batch.undone),
            ReplaceAction::SaveBatch => state.batch.is_some() && state.task.is_none() && state.pending.is_none(),
            ReplaceAction::Check { .. } => !state.applied,
            _ => true,
        };
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        background.0 = if selected {
            colors::SELECT_BG
        } else if enabled && *interaction != UiInteraction::None {
            colors::SELECT_BG_HOVER
        } else {
            colors::SLOT_BG
        };
        *border = BorderColor::all(if selected && enabled {
            colors::INPUT_ACTIVE_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
    }
}
