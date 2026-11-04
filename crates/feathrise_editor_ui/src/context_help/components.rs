//! 问号直接持有标题；原正文作为其隐藏子实体，由根应用原位移挂到唯一帮助窗口。

use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub struct ContextHelpButton(pub String);

#[derive(Component, Clone, Default)]
pub struct ContextHelpBody;
