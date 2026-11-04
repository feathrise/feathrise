//! 后台源码代次、完整构建版本与主线程回执；不持有 ECS 或作者文档。

use crate::{compiler_diagnostics::CompilerDiagnostic, native_play::PreparedGame};
use crate::code_iteration::hotpatch::PreparedPatch;
use feathrise_editor_core::ProjectBuildProfile;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BuildIdentity {
    pub project_id: String,
    pub root: PathBuf,
    pub profile: ProjectBuildProfile,
    /// 保存/配置代次跨取消恢复严格递增；即使源码改后还原，旧版本也失去采用权。
    pub generation: u64,
    pub source_sha256: String,
    pub context_sha256: String,
}

impl BuildIdentity {
    pub(super) fn json(&self) -> serde_json::Value {
        serde_json::json!({"project_id":self.project_id,"root":self.root,"profile":self.profile.cargo_profile(),
            "generation":self.generation,"source_sha256":self.source_sha256,"context_sha256":self.context_sha256})
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedVersion {
    pub identity: BuildIdentity,
    pub artifact: PreparedArtifact,
}

/// 原地补丁和完整运行版本没有可互换的采用语义；补丁绝不能进入会终止旧Job的重启链。
#[derive(Clone, Debug)]
pub(crate) enum PreparedArtifact {
    Restart(Arc<PreparedGame>),
    Patch(Arc<PreparedPatch>),
}

#[derive(Debug)]
pub(crate) enum Event {
    Waiting {
        generation: u64,
    },
    Building {
        identity: BuildIdentity,
    },
    Finished {
        identity: BuildIdentity,
        result: Result<Arc<PreparedVersion>, String>,
        diagnostics: Vec<CompilerDiagnostic>,
    },
    Failed(String),
}

pub(super) struct BuildRequest {
    pub generation: u64,
    pub profile: ProjectBuildProfile,
    pub source: super::source::SourceSnapshot,
}

/// 首次打开只建立基线；取消关闭后的恢复必须补编译停监听期间的当前源码。
#[derive(Clone, Copy)]
pub(super) enum WatchStart {
    BaselineOnly { generation: u64 },
    RebuildCurrentSource { generation: u64 },
}

impl WatchStart {
    pub(super) fn generation(self) -> u64 {
        match self {
            Self::BaselineOnly { generation } | Self::RebuildCurrentSource { generation } => generation,
        }
    }

    pub(super) fn rebuild_current_source(self) -> bool {
        matches!(self, Self::RebuildCurrentSource { .. })
    }
}
