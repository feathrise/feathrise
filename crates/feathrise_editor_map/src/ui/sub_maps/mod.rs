//! 完整子地图作者入口；保存引用、整体位置与朝向，内部作者值始终留在源地图。

mod components;
mod resources;
mod scene;
mod systems;
mod numeric;
mod scene_menu;
pub(crate) use systems::apply;
pub(crate) use resources::SubMapEditorState;
pub(crate) use scene::{panel, properties_panel, creation_button};
pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<SubMapEditorState>()
        .add_systems(Update, (systems::input, numeric::input, scene_menu::rename).chain()
            .after(crate::ui::scene_context_menu::SceneMenuDispatch).in_set(crate::MapEditorSystemSet::Input))
        .add_systems(Update, scene_menu::poll.in_set(crate::MapEditorSystemSet::Tasks))
        .add_systems(Update, scene_menu::bind_rows.after(systems::sync).in_set(crate::MapEditorSystemSet::Presentation))
        .add_systems(Update, (systems::poll, systems::publish).chain().in_set(crate::MapEditorSystemSet::Tasks))
        .add_systems(Update, (scene::setup, systems::sync).chain().in_set(crate::MapEditorSystemSet::Presentation));
}
