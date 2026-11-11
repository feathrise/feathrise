//! 每份文档独立的有界撤销/重做；共享业务草稿快照，仍使用唯一原生输入与排版。

use bevy::prelude::*;

mod components;
mod constants;
mod control;
mod journal;
mod resources;
mod scene;
#[cfg(test)]
mod tests;
mod types;
#[cfg(test)]
pub(super) use tests::{gpu_assert_history, gpu_chord, gpu_click_history};

pub(super) use resources::HistoryCapture;
pub(super) use types::{DocumentHistory, EditContext, HistorySystemSet, Selection};

/// 只读验收原历史长度；普通构建不公开内部快照或增加写入入口。
#[cfg(feature = "validation")]
pub(super) fn validation_lengths(history: &DocumentHistory) -> (usize, usize) {
    (history.undo.len(), history.redo.len())
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<HistoryCapture>()
        .add_systems(
            Update,
            control::prepare.before(bevy::text::apply_text_edits),
        )
        .add_systems(
            Update,
            control::input
                .in_set(HistorySystemSet::Input)
                .after(super::interaction::actions)
                .before(crate::ui::actions::handle_editor_shortcuts)
                .in_set(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            scene::sync
                .after(super::presentation::sync)
                .in_set(crate::SceneEditorSystemSet::Presentation),
        );
}
