//! 两个菜单共用固定动作池；安装检测只切显隐，不反复创建菜单实体。
use super::types::ExternalFileAction;

pub(super) const MAX_PATH_BYTES: usize = 4096; // 安装身份与目录队列只保留短路径，限制后台临时内存。

pub(crate) const EXTERNAL_MENU_ITEMS: [(&str, ExternalFileAction); 7] = [
    ("用 VS Code 打开", ExternalFileAction::VsCode(None)),
    ("用 VS Code 打开工程并定位文件", ExternalFileAction::VsCodeProject(None)),
    ("用 Zed 打开", ExternalFileAction::Zed(None)),
    ("用 Zed 打开工程并定位文件", ExternalFileAction::ZedProject(None)),
    ("用 RustRover 打开", ExternalFileAction::RustRover(None)),
    ("用 RustRover 打开工程并定位文件", ExternalFileAction::RustRoverProject(None)),
    ("打开所在文件夹", ExternalFileAction::Folder),
];
