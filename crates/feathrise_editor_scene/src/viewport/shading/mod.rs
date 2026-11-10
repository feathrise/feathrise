//! Blender四球视图着色：正式按钮、相机专属显示和保留原顶点着色器的线框管线。
mod camera;
mod components;
mod render;
mod resources;
pub(crate) mod types;
mod ui;
pub(crate) use components::ViewShadingRoot;

/// 单测与显式窗口验收转发原可见四球按钮的点击协议，不直接改ShadingState。
#[cfg(any(test, feature = "validation"))]
pub(crate) fn validation_press_mode(world: &mut bevy::prelude::World, index: u8) -> Result<bevy::prelude::Entity, String> {
    use bevy::prelude::*;
    let mode = match index {
        0 => types::ShadingMode::Wireframe, 1 => types::ShadingMode::Solid,
        2 => types::ShadingMode::MaterialPreview, 3 => types::ShadingMode::Rendered,
        _ => return Err("四球验收模式越界".into()),
    };
    let entity = world.query::<(Entity, &components::ShadingButton, &InheritedVisibility)>()
        .iter(world).find_map(|(entity,button,visible)|
            (visible.get() && matches!(button.0, types::ShadingAction::Mode(current) if current==mode)).then_some(entity))
        .ok_or("没有当前实际可见的原四球按钮")?;
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(entity)
}
#[cfg(test)]
pub(crate) use resources::ShadingState;
#[cfg(test)]
pub(crate) use types::ShadingMode;
#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod wireframe_gpu_tests;
#[cfg(test)]
mod tests;

/// 网格GPU夹具复用正式着色与相机附件，覆盖合成顺序而不创建编辑器UI。
#[cfg(test)]
pub(crate) fn install_grid_gpu_validation(app: &mut App) {
    app.init_resource::<ShadingState>()
        .add_plugins(render::ShadingRenderPlugin)
        .add_systems(Update, camera::sync);
}

use bevy::prelude::*;

/// 截图夹具也装正式节点和定位系统，避免只验独立控件而遗漏窄视口的互相遮挡。
#[cfg(test)]
pub(crate) fn install_snapshot_controls(app: &mut App) {
    app.init_resource::<resources::ShadingState>().add_systems(
        Update,
        (
            ui::setup,
            super::orientation_axis::setup_editor_orientation_axis,
            ui::style,
            super::orientation_axis::sync_editor_orientation_axis,
            ui::place_overlays,
        )
            .chain(),
    );
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<resources::ShadingState>()
        .add_plugins(render::ShadingRenderPlugin)
        .add_systems(Update, ui::setup.in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(
            Update,
            ui::close_menu_escape
                .in_set(crate::SceneEditorSystemSet::Input)
                .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
                .before(crate::authoring::mesh_edit::MeshEditInput),
        )
        .add_systems(
            Update,
            ui::input
                .in_set(crate::SceneEditorSystemSet::Input)
                .after(crate::authoring::mesh_edit::MeshEditInput)
                .before(crate::viewport::placement_move::handle_placement_move)
                .before(crate::viewport::control_editor_viewport),
        )
        .add_systems(
            Update,
            (ui::style, ui::place_overlays, camera::sync)
                .chain()
                .in_set(crate::SceneEditorSystemSet::Presentation),
        );
}
