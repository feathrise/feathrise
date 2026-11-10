//! 内存作者快照的唯一捕获/编码槽；旧任务直到实际结束仍占槽，切换场景只取消采用资格。

use super::capture::{Capture, SourceIdentity};
use bevy::{prelude::*, tasks::Task};
use std::{collections::BTreeSet, sync::Arc};

/// 与同一完整字节快照一起后台冻结；只有真实采用回执才能给新对象发临时姿态。
#[derive(Default)]
pub struct EmbeddedPoseTargets {
    pub subscenes: BTreeSet<feathrise_scene_content::SceneInstanceId>,
    pub terrains: BTreeSet<feathrise_scene_content::TerrainId>,
}
impl EmbeddedPoseTargets {
    pub(super) fn capture(manifest: &feathrise_scene_content::SceneManifestAsset) -> Self {
        Self {
            subscenes: manifest.subscenes.iter().map(|value| value.id.clone()).collect(),
            terrains: manifest.terrains.iter().flat_map(|values| values.iter()).map(|value| value.id.clone()).collect(),
        }
    }
}

/// 编码完成的完整不可变快照；宿主只传字节，不取得作者资源或 GPU 句柄。
pub struct EmbeddedSnapshot {
    pub scene_id: String,
    pub author_revision: u64,
    pub bytes: Arc<Vec<u8>>,
    pub pose_targets: Arc<EmbeddedPoseTargets>,
}

#[derive(Resource, Default)]
pub struct EmbeddedSnapshotState {
    /// 原独立 World 模式默认关闭，不为对照模式增加整图捕获工作。
    pub enabled: bool,
    pub revision: u64,
    pub scene_id: Option<String>,
    pub ready: Option<Arc<EmbeddedSnapshot>>,
    /// 当前作者数据仍在收口、捕获或编码；纯浮层遮挡和鼠标静默释放不改变此状态。
    pub pending: bool,
    pub error: Option<String>,
    pub(super) source: Option<SourceIdentity>,
    pub(super) capture: Option<Capture>,
    pub(super) task: Option<Task<(SourceIdentity, u64, Result<Arc<EmbeddedSnapshot>, String>)>>,
}
