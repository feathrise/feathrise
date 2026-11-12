//! 统一网格创建面板的立方体请求、半透明点放预览与空场景显式工作平面。
//!
//! 【需求设计】基础形状复用普通摆放的唯一真实地面与吸附协议；编辑期token只表达
//! 当前添加意图，持久记录使用Cuboid参数和空asset_path，不能把token交给AssetServer。

mod components;
mod placement;
mod resources;
mod systems;

pub(crate) use placement::{is_cuboid_selection, plane_hit, work_plane_is_available};
pub(crate) use resources::{BasicShapeState, CuboidCreationRequest};

pub(crate) use feathrise_editor_animation_model::media::CUBOID_SELECTION;

pub(crate) fn install(app: &mut bevy::prelude::App) {
    use crate::SceneEditorSystemSet;
    use bevy::prelude::*;
    app.init_resource::<BasicShapeState>()
        .add_systems(Startup, placement::setup_preview)
        .add_systems(Update, systems::control_menu
            .after(crate::ui::asset_context_menu::AssetMenuInput)
            .after(crate::ui::terrain_objects::CreationRequestDispatch)
            .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, placement::update_preview
            .after(crate::viewport::resolve_vegetation_ground_cursor)
            .after(crate::viewport::snapping::update_mesh_cursor)
            .in_set(SceneEditorSystemSet::Cursor));
}

#[cfg(test)]
mod creation_tests;
#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod pointer_tests;
#[cfg(test)]
mod tests;
