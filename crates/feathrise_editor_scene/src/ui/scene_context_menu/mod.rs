//! 场景树右键菜单和双击操作共用冻结的树行身份，作者操作由各自入口处理。

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
pub(crate) use types::{SceneObjectKey, SceneObjectRequest, SceneObjectTarget};

/// 自有验收从本帧真实地形树行冻结完整目标，再由原菜单分派和唯一选择出口接管模型/地形身份。
#[cfg(any(test, feature = "validation"))]
pub(crate) fn queue_terrain_focus_for_validation(world: &mut bevy::prelude::World,
    id: &feathrise_scene_content::TerrainId) -> Result<bool, String> {
    queue_terrain_action_for_validation(world, id, SceneMenuAction::Focus)
}

#[cfg(any(test, feature = "validation"))]
pub(crate) fn queue_terrain_action_for_validation(world: &mut bevy::prelude::World,
    id: &feathrise_scene_content::TerrainId, action: SceneMenuAction) -> Result<bool, String> {
    queue_visible_scene_action(world, action, |key| {
        matches!(key, SceneObjectKey::Terrain { id: row_id, .. } if row_id == id)
    })
}

#[cfg(any(test, feature = "validation"))]
pub(crate) fn queue_blender_action_for_validation(world: &mut World, id: &str) -> Result<bool, String> {
    queue_visible_scene_action(world, SceneMenuAction::EditInBlender, |key| {
        matches!(key, SceneObjectKey::Placement { id: row_id, mesh: None, .. } if row_id == id)
    })
}

#[cfg(feature = "validation")]
pub(crate) fn visible_action_for_validation(world: &mut World, action: SceneMenuAction) -> Option<Entity> {
    let buttons: Vec<_> = world.query::<(Entity, &components::MenuButton, &ComputedNode)>().iter(world)
        .filter_map(|(entity, button, layout)| (button.0 == action && !layout.is_empty()).then_some(entity)).collect();
    let mut nodes = world.query::<(&Node, Option<&ChildOf>)>();
    let nodes = nodes.query(world);
    buttons.into_iter().find(|&button| crate::viewport::modes::controls_visible(button, &nodes))
}

/// 验收读取生产菜单面板实体及实际布局。主菜单在打开侧面板前也能测量。
#[cfg(feature = "validation")]
pub(crate) fn scene_menu_panels_for_validation(world: &mut World) -> Option<(Entity, Option<Entity>)> {
    use components::{NewGlbPopup, Popup};
    let panels: Vec<_> = world.query_filtered::<(Entity, Has<NewGlbPopup>, &ComputedNode),
        Or<(With<Popup>, With<NewGlbPopup>)>>().iter(world)
        .filter_map(|(entity, side, layout)| (!layout.is_empty()).then_some((entity, side))).collect();
    let mut nodes = world.query::<(&Node, Option<&ChildOf>)>();
    let nodes = nodes.query(world);
    let visible = |side| panels.iter().find_map(|&(entity, is_side)| {
        (side == is_side && crate::viewport::modes::controls_visible(entity, &nodes)).then_some(entity)
    });
    Some((visible(false)?, visible(true)))
}

#[cfg(feature = "validation")]
pub(crate) fn visible_rename_controls_for_validation(world: &mut World) -> Option<(Entity, Entity)> {
    let input = world.query_filtered::<Entity, With<components::RenameInput>>().single(world).ok()?;
    let confirm = world.query_filtered::<Entity, With<components::RenameConfirm>>().single(world).ok()?;
    let mut nodes = world.query::<(&Node, Option<&ChildOf>)>();
    let nodes = nodes.query(world);
    [input, confirm].into_iter().all(|entity| crate::viewport::modes::controls_visible(entity, &nodes))
        .then_some((input, confirm))
}

#[cfg(feature = "validation")]
pub(crate) fn press_action_for_validation(world: &mut World, action: SceneMenuAction) -> Option<Entity> {
    let button = visible_action_for_validation(world, action)?;
    world.entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Some(button)
}

/// 验收从本帧真实可见的场景树行发出菜单命令，沿用冻结、静默门和业务分派。
#[cfg(any(test, feature = "validation"))]
fn queue_visible_scene_action(
    world: &mut World,
    action: SceneMenuAction,
    matches_key: impl Fn(&SceneObjectKey) -> bool,
) -> Result<bool, String> {
    use crate::authoring::EditorAuthoringSession;

    let mut rows = world.query::<(Entity, &SceneObjectRow, &Node)>();
    let Some((row, target)) = rows.iter(world).find_map(|(entity, row, node)| {
        let target = row.0.as_ref()?;
        (node.display != Display::None && matches_key(&target.key))
            .then(|| (entity, target.clone()))
    }) else {
        return Ok(false);
    };
    let mut nodes = world.query::<(&Node, Option<&ChildOf>)>();
    let session = world.resource::<EditorAuthoringSession>();
    if !crate::viewport::modes::controls_visible(row, &nodes.query(world))
        || !target.current(world.resource::<crate::resources::SceneEditorState>(), session)
    {
        return Ok(false);
    }
    if !target.allows(action) || !target.menu_has(action, session) {
        return Err("当前场景树行不允许此菜单动作".into());
    }
    let mut menu = world.resource_mut::<SceneContextMenuState>();
    if menu.queued.is_some() {
        return Ok(false);
    }
    menu.queued = Some((
        types::MenuTarget { row, object: target.clone() },
        types::SceneObjectRequest { target, action, name: String::new() },
    ));
    Ok(true)
}

/// 显式隔离工程验收只冻结当前真实树行，再交原dispatch复核；不直接执行领域写侧。
#[cfg(feature = "validation")]
pub fn queue_domain_action_for_validation(
    world: &mut bevy::prelude::World,
    kind: crate::game_authoring::scene_interaction::DomainItemKind,
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
        world.resource::<crate::resources::SceneEditorState>(),
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
        .add_message::<super::scene_motion::OpenSceneAnimation>()
        .add_systems(
            Update,
            systems::control_menu
                .after(super::asset_context_menu::AssetMenuInput)
                .before(
                    super::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
                )
                .in_set(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            scene::scroll
                .after(systems::control_menu)
                .before(
                    super::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
                )
                .in_set(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            systems::queue_tree_double_click
                .after(super::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
                .before(super::scene_layers::read_tree_selection)
                .before(super::gltf_browser::handle_gltf_browser_input)
                .before(SceneMenuDispatch)
                .in_set(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (systems::dispatch, focus::select_target)
                .chain()
                .after(
                    super::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
                )
                .in_set(SceneMenuDispatch)
                .in_set(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            scene::setup.in_set(crate::SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            scene::sync.in_set(crate::SceneEditorSystemSet::Presentation),
        );
}

pub(crate) fn install(app: &mut App) {
    install_controls(app);
    app.add_systems(
        Update,
        (systems::send_blender_action, systems::reveal_glb_file, systems::queue_main_glb_shape)
            .after(focus::select_target)
            .in_set(SceneMenuDispatch)
            .in_set(crate::SceneEditorSystemSet::Input),
    )
    .add_systems(
        Update,
        crate::authoring::placement_batch::handle_scene_menu
            .after(SceneMenuDispatch)
            .after(crate::authoring::placement_batch::handle_placement_batch)
            .in_set(crate::SceneEditorSystemSet::Input),
    )
    .add_systems(
        Update,
        super::gltf_browser::bind_scene_menu_rows
            .after(super::gltf_browser::sync_gltf_browser_rows)
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
}

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod pointer_tests;
#[cfg(test)]
mod tests;
