//! 管理字段、对象/目录动作和轻量Arc差分；完整预检后共同进入一次历史。

use feathrise_editor_ui::EditorButtonClick;
use super::{components::*, resources::*};
use crate::authoring::SceneObjectChange;
use crate::authoring::{EditorAuthoringSession, EditorCommand};
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState};
use crate::ui::gltf_browser::EditorGltfBrowserState;
use crate::viewport::content_selection::LayerCandidateState;
use crate::viewport::multi_select::{PlacementSelectionSet, SelectedMember};
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use std::sync::Arc;

/// 捕获只复制短ID及元数据Arc；大模型和完整水体记录不进入管理历史。
pub(super) fn advance_management(
    pending: &mut PendingManagement,
    session: &EditorAuthoringSession,
) -> Result<bool, String> {
    let draft = session.draft.as_ref().ok_or("地图草稿已离开")?;
    if pending.map != draft.map_id
        || pending.revision != session.revision
        || pending.structure != session.placement_structure_revision
    {
        return Err("地图或作者版本已变化，本次对象管理已取消".into());
    }
    // 跨帧世界锚点必须来自同一完整姿态；不能给旧前缀盖上新代次再发布选集。
    if pending.action == ObjectAction::SelectGroup
        && (pending.terrain_revision != session.terrain_revision
            || pending.derived_pose_revision != session.placement_derived_pose_revision)
    {
        return Err("对象世界姿态已变化，本次选组已取消，原选择已保留".into());
    }
    let total = match &pending.targets {
        ObjectTargets::Indices(indices) => indices.len(),
        ObjectTargets::Group(_) => draft.placements.placements.len(),
    };
    let end = (pending.cursor + 128).min(total);
    while pending.cursor < end {
        let index = match &pending.targets {
            ObjectTargets::Indices(indices) => indices[pending.cursor],
            ObjectTargets::Group(_) => pending.cursor,
        };
        let record = draft
            .placements
            .placements
            .get(index)
            .ok_or("管理对象下标已经失效")?;
        pending.cursor += 1;
        if matches!(&pending.targets, ObjectTargets::Group(group) if record.editor.group != *group)
        {
            continue;
        }
        if pending.action == ObjectAction::SelectGroup {
            if record.building_preset_id.is_none()
                && record.editor.selectable()
                && !record
                    .terrain_patch
                    .as_ref()
                    .is_some_and(|p| p.sculpted_volume)
            {
                // 目录选组与视口框选共用世界原点；失效独立源不再提供可变换的选择资格。
                let Ok(position) = crate::authoring::placement_pose::placement_record_world_position(draft, record) else { continue; };
                if pending.selected.len() == feathrise_map_content::MAX_COURTYARD_MEMBERS_PER_PRESET {
                    return Err("组内可选对象超过256项；请分批选择，原选择已保留".into());
                }
                pending.selected.push(SelectedMember {
                    id: record.id.clone(),
                    index,
                    position,
                });
            }
            continue;
        }
        let mut next = record.editor.as_ref().clone();
        if !pending.flag_set {
            pending.flag = !(if pending.action == ObjectAction::ToggleHidden {
                record.editor.hidden
            } else {
                record.editor.locked
            });
            pending.flag_set = true; // 目录以首个成员冻结共同目标，后续帧和成员不能各自反转。
        }
        match pending.action {
            ObjectAction::Rename => next.name = pending.name.clone(),
            ObjectAction::AssignGroup => next.group = pending.group.clone(),
            ObjectAction::Ungroup => next.group.clear(),
            ObjectAction::ToggleHidden => next.hidden = pending.flag,
            ObjectAction::ToggleLocked => next.locked = pending.flag,
            _ => return Err("无效的对象管理动作".into()),
        }
        next.validate()?;
        if next != *record.editor {
            pending.heap_bytes += record.id.len()
                + record.editor.name.capacity()
                + record.editor.group.capacity()
                + next.name.capacity()
                + next.group.capacity();
            pending.changes.push(SceneObjectChange {
                index,
                id: record.id.clone(),
                before: record.editor.clone(),
                after: Arc::new(next),
            });
        }
    }
    Ok(pending.cursor == total)
}

pub(super) fn adopt_management(
    pending: &mut PendingManagement,
    session: &mut EditorAuthoringSession,
    editor: &mut MapEditorState,
) -> Result<(usize, Vec<EditorCommand>), String> {
    let draft = session
        .draft
        .as_mut()
        .filter(|draft| draft.map_id == pending.map)
        .ok_or("管理对象所属地图已经变化")?;
    if session.revision != pending.revision
        || session.placement_structure_revision != pending.structure
    {
        return Err("作者版本已经变化，未应用任何对象管理数据".into());
    }
    let total = match &pending.targets {
        ObjectTargets::Indices(indices) => indices.len(),
        ObjectTargets::Group(_) => draft.placements.placements.len(),
    };
    if pending.cursor != total {
        return Err("场景对象尚未收集完成，未应用任何管理数据".into());
    }
    // 下标、稳定ID及Arc身份全通过才替换小型字段；原值继续由历史持有，不集中析构大资源。
    for change in &pending.changes {
        let record = draft
            .placements
            .placements
            .get(change.index)
            .filter(|record| record.id == change.id)
            .ok_or("对象已被删除或重排")?;
        if !Arc::ptr_eq(&record.editor, &change.before) {
            return Err("对象管理元数据已经变化".into());
        }
    }
    let count = pending.changes.len();
    let mut retired = Vec::new();
    if count != 0 {
        for change in &pending.changes {
            draft.placements.placements[change.index].editor = change.after.clone();
        }
        retired = std::mem::take(&mut session.redo); // 旧redo深内容交由唯一后台槽回收。
        let change = crate::authoring::SceneObjectBatchChange {
            heap_bytes: pending.heap_bytes
                + pending.changes.capacity() * std::mem::size_of::<SceneObjectChange>(),
            changes: std::mem::take(&mut pending.changes),
        };
        session.push_bounded_water_history(EditorCommand::SceneObjectsChanged(Box::new(change))); // 裁剪完整历史前缀时同步移动多地形归属游标。
        session.mark_scene_objects_changed(editor);
    }
    Ok((count, retired))
}

fn retire<T: Send + 'static>(state: &mut SceneObjectState, value: T) {
    debug_assert!(state.retirement.is_none());
    state.retirement = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
        drop(value);
    }));
}

/// 固定六个操作和三个文本入口只按实际资格回显；保护对象始终保留恢复入口。
pub(super) fn style_buttons(
    mut commands: Commands,
    editor: Res<MapEditorState>,
    state: Res<SceneObjectState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    selection: Res<PlacementSelectionSet>,
    mut searches: Query<&mut Node, With<SceneAssetSearch>>,
    mut buttons: Query<(
        Entity,
        &ObjectButton,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
        Has<bevy::ui::InteractionDisabled>,
    )>,
) {
    use crate::ui::theme as colors;
    // 没有摆放资产时不保留空搜索块；有搜索词时继续显示，避免过滤到零条后无法清除。
    let search_visible = !browser.object_search.is_empty() || session.draft.as_ref().is_some_and(|draft| !draft.placements.placements.is_empty());
    for mut node in &mut searches {
        let display = if search_visible { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    let ready = editor.open
        && !editor.close_requested
        && !editor.map_transition_in_progress
        && !session.saving
        && !state.busy();
    let target = browser.selected_placement_id.is_some()
        || browser.selected_group.is_some()
        || (selection.ready_for(&session) && selection.members.len() > 1);
    let single = browser.selected_placement_id.is_some()
        && browser.selected_group.is_none()
        && selection.members.len() <= 1;
    for (entity, button, interaction, mut background, mut border, disabled) in &mut buttons {
        let enabled = ready
            && match button.0 {
                ObjectAction::Focus(ObjectField::Search) => true,
                ObjectAction::Rename | ObjectAction::Focus(ObjectField::Name) => {
                    single && state.retirement.is_none()
                }
                _ => target && state.retirement.is_none(),
            };
        if disabled == enabled {
            if enabled {
                commands
                    .entity(entity)
                    .remove::<bevy::ui::InteractionDisabled>();
            } else {
                commands
                    .entity(entity)
                    .insert(bevy::ui::InteractionDisabled);
            }
        }
        let color = if !enabled {
            colors::INPUT_BG
        } else {
            match interaction {
                Interaction::Pressed => colors::SELECT_BG_PRESSED,
                Interaction::Hovered => colors::SELECT_BG_HOVER,
                Interaction::None => colors::SLOT_BG,
            }
        };
        if background.0 != color {
            background.0 = color;
        }
        let color = if enabled && *interaction != Interaction::None {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        };
        *border = BorderColor::all(color);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_objects(
    mut commands: Commands,
    buttons: (
        Query<(&EditorButtonClick, &ObjectButton), Changed<EditorButtonClick>>,
        Query<
            (
                &crate::components::MapEditorActionButton,
                &EditorButtonClick,
                Has<bevy::ui::InteractionDisabled>,
            ),
            Changed<EditorButtonClick>,
        >,
    ),
    mut fields: Query<(Entity, &ObjectInput, &mut EditableText)>,
    mut summaries: Query<&mut Text, With<ObjectSummary>>,
    mut focus: ResMut<InputFocus>,
    mut state: ResMut<SceneObjectState>,
    mut browser: ResMut<EditorGltfBrowserState>,
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut selection: ResMut<PlacementSelectionSet>,
    mut candidates: ResMut<LayerCandidateState>,
    guards: (
        Res<MapEditorNumericScrubMouseGuard>,
        Res<crate::viewport::placement_move::PlacementMoveState>,
        Res<crate::authoring::placement_batch::PlacementBatchState>,
    ),
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut initialized: Local<std::collections::BTreeSet<Entity>>,
) {
    let (mouse, moving, batch) = guards;
    if let Some(task) = state.retirement.as_mut()
        && bevy::tasks::futures::check_ready(task).is_some()
    {
        state.retirement = None;
    }
    if state.busy() && editor.open {
        if keyboard.just_pressed(KeyCode::F12) {
            editor.toggle_close_request();
            keyboard.clear_just_pressed(KeyCode::F12);
        } else if buttons.1.iter().any(|(button, interaction, disabled)| {
            button.0 == crate::types::EditorAction::Close
                && interaction.is_clicked()
                && !disabled
        }) {
            editor.request_close();
        }
    } // 批量捕获也必须允许关闭；通用输入门不能永久等待自己的任务。
    if let Some(mut pending) = state.pending.take() {
        if editor.open && !editor.close_requested {
            match advance_management(&mut pending, &session) {
                Ok(false) => {
                    editor.status = format!("正在整理场景对象：已检查{}项", pending.cursor);
                    state.pending = Some(pending);
                }
                Ok(true) if pending.action == ObjectAction::SelectGroup => {
                    if pending.selected.is_empty() {
                        editor.status = "此组没有可选择的场景对象，建筑请使用单栋或院落工具".into();
                    } else {
                        selection.publish(pending.selected, &session);
                        candidates.freeze_selection();
                        crate::viewport::multi_select::publish_primary(
                            &selection,
                            &mut browser,
                            &mut candidates,
                        );
                        editor.status = format!("已选择{}个组内场景对象", selection.members.len());
                    }
                }
                Ok(true) => match adopt_management(&mut pending, &mut session, &mut editor) {
                    Ok((count, retired)) => {
                        if !retired.is_empty() {
                            retire(&mut state, retired);
                        }
                        editor.status = if count == 0 {
                            "对象管理值未变化".into()
                        } else {
                            format!("已更新{count}个场景对象，可一次撤销")
                        };
                        if count != 0 {
                            if pending.action == ObjectAction::AssignGroup {
                                browser.selected_group =
                                    (!pending.group.is_empty()).then(|| pending.group.clone());
                                if browser.selected_group.is_some() {
                                    browser.selected_placement_id = None;
                                    browser.selection = None;
                                }
                            } else if pending.action == ObjectAction::Ungroup {
                                browser.selected_group = None;
                            }
                            if matches!(
                                pending.action,
                                ObjectAction::ToggleHidden | ObjectAction::ToggleLocked
                            ) && pending.flag
                            {
                                selection.clear();
                            }
                            candidates.clear_for_scene_tree();
                        }
                    }
                    Err(error) => {
                        editor.status = error;
                        retire(&mut state, pending);
                    }
                },
                Err(error) => {
                    editor.status = error;
                    retire(&mut state, pending);
                }
            }
        } else {
            retire(&mut state, pending);
        } // F12取消不改变任何记录；清理在后台继续，不占关闭门。
    }
    let current = session.draft.as_ref().map(|draft| {
        (
            draft.map_id.clone(),
            browser.selected_placement_id.clone(),
            browser.selected_group.clone(),
            session.scene_objects_revision,
            selection.revision,
        )
    });
    if state.bound != current {
        state.bound = current;
        state.name.clear();
        state.group = browser.selected_group.clone().unwrap_or_default();
        if let Some(id) = browser.selected_placement_id.as_deref()
            && let Some(index) =
                browser.placement_record_index(id, session.placement_structure_revision)
            && let Some(record) = session
                .draft
                .as_ref()
                .and_then(|draft| draft.placements.placements.get(index))
                .filter(|r| r.id == id)
        {
            state.name = record.editor.name.clone();
            state.group = record.editor.group.clone();
        }
        if fields
            .iter()
            .any(|(entity, _, _)| focus.get() == Some(entity))
        {
            focus.clear();
        }
    }
    for (entity, kind, mut input) in &mut fields {
        let kind = kind.0;
        let desired = match kind {
            ObjectField::Search => &browser.object_search,
            ObjectField::Name => &state.name,
            ObjectField::Group => &state.group,
        };
        if initialized.insert(entity) {
            *input = crate::ui::text_input::single_line_input(desired, 128);
            commands
                .entity(entity)
                .insert(bevy::text::LineHeight::Px(18.0));
        } else if editor.open && focus.get() == Some(entity) {
            let value = input.value().to_string();
            match kind {
                ObjectField::Search if value != browser.object_search => {
                    browser.object_search = value;
                    browser.request_object_rows();
                }
                ObjectField::Name => state.name = value,
                ObjectField::Group => state.group = value,
                _ => {}
            }
        } else if input.value().to_string() != *desired {
            *input = crate::ui::text_input::single_line_input(desired, 128);
        }
    }
    let context = if let Some(group) = &browser.selected_group {
        format!("当前组：{group}")
    } else if selection.members.len() > 1 {
        format!("管理选集：{}个对象", selection.members.len())
    } else if let Some(id) = &browser.selected_placement_id {
        let metadata = browser
            .placement_record_index(id, session.placement_structure_revision)
            .and_then(|index| session.draft.as_ref()?.placements.placements.get(index))
            .map(|record| &record.editor);
        format!(
            "当前对象：{id} · {} · {}",
            if metadata.is_some_and(|m| m.hidden) {
                "已隐藏"
            } else {
                "可见"
            },
            if metadata.is_some_and(|m| m.locked) {
                "已锁定"
            } else {
                "可编辑"
            }
        )
    } else {
        "选择对象或目录后进行管理".into()
    };
    for mut text in &mut summaries {
        if text.0 != context {
            text.0 = context.clone();
        }
    }
    if !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || editor.map_switch_confirmation.is_some()
        || editor.map_transition_in_progress
        || session.saving
        || state.busy()
        || moving.owns_mouse()
        || batch.owns_mouse()
        || mouse.blocked()
    {
        return;
    }
    let Some(action) = buttons.0.iter().find_map(|(interaction, button)| {
        (interaction.is_clicked()).then_some(button.0)
    }) else {
        return;
    };
    if let ObjectAction::Focus(kind) = action {
        if let Some((entity, _, _)) = fields.iter().find(|(_, field, _)| field.0 == kind) {
            focus.set(entity, FocusCause::Navigated);
        }
        return;
    }
    if state.retirement.is_some() {
        editor.status = "上一批对象管理缓存正在后台释放，请稍候重试".into();
        return;
    }
    if fields
        .iter()
        .any(|(entity, _, input)| focus.get() == Some(entity) && input.is_composing())
    {
        editor.status = "请先确认对象名称或组名的输入法候选".into();
        return;
    }
    focus.clear();
    let Some(draft) = session
        .draft
        .as_ref()
        .filter(|draft| editor.selected_map_id.as_ref() == Some(&draft.map_id))
    else {
        editor.status = "请先打开一张地图".into();
        return;
    };
    let targets = if let Some(group) = &browser.selected_group {
        if action == ObjectAction::Rename {
            editor.status = "请选择单个对象后修改名称；组名使用应用组名".into();
            return;
        }
        ObjectTargets::Group(group.clone())
    } else if selection.ready_for(&session) && selection.members.len() > 1 {
        if action == ObjectAction::Rename {
            editor.status = "名称属于单个对象，请先单选".into();
            return;
        }
        ObjectTargets::Indices(selection.members.iter().map(|m| m.index).collect())
    } else if let Some(index) = browser
        .selected_placement_id
        .as_deref()
        .and_then(|id| browser.placement_record_index(id, session.placement_structure_revision))
    {
        ObjectTargets::Indices(vec![index])
    } else {
        editor.status = "请先选择要管理的对象或目录".into();
        return;
    };
    let targets = if action == ObjectAction::SelectGroup {
        let group = match &targets {
            ObjectTargets::Group(group) => group.clone(),
            ObjectTargets::Indices(indices) => {
                draft.placements.placements[indices[0]].editor.group.clone()
            }
        };
        if group.is_empty() {
            editor.status = "当前对象尚未归组".into();
            return;
        }
        ObjectTargets::Group(group)
    } else {
        targets
    };
    let current_flag = match &targets {
        ObjectTargets::Indices(indices) => draft
            .placements
            .placements
            .get(indices[0])
            .map(|r| {
                if action == ObjectAction::ToggleHidden {
                    r.editor.hidden
                } else {
                    r.editor.locked
                }
            })
            .unwrap_or(false),
        ObjectTargets::Group(_) => false,
    };
    let flag_set = matches!(targets, ObjectTargets::Indices(_));
    state.pending = Some(PendingManagement {
        map: draft.map_id.clone(),
        revision: session.revision,
        structure: session.placement_structure_revision,
        terrain_revision: session.terrain_revision,
        derived_pose_revision: session.placement_derived_pose_revision,
        targets,
        action,
        name: state.name.trim().to_string(),
        group: state.group.trim().to_string(),
        flag: !current_flag,
        cursor: 0,
        flag_set,
        changes: Vec::new(),
        heap_bytes: 0,
        selected: Vec::new(),
    });
}
