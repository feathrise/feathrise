//! 编译诊断使用原作者弹窗配方和固定行池；点击只发原中央文档定位消息，完整终端仍可原生选择复制。

#[path = "diagnostics_ui/components.rs"]
mod components;
#[path = "diagnostics_ui/resources.rs"]
mod resources;
#[path = "diagnostics_ui/scene.rs"]
mod scene;
#[path = "diagnostics_ui/systems.rs"]
mod systems;
#[cfg(feature = "validation")]
#[path = "diagnostics_ui/validation.rs"]
pub(crate) mod validation;
#[cfg(test)]
#[path = "diagnostics_ui/tests.rs"]
mod tests;

pub(crate) use resources::DiagnosticsOverlayState;
use bevy::prelude::*;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DiagnosticsInput;

pub(super) fn install(app: &mut App) {
    app.init_resource::<DiagnosticsOverlayState>()
        .add_systems(Update, scene::mount.before(systems::present))
        .add_systems(Update, systems::control.in_set(DiagnosticsInput)
            .after(crate::native_project_close::ProjectCloseInput)
            .after(crate::project_export::ExportInput)
            .before(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
            .before(feathrise_editor_core::MapEditorSystemSet::Input))
        .add_systems(Update, systems::present.after(systems::control));
}
