//! 网格的列表、作者参数与异步结构事务；场景环境和其他作者域不在这里复制。

mod components;
mod creation; // 生成器能力预检与共享初始规格保持一致，原始GLB不受生成格距限制。
mod creation_placement; // 统一创建请求与冻结射线点放，不在帧关键路径同步查询整张地形。
mod import_glb; // Scene0真实范围与作者变换在后台读取，不改写原模型。
mod jobs;
mod height; // 作者高度域只改合法范围，体素米制实高沿用完整姿态缩放。
mod stitches;
mod names;
mod numeric;
mod lod_list; // 完整高度的连续LOD列表，共用既有属性滚动与固定可见行池。
mod presentation; // 常驻列表和分区原位同步，不复制作者状态。
mod row_controls; // 固定行图标与场景列表共用选择高亮；目标随真实呈现冻结。
mod scene_menu;
mod resources;
mod scene;
mod systems;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) mod page_tests;
mod types;
pub(crate) use import_glb::read as read_glb;
pub(crate) use resources::TerrainObjectsEditorState;
pub(super) use resources::PanelWorkspaceGate;
pub(crate) use scene::{panel, properties_panel, scene_list_panel};
pub(crate) use systems::poll as poll_objects;
pub(crate) use systems::selected;
pub(crate) use systems::start;
pub(crate) use types::Mutation;
pub(crate) use types::{CreationSource, CreationPlacementMode};

/// 与常驻验收调用方保持相同编译边界，普通构建不包含操作入口。
#[cfg(any(test, feature = "validation"))]
pub(crate) fn validation_select_row(world: &mut bevy::prelude::World, id: &feathrise_scene_content::TerrainId)
    -> Result<bevy::prelude::Entity, String> {
    use bevy::prelude::*;
    let index = world.resource::<crate::authoring::EditorAuthoringSession>().draft.as_ref()
        .and_then(|draft| draft.manifest.terrains.as_ref())
        .filter(|objects| objects.len() <= 8)
        .and_then(|objects| objects.iter().position(|object| &object.id == id))
        .ok_or("地形手柄验收只接受八个以内的自有地形对象")?;
    let start = world.resource::<TerrainObjectsEditorState>().list_start;
    let entity = world.query::<(Entity, &components::Row)>().iter(world)
        .find_map(|(entity, row)| (start + row.0 == index).then_some(entity))
        .ok_or("原地形场景行尚未建立")?;
    let mut nodes = world.query::<(&Node, Option<&ChildOf>)>();
    if !crate::viewport::modes::controls_visible(entity, &nodes.query(world)) {
        return Err("原地形场景行不可见".into());
    }
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(entity)
}

/// 统一面板先完成数值提交与请求分派，基础形状再在同一输入帧消费请求并捕获释放。
#[derive(bevy::prelude::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CreationRequestDispatch;

pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.add_systems(Update,stitches::draw_preview.in_set(crate::SceneEditorSystemSet::Cursor).after(systems::poll));
    app.init_resource::<TerrainObjectsEditorState>()
        .add_systems(Update, creation_placement::control.after(numeric::input)
            .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(CreationRequestDispatch)
            .in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, creation_placement::point_place
            .after(crate::viewport::snapping::update_mesh_cursor)
            .before(crate::viewport::select_editor_viewport_content)
            .in_set(crate::SceneEditorSystemSet::Cursor))
        .init_resource::<resources::NameInputBinding>()
        .init_resource::<feathrise_scene::TerrainObjectEditorVisibility>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .add_message::<feathrise_editor_ui::NumericInputSetValue>()
        .add_systems(Update, scene_menu::input.after(crate::ui::scene_context_menu::SceneMenuDispatch)
            .in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, scene_menu::bind_rows.after(row_controls::sync)
            .in_set(crate::SceneEditorSystemSet::Presentation))
        .add_systems(
            Update,
            (names::focus, numeric::freeze, systems::input, names::input, numeric::input)
                .chain()
                .in_set(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            systems::poll.in_set(crate::SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (scene::setup, systems::sync, row_controls::sync, names::sync)
                .chain()
                .in_set(crate::SceneEditorSystemSet::Presentation),
        )
        .add_systems(Update, creation::sync_feedback.in_set(crate::SceneEditorSystemSet::Presentation));
}
