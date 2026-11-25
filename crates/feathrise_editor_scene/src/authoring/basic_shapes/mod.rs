//! 统一网格创建面板的基础形状请求、半透明点放预览与空场景显式工作平面。
//!
//! 【需求设计】基础形状复用普通摆放的唯一真实地面与吸附协议；编辑期token只表达
//! 当前添加意图，持久记录使用Cuboid或开口EditableMesh和空asset_path，不能把token交给AssetServer。

mod components;
mod placement;
mod resources;
mod systems;

pub(crate) use placement::{is_cuboid_selection, plane_hit, work_plane_is_available};
pub(crate) use resources::{BasicShapeState, CuboidCreationRequest};

pub(crate) use feathrise_editor_animation_model::media::CUBOID_SELECTION;

pub(crate) fn creation_label(target: &feathrise_scene_content::ScenePlacementAssetTarget) -> &'static str {
    if let Some(label) = super::local_lights::label(target) { return label; }
    if matches!(target, feathrise_scene_content::ScenePlacementAssetTarget::EditableMesh { geometry, .. } if geometry.is_open()) {
        "平面"
    } else { "立方体" }
}

/// 新对象还未进入后台生成的树索引。直接把刚追加的作者记录发布给树和视口，避免旧选集复核时抢回属性栏。
pub(super) fn select_created(
    id: String, session: &super::EditorAuthoringSession, browser: &mut crate::ui::gltf_browser::EditorGltfBrowserState,
    selection: Option<&mut crate::viewport::multi_select::PlacementSelectionSet>, mut candidates: Option<&mut crate::viewport::LayerCandidateState>,
) {
    if let Some(candidates) = candidates.as_deref_mut() { candidates.clear_for_scene_tree(); }
    if let Some(selection) = selection && let Some(draft) = session.draft.as_ref()
        && let Some((index, record)) = draft.placements.placements.iter().enumerate().next_back()
        && record.id == id && let Ok(position) = super::placement_pose::placement_record_world_position(draft, record) {
        selection.publish(vec![crate::viewport::multi_select::SelectedMember { id: id.clone(), index, position }], session);
        if let Some(candidates) = candidates { crate::viewport::multi_select::publish_primary(selection, browser, candidates); }
    }
    browser.select_primitive_placement_from_viewport(id);
}

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
