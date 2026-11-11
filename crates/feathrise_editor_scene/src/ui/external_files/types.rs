//! 外部打开动作只携带路径和可选真实行列，不接收拼接的 Shell 命令。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExternalFileAction {
    VsCode(Option<(usize, usize)>), // 1起始的逻辑行与字符列；没有定位时只打开文件。
    VsCodeProject(Option<(usize, usize)>), // 打开当前工作树项目；文件默认定位首行，目录只打开项目根。
    Zed(Option<(usize, usize)>),
    ZedProject(Option<(usize, usize)>),
    RustRover(Option<(usize, usize)>),
    RustRoverProject(Option<(usize, usize)>),
    // 源码安全根始终是当前项目；是否把项目交给IDE独立选择，两种方式均核验磁盘摘要。
    ProjectSource { editor: ExternalEditor, line: usize, column: usize, digest: [u8; 32], in_project: bool },
    Folder,
}

pub(crate) use feathrise_editor_core::ExternalEditor;

impl ExternalFileAction {
    pub fn editor(self) -> Option<ExternalEditor> {
        match self {
            Self::VsCode(_) | Self::VsCodeProject(_) => Some(ExternalEditor::VsCode),
            Self::Zed(_) | Self::ZedProject(_) => Some(ExternalEditor::Zed),
            Self::RustRover(_) | Self::RustRoverProject(_) => Some(ExternalEditor::RustRover),
            Self::ProjectSource { editor, .. } => Some(editor),
            Self::Folder => None,
        }
    }
    pub fn in_project(self) -> bool {
        matches!(self, Self::VsCodeProject(_) | Self::ZedProject(_) | Self::RustRoverProject(_) | Self::ProjectSource { in_project: true, .. })
    }
    pub fn location(self) -> Option<(usize, usize)> {
        match self {
            Self::VsCode(pos) | Self::VsCodeProject(pos) | Self::Zed(pos) | Self::ZedProject(pos)
            | Self::RustRover(pos) | Self::RustRoverProject(pos) => pos,
            Self::ProjectSource { line, column, .. } => Some((line, column)),
            Self::Folder => None,
        }
    }
}

/// 检测只返回已存在的可执行入口与固定前置参数，不缓存可执行的字符串命令。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EditorLauncher {
    pub executable: std::path::PathBuf,
    pub prefix_args: Vec<std::ffi::OsString>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DetectedEditors {
    pub launchers: [Option<EditorLauncher>; 3],
}

pub(super) enum ExternalTaskReply {
    Detected(DetectedEditors),
    Opened { result: Result<String, String>, detected: DetectedEditors }, // 自动恢复入口后同一回执更新菜单缓存。
}
