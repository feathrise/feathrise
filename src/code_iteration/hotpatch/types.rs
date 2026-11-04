//! 受控fat基线与补丁产物身份；只共享不可变版本，不持有作者World或跨进程ECS引用。

use crate::build_artifacts::PublishedBundle;
use super::super::backend::SourceSnapshot;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Debug)]
pub(crate) struct FatBaseline {
    pub client_json: Arc<Vec<u8>>,
    pub source: SourceSnapshot,
    pub base_build: String,
    pub directory: PathBuf,
    pub patch_root: PathBuf,
    pub features: String,
    pub bundle: PublishedBundle,
}

/// ASLR来自当前真实运行实例；源码generation与场景generation保持独立。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PatchTarget {
    pub project_id: String,
    pub session: u64,
    pub pid: u32,
    pub world_id: u64,
    pub aslr_reference: u64,
    pub base_build: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PatchContext {
    pub target: PatchTarget,
    pub baseline: Arc<FatBaseline>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedPatch {
    pub target: PatchTarget,
    pub source_generation: u64,
    pub artifact: String,
    pub sha256: String,
    /// 基线租约一直覆盖候选、发送与实际运行；旧DLL不主动卸载。
    pub baseline: Arc<FatBaseline>,
}
