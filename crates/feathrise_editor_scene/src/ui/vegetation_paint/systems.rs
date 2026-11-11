//! 绘制范围控件接线；改组只写小型关联元数据，首次有效刷印才申请本地密度。
use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use super::components::*;
use crate::{
    authoring::{
        EditorAuthoringSession, EditorCommand, VegetationBrushStroke,
        vegetation_paint_scope::{
            PaintScope, VegetationPaintChange, VegetationPaintSettings, VegetationPaintState,
        },
    },
    resources::SceneEditorState,
    ui::theme as colors,
};
use bevy::prelude::*;
use feathrise_scene_content::*;
use std::{collections::BTreeMap, sync::Arc};

fn same_arc<T>(a: Option<&Arc<[T]>>, b: Option<&Arc<[T]>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// 所有数量只用于滚动范围；每帧最多重绑两套十行池，不按场景大小生成按钮。
#[allow(clippy::type_complexity)]
pub(super) fn sync(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut settings: ResMut<VegetationPaintSettings>,
    mut controls: Query<
        (&PaintAction, &UiInteraction, &mut BackgroundColor, &mut Node),
        (
            Without<PaintRow>,
            Without<PaintList>,
            Without<PaintListContent>,
        ),
    >,
    mut views: Query<
        (&PaintList, &mut Node, &mut ScrollPosition),
        (
            Without<PaintRow>,
            Without<PaintListContent>,
            Without<PaintAction>,
        ),
    >,
    mut contents: Query<
        (&PaintListContent, &mut Node),
        (Without<PaintRow>, Without<PaintList>, Without<PaintAction>),
    >,
    mut rows: Query<
        (
            Ref<PaintRow>,
            &UiInteraction,
            &mut PaintAction,
            &mut Node,
            &mut BackgroundColor,
        ),
        (Without<PaintList>, Without<PaintListContent>),
    >,
    mut texts: Query<(&PaintRowText, &mut Text)>,
    mut previous: Local<Option<super::types::PanelCache>>,
    mut starts: Local<[usize; 2]>,
) {
    use crate::ui::list_layout::{SCENE_OBJECT_ROW_PITCH, fit_scene_object_list};
    let draft = session.draft.as_ref();
    let map = draft.map(|d| d.scene_id.clone());
    if settings.scene_id != map {
        settings.scene_id = map.clone();
        settings.scope = PaintScope::Current;
        settings.group = None;
    }
    let groups = draft.map(|d| &d.manifest.vegetation_paint_groups);
    let objects = draft.and_then(|d| d.manifest.terrains.as_ref());
    let changed = previous.as_ref().is_none_or(|old| {
        old.scene_id != map
            || old.scope != settings.scope
            || old.group != settings.group
            || !same_arc(old.groups.as_ref(), groups)
            || !same_arc(old.objects.as_ref(), objects)
            || !Arc::ptr_eq(&old.hidden, &editor.terrain_hidden)
            || old.locked != editor.terrain_locked
    });
    let reset = previous.as_ref().is_none_or(|old| old.scene_id != map);
    let reveal = previous
        .as_ref()
        .is_none_or(|old| old.group != settings.group);
    if changed {
        let selected = groups.and_then(|g| {
            settings
                .group
                .as_ref()
                .and_then(|id| g.iter().position(|v| v.id == *id))
                .or_else(|| (!g.is_empty()).then_some(0))
        });
        settings.group = selected.map(|index| groups.unwrap()[index].id.clone());
        *previous = Some(super::types::PanelCache {
            scene_id: map,
            groups: groups.cloned(),
            objects: objects.cloned(),
            scope: settings.scope,
            group: settings.group.clone(),
            selected,
            hidden: editor.terrain_hidden.clone(),
            locked: editor.terrain_locked.clone(),
        });
    }
    let Some(cache) = previous.as_ref() else {
        return;
    };
    let selected = cache.selected.and_then(|i| groups.and_then(|g| g.get(i)));
    let cross = settings.scope == PaintScope::Cross;
    for (action, interaction, mut color, mut node) in &mut controls {
        let selected = matches!(action, PaintAction::Current) && !cross
            || matches!(action, PaintAction::Cross) && cross;
        let target = if selected {
            colors::SELECT_BG
        } else if *interaction == UiInteraction::Hovered {
            colors::SELECT_BG_HOVER
        } else {
            colors::SLOT_BG
        };
        if color.0 != target {
            color.0 = target;
        }
        if matches!(action, PaintAction::Bind | PaintAction::Delete) {
            let display = if cross && cache.selected.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
    }
    let counts = [
        if cross {
            groups.map_or(0, |g| g.len())
        } else {
            0
        },
        if cross && selected.is_some() {
            objects.map_or(0, |o| o.len())
        } else {
            0
        },
    ];
    let added = rows.iter().any(|(row, ..)| row.is_added());
    let old_starts = *starts;
    for (kind, mut viewport, mut scroll) in &mut views {
        if changed || added || scroll.is_changed() {
            if let Some((_, mut content)) =
                contents.iter_mut().find(|(content, _)| content.0 == kind.0)
            {
                starts[kind.0.slot()] = fit_scene_object_list(
                    counts[kind.0.slot()],
                    reset,
                    if kind.0 == ListKind::Groups && reveal {
                        cache.selected
                    } else {
                        None
                    },
                    &mut viewport,
                    &mut content,
                    &mut scroll,
                );
            }
        }
    }
    for (row, interaction, mut action, mut node, mut color) in &mut rows {
        let index = starts[row.kind.slot()] + row.slot;
        let in_range = index < counts[row.kind.slot()];
        if changed || added || old_starts != *starts {
            node.display = if in_range {
                Display::Flex
            } else {
                Display::None
            };
            node.top = px(index as f32 * SCENE_OBJECT_ROW_PITCH);
            if !in_range {
                continue;
            }
            let next = match row.kind {
                ListKind::Groups => PaintAction::Group(groups.unwrap()[index].id.clone()),
                ListKind::Targets => PaintAction::Target(objects.unwrap()[index].id.clone()),
            };
            *action = next;
        }
        let is_selected = match &*action {
            PaintAction::Group(id) => settings.group.as_ref() == Some(id),
            PaintAction::Target(id) => selected.is_some_and(|g| g.targets.contains(id)),
            _ => false,
        };
        let target = if is_selected {
            colors::SELECT_BG
        } else if *interaction == UiInteraction::Hovered {
            colors::SELECT_BG_HOVER
        } else {
            colors::SLOT_BG
        };
        if color.0 != target {
            color.0 = target;
        }
    }
    if changed || added || old_starts != *starts {
        for (row, mut text) in &mut texts {
            let index = starts[row.kind.slot()] + row.slot;
            let label = match row.kind {
                ListKind::Groups => groups.and_then(|g| g.get(index)).map(|g| {
                    format!(
                        "{} · {}目标 / {}成员",
                        g.label,
                        g.targets.len(),
                        g.members.len()
                    )
                }),
                ListKind::Targets => {
                    objects
                        .and_then(|o| o.get(index))
                        .zip(selected)
                        .map(|(object, group)| {
                            let status = if !object.enabled {
                                "已禁用"
                            } else if editor.terrain_hidden.contains(&object.id) {
                                "已隐藏"
                            } else if editor.terrain_locked.contains(&object.id) {
                                "已锁定"
                            } else if object.terrain.kind == SceneTerrainKind::Voxel {
                                "三维工具"
                            } else if group.members.contains_key(&object.id) {
                                "成员层"
                            } else {
                                "首次落笔创建"
                            };
                            format!(
                                "[{}] {} · {status}",
                                if group.targets.contains(&object.id) {
                                    "✓"
                                } else {
                                    " "
                                },
                                object.display_name
                            )
                        })
                }
            };
            if let Some(label) = label
                && text.0 != label
            {
                text.0 = label;
            }
        }
    }
}

pub(super) fn handle(
    buttons: Query<(&EditorButtonClick, &PaintAction), Changed<EditorButtonClick>>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut settings: ResMut<VegetationPaintSettings>,
    state: Res<VegetationPaintState>,
    stroke: Res<VegetationBrushStroke>,
    presets: Option<Res<VegetationPresetRegistry>>,
    guard: Res<crate::resources::SceneEditorNumericScrubMouseGuard>,
) {
    if guard.blocked() || !editor.open {
        return;
    } // 列表重建不能让同次按住重复创建绘制组。
    for (interaction, action) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        match action {
            PaintAction::Current => {
                settings.scope = PaintScope::Current;
                continue;
            }
            PaintAction::Cross => {
                settings.scope = PaintScope::Cross;
                continue;
            }
            PaintAction::Group(id) => {
                if !state.busy() {
                    settings.group = Some(id.clone());
                }
                continue;
            }
            _ => {}
        }
        if state.busy() || stroke.active || session.saving {
            editor.status = "请等待本笔植被完成，再调整绘制组".into();
            continue;
        }
        let result = edit_group(
            action,
            &mut session,
            &mut editor,
            &mut settings,
            presets.as_deref(),
        );
        if let Err(error) = result {
            editor.status = error;
        }
    }
}

fn edit_group(
    action: &PaintAction,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    settings: &mut VegetationPaintSettings,
    presets: Option<&VegetationPresetRegistry>,
) -> Result<(), String> {
    let draft = session.draft.as_mut().ok_or("场景作者数据尚未就绪")?;
    if draft.manifest.terrains.is_none() {
        return Err("跨网格绘制需要独立网格场景；旧场景仍可使用当前网格绘制".into());
    }
    let before = draft.manifest.vegetation_paint_groups.clone();
    let mut after = before.to_vec();
    match action {
        PaintAction::New => {
            let template = editor
                .vegetation_selected_preset_id
                .as_deref()
                .and_then(|id| presets?.resolve(id))
                .map(|p| p.create_scene_layer(""))
                .or_else(|| {
                    let v = draft.vegetation.as_ref()?;
                    let a = v
                        .authoring
                        .as_ref()?
                        .layers
                        .get(editor.selected_vegetation_layer)?;
                    v.layers.iter().find(|l| l.id == a.output_layer_id).cloned()
                })
                .ok_or("请先选择植被预设或已有作者层")?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let id = format!("draw-{stamp:x}");
            after.push(SceneVegetationPaintGroup {
                id: id.clone(),
                label: format!("绘制组 {} · {}", after.len() + 1, template.label),
                template,
                targets: draft
                    .terrain_documents
                    .active
                    .as_ref()
                    .map(|a| a.definition.id.clone())
                    .into_iter()
                    .collect(),
                members: BTreeMap::new(),
            });
            settings.group = Some(id);
            settings.scope = PaintScope::Cross;
        }
        PaintAction::Delete => {
            after.retain(|g| Some(&g.id) != settings.group.as_ref());
        }
        PaintAction::Target(id) => {
            let group = after
                .iter_mut()
                .find(|g| Some(&g.id) == settings.group.as_ref())
                .ok_or("请先选择绘制组")?;
            if !group.targets.remove(id) {
                group.targets.insert(id.clone());
            }
        }
        PaintAction::Bind => {
            let active = draft
                .terrain_documents
                .active
                .as_ref()
                .ok_or("请先选择当前网格")?;
            let vegetation = draft.vegetation.as_ref().ok_or("当前网格没有植被数据")?;
            let layer = vegetation
                .authoring
                .as_ref()
                .and_then(|a| a.layers.get(editor.selected_vegetation_layer))
                .ok_or("请先选择要绑定的作者层")?;
            if after
                .iter()
                .any(|g| g.members.get(&active.definition.id) == Some(&layer.id))
            {
                return Err("此作者层已经属于一个绘制组".into());
            }
            let group = after
                .iter_mut()
                .find(|g| Some(&g.id) == settings.group.as_ref())
                .ok_or("请先选择绘制组")?;
            if layer.output_layer_id != group.template.id {
                return Err("当前作者层预设与绘制组不同，不能绑定".into());
            }
            if group.members.contains_key(&active.definition.id) {
                return Err("当前网格已有本组成员，不能覆盖关联".into());
            }
            group
                .members
                .insert(active.definition.id.clone(), layer.id.clone());
            group.targets.insert(active.definition.id.clone());
        }
        _ => return Ok(()),
    }
    let after: Arc<[_]> = after.into();
    draft.manifest.vegetation_paint_groups = after.clone();
    draft.terrain_documents.paint_groups_dirty = true;
    session.push_bounded_water_history(EditorCommand::VegetationPaintChanged(Box::new(
        VegetationPaintChange {
            before_groups: before,
            after_groups: after,
            canvases: BTreeMap::new(),
        },
    )));
    session.mark_changed(editor);
    editor.status = "绘制组关联已更新；目标网格仅在真正落笔后创建本地作者层".into();
    Ok(())
}
