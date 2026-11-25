//! 作者明确请求导出已保存的演出，宿主定帧渲染到独立视频文件。
use bevy::prelude::Message;
use std::path::PathBuf;

#[derive(Message, Clone, Debug)]
pub struct OpenCinematicExport {
    pub asset_root: PathBuf,
    pub asset_path: PathBuf,
}
