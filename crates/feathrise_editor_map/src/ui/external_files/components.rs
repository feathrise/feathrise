//! 标题和关闭键共享所属文件身份；右键后台标签不能误用活动文件。

use bevy::prelude::*;

#[derive(Component, Clone)]
pub(crate) struct ExternalFileTab(pub String);
