//! 一体紧凑顶栏：复用原作者控件，窗口操作统一交给真实顶层窗口，工程子窗口不拥有退出权。
mod components;
mod glyphs;
#[cfg(target_os = "windows")]
mod native;
#[cfg(target_os = "windows")]
mod input_surface;
#[cfg(windows)]
pub(crate) mod placement;
#[cfg(all(feature = "validation", windows))]
pub(crate) mod placement_validation;
mod resources;
mod scene;
mod systems;
mod preview;
mod types;
#[cfg(all(feature = "validation", target_os = "windows"))]
mod validation;
#[cfg(all(feature = "validation", target_os = "windows"))]
mod frame_validation;
#[cfg(all(feature = "validation", target_os = "windows"))]
pub(crate) mod preview_validation;

use bevy::prelude::*;
pub(crate) use components::ChromeSlot;
pub(crate) use resources::ChromeState;
pub(crate) use resources::PreviewChromeLayout;
#[cfg(feature = "validation")]
pub(crate) use preview::probe as preview_chrome_probe;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ChromeMount;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ChromeInput;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ChromeMeasure;

pub(crate) struct WindowChromePlugin;
impl Plugin for WindowChromePlugin {
    fn build(&self, app: &mut App) {
        if !cfg!(target_os = "windows") {
            return;
        }
        app.init_resource::<ChromeState>()
            .init_resource::<resources::ChromeParts>()
            .init_resource::<PreviewChromeLayout>()
            .add_systems(
                Update,
                scene::mount
                    .in_set(ChromeMount)
                    .after(crate::workspace_ui::WorkspaceUiSet::Presentation)
                    .after(crate::viewport_mode::ViewportModeInput),
            )
            .add_systems(
                Update,
                systems::input
                    .in_set(ChromeInput)
                    .before(crate::workspace_ui::WorkspaceUiSet::Input)
                    .after(feathrise_editor_ui::update_editor_button_clicks)
                    .after(crate::native_project_close::ProjectCloseInput)
                    .after(crate::native_plugins::management::PluginManagerInput)
                    .before(crate::viewport_mode::ViewportModeInput)
                    .before(feathrise_editor_core::SceneEditorSystemSet::Input),
            )
            .add_systems(Update, systems::present.after(ChromeMount))
            .add_systems(
                PostUpdate,
                systems::measure.in_set(ChromeMeasure).after(bevy::ui::UiSystems::PostLayout),
            );
        #[cfg(target_os = "windows")]
        native::install(app);
    }
}
