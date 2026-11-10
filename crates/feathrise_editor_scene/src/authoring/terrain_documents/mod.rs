//! 多地形共享既有作者工具，每个对象独立持有画布、源版本与脏责任；场景文档始终保留容器清单。
mod load;
pub(crate) use load::load_vegetation_canvas;
mod resources;
mod systems;
mod transitions;
mod types;
pub(crate) mod root_water;
pub(crate) use transitions::ensure_primary_canvas;
pub(crate) use transitions::restore_legacy;
pub(crate) use transitions::height_domain_mesh;
mod paint_space;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod root_water_focus_tests;
pub(crate) use load::prepare_sources;
pub(crate) use paint_space::{PaintMetricSpace, canvas_transform, canvas_visible, canvas_writable};
pub(crate) use resources::TerrainDocumentTask;
pub(crate) use resources::CanvasIdle; // 列表/场景树恢复水源也必须服从原画布的笔划与后台事务门。
pub(crate) use systems::{request_focus, scope_history};
#[cfg(test)]
pub(crate) use types::RootWaterCanvas;
pub(crate) use types::{TerrainCanvas, TerrainDocumentIdentity, TerrainDocumentStore};

/// R8画布使用对象源坐标；角色和摆放仍使用世界坐标，转换只发生在画布作者入口。
pub(crate) fn paint_point(
    session: &super::EditorAuthoringSession,
    editor: &crate::resources::SceneEditorState,
    point: Option<bevy::prelude::Vec3>,
) -> Option<bevy::prelude::Vec3> {
    paint_point_on_terrain(session, editor, editor.brush_terrain_id.as_ref(), point)
}

/// 异步完成的画笔点保留其真实命中对象；严格当前悬停可以为空，不能因此丢失已验证点。
pub(crate) fn paint_point_on_terrain(
    session: &super::EditorAuthoringSession,
    editor: &crate::resources::SceneEditorState,
    terrain: Option<&feathrise_scene_content::TerrainId>,
    point: Option<bevy::prelude::Vec3>,
) -> Option<bevy::prelude::Vec3> {
    let draft = session.draft.as_ref()?;
    if editor.terrain_volume_field_active {
        return None;
    }
    if !canvas_writable(session, editor) {
        return None;
    }
    if draft.manifest.terrains.is_some() {
        let active = draft.terrain_documents.active.as_ref()?;
        if draft.terrain_documents.loading
            || editor
                .terrain_canvas_target
                .as_ref()
                .is_some_and(|id| id != &active.definition.id)
            || terrain != Some(&active.definition.id)
            || draft
                .terrain_ground
                .as_ref()
                .is_none_or(|ground| ground.revision != session.terrain_revision)
        {
            return None;
        }
    }
    point.map(|point| draft.terrain_point(point))
}

pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<TerrainDocumentTask>()
        .init_resource::<feathrise_scene::TerrainObjectAuthoringTarget>()
        .configure_sets(
            Update,
            feathrise_scene::TerrainAuthoringApply
                .after(crate::SceneEditorSystemSet::Tasks)
                .before(crate::SceneEditorSystemSet::Cursor)
                .before(feathrise_scene::TerrainTopologySystems::Collect),
        )
        .add_systems(
            Update,
            systems::focus
                .in_set(crate::SceneEditorSystemSet::Tasks)
                .after(crate::ui::terrain_objects::poll_objects)
                .after(crate::viewport::apply_layer_candidate) // 先采用本帧新选择，旧画布完成时才能判断网格是否仍拥有属性栏。
                .before(super::terrain_ground::capture_objects),
        );
}
