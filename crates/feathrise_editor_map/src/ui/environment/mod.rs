//! F12环境页的地图持久参数、运行时调试及全局默认；领域输入与光照运行时保持原所有权。

mod components;
mod helpers;
mod scene;
mod systems;
mod types;

pub(in crate::ui) use scene::environment_page;
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
        .init_resource::<GlobalEnvironmentAuthoringState>();
    app.world_mut().resource_mut::<GlobalEnvironmentAuthoringState>().initialized = true;
    app.world_mut().resource_mut::<WorldClock>().set_hour(12.0);
    app.add_systems(Update, (setup_environment_numeric_inputs, setup_celestial_numeric_inputs,
        setup_world_environment_controls, setup_world_environment_slider_layout, sync_environment_numeric_inputs,
        sync_celestial_controls, sync_world_environment_controls, sync_editor_environment_text)
        .chain().in_set(crate::MapEditorSystemSet::Presentation));
}

#[cfg(test)]
mod tests;
