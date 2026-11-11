//! 资产行双击的显式中央打开请求；只传路径身份，不在目录点击系统加载或改写文件。

use bevy::prelude::*;

#[derive(Message, Clone)]
pub(crate) struct OpenAssetPreview {
    pub path: String,
}

/// 目录只提交已绑定的工程资产相对路径及 GLB JSON 原片段索引。
#[derive(Message)]
pub(crate) struct PickProjectAnimationClip {
    pub asset_path: String,
    pub clip_index: usize,
}
