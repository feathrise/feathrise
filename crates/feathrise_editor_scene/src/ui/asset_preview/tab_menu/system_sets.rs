//! 文件菜单先取得鼠标权，普通预览和资产菜单随后使用同一输入资格。
use bevy::prelude::*;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AssetTabMenuInputSet;
