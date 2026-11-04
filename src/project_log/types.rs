//! 工程日志的会话身份与实际输出来源；不复用业务状态提示或IPC正文。

use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LogOwner {
    pub project_id: Arc<str>,
    pub instance: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LogSource {
    Build,
    Stdout,
    Stderr,
}

/// 同一工程可以同时准备3D视口和作者预览，各自只撤销同类旧任务的采用权。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LogTaskKind {
    #[default]
    General,
    Project3d,
    AuthorPreview,
    SavedPlay,
    CodeBuild,
    Export,
}

impl LogTaskKind {
    pub(super) const COUNT: usize = 6;
    pub(super) fn index(self) -> usize { self as usize }
    pub(super) fn label(self) -> &'static str {
        match self { Self::General => "工程输出", Self::Project3d => "工程3D", Self::AuthorPreview => "作者预览", Self::SavedPlay => "工程运行", Self::CodeBuild => "代码编译", Self::Export => "工程导出" }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LogTask {
    pub kind: LogTaskKind,
    pub number: u64,
}

#[cfg(feature = "validation")]
impl LogSource {
    pub(super) fn label(self) -> &'static str {
        match self { Self::Build => "build", Self::Stdout => "stdout", Self::Stderr => "stderr" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LogEventKind {
    /// 已完成的逻辑行提交到历史，同时清除该来源的临时尾行。
    Line,
    /// 当前尚未换行的可见内容；空文本表示终端已撤回该来源的尾行。
    Tail,
}

/// 颜色只描述可见文字；日志来源和复制正文都不因终端样式改变。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LogStyle {
    pub fg: Option<[u8; 3]>,
    pub bold: bool,
}

/// UTF-8正文内的半开字节区间；跨虚拟页时由阅读状态重新定位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LogSpan {
    pub start: usize,
    pub end: usize,
    pub style: LogStyle,
}

#[derive(Debug)]
pub(super) struct LogEvent {
    pub owner: LogOwner,
    pub generation: u64,
    pub sequence: u64,
    pub task: LogTask,
    pub source: LogSource,
    pub kind: LogEventKind,
    pub text: String,
    pub spans: Vec<LogSpan>,
}

pub(super) struct LogLine {
    pub task: LogTask,
    // 显式诊断保留真实管道来源；不影响主线按字节偏移维护阅读选区的逻辑。
    #[cfg(feature = "validation")]
    pub source: LogSource,
    pub text: String,
    pub spans: Vec<LogSpan>,
    pub start: u64,
}
