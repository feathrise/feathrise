//! 蒙皮权重工作区。绑定原模型资产身份，复用中央视口和动作容器。

mod asset;
mod edit;
mod fields;
mod file;
mod influences;
mod input;
pub(crate) mod panel;
mod posed;
mod preview;
#[cfg(feature = "validation")]
mod problem_validation;
pub(crate) mod project_close;
mod resources;
mod spatial;
mod systems;
#[cfg(feature = "validation")]
mod validation;

pub(super) use resources::TreePick;
pub(crate) use resources::WeightEditor;
#[cfg(feature = "validation")]
pub(super) use validation::install as install_validation;

use crate::SceneEditorSystemSet;
use bevy::prelude::*;

pub(super) fn install(app: &mut App) {
    app.init_resource::<WeightEditor>()
        .init_resource::<preview::WeightPreview>()
        .add_message::<TreePick>()
        .add_message::<AssetEvent<bevy::gltf::Gltf>>()
        .add_observer(preview::ready)
        .add_systems(Update,
            (systems::actions, input::input).chain()
                .after(super::workspace::actions)
                .after(super::interaction::handle_asset_preview_actions)
                .after(super::gltf_tree::weight_row_input)
                .before(super::opening::open)
                .before(super::navigation::control_asset_preview)
                .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
                .in_set(SceneEditorSystemSet::Input))
        .add_systems(Update,
            fields::input
                .run_if(resource_exists::<Messages<feathrise_editor_ui::NumericInputCommitted>>)
                .in_set(SceneEditorSystemSet::Input))
        .add_systems(Update,
            (systems::poll.after(super::loading::poll_asset_preview_loading), fields::setup, input::setup_overlay)
                .in_set(SceneEditorSystemSet::Tasks))
        .add_systems(Update,
            systems::sync_sidebar.before(crate::ui::sidebars::sync_editor_sidebar_presentation)
                .in_set(SceneEditorSystemSet::Presentation))
        .add_systems(Update,
            (panel::present, input::present_overlay,
                fields::sync.run_if(resource_exists::<Messages<feathrise_editor_ui::NumericInputSetValue>>))
                .in_set(SceneEditorSystemSet::Presentation))
        .add_systems(PostUpdate, preview::update)
        .add_systems(PostUpdate, posed::capture.after(preview::update)
            .after(bevy::transform::TransformSystems::Propagate)
            .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate));
}
