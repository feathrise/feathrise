//! 完整子场景作者入口；保存引用、整体位置与朝向，内部作者值始终留在源场景。

mod components;
mod resources;
mod scene;
mod systems;
mod numeric;
mod reference_coordinates;
mod scene_menu;
pub(crate) use systems::apply;
pub(crate) use resources::SubsceneEditorState;
pub(crate) use reference_coordinates::{ReferenceCoordinates, SceneReferenceCoordinates};
pub(crate) use scene::{panel, properties_panel, creation_button};
pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<SubsceneEditorState>()
        .init_resource::<SceneReferenceCoordinates>()
        .add_observer(reference_coordinates::index_reference)
        .add_observer(reference_coordinates::unindex_reference)
        .add_systems(Update, reference_coordinates::clear_inactive.after(crate::SceneDocumentTransitionSet)
            .before(crate::SceneEditorSystemSet::Presentation))
        .add_systems(Update, (systems::input, numeric::input, scene_menu::rename).chain()
            .after(crate::ui::scene_context_menu::SceneMenuDispatch).in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, scene_menu::poll.in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(Update, scene_menu::bind_rows.after(systems::sync).in_set(crate::SceneEditorSystemSet::Presentation))
        .add_systems(Update, (systems::poll, systems::publish).chain().in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(Update, (scene::setup, systems::sync).chain().in_set(crate::SceneEditorSystemSet::Presentation));
}
