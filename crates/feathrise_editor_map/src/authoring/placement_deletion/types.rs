//! 删除意图、后台准备源与弱身份契约；类型本身不承担世界调度。

use std::{collections::BTreeMap, sync::{Arc, Weak, atomic::AtomicBool}};

pub(super) type DeletedIds = BTreeMap<String, Arc<()>>;

/// 文档只移动所有权；独立修订同时覆盖不推进作者修订的保存确认。
#[derive(Debug)]
pub(crate) struct PlacementDeletionIntents {
    pub(super) scope: Arc<AtomicBool>,
    pub(super) ids: Arc<DeletedIds>,
    pub(super) revision: u64,
}

/// 固定预算捕获只冻结弱文档身份，避免过早共享整表阻塞作者写入。
#[derive(Clone, Debug)]
pub(crate) struct PlacementDeletionIdentity {
    pub(super) scope: Weak<AtomicBool>,
}

#[derive(Debug)]
pub(super) struct FrozenDeletionIdentity {
    pub(super) scope: Weak<AtomicBool>,
    pub(super) ids: Weak<DeletedIds>,
    pub(super) revision: u64,
}

/// O(1)冻结源；完整表复制和遍历必须由后台任务完成。
#[derive(Debug)]
pub(crate) struct PlacementDeletionSource {
    pub(super) identity: FrozenDeletionIdentity,
    pub(super) ids: Arc<DeletedIds>,
}

#[derive(Debug)]
pub(crate) struct PreparedPlacementDeletions {
    pub(super) identity: FrozenDeletionIdentity,
    pub(super) ids: Arc<DeletedIds>,
}

/// 完成值只保留弱源证据；不能继续共享作者表而永久关闭写门。
#[derive(Debug)]
pub(crate) struct PreparedDeletionSnapshot {
    pub(super) identity: FrozenDeletionIdentity,
    pub(super) snapshots: Vec<PlacementDeletionSnapshot>,
}

/// 保存弱票据不延长已撤销删除或已丢弃文档的生命。
#[derive(Clone, Debug)]
pub(crate) struct PlacementDeletionSnapshot {
    pub(crate) id: String,
    pub(super) ticket: Weak<()>,
    pub(super) scope: Weak<AtomicBool>,
}

#[derive(Clone)]
pub(super) struct DocumentIndex {
    pub(super) scope: Weak<AtomicBool>,
    pub(super) ids: Weak<DeletedIds>,
}
