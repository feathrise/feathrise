//! 非活动动画源也由原GLB保存事务收口，未插键姿态必须回到原编辑器显式处理。

use super::AnimationEditorState;
use feathrise_editor_animation_model::assembly::AnimationOperations;
use crate::standalone::project_close::types::{ProjectCloseSnapshot, SaveItem, SaveTarget};
use bevy::prelude::*;
use std::path::Path;

pub(crate) fn collect(
    world: &World,
    snapshot: &mut ProjectCloseSnapshot,
    items: &mut Vec<SaveItem>,
) {
    let Some(state) = world.get_resource::<AnimationEditorState>() else {
        return;
    };
    if state.busy() {
        snapshot.pending("动画读取、编辑或GLB保存尚未完成");
    }
    if state.drag.is_some() || state.gesture.is_some() || state.pose_mouse_owned {
        snapshot.blocked("请先结束动画时间轴或骨骼拖动");
    }
    // HashMap迭代顺序不能影响保存次序与失败项显示。
    let mut sessions = state.sessions.iter().collect::<Vec<_>>();
    sessions.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (path, session) in sessions {
        if session.pose.is_some() {
            snapshot.blocked(format!(
                "动画{}仍有未插键姿态，请先插入姿态或重置姿态",
                path.display()
            ));
        }
        if session.dirty() {
            snapshot.draft(
                items,
                SaveTarget::Animation(path.clone()),
                format!("动画：{}", path.display()),
            );
        }
    }
}

pub(crate) fn request(world: &mut World, path: &Path) -> Result<(), String> {
    let save = world.get_resource::<AnimationOperations>().ok_or("动画保存操作尚未初始化")?.save;
    let mut state = world
        .get_resource_mut::<AnimationEditorState>()
        .ok_or("动画会话尚未初始化")?;
    save(&mut state, path)
}

pub(crate) fn poll(world: &World, path: &Path) -> Result<bool, String> {
    let state = world
        .get_resource::<AnimationEditorState>()
        .ok_or("动画会话已变化")?;
    if let Some((source, result)) = state
        .project_save_receipt
        .as_ref()
        .filter(|(source, _)| source == path)
    {
        let _ = source;
        result.as_ref().map_err(Clone::clone)?;
    } else if !state.busy() {
        return Err(format!("动画{}缺少原保存成功回执", path.display()));
    }
    if state.busy() {
        return Ok(false);
    }
    let session = state
        .sessions
        .get(path)
        .ok_or("动画源会话已变化，已停止工程关闭")?;
    if session.dirty() {
        return Err(format!(
            "动画{}仍有未保存修改：{}",
            path.display(),
            state.status
        ));
    }
    Ok(true)
}
