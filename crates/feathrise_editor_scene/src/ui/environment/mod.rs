//! 环境页的场景配置和全局方案库入口。

mod components;
mod helpers;
mod library;
mod scene;
mod systems;
mod types;

pub(in crate::ui) use scene::environment_page;
pub(crate) use library::{EnvironmentPage, EnvironmentPageState, handle_environment_library_buttons, sync_environment_library};
pub(crate) use systems::sync_editor_environment_text;

#[cfg(test)]
pub(crate) fn install_snapshot_presentation(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    use crate::{authoring::*, world_environment_controls::*};
    use feathrise_atmosphere::{WorldClock, WorldEnvironmentModifiers, WeatherDebugOverride, WeatherState};
    app.init_resource::<WorldClock>().init_resource::<WorldEnvironmentModifiers>()
        .init_resource::<WeatherDebugOverride>().init_resource::<WeatherState>()
        .init_resource::<CelestialNumericInputState>().init_resource::<CelestialSectionState>()
        .init_resource::<EnvironmentNumericInputState>().init_resource::<GlobalEnvironmentSaveTask>()
        .init_resource::<GlobalEnvironmentAuthoringState>().init_resource::<EnvironmentPageState>();
    app.world_mut().resource_mut::<GlobalEnvironmentAuthoringState>().initialized = true;
    app.world_mut().resource_mut::<WorldClock>().set_hour(12.0);
    app.add_systems(Update, (setup_environment_numeric_inputs, setup_celestial_numeric_inputs,
        setup_world_environment_controls, setup_world_environment_slider_layout, sync_environment_numeric_inputs,
        sync_library_sky_inputs, sync_celestial_controls, sync_world_environment_controls,
        sync_editor_environment_text, sync_environment_library)
        .chain().in_set(crate::SceneEditorSystemSet::Presentation));
}

#[cfg(test)]
mod tests;
