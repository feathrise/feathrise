//! 场景管理模态共用表现：有界正文、固定操作及危险样式，不接管文件事务。

pub(crate) mod components;
pub(crate) mod scene;
mod systems;
mod types;

pub(crate) use systems::ManagementWheelGuard;
pub(crate) use types::DialogKind;

use bevy::prelude::*;

pub(crate) fn install(app: &mut App) {
    app.add_systems(
        Update,
        (systems::scroll_dialogs, systems::sync_button_styles)
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
}

#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod tests;
