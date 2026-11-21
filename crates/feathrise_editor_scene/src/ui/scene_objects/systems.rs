//! 管理字段、对象/目录动作和轻量Arc差分；完整预检后共同进入一次历史。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use super::{components::*, resources::*};
use super::metadata::{METADATA_PER_FRAME, MetadataBatch, effective_metadata};
use super::participation_previews::{PREVIEW_CHANGES_PER_FRAME, ParticipationPreviews};
use crate::authoring::SceneObjectChange;
use crate::authoring::placement_participation;
use crate::authoring::{EditorAuthoringSession, EditorCommand};
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState};
use crate::ui::{
    gltf_browser::{EditorGltfBrowserEntryKind, EditorGltfBrowserState, PlacementTreeToggle, SceneEditorGltfBrowserRow},
    scene_sidebar::TreeRowToggle,
};
use crate::viewport::content_selection::LayerCandidateState;
use crate::viewport::multi_select::{PlacementSelectionSet, SelectedMember};
use bevy::input_focus::{InputFocus, tab_navigation::TabIndex};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use feathrise_scene_content::MAX_PLACEMENT_EDITOR_NAME_CHARS;
use std::sync::Arc;

fn current_flag(
    action: ObjectAction,
    record: &feathrise_scene_content::ScenePlacementRecord,
) -> bool {
    match action {
        ObjectAction::ToggleParticipation => placement_participation::participates(&record.editor),
        ObjectAction::ToggleHidden => record.editor.hidden,
        _ => record.editor.locked,
    }
}

/// 捕获只复制短ID及元数据Arc；大模型和完整水体记录不进入管理历史。
pub(super) fn advance_management(
    pending: &mut PendingManagement,
    session: &EditorAuthoringSession,
) -> Result<bool, String> {
    let draft = session.draft.as_ref().ok_or("场景草稿已离开")?;
    if pending.scene_id != draft.scene_id
        || !pending.document.matches(&session.placement_deletions)
        || pending.revision != session.revision
        || pending.structure != session.placement_structure_revision
    {
        return Err("场景或作者版本已变化，本次对象管理已取消".into());
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
    let end = (pending.cursor + METADATA_PER_FRAME).min(total);
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
                if pending.selected.len() == feathrise_scene_content::MAX_COURTYARD_MEMBERS_PER_PRESET {
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
        if pending.action == ObjectAction::ToggleParticipation && record.editor.locked {
            return Err("请先解锁选定对象或组内对象，再修改参与状态".into());
        }
        if !pending.flag_set {
            pending.flag = !current_flag(pending.action, record);
            pending.flag_set = true; // 目录以首个成员冻结共同目标，后续帧和成员不能各自反转。
        }
        let mut next = record.editor.as_ref().clone();
        match pending.action {
            ObjectAction::Rename => next.name = pending.name.clone(),
            ObjectAction::AssignGroup => next.group = pending.group.clone(),
            ObjectAction::Ungroup => next.group.clear(),
            ObjectAction::ToggleParticipation => placement_participation::set_participation(&mut next, pending.flag)?,
            ObjectAction::ToggleHidden => next.hidden = pending.flag,
            ObjectAction::ToggleLocked => next.locked = pending.flag,
            _ => return Err("无效的对象管理动作".into()),
        }
        next.validate()?;
        if next != *record.editor {
            let next = Arc::new(next);
            pending.heap_bytes += record.id.len()
                + record.editor.name.capacity()
                + record.editor.group.capacity()
                + next.name.capacity()
                + next.group.capacity();
            if pending.metadata.insert(record.id.clone(), (index, next.clone())).is_some() {
                return Err(format!("场景对象标识重复，未完成管理操作：{}", record.id));
            }
            pending.changes.push(SceneObjectChange {
                index,
                id: record.id.clone(),
                before: record.editor.clone(),
                after: next,
            });
        }
    }
    Ok(pending.cursor == total)
}

pub(super) fn adopt_management(
    pending: &mut PendingManagement,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
) -> Result<(usize, Vec<EditorCommand>, MetadataBatch), String> {
    let draft = session
        .draft
        .as_ref()
        .filter(|draft| draft.scene_id == pending.scene_id)
        .ok_or("管理对象所属场景已经变化")?;
    if !pending.document.matches(&session.placement_deletions)
        || session.revision != pending.revision
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
    let count = pending.changes.len();
    let mut retired = Vec::new();
    if count != 0 {
        // 下标及原字段按帧捕获，完整作者与结构身份接续到发布点。这里只转移候选表和历史所有权。
        retired = std::mem::take(&mut session.redo); // 旧redo深内容交由唯一后台槽回收。
        let change = crate::authoring::SceneObjectBatchChange {
            heap_bytes: pending.heap_bytes
                + pending.changes.capacity() * std::mem::size_of::<SceneObjectChange>(),
            changes: std::mem::take(&mut pending.changes),
        };
        session.push_bounded_history_retiring(EditorCommand::SceneObjectsChanged(Box::new(change)), &mut retired);
        session.mark_scene_objects_changed(editor);
    }
    Ok((count, retired, std::mem::take(&mut pending.metadata)))
}

pub(super) fn retire<T: Send + 'static>(state: &mut SceneObjectState, value: T) {
    debug_assert!(state.retirement.is_none());
    state.retirement = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
        drop(value);
    }));
}

/// 名称和分组控件按实际资格回显；显隐与锁定的入口位于场景树行。
pub(super) fn style_buttons(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    state: Res<SceneObjectState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    selection: Res<PlacementSelectionSet>,
    focus: Res<InputFocus>,
    mut controls: Query<
        (Entity, Option<&ObjectButton>, Option<&ObjectInput>, &UiInteraction,
            &mut BackgroundColor, &mut BorderColor, Has<InteractionDisabled>),
        Or<(With<ObjectButton>, With<ObjectInput>)>,
    >,
) {
    use crate::ui::theme as colors;
    let ready = editor.open
        && !editor.close_requested
        && !editor.new_scene_dialog_open
        && editor.scene_switch_confirmation.is_none()
        && !editor.scene_transition_in_progress
        && !session.saving
        && !state.busy();
    let target = browser.selected_placement_id.is_some()
        || browser.selected_group.is_some()
        || (selection.ready_for(&session) && selection.members.len() > 1);
    let single = browser.selected_placement_id.is_some()
        && browser.selected_group.is_none()
        && selection.members.len() <= 1;
    for (entity, button, field, interaction, mut background, mut border, disabled) in &mut controls {
        let enabled = ready
            && match (button, field) {
                (Some(ObjectButton(ObjectAction::Rename)), _) | (_, Some(ObjectInput(ObjectField::Name))) => {
                    single && state.retirement.is_none()
                }
                _ => target && state.retirement.is_none(),
            };
        if disabled == enabled {
            let mut control = commands.entity(entity);
            if enabled {
                control.remove::<InteractionDisabled>();
                if field.is_some() {
                    control.insert(TabIndex(0));
                }
            } else {
                // 原生焦点按 TabIndex 命中，禁用字段时也撤掉它的焦点资格。
                control.insert(InteractionDisabled).remove::<TabIndex>();
            }
        }
        let color = if !enabled {
            colors::INPUT_BG
        } else {
            match interaction {
                UiInteraction::Pressed => colors::SELECT_BG_PRESSED,
                UiInteraction::Hovered => colors::SELECT_BG_HOVER,
                UiInteraction::None => colors::SLOT_BG,
            }
        };
        if background.0 != color {
            background.0 = color;
        }
        let color = if enabled && field.is_some() && focus.get() == Some(entity) {
            colors::INPUT_ACTIVE_BORDER
        } else if enabled && *interaction != UiInteraction::None {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        };
        *border = BorderColor::all(color);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_objects(
    buttons: (
        Query<(&EditorButtonClick, &ObjectButton), Changed<EditorButtonClick>>,
        Query<
            (
                &crate::components::SceneEditorActionButton,
                &EditorButtonClick,
                Has<bevy::ui::InteractionDisabled>,
            ),
            Changed<EditorButtonClick>,
        >,
        Query<
            (Entity, &EditorButtonClick, &PlacementTreeToggle, &ChildOf),
            (Changed<EditorButtonClick>, Without<InteractionDisabled>),
        >,
    ),
    mut fields: Query<(Entity, &ObjectInput, &mut EditableText)>,
    mut summaries: Query<&mut Text, With<ObjectSummary>>,
    mut focus: ResMut<InputFocus>,
    mut state: ResMut<SceneObjectState>,
    mut browser: ResMut<EditorGltfBrowserState>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut selection: ResMut<PlacementSelectionSet>,
    mut candidates: ResMut<LayerCandidateState>,
    guards: (
        Res<SceneEditorNumericScrubMouseGuard>,
        Res<crate::viewport::placement_move::PlacementMoveState>,
        Res<crate::authoring::placement_batch::PlacementBatchState>,
    ),
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    tree: (Query<&SceneEditorGltfBrowserRow>, Query<(&Node, Option<&ChildOf>)>),
    mut previews: ParticipationPreviews,
) {
    let (mouse, moving, batch) = guards;
    let mut preview_budget = PREVIEW_CHANGES_PER_FRAME;
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
    if state.history.is_some() { state.check_metadata_history(&session, &mut editor); }
    if state.metadata_write.is_some() {
        state.write_metadata(&mut session, &browser, Some((&mut previews, &mut preview_budget)));
    }
    if let Some(mut pending) = state.pending.take() {
        if editor.open && !editor.close_requested && !editor.scene_transition_in_progress
            && editor.pending_scene_transition.is_none() && editor.scene_switch_confirmation.is_none()
            && editor.selected_scene_id.as_ref() == Some(&pending.scene_id) {
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
                Ok(true) => {
                    let protects_objects = match pending.action {
                        ObjectAction::ToggleHidden | ObjectAction::ToggleLocked => pending.flag,
                        ObjectAction::ToggleParticipation => !pending.flag,
                        _ => false,
                    };
                    let clear_selection = protects_objects && selection.members.iter()
                        .any(|member| pending.metadata.contains_key(&member.id));
                    match adopt_management(&mut pending, &mut session, &mut editor) {
                        Ok((count, retired, metadata)) => {
                            state.publish_metadata(&session, metadata);
                            if count != 0 {
                                state.write_metadata(&mut session, &browser, Some((&mut previews, &mut preview_budget)));
                            }
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
                                if clear_selection {
                                    selection.clear();
                                }
                                candidates.clear_for_scene_tree();
                            }
                        }
                        Err(error) => {
                            editor.status = error;
                            retire(&mut state, pending);
                        }
                    }
                }
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
            draft.scene_id.clone(),
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
        if let Some(draft) = session.draft.as_ref()
            && let Some(id) = browser.selected_placement_id.as_deref()
            && let Some(index) =
                browser.placement_record_index(id, session.placement_structure_revision)
            && let Some(record) = draft.placements.placements.get(index)
                .filter(|r| r.id == id)
        {
            let metadata = effective_metadata(Some(&state), &session, record);
            let (name, group) = (metadata.name.clone(), metadata.group.clone());
            state.name = name;
            state.group = group;
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
            ObjectField::Name => &state.name,
            ObjectField::Group => &state.group,
        };
        if editor.open && focus.get() == Some(entity) {
            let value = input.value().to_string();
            match kind {
                ObjectField::Name => state.name = value,
                ObjectField::Group => state.group = value,
            }
        } else if input.value().to_string() != *desired {
            // 切换对象时保留 Bevy 已同步的字体与布局，只替换文本值。
            crate::ui::text_input::replace_single_line_value(&mut input, desired, MAX_PLACEMENT_EDITOR_NAME_CHARS);
        }
    }
    let context = if let Some(group) = &browser.selected_group {
        format!("当前组：{group}")
    } else if selection.members.len() > 1 {
        format!("管理选集：{}个对象", selection.members.len())
    } else if let Some(id) = &browser.selected_placement_id {
        let metadata = session.draft.as_ref().and_then(|draft| {
            let index = browser.placement_record_index(id, session.placement_structure_revision)?;
            let record = draft.placements.placements.get(index)?;
            Some(effective_metadata(Some(&state), &session, record))
        });
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
        || editor.new_scene_dialog_open
        || editor.scene_switch_confirmation.is_some()
        || editor.scene_transition_in_progress
        || session.saving
        || state.busy()
        || moving.owns_mouse()
        || batch.owns_mouse()
        || mouse.blocked()
    {
        return;
    }
    // 行内操作按释放时仍匹配的行来提交，不借用右侧属性栏当前选择。
    let direct = buttons.2.iter().find_map(|(entity, click, button, parent)| {
        if !click.is_clicked() || !crate::viewport::modes::controls_visible(entity, &tree.1) { return None; }
        let row = tree.0.get(parent.parent()).ok()?;
        let targets = match button.target(&browser, &session, row)? {
            EditorGltfBrowserEntryKind::Placement { placement_id, .. } => ObjectTargets::Indices(vec![
                browser.placement_record_index(placement_id, session.placement_structure_revision)?,
            ]),
            EditorGltfBrowserEntryKind::Group { name, .. } => ObjectTargets::Group(name.clone()),
            _ => return None,
        };
        let action = match button.kind {
            TreeRowToggle::Participation => ObjectAction::ToggleParticipation,
            TreeRowToggle::Visibility => ObjectAction::ToggleHidden,
            TreeRowToggle::Lock => ObjectAction::ToggleLocked,
        };
        Some((action, Some(targets)))
    });
    let Some((action, direct_targets)) = direct.or_else(|| buttons.0.iter().find_map(|(interaction, button)| {
        interaction.is_clicked().then_some((button.0, None))
    })) else {
        return;
    };
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
        .filter(|draft| editor.selected_scene_id.as_ref() == Some(&draft.scene_id))
    else {
        editor.status = "请先打开一个场景".into();
        return;
    };
    let targets = if let Some(targets) = direct_targets {
        targets
    } else if let Some(group) = &browser.selected_group {
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
    let flag = match &targets {
        ObjectTargets::Indices(indices) => draft
            .placements
            .placements
            .get(indices[0])
            .map(|record| current_flag(action, record))
            .unwrap_or(false),
        ObjectTargets::Group(_) => false,
    };
    let flag_set = matches!(targets, ObjectTargets::Indices(_));
    state.pending = Some(PendingManagement {
        scene_id: draft.scene_id.clone(),
        document: session.placement_deletions.identity(),
        revision: session.revision,
        structure: session.placement_structure_revision,
        terrain_revision: session.terrain_revision,
        derived_pose_revision: session.placement_derived_pose_revision,
        targets,
        action,
        name: state.name.trim().to_string(),
        group: state.group.trim().to_string(),
        flag: !flag,
        cursor: 0,
        flag_set,
        changes: Vec::new(),
        metadata: Default::default(),
        heap_bytes: 0,
        selected: Vec::new(),
    });
}
