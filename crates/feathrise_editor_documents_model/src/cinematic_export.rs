//! 作者明确请求导出已保存的演出，宿主定帧渲染到独立视频文件。
use bevy::prelude::Message;
use std::path::PathBuf;

#[derive(Message, Clone, Debug)]
pub struct OpenCinematicExport {
    pub asset_root: PathBuf,
    pub asset_path: PathBuf,
    /// 开窗时冻结作者正在查看的已载入场景，仍可在导出面板换成其他场景。
    pub scene: Option<String>,
}
