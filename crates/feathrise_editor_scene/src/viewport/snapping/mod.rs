//! 可视参考网格与统一落点吸附；偏好仍由EditorSettings唯一拥有，地形补查共享不可变输入。
mod components;
mod cursor;
mod grid;
pub(crate) mod grid_render;
mod resources;
pub(crate) mod surface;
mod types;
mod ui;

pub(crate) use cursor::update_mesh_cursor;
pub use grid::{EditorGridGizmos, EditorGridOutlineGizmos};
pub(crate) use grid::{configure_grid, draw_grid};
pub(crate) use resources::MeshSnapCursor;
pub(crate) use ui::{handle_buttons, sync_buttons, toolbar};

/// 单测与显式窗口验收通过实际网格按钮走原设置业务链，普通构建不提供入口。
#[cfg(any(test, feature = "validation"))]
pub(crate) fn validation_toggle_grid(world: &mut bevy::prelude::World) -> Result<bevy::prelude::Entity, String> {
    use bevy::prelude::*;
    let entity = world.query::<(Entity, &components::SnapButton, &InheritedVisibility)>()
        .iter(world).find_map(|(entity,button,visible)|
            (visible.get() && button.0==components::SnapAction::Grid).then_some(entity))
        .ok_or("没有当前实际可见的原网格按钮")?;
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(entity)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cursor_tests;

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
pub(crate) use components::{SnapAction, SnapButton};

#[cfg(test)]
mod grid_lifecycle_tests;
