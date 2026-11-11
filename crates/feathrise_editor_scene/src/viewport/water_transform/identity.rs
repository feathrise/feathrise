//! 静水手柄冻结作者画布、R8源和完整TRS；同名兄弟画布不能继承旧手势。

use super::*;
use crate::authoring::EditorSceneDraft;
use feathrise_scene_content::{SceneTerrainObject, SceneWaterLayer};
use std::sync::Arc;

pub(super) struct Source {
    scene_id: feathrise_scene_content::SceneId,
    terrain: Option<Arc<SceneTerrainObject>>,
    water_path: String,
    terrain_hash: Option<[u8; 32]>,
    mask: Arc<Vec<u8>>,
    pub frame: Mat4,
}

impl Source {
    pub fn capture(draft: &EditorSceneDraft, id: &str) -> Option<Self> {
        let terrain = draft.terrain_documents.active.as_ref().map(|active| active.definition.clone());
        let frame = terrain.as_ref().map_or(Mat4::IDENTITY, |object| object.transform.as_transform().to_matrix());
        let up = frame.transform_vector3(Vec3::Y);
        // 世界Y手柄不能在源Y几乎水平时放大为无限水位，完整源姿态始终保留。
        if !frame.is_finite() || !frame.inverse().is_finite() || up.y.abs() <= up.length() * 0.0001 {
            return None;
        }
        Some(Self {
            scene_id: draft.scene_id.clone(), terrain, water_path: draft.water_asset_path.clone(),
            terrain_hash: draft.terrain_source_sha256,
            mask: draft.water_masks.get(id)?.clone(), frame,
        })
    }

    pub fn matches(&self, session: &EditorAuthoringSession, id: &str) -> bool {
        let Some(draft) = session.draft.as_ref() else { return false; };
        let same_terrain = match (&self.terrain, &draft.terrain_documents.active) {
            (None, None) => true,
            (Some(before), Some(current)) => Arc::ptr_eq(before, &current.definition),
            _ => false,
        };
        same_terrain && draft.scene_id == self.scene_id && draft.water_asset_path == self.water_path
            && draft.terrain_source_sha256 == self.terrain_hash
            && draft.water_masks.get(id).is_some_and(|mask| Arc::ptr_eq(mask, &self.mask))
    }
}

pub(super) fn selected_layer<'a>(
    editor: &SceneEditorState, session: &'a EditorAuthoringSession,
    candidates: &LayerCandidateState, terrains: &TerrainObjectsEditorState,
) -> Option<&'a SceneWaterLayer> {
    let draft = session.draft.as_ref()?;
    if editor.selected_scene_id.as_ref() != Some(&draft.scene_id)
        || !candidates.author_canvas_matches(draft) || !candidates.selected_matches_session(session)
        || terrains.busy() || draft.terrain_documents.loading {
        return None;
    }
    if let Some(active) = &draft.terrain_documents.active {
        let object = &active.definition;
        if !object.enabled || terrains.locked.contains(&object.id) || terrains.hidden.contains(&object.id) {
            return None;
        }
    }
    let water = draft.water.as_ref().filter(|water| water.scene_id == draft.scene_id)?;
    let layer = water.layers.get(editor.selected_water_layer)?;
    matches!(candidates.selected.as_ref(), Some(LayerTarget::Water(WaterAuthoringFeatureKind::StillWater, id)) if id == &layer.id)
        .then_some(layer)
}

pub(super) fn origin(
    session: &EditorAuthoringSession, selected: &SelectedWaterViewportState,
    id: &str, level: f32, frame: Mat4,
) -> Option<Vec3> {
    let center = selected.still_water_center(session, id)?;
    let local = frame.inverse().transform_point3(center).with_y(level);
    let point = frame.transform_point3(local);
    point.is_finite().then_some(point) // 仅校正水位；中心XZ来自后台完成范围，绝不扫描R8。
}
