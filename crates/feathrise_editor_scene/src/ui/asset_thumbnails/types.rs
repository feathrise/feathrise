//! 缩略图完整身份、后台文件完成值与固定节点游标。

use bevy::prelude::*;
use feathrise_scene_content::SceneAssetKind;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WindowKey {
    pub folder: String,
    pub start: usize,
    pub count: usize,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct FileStamp {
    pub length: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourceInfo {
    pub primary: FileStamp,
    pub stamp: u64,
    // GLTF外部缓冲与纹理也参与失效检查；Arc使主线程只冻结浅快照。
    pub dependencies: Arc<[String]>,
}

#[derive(Clone)]
pub(super) struct InspectionInput {
    pub path: String,
    pub kind: SceneAssetKind,
    pub previous: Option<SourceInfo>,
}

pub(super) struct InspectionResult {
    pub epoch: u64,
    pub sources: Vec<(String, Result<SourceInfo, String>)>,
}

#[derive(Clone)]
pub(super) enum ThumbnailResult {
    Queued,
    Ready(Handle<Image>),
    Failed(String),
}

pub(super) struct CacheEntry {
    pub source: Result<SourceInfo, String>,
    pub result: ThumbnailResult,
    pub used: u64,
}

/// 逐个推进子节点，不能一次extend整个宽树的Children。
#[derive(Clone, Copy)]
pub(super) struct NodeCursor {
    pub entity: Entity,
    pub child: usize,
    pub entered: bool,
    pub hidden: bool,
}

impl NodeCursor {
    pub fn new(entity: Entity) -> Self {
        Self {
            entity,
            child: 0,
            entered: false,
            hidden: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum JobPhase {
    Loading,
    Scanning,
    RevealPending,
    Rendering,
}
