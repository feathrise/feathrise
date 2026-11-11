//! 场景对象管理的UI、轻量作者事务及仅在F12生效的显示覆盖。

mod components;
mod resources;
mod types;
mod scene;
mod systems;
mod scene_menu;
mod visibility;

pub(crate) use resources::SceneObjectState;
pub(crate) use types::PlacementVisibilityIntent;
pub(crate) use scene::{panel, properties_panel};
pub(crate) use visibility::{ObjectVisibilityState, placement_base_visibility, restore_placement_base_visibility};

pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    install_visibility_observers(app);
    app.init_resource::<SceneObjectState>()
        .add_systems(Update, scene_menu::input
            .after(crate::ui::scene_context_menu::SceneMenuDispatch)
            .after(systems::handle_objects).in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, systems::handle_objects
            .after(crate::authoring::placement_batch::handle_placement_batch)
            .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, systems::style_buttons.in_set(crate::SceneEditorSystemSet::Presentation))
        .add_systems(Update, visibility::sync_visibility
            .before(crate::ui::close::finish_scene_editor_close)
            .in_set(crate::SceneEditorSystemSet::Presentation));
}

fn install_visibility_observers(app: &mut bevy::prelude::App) {
    app.init_resource::<visibility::ObjectVisibilityState>()
        .add_observer(visibility::register_root)
        .add_observer(visibility::remove_root)
        .add_observer(visibility::register_deletion_suppression)
        .add_observer(visibility::remove_deletion_suppression)
        .add_observer(visibility::remember_external_visibility);
}

#[cfg(test)]
pub(crate) fn install_visibility_observers_for_tests(app: &mut bevy::prelude::App) {
    install_visibility_observers(app); // 夹具复用实际生产观察器，不伪造外部显隐票据。
}

pub(crate) fn visibility_restored(state: Option<&visibility::ObjectVisibilityState>) -> bool {
    state.is_none_or(|state| state.restored())
}

#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod builtin_visibility_tests;
#[cfg(test)]
mod preview_visibility_tests;
