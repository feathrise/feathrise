//! 独立地形范围高亮复用统一跨类别点选；不同对象共用同一个候选排序和遮挡规则。
use super::tool_preview::AuthorToolFrame;
use feathrise_author_tools::{AuthorTool, SelectionPreview};
use crate::{
    authoring::EditorAuthoringSession, resources::MapEditorState,
    ui::terrain_objects::TerrainObjectsEditorState,
};
use bevy::prelude::*;

pub(crate) fn install(app: &mut App) {
    super::terrain_transform::install(app);
    super::water_transform::install(app);
    app.add_systems(Update, draw.in_set(crate::MapEditorSystemSet::Presentation));
}

fn draw(
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    selection: Res<TerrainObjectsEditorState>,
    preview: Res<feathrise_scene::TerrainObjectTransformPreview>,
    candidates: Res<super::LayerCandidateState>,
    mut tools: ResMut<AuthorToolFrame>,
) {
    if !editor.open {
        return;
    }
    let Some(object) = crate::ui::terrain_objects::selected(&session, &selection) else {
        return;
    };
    if !object.enabled || selection.hidden.contains(&object.id) {
        return;
    }
    if !matches!(&candidates.selected, Some(super::LayerTarget::TerrainObject(id)) if id == object.id.as_str())
    {
        return;
    }
    let fallback_min = object.voxel.as_ref().map_or(
        Vec3::new(
            object.world_min[0],
            object.terrain.base_height,
            object.world_min[1],
        ),
        |v| Vec3::from_array(v.min),
    );
    let fallback_max = object.voxel.as_ref().map_or(
        Vec3::new(
            object.world_max[0],
            object.terrain.base_height + object.terrain.height_scale,
            object.world_max[1],
        ),
        |v| Vec3::from_array(v.max),
    );
    // Clipmap的旧量程只用于旧页解码；高亮复用同代真实地面包围，不能把高山截在创建高度内。
    let (min, max) = session.draft.as_ref().and_then(|draft| draft.terrain_ground.as_ref())
        .filter(|ground| ground.revision == session.terrain_revision)
        .and_then(|ground| ground.local_bounds(&object.id))
        .unwrap_or((fallback_min, fallback_max));
    let matrix = preview
        .for_object(&session.draft.as_ref().unwrap().map_id, &object.id)
        .unwrap_or(object.transform)
        .as_transform()
        .to_matrix();
    let half = (max - min) * 0.5;
    tools.push(AuthorTool::Selection(SelectionPreview {
        target: format!("terrain:{}/{}", session.draft.as_ref().unwrap().map_id.as_str(), object.id.as_str()),
        center: matrix.transform_point3((min + max) * 0.5).to_array(),
        half_axes: [Vec3::X * half.x, Vec3::Y * half.y, Vec3::Z * half.z]
            .map(|axis| matrix.transform_vector3(axis).to_array()),
        color: Color::srgb(0.745, 0.529, 0.243).to_linear().to_f32_array(),
    }));
}
