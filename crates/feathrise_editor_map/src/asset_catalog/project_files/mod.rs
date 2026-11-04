//! 工程根文件浏览的独立会话；共用资产目录IO算法，不向资产选择或AssetServer发布源码路径。

mod resources;
mod systems;
mod types;

pub(crate) use resources::EditorProjectFileCatalog;
pub(crate) use systems::{poll_project_file_scan, start_project_file_scan};
pub(crate) use types::{ProjectFileIdentity, ProjectFileNavigation};

#[cfg(test)]
mod tests;
