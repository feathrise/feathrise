//! 文件菜单通过自己的模态/中文输入门后发布一次保存意图，不让菜单捕获帧误拦已确认操作。

use bevy::prelude::*;

#[derive(Message)]
pub(crate) struct SaveAllDocuments;
