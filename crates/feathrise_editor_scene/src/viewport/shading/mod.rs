//! Blender四球视图着色：正式按钮、相机专属显示和保留原顶点着色器的线框管线。
mod camera;
mod components;
mod render;
mod resources;
pub(crate) mod types;
mod ui;
pub(crate) use components::ViewShadingRoot;
pub(crate) use components::{ShadingHost, ShadingToolbarRoot};
pub(crate) use resources::AssetPreviewShadingState;
pub(crate) use ui::{ShadingClicks, spawn_toolbar};

/// 单测与显式窗口验收转发原可见四球按钮的点击协议，不直接改ShadingState。
#[cfg(any(test, feature = "validation"))]
pub(crate) fn validation_press_mode(world: &mut bevy::prelude::World, index: u8) -> Result<bevy::prelude::Entity, String> {
    use bevy::prelude::*;
    let mode = *types::ShadingMode::ALL.get(usize::from(index)).ok_or("四球验收模式越界")?;
    let entity = world.query::<(Entity, &components::ShadingButton, &InheritedVisibility, Option<&ShadingHost>)>()
        .iter(world).find_map(|(entity, button, visible, host)|
            (visible.get() && host.copied().unwrap_or_default() == ShadingHost::Map
                && matches!(button.0, types::ShadingAction::Mode(current) if current == mode)).then_some(entity))
        .ok_or("没有当前实际可见的原四球按钮")?;
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(entity)
}

/// 真实窗口验收只定位当前模型四球按钮，由原指针系统完成按下和松开。
#[cfg(feature = "validation")]
pub(crate) fn validation_asset_mode_button(world: &mut World, index: u8) -> Option<Entity> {
    let mode = *types::ShadingMode::ALL.get(usize::from(index))?;
    validation_asset_button(world, |action| matches!(action, types::ShadingAction::Mode(current) if current == mode))
}

#[cfg(feature = "validation")]
pub(crate) fn validation_asset_menu_button(world: &mut World) -> Option<Entity> {
    validation_asset_button(world, |action| matches!(action, types::ShadingAction::Menu))
}

#[cfg(feature = "validation")]
fn validation_asset_button(
    world: &mut World,
    predicate: impl Fn(types::ShadingAction) -> bool,
) -> Option<Entity> {
    world.query::<(Entity, &components::ShadingButton, &ShadingHost, &InheritedVisibility, &ComputedNode)>()
        .iter(world).find_map(|(entity, button, host, visible, node)|
            (*host == ShadingHost::Asset && visible.get() && node.size().min_element() > 0.0
                && predicate(button.0)).then_some(entity))
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
