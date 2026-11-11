//! 调度应用显式安装的领域作者；只使用稳定身份与原事务回执，不理解其数据结构。

use super::{resources::ProjectAuthorAdapters, types::*};
use bevy::prelude::*;

pub(super) fn collect(
    world: &World,
    snapshot: &mut ProjectCloseSnapshot,
    items: &mut Vec<SaveItem>,
) {
    if let Some(adapters) = world.get_resource::<ProjectAuthorAdapters>() {
        for adapter in &adapters.entries {
            let state = (adapter.collect)(world);
            if let Some(reason) = state.pending {
                snapshot.pending(reason);
            }
            if let Some(reason) = state.blocked {
                snapshot.blocked(reason);
            }
            for document in state.unsaved {
                snapshot.draft(
                    items,
                    SaveTarget::Domain(adapter.domain, document.id),
                    document.label,
                );
            }
        }
    }
}

pub(super) fn find(world: &World, domain: &str) -> Result<ProjectAuthorAdapter, String> {
    world
        .get_resource::<ProjectAuthorAdapters>()
        .and_then(|adapters| {
            adapters
                .entries
                .iter()
                .find(|adapter| adapter.domain == domain)
        })
        .copied()
        .ok_or_else(|| format!("作者保存域未安装或已变化：{domain}；草稿和失败回执保留"))
}

pub(in super::super) fn saved_play_blocker(world: &World) -> Option<String> {
    world
        .get_resource::<ProjectAuthorAdapters>()?
        .entries
        .iter()
        .find_map(|adapter| (adapter.saved_play_blocker)(world))
}

pub(in super::super) fn exit_blocker(world: &World) -> Option<String> {
    let mut snapshot = ProjectCloseSnapshot::default();
    collect(world, &mut snapshot, &mut Vec::new());
    snapshot.pending.or(snapshot.blocked).or_else(|| {
        (!snapshot.unsaved.is_empty()).then(|| {
            format!(
                "存在未保存的作者文档，请先保存或使用原标签关闭确认丢弃：{}",
                snapshot.unsaved.join("；")
            )
        })
    })
}
