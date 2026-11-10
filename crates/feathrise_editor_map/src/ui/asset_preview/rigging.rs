//! 独立 GLB 的人形绑骨工作流，复用资源预览画布、当前工程资产根和动画时间轴。

use crate::MapEditorSystemSet;
use bevy::prelude::*;

#[path = "rigging_backend.rs"]
mod backend;
#[path = "rigging_context.rs"]
mod context;
#[path = "rigging_landmarks.rs"]
mod landmarks;
#[path = "rigging_resources.rs"]
mod resources;
#[path = "rigging_scene.rs"]
pub(super) mod scene;
#[path = "rigging_systems.rs"]
mod systems;
#[path = "rigging_types.rs"]
mod types;

pub(crate) use resources::RiggingState;
#[cfg(feature = "validation")]
pub(crate) use types::{RigAction, RigLandmarkDot};

pub(super) fn install(app: &mut App) {
    app.init_resource::<RiggingState>()
        .add_systems(
            Update,
            (systems::actions, landmarks::input)
                .chain()
                // 原资产行打开位于共用鼠标门之后，侧栏发出的打开意图由它随后消费。
                .after(super::workspace::actions)
                .after(super::interaction::handle_asset_preview_actions)
                .after(crate::map_browser::handle_map_browser_input)
                .before(super::animation::receive_rig_animation)
                .before(super::navigation::control_asset_preview)
                .before(crate::authoring::mesh_edit::MeshEditInput)
                .before(crate::ui::actions::handle_editor_shortcuts)
                .before(crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard)
                .in_set(MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            systems::poll
                .after(super::loading::poll_asset_preview_loading)
                .in_set(MapEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (systems::present, landmarks::present)
                .after(super::presentation::sync_asset_preview_controls)
                .in_set(MapEditorSystemSet::Presentation),
        );
}
