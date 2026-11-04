//! 显式批量保存本次替换或全部会话草稿；共用唯一IO槽与逐文件受检写入，不丢失败草稿。

use bevy::prelude::*;

mod components;
mod control;
#[cfg(test)]
mod gpu_tests;
mod messages;
mod presentation;
mod resources;
#[cfg(test)]
mod tests;
mod types;
mod worker;

pub(super) use control::{adopt, request};
#[cfg(test)]
pub(super) use gpu_tests::gpu_assert_batch_save;
pub(crate) use messages::SaveAllDocuments;
pub(crate) use resources::DocumentBatchSaveState;
pub(super) use types::{SaveReply, SaveScope};

pub(super) fn install(app: &mut App) {
    app.init_resource::<DocumentBatchSaveState>()
        .add_message::<SaveAllDocuments>()
        .add_systems(
            Update,
            (control::input, control::scroll)
                .chain()
                .after(super::interaction::actions)
                .before(super::navigation::NavigationSystemSet::Input)
                .before(crate::ui::actions::handle_editor_shortcuts)
                .before(crate::ui::systems::scroll_editor_side_panels)
                .in_set(crate::MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            presentation::sync
                .after(super::presentation::sync)
                .in_set(crate::MapEditorSystemSet::Presentation),
        );
}
