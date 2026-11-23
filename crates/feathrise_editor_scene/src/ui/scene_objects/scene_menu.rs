//! 对象菜单的名称、显隐及锁定进入原单对象管理事务，不借当前多选或组名。

use super::{
    SceneObjectState,
    components::ObjectAction,
    resources::{ObjectTargets, PendingManagement},
};
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand, placement_folders},
    resources::SceneEditorState,
    ui::scene_context_menu::{SceneContextMenuState, SceneMenuAction, SceneObjectKey, SceneObjectTarget},
};
use bevy::prelude::*;

pub(super) fn input(
    menu: Option<Res<SceneContextMenuState>>,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut state: ResMut<SceneObjectState>,
) {
    let Some(request) = menu.as_ref().and_then(|menu| menu.ready.as_ref()) else {
        return;
    };
    let SceneObjectKey::Placement { index, .. } = &request.target.key else {
        return;
    };
    let action = match request.action {
        SceneMenuAction::Rename => ObjectAction::Rename,
        SceneMenuAction::Visibility => ObjectAction::ToggleHidden,
        SceneMenuAction::Lock => ObjectAction::ToggleLocked,
        _ => return,
    };
    if !request.target.current(&editor, &session)
        || !request.target.allows(request.action)
        || state.busy()
        || state.retirement.is_some()
    {
        return;
    }
    queue_management(&mut state, &session, &request.target, *index, action, request.name.clone(), String::new(),
        if request.action == SceneMenuAction::Visibility { !request.target.hidden } else { !request.target.locked });
}

/// 目录拖放更新摆放的轻量管理字段，沿用已有整笔历史与分帧采用。
pub(crate) fn queue_group_drop(
    state: &mut SceneObjectState, session: &EditorAuthoringSession,
    source: &SceneObjectTarget, group: Option<String>,
) -> bool {
    let SceneObjectKey::Placement { index, id, mesh: None } = &source.key else { return false; };
    let Some(record) = session.draft.as_ref().filter(|draft| draft.scene_id == source.scene_id)
        .and_then(|draft| draft.placements.placements.get(*index)).filter(|record| record.id == *id) else { return false; };
    let group = group.unwrap_or_default();
    if !state.accepts_commands() || record.editor.locked || record.editor.group == group { return false; }
    let action = if group.is_empty() { ObjectAction::Ungroup } else { ObjectAction::AssignGroup };
    queue_management(state, session, source, *index, action, String::new(), group, false);
    true
}

pub(crate) fn create_folder(
    state: &mut SceneObjectState, session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState, name: &str, parent: Option<&str>,
) -> Result<(), String> {
    if !state.accepts_commands() { return Err("请等待上一项场景对象修改完成".into()); }
    placement_folders::validate_name(name)?;
    let draft = session.draft.as_mut().ok_or("还没有可编辑的场景")?;
    if draft.object_folders.contains(name) { return Err("已存在同名文件夹".into()); }
    if let Some(parent) = parent {
        // 复用父级变更事务。撤销时会一起移除这次新建的子文件夹。
        return move_folder(state, session, editor, name, Some(parent)).map(|_| ());
    }
    std::sync::Arc::make_mut(&mut draft.object_folders).insert(name.to_owned());
    let mut retired = std::mem::take(&mut session.redo);
    session.push_bounded_history_retiring(EditorCommand::PlacementFolderAdded(name.to_owned()), &mut retired);
    if !retired.is_empty() { super::systems::retire(state, retired); }
    session.mark_placement_structure_changed(editor);
    Ok(())
}

pub(crate) fn remove_empty_folder(
    state: &mut SceneObjectState, session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState, name: &str,
) -> Result<(), String> {
    if !state.accepts_commands() { return Err("请等待上一项场景对象修改完成".into()); }
    let draft = session.draft.as_mut().ok_or("还没有可编辑的场景")?;
    let change = draft.object_folders.prepare_empty_removal(name)?;
    change.apply(&mut draft.object_folders, true)?;
    let mut retired = std::mem::take(&mut session.redo);
    session.push_bounded_history_retiring(EditorCommand::PlacementFolderRemoved(change), &mut retired);
    if !retired.is_empty() { super::systems::retire(state, retired); }
    session.mark_changed(editor);
    Ok(())
}

/// 拖放整夹只换一条父级边，GLB 的直属归类和位姿由原摆放记录继续持有。
pub(crate) fn move_folder(
    state: &mut SceneObjectState, session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState, name: &str, parent: Option<&str>,
) -> Result<bool, String> {
    if !state.accepts_commands() { return Err("请等待上一项场景对象修改完成".into()); }
    let draft = session.draft.as_mut().ok_or("还没有可编辑的场景")?;
    let Some(change) = draft.object_folders.prepare_move(name, parent)? else { return Ok(false); };
    change.apply(&mut draft.object_folders, false)?;
    let mut retired = std::mem::take(&mut session.redo);
    session.push_bounded_history_retiring(EditorCommand::PlacementFolderMoved(change), &mut retired);
    if !retired.is_empty() { super::systems::retire(state, retired); }
    session.mark_changed(editor); // 浏览器用文件夹快照身份刷新树行，沿用原模型签名。
    Ok(true)
}

fn queue_management(
    state: &mut SceneObjectState, session: &EditorAuthoringSession, target: &SceneObjectTarget,
    index: usize, action: ObjectAction, name: String, group: String, flag: bool,
) {
    state.pending = Some(PendingManagement {
        scene_id: target.scene_id.clone(),
        document: session.placement_deletions.identity(),
        revision: session.revision,
        structure: session.placement_structure_revision,
        terrain_revision: session.terrain_revision,
        derived_pose_revision: session.placement_derived_pose_revision,
        targets: ObjectTargets::Indices(vec![index]),
        action,
        name,
        group,
        flag,
        flag_set: true,
        cursor: 0,
        changes: Vec::new(),
        metadata: Default::default(),
        heap_bytes: 0,
        selected: Vec::new(),
    });
}
