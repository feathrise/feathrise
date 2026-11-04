//! F12固定底栏、真实操作回执与非模态完整诊断，不拥有保存或加载事务。

mod components;
mod helpers;
mod inputs;
mod resources;
mod scene;
mod systems;
mod types;

pub(crate) use resources::EditorStatusBarState;
pub(crate) use components::{PanelStatusText, StatusPopup};
pub(super) use scene::{footer, popup};

use bevy::prelude::*;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<EditorStatusBarState>()
        .init_resource::<Clipboard>()
        .add_systems(
            PreUpdate,
            systems::track_pointer.after(bevy::ui::ui_focus_system),
        )
        .add_systems(
            Update,
            systems::control_details
                .in_set(crate::MapEditorSystemSet::Input)
                .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .after(crate::map_browser::handle_map_browser_input)
                .before(
                    super::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
                ),
        )
        .add_systems(
            PostUpdate,
            (systems::hide_panel_status_texts, systems::observe_status, systems::sync_popup_layout, systems::sync_presentation)
                .chain()
                .after(feathrise_editor_ui::feedback::PanelStatusProjection)
                .before(bevy::ui::UiSystems::Prepare)
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod layout_tests;

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
pub(crate) mod feedback_tests;
