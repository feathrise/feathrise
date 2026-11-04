//! 空间笔刷先摆放再执行；固定大小预览与完整密度事务分离，沿用原视口手柄数学。
mod components;
mod path;
mod request;
mod resources;
mod systems;
mod types;
mod ui;
use crate::MapEditorSystemSet;
use bevy::prelude::*;
pub(crate) use resources::SpatialBrushState;
pub(crate) use types::SpatialBrushRequest;
pub(crate) use types::SpatialBrushGizmos; // 常驻视口仅按组身份只读投影原工具线条。
pub(crate) fn install(app: &mut App) {
    app.init_resource::<SpatialBrushState>()
        .init_gizmo_group::<types::SpatialBrushGizmos>()
        .add_systems(Startup, systems::configure_gizmos)
        .add_systems(Update, ui::setup.after(MapEditorSystemSet::Tasks).before(MapEditorSystemSet::Presentation))
        .add_systems(Update, ui::input.after(MapEditorSystemSet::Input).after(super::modes::sync_mode).after(path::input).before(MapEditorSystemSet::Tasks))
        .add_systems(Update, path::input.after(MapEditorSystemSet::Input).after(super::modes::sync_mode).before(MapEditorSystemSet::Tasks))
        .add_systems(Update, path::drive.after(ui::input).after(path::input).before(MapEditorSystemSet::Tasks))
        .add_systems(Update, systems::handles.in_set(MapEditorSystemSet::Input)
            .before(crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard)
            .before(super::control_editor_viewport))
        .add_systems(Update, systems::place.after(MapEditorSystemSet::Cursor).before(MapEditorSystemSet::Presentation))
        .add_systems(Update, (ui::sync, path::sync, systems::draw).in_set(MapEditorSystemSet::Presentation));
}
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod place_tests;
#[cfg(test)]
mod tests;
