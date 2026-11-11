//! 编辑器公共问号帮助：唯一根浮层借用原正文实体，不让操作说明挤占作者面板。

mod components;
mod resources;
mod scene;
mod systems;
mod types;

pub(crate) use components::{ContextHelpMounted, ContextHelpPopup};
pub(crate) use resources::EditorContextHelpState;
pub(super) use scene::popup;
pub(crate) use scene::{help_button, section_header};

/// GPU夹具在真实ui_focus命中后立即执行正式帮助输入，避免下一帧清掉模拟鼠标边沿。
#[cfg(test)]
pub(crate) fn handle_input_for_test(world: &mut bevy::prelude::World) {
    use bevy::ecs::system::RunSystemOnce;
    world.run_system_once(systems::handle_input).unwrap();
}

use bevy::prelude::*;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<EditorContextHelpState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_systems(
            PreUpdate,
            systems::track_pointer.after(bevy::ui::ui_focus_system),
        )
        .add_systems(
            Update,
            systems::handle_input
                .in_set(crate::SceneEditorSystemSet::Input)
                .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .before(super::editor_settings::handle_settings_input)
                .before(
                    super::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
                ),
        )
        .add_systems(
            PostUpdate,
            (systems::sync_popup, systems::position_popup).chain()
                .before(bevy::ui::UiSystems::Prepare)
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
}

#[cfg(test)]
mod tests;
