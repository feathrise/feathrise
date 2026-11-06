//! 工程与资产右键菜单的临时会话状态，不写入任何作者文件或撤销历史。

use bevy::prelude::{Resource, Vec2}; // 一个路径快照和逻辑像素锚点足够驱动菜单。

use super::types::AssetMenuTarget; // 固定行池索引必须与来源修订共同使用。

/// 菜单关闭后仍保留本帧鼠标所有权，由公共门覆盖后续松键边沿。
#[derive(Resource, Default)] // 初始无菜单，不影响现有左键选择与预览。
pub(crate) struct AssetContextMenuState {
    pub(super) target: Option<AssetMenuTarget>, // 同一时刻只保存明确右键点中的一个条目。
    pub(super) anchor: Vec2,                    // 相对F12根的UI逻辑像素，不混用窗口DPI和UiScale。
    pub(super) captured_this_frame: bool,       // 关闭菜单的同一次点击不得落到底层作者输入。
}
