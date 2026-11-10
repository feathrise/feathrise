//! 资产行双击的显式中央打开请求；只传路径身份，不在目录点击系统加载或改写文件。

use bevy::prelude::*;

#[derive(Message, Clone)]
pub(crate) struct OpenAssetPreview {
    pub path: String,
}
