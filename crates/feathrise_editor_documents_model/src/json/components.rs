//! JSON工具的固定按钮；诊断整行可点击，避免额外问题列表或第二份输入。

use bevy::prelude::*;

#[derive(Component, Default, Clone, Copy, PartialEq, Eq)]
pub enum JsonAction {
    #[default]
    Format,
    Locate,
}

#[derive(Component, Default, Clone)]
pub struct DiagnosticLabel;
