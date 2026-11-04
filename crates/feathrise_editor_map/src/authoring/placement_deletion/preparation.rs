//! 删除表的后台物化和弱保存证据捕获；主线程只换表并后台回收旧值。

use std::{collections::BTreeMap, sync::{Arc, atomic::AtomicBool}};
use bevy::prelude::Commands;
use feathrise_map_content::MapId;
use super::{types::*, register_document, scope_active};
use crate::authoring::{EditorAuthoringSession, AuthoringRetirement};

impl Default for PlacementDeletionIntents {
    fn default() -> Self {
        Self { scope: Arc::new(AtomicBool::new(true)), ids: Arc::new(BTreeMap::new()), revision: 0 }
    }
}

impl PlacementDeletionIntents {
    pub(crate) fn revision(&self) -> u64 { self.revision }
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize { self.ids.len() }
    pub(crate) fn is_empty(&self) -> bool { self.ids.is_empty() }
    pub(crate) fn write_shared(&self) -> bool { Arc::strong_count(&self.ids) > 1 }
    pub(crate) fn identity(&self) -> PlacementDeletionIdentity {
        PlacementDeletionIdentity { scope: Arc::downgrade(&self.scope) }
    }
    pub(crate) fn source(&self) -> PlacementDeletionSource {
        PlacementDeletionSource {
            identity: FrozenDeletionIdentity { scope: Arc::downgrade(&self.scope),
                ids: Arc::downgrade(&self.ids), revision: self.revision },
            ids: self.ids.clone(),
        }
    }

    /// 兼容旧小型测试入口；生产保存路径使用source().snapshot()在后台捕获。
    #[cfg(test)]
    pub(crate) fn snapshots(&self) -> Vec<PlacementDeletionSnapshot> {
        self.ids.iter().map(|(id, ticket)| PlacementDeletionSnapshot {
            id: id.clone(), ticket: Arc::downgrade(ticket), scope: Arc::downgrade(&self.scope),
        }).collect()
    }

    pub(super) fn ids_mut(&mut self) -> &mut DeletedIds {
        // 后台源仅短暂强共享；调用方必须先通过写门，禁止make_mut退化成整表同步复制。
        assert_eq!(Arc::strong_count(&self.ids), 1, "删除表共享期间必须先等待后台准备完成");
        Arc::make_mut(&mut self.ids)
    }
}

impl PlacementDeletionIdentity {
    pub(crate) fn matches(&self, intents: &PlacementDeletionIntents) -> bool {
        scope_active(&self.scope) && self.scope.ptr_eq(&Arc::downgrade(&intents.scope))
    }
}

impl FrozenDeletionIdentity {
    fn matches(&self, intents: &PlacementDeletionIntents) -> bool {
        scope_active(&self.scope) && self.scope.ptr_eq(&Arc::downgrade(&intents.scope))
            && self.revision == intents.revision && self.ids.ptr_eq(&Arc::downgrade(&intents.ids))
    }
}

impl PlacementDeletionSource {
    /// 仅在worker调用；恢复项最后执行，上限按整笔最终结果校验。
    pub(crate) fn prepare<'a, 'b>(self, deleted: impl Iterator<Item = &'a str>,
        restored: impl Iterator<Item = &'b str>) -> Result<PreparedPlacementDeletions, String> {
        let Self { identity, ids } = self;
        let mut candidate = (*ids).clone();
        for id in deleted { candidate.insert(id.to_owned(), Arc::new(())); }
        for id in restored { candidate.remove(id); }
        // 完成值不能继续持有源表strong；即使结果尚未采用，作者写门也能重新打开。
        drop(ids);
        if candidate.len() > feathrise_map_content::MAX_MAP_PLACEMENTS_PER_MAP {
            return Err("未确认的摆放删除已达8192项，请先保存并等待正式采用后再删除；本次操作未执行".to_owned());
        }
        Ok(PreparedPlacementDeletions { identity, ids: Arc::new(candidate) })
    }

    /// 仅在worker遍历；返回数组仅保留弱票据，保存不会让撤销票据续命。
    pub(crate) fn snapshot(self) -> PreparedDeletionSnapshot {
        let Self { identity, ids } = self;
        let snapshots = ids.iter().map(|(id, ticket)| PlacementDeletionSnapshot {
            id: id.clone(), ticket: Arc::downgrade(ticket), scope: identity.scope.clone(),
        }).collect();
        drop(ids);
        PreparedDeletionSnapshot { identity, snapshots }
    }
}

impl PreparedPlacementDeletions {
    pub(crate) fn matches(&self, intents: &PlacementDeletionIntents) -> bool { self.identity.matches(intents) }
}

impl PreparedDeletionSnapshot {
    pub(crate) fn matches(&self, intents: &PlacementDeletionIntents) -> bool { self.identity.matches(intents) }
    pub(crate) fn take(&mut self) -> Vec<PlacementDeletionSnapshot> { std::mem::take(&mut self.snapshots) }
}

/// 调用方先核验完整身份；采用与旧值移交都为O(1)，不在输入帧析构完整表。
pub(crate) fn apply_prepared(commands: &mut Commands, session: &mut EditorAuthoringSession,
    map: &MapId, prepared: PreparedPlacementDeletions) {
    assert!(prepared.matches(&session.placement_deletions), "只能采用同文档同删除修订的准备结果");
    let old = std::mem::replace(&mut session.placement_deletions.ids, prepared.ids);
    session.placement_deletions.revision = session.placement_deletions.revision.wrapping_add(1);
    register_document(commands, map, &session.placement_deletions);
    commands.queue(move |world: &mut bevy::prelude::World| {
        world.get_resource_or_init::<AuthoringRetirement>().push(old);
    });
}
