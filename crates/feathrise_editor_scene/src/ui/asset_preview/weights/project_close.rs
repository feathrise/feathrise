//! 工程关闭和保存全部使用同一个权重另存事务，并等待原回执。

use super::{resources::WeightEditor, systems::begin_save};
use crate::standalone::project_close::types::{ProjectCloseSnapshot, SaveItem, SaveTarget};
use bevy::prelude::*;
use std::path::Path;

pub(crate) fn collect(
    world: &World,
    snapshot: &mut ProjectCloseSnapshot,
    items: &mut Vec<SaveItem>,
) {
    let Some(state) = world.get_resource::<WeightEditor>() else {
        return;
    };
    if state.work.is_some() || state.stroke.is_some() {
        snapshot.pending("正在处理蒙皮笔画或另存模型");
    }
    if let Some(session) = state.session.as_ref().filter(|session| session.dirty()) {
        snapshot.draft(
            items,
            SaveTarget::Weights(session.key.root.join(&session.key.path)),
            format!("蒙皮权重 · {}", session.key.path),
        );
    }
}

pub(crate) fn request(world: &mut World, path: &Path) -> Result<(), String> {
    let mut state = world
        .get_resource_mut::<WeightEditor>()
        .ok_or("蒙皮工具已关闭")?;
    if !state
        .session
        .as_ref()
        .is_some_and(|session| session.key.root.join(&session.key.path) == path)
    {
        return Err("待保存的蒙皮草稿身份已变化".into());
    }
    begin_save(&mut state)
}

pub(crate) fn poll(world: &World, path: &Path) -> Result<bool, String> {
    let state = world
        .get_resource::<WeightEditor>()
        .ok_or("蒙皮工具已关闭")?;
    if state.work.is_some() {
        return Ok(false);
    }
    let (_, receipt) = state
        .save_receipt
        .as_ref()
        .filter(|(source, _)| source == path)
        .ok_or("缺少权重另存回执")?;
    receipt.as_ref().map_err(Clone::clone)?;
    Ok(!state.dirty())
}
