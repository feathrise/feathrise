//! 在后台交接共享全局命令，并保持各地图原来的局部/全局撤销顺序。

use crate::authoring::EditorCommand;
use std::collections::{BTreeMap, VecDeque};

#[derive(Default)]
pub(super) struct MapHistoryOrder {
    pub(super) undo: Vec<(u64, usize)>,
    pub(super) redo: Vec<(u64, usize)>,
}

/// 全局命令只保留一份；后台地图记住稳定命令号及其前方局部命令数。
pub(super) fn split_history(
    history: Vec<EditorCommand>,
) -> (Vec<EditorCommand>, Vec<EditorCommand>, Vec<(u64, usize)>) {
    let (mut local, mut global, mut order) = (Vec::new(), Vec::new(), Vec::new());
    for command in history {
        if let Some(sequence) = command.global_history_id() {
            order.push((sequence, local.len()));
            global.push(command);
        } else {
            local.push(command);
        }
    }
    (local, global, order)
}

/// 【功能设计】返回旧图时，仍存在的共享命令回到原局部位置；别图新增的命令排在末尾。
/// 不能简单extend，否则“地图操作→全局操作→地图操作”会在切标签后倒置撤销顺序。
pub(super) fn merge_history(
    local: Vec<EditorCommand>,
    global: Vec<EditorCommand>,
    order: Vec<(u64, usize)>,
) -> Vec<EditorCommand> {
    let positions: BTreeMap<_, _> = order.into_iter().collect();
    let local_len = local.len();
    let mut slots: Vec<_> = global
        .into_iter()
        .map(|command| {
            let position = command
                .global_history_id()
                .and_then(|id| positions.get(&id).copied())
                .unwrap_or(local_len)
                .min(local_len);
            (position, command)
        })
        .collect();
    slots.sort_by_key(|(position, _)| *position); // 稳定排序保留同一局部位置上的全局栈顺序。
    let mut slots: VecDeque<_> = slots.into();
    let mut merged = Vec::with_capacity(local_len + slots.len());
    for (index, command) in local.into_iter().enumerate() {
        while slots
            .front()
            .is_some_and(|(position, _)| *position == index)
        {
            merged.push(slots.pop_front().unwrap().1);
        }
        merged.push(command);
    }
    merged.extend(slots.into_iter().map(|(_, command)| command));
    merged
}
