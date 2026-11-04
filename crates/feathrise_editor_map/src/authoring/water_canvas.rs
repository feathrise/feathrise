//! 静水画布独立于地形命中：根水使用地图坐标，旧网格水仍保留所属源的完整姿态与写保护。

use bevy::prelude::*;
use feathrise_map_content::{MapId, MapWaterLayer, TerrainId};
use super::{EditorAuthoringSession, EditorMapDraft};
use crate::resources::MapEditorState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WaterCanvasIdentity {
    map: MapId,
    terrain: Option<TerrainId>,
    path: String,
    frame: [u32; 16],
}

pub(crate) fn transform(draft: &EditorMapDraft) -> Transform {
    draft.terrain_documents.active.as_ref().map_or(Transform::IDENTITY,
        |active| active.definition.transform.as_transform())
}

pub(crate) fn source_frame(draft: &EditorMapDraft) -> Mat4 { transform(draft).to_matrix() }

pub(crate) fn identity(draft: &EditorMapDraft) -> WaterCanvasIdentity {
    WaterCanvasIdentity {
        map: draft.map_id.clone(),
        terrain: draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone()),
        path: draft.water_asset_path.clone(),
        frame: source_frame(draft).to_cols_array().map(f32::to_bits),
    }
}

/// 独立水面显式范围优先；旧层继续使用原网格源范围，不改变既有R8纹素地址。
pub(crate) fn layer_bounds(draft: &EditorMapDraft, layer: &MapWaterLayer) -> ([f32; 2], [f32; 2]) {
    let manifest = draft.terrain_manifest();
    layer.effective_canvas_bounds(manifest.world_min, manifest.world_max)
}

pub(crate) fn visible(session: &EditorAuthoringSession, editor: &MapEditorState) -> bool {
    let Some(draft) = session.draft.as_ref() else { return false; };
    if draft.terrain_documents.loading { return false; }
    let Some(active) = draft.terrain_documents.active.as_ref() else {
        return true; // 根水文档不以“存在地形网格”为前提。
    };
    active.definition.enabled && !editor.terrain_hidden.contains(&active.definition.id)
        && editor.terrain_canvas_target.as_ref() == Some(&active.definition.id)
}

pub(crate) fn writable(session: &EditorAuthoringSession, editor: &MapEditorState) -> bool {
    !session.water_preset_delete_pending && visible(session, editor) && session.draft.as_ref().is_some_and(|draft|
        draft.terrain_documents.active.as_ref().is_none_or(|active|
            !editor.terrain_locked.contains(&active.definition.id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::{EditorCommand, terrain_documents::{self, TerrainDocumentIdentity}};
    use std::sync::Arc;

    #[test]
    fn root_water_is_writable_without_a_mesh_and_history_does_not_follow_later_private_focus() {
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = TerrainId::parse("water-private").unwrap();
        let source = object.source_manifest(&draft.manifest);
        draft.manifest.terrain = feathrise_map_content::MapTerrainDescriptor::empty();
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.terrain_mesh = None; draft.terrain_ground = None;
        let mut session = EditorAuthoringSession { draft: Some(draft), ..default() };
        let editor = MapEditorState::default();
        assert!(writable(&session, &editor));
        assert_eq!(source_frame(session.draft.as_ref().unwrap()), Mat4::IDENTITY);
        session.undo.push(EditorCommand::WaterLevelChanged { layer_id: "lake".into(), before: 0.0, after: 3.0 });
        terrain_documents::scope_history(&mut session);
        assert!(matches!(session.undo[0], EditorCommand::RootWater(_)));
        session.draft.as_mut().unwrap().terrain_documents.active = Some(TerrainDocumentIdentity {
            definition: Arc::new(object.clone()), manifest: source, folder: "private".into(),
        });
        session.undo.push(EditorCommand::WaterLevelChanged { layer_id: "lake".into(), before: 2.0, after: 4.0 });
        terrain_documents::scope_history(&mut session);
        assert!(matches!(session.undo[0], EditorCommand::RootWater(_)), "相同层ID也不能把根水历史重新归入当前网格");
        assert!(matches!(&session.undo[1], EditorCommand::TerrainCanvas { id, .. } if id == &object.id));
    }
}
