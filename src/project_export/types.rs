//! 工程导出的完成回执；只携带已发布目录与统计，不把大型文件清单送回作者帧。

use std::path::PathBuf;

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct ExportReceipt {
    pub directory: PathBuf,
    pub executable: PathBuf,
    pub files: usize,
    pub bytes: u64,
    /// 新包已经完整发布时，旧备份被占用只形成提示，不把成功误报为失败。
    pub warning: Option<String>,
}
