//! 空气墙单对象事务：保留稳定ID与列表位置，避免每个操作复制整个场景。
use crate::authoring::{EditorAuthoringSession, EditorCommand};
use crate::resources::SceneEditorState;
use feathrise_scene_content::{SceneBoundariesAsset, SceneBoundaryPath};

pub(crate) fn apply_path_version(
    document: &mut SceneBoundariesAsset,
    index: usize,
    previous: Option<&SceneBoundaryPath>,
    next: Option<&SceneBoundaryPath>,
) -> Result<(), String> {
    let id = previous.or(next).ok_or("空的空气墙事务")?.id.as_str(); // 一条命令只处理一个稳定ID。
    if next.is_some_and(|path| path.id != id) {
        return Err("空气墙事务不能改变稳定ID".into());
    }
    let current = document.paths.iter().position(|path| path.id == id); // 对象顺序变化仍按ID找到原墙。
    if current.and_then(|i| document.paths.get(i)) != previous {
        return Err("空气墙已被其他操作修改，拒绝覆盖".into());
    }
    match (current, next) {
        (Some(i), Some(path)) => document.paths[i] = path.clone(), // 仅复制一面墙的控制点。
        (Some(i), None) => {
            document.paths.remove(i);
        } // 删除由同一事务可逆恢复。
        (None, Some(path)) => document
            .paths
            .insert(index.min(document.paths.len()), path.clone()), // 恢复原顺序。
        (None, None) => {}
    }
    Ok(())
}

pub(crate) fn commit_path_change(
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    index: usize,
    before: Option<SceneBoundaryPath>,
    after: Option<SceneBoundaryPath>,
    status: impl Into<String>,
) -> Result<(), String> {
    if before == after {
        return Ok(());
    } // 无变化不制造撤销记录。
    let document = session
        .draft
        .as_mut()
        .and_then(|draft| draft.boundaries.as_mut())
        .ok_or("空气墙草稿尚未就绪")?;
    let id = before
        .as_ref()
        .or(after.as_ref())
        .ok_or("空事务")?
        .id
        .as_str();
    let current = document.paths.iter().find(|path| path.id == id);
    if current != after.as_ref() {
        apply_path_version(document, index, before.as_ref(), after.as_ref())?;
    } // 实时拖动已写入最终值时只登记历史。
    session.undo.push(EditorCommand::BoundaryPathChanged {
        index,
        before,
        after,
    }); // 对象级历史不持有全图快照。
    session.redo.clear(); // 新分支淘汰重做。
    session.mark_boundaries_changed(editor); // 更新保存与碰撞期望版本。
    editor.status = status.into();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;

    fn document() -> SceneBoundariesAsset {
        let map = feathrise_scene_content::SceneId::parse("MAP-wall-history").unwrap();
        SceneBoundariesAsset::rectangular_default(&map, [-50.0; 2], [50.0; 2], 0.0, 20.0).unwrap()
    }

    #[test]
    fn create_delete_and_inverse_restore_original_order() {
        let mut doc = document();
        let first = doc.paths[0].clone();
        let mut second = first.clone();
        second.id = "second".into();
        apply_path_version(&mut doc, 1, None, Some(&second)).unwrap(); // 新建对象。
        apply_path_version(&mut doc, 0, Some(&first), None).unwrap(); // 删除首对象。
        assert_eq!(doc.paths, vec![second.clone()]);
        apply_path_version(&mut doc, 0, None, Some(&first)).unwrap(); // 撤销删除恢复原序。
        assert_eq!(doc.paths, vec![first, second.clone()]);
        apply_path_version(&mut doc, 1, Some(&second), None).unwrap(); // 撤销新建只移除该ID。
        assert_eq!(doc.paths.len(), 1);
    }

    #[test]
    fn stale_index_uses_id_but_stale_value_cannot_overwrite() {
        let mut doc = document();
        let before = doc.paths[0].clone();
        let mut other = before.clone();
        other.id = "other".into();
        doc.paths.insert(0, other.clone()); // 索引重排。
        let mut after = before.clone();
        after.label = "改名".into();
        apply_path_version(&mut doc, 0, Some(&before), Some(&after)).unwrap();
        assert_eq!(doc.paths[0], other); // 原索引现在属于其他墙，不应覆盖。
        assert_eq!(doc.paths[1], after);
        assert!(apply_path_version(&mut doc, 0, Some(&before), None).is_err()); // 旧值不能删除新分支。
        assert_eq!(doc.paths.len(), 2);
    }

    #[test]
    fn already_applied_drag_registers_one_single_wall_command() {
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        let mut doc = document();
        doc.scene_id = draft.scene_id.clone();
        let before = doc.paths[0].clone();
        let mut after = before.clone();
        after.thickness = 5.0;
        doc.paths[0] = after.clone(); // 模拟拖动实时终值。
        draft.boundaries = Some(doc);
        let mut session = EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        };
        let mut editor = SceneEditorState::default();
        commit_path_change(
            &mut session,
            &mut editor,
            0,
            Some(before.clone()),
            Some(after.clone()),
            "完成",
        )
        .unwrap();
        assert_eq!(session.undo.len(), 1);
        assert!(
            matches!(&session.undo[0], EditorCommand::BoundaryPathChanged { before: Some(old), after: Some(new), .. } if *old == before && *new == after)
        );
        assert_eq!(
            session
                .draft
                .as_ref()
                .unwrap()
                .boundaries
                .as_ref()
                .unwrap()
                .paths[0],
            after
        );
    }
}
