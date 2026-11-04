//! 官方输入采用前的唯一轻量记录，不复制正文；中文候选保留提交前选区。

use super::super::types::SourceKey;
use super::types::Selection;
use bevy::prelude::*;
use std::time::Instant;

#[derive(Resource, Default)]
pub(in super::super) struct HistoryCapture {
    pub(super) key: Option<SourceKey>,
    pub(super) before: Selection,
    pub(super) typing: bool,
    pub(super) at: Option<Instant>,
    pub(super) ime_before: Option<Selection>,
}

impl HistoryCapture {
    pub(in super::super) fn context(
        &self,
        key: &SourceKey,
        after: Selection,
    ) -> super::types::EditContext {
        if self.key.as_ref() == Some(key) {
            super::types::EditContext {
                before: self.before,
                after,
                typing: self.typing,
                at: self.at.unwrap_or_else(Instant::now),
            }
        } else {
            super::types::EditContext { after, ..default() }
        }
    }
}
