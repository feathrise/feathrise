//! 明确左击“关闭全部”的一次性意图；批量关闭不写盘，也不删除文档草稿。
use bevy::prelude::*;

#[derive(Message)]
pub(super) struct CloseAllAssetTabs;

/// 只关闭未保存标记为空的文档，以及始终只读的图片/模型；不隐式保存或丢弃草稿。
#[derive(Message)]
pub(super) struct CloseUnmodifiedAssetTabs;
