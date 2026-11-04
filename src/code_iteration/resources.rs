//! 代码迭代只持有构建版本与短状态；作者文档、历史、选择和相机仍由原模块拥有。

use super::{Backend, BuildIdentity, PreparedVersion, types::CodeIterationPhase};
use crate::{compiler_diagnostics::CompilerDiagnostic, project::Project, viewport_mode::ViewportMode};
use bevy::prelude::*;
use std::{path::PathBuf, sync::Arc};

#[derive(Resource)]
pub(crate) struct CodeIterationState {
    pub(crate) project_id: String,
    pub(crate) project_root: PathBuf,
    pub(crate) source_context: u64,
    pub(crate) diagnostics: Arc<Vec<CompilerDiagnostic>>,
    pub(crate) diagnostic_revision: u64,
    pub(crate) diagnostics_open: bool,
    pub(crate) phase: CodeIterationPhase,
    pub(crate) status: String,
    pub(super) generation: u64,
    pub(super) pending: Option<Arc<PreparedVersion>>,
    pub(super) applying: Option<ApplyFlight>,
    pub(super) applied: Option<BuildIdentity>,
}

impl CodeIterationState {
    pub(super) fn new(project: &Project) -> Self {
        Self {
            project_id: project.id.clone(), project_root: project.root.clone(), source_context: 0,
            diagnostics: Arc::new(Vec::new()), diagnostic_revision: 0, diagnostics_open: false,
            phase: CodeIterationPhase::Watching,
            status: "代码：保存源码后自动编译".into(), generation: 0, pending: None, applying: None, applied: None,
        }
    }
}

pub(super) struct ApplyFlight {
    pub(super) token: u64,
    pub(super) mode: ViewportMode,
    pub(super) version: Arc<PreparedVersion>,
}

/// 停止控制牌不可逆；取消关闭只能等待旧实例退役后建立新实例，不能清标志复活旧线程。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BackendLifecycle { Running, Closing, Resuming }

/// 重建仍绑定本作者最初的工程与缓存；配置在真正恢复时取当前选择，不重新打开或改写工程身份。
#[derive(Clone)]
pub(super) struct BackendSettings {
    pub(super) project: Project,
    pub(super) target: PathBuf,
    pub(super) logs: Option<crate::project_log::ProjectLogChannel>,
}

/// Backend拥有唯一监听/构建槽；确认前保留原服务，取消收尾通过独立恢复阶段接续。
pub(super) struct CodeIterationRuntime {
    pub(super) backend: Option<Backend>,
    pub(super) profile: feathrise_editor_core::ProjectBuildProfile,
    pub(super) settings: BackendSettings,
    pub(super) lifecycle: BackendLifecycle,
    pub(super) last_generation: u64,
    pub(super) failure: Option<String>,
}
