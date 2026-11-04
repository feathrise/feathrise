//! 两种右键菜单的单次路径复制意图；保留资产身份与明确选择的路径形式。

use bevy::prelude::Message; // 意图与平台剪贴板分离，便于无副作用自动验证。
use super::types::AssetPathCopyKind;

/// 只有明确点击复制项才发送，不随右键打开或普通选择触发。
#[derive(Message)] // 每帧最多由唯一菜单发出一条。
pub(crate) struct CopyAssetPath {
    pub path: String, // assets内的冻结相对路径，不附加引号或GLB子资源后缀。
    pub kind: AssetPathCopyKind,
}
