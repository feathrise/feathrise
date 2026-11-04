//! 真实目录选择和右键菜单只发布轻量意图，不承担磁盘记录工作。

use bevy::prelude::Message;

#[derive(Message)]
pub(crate) struct SetAssetFavorite {
    pub(crate) path: String, // 相对 assets 根，由真实条目冻结。
    pub(crate) is_directory: bool,
    pub(crate) favorite: bool, // 冻结菜单显示的收藏/取消意图，而非对未知磁盘基线执行反转。
}

#[derive(Message)]
pub(crate) struct AssetVisited {
    pub(crate) path: String,
    pub(crate) is_directory: bool,
}
