//! 焦点边沿合并为一个核对意图；IME 忙碌时至多保留一份回执。

use super::types::CheckReply;

#[derive(Default)]
pub(in super::super) struct ExternalCheckState {
    pub available: bool,
    pub requested: bool,
    pub was_open: bool,
    pub was_focused: bool,
    pub ready: Option<CheckReply>,
}
