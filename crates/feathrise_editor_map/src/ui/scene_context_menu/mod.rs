//! 场景树对象右键菜单：固定浮层、中文重命名、冻结目标和原作者操作的单帧请求。

mod components;
mod focus;
mod resources;
mod scene;
mod systems;
mod types;

pub(crate) use components::SceneObjectRow;
#[cfg(test)]
pub(crate) use focus::select_target as select_target_for_tests; // 保存回归运行正式树选择系统，不在夹具里手动恢复根水。
pub use resources::SceneContextMenuState;
pub(crate) use systems::MenuContext;
pub use types::SceneMenuAction;
#[cfg(test)]
pub(crate) use types::SceneObjectRequest;
pub(crate) use types::{SceneObjectKey, SceneObjectTarget};

/// 显式隔离工程验收只冻结当前真实树行，再交原dispatch复核；不直接执行领域写侧。
#[cfg(feature = "validation")]
pub fn queue_domain_action_for_validation(
    world: &mut bevy::prelude::World,
    kind: crate::game_authoring::map_interaction::DomainItemKind,
    id: &str,
    action: SceneMenuAction,
    name: &str,
) -> Result<bool, String> {
    let target = world.query::<(bevy::prelude::Entity, &components::SceneObjectRow, &bevy::prelude::Node)>()
        .iter(world).find_map(|(entity, row, node)| {
            let target = row.0.as_ref()?;
            (node.display != bevy::prelude::Display::None && matches!(&target.key,
                SceneObjectKey::Domain { kind: row_kind, id: row_id, .. } if *row_kind == kind && row_id == id))
                .then(|| (entity, target.clone()))
        });
    let Some((row, target)) = target else {
        return Ok(false);
    };
    if !target.current(
        world.resource::<crate::resources::MapEditorState>(),
        world.resource::<crate::authoring::EditorAuthoringSession>(),
    ) || !target.allows(action)
    {
        return Err("当前场景行已过期、锁定或不允许此菜单动作".into());
    }
    if action == SceneMenuAction::Rename {
        target.validate_name(name)?;
    }
    let mut menu = world.resource_mut::<SceneContextMenuState>();
    if menu.queued.is_some() {
        return Ok(false);
    }
    menu.queued = Some((
        types::MenuTarget {
            row,
            object: target.clone(),
        },
        types::SceneObjectRequest {
            target,
            action,
            name: name.into(),
        },
    ));
    Ok(true)
}

use bevy::prelude::*;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneMenuDispatch;

fn install_controls(app: &mut App) {
    app.init_resource::<SceneContextMenuState>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_systems(
            Update,
            systems::control_menu
                .after(super::asset_context_menu::AssetMenuInput)
                .before(
                    super::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
                )
                .in_set(crate::MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            scene::scroll
                .after(systems::control_menu)
                .before(
                    super::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
                )
                .in_set(crate::MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (systems::dispatch, focus::select_target)
                .chain()
                .after(
                    super::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
                )
                .in_set(SceneMenuDispatch)
                .in_set(crate::MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            scene::setup.in_set(crate::MapEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            scene::sync.in_set(crate::MapEditorSystemSet::Presentation),
        );
}

pub(crate) fn install(app: &mut App) {
    install_controls(app);
    app.add_systems(
        Update,
        crate::authoring::placement_batch::handle_scene_menu
            .after(SceneMenuDispatch)
            .after(crate::authoring::placement_batch::handle_placement_batch)
            .in_set(crate::MapEditorSystemSet::Input),
    )
    .add_systems(
        Update,
        super::gltf_browser::bind_scene_menu_rows
            .after(super::gltf_browser::sync_gltf_browser_rows)
            .in_set(crate::MapEditorSystemSet::Presentation),
    );
}

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod pointer_tests;
#[cfg(test)]
mod tests;
