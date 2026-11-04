//! 多网格河流新增点只消费按下时冻结的原Collider点击；不把上一帧悬停或另一水体结果补作新点。

use super::{helpers::active_feature_identity, types::ObjectWaterPointClick};
use crate::{
    authoring::{EditorAuthoringSession, terrain_ground::TerrainGroundCursor},
    resources::MapEditorState,
};
use bevy::prelude::*;

pub(super) fn capture(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    ground: &TerrainGroundCursor,
) -> Option<ObjectWaterPointClick> {
    let draft = session.draft.as_ref()?;
    let active = draft.terrain_documents.active.as_ref()?;
    if draft.manifest.terrains.is_none()
        || draft.terrain_documents.loading
        || editor.terrain_canvas_target.as_ref() != Some(&active.definition.id)
    {
        return None;
    }
    let (feature, waterfall) = active_feature_identity(draft.water.as_ref()?, editor)?;
    Some(ObjectWaterPointClick {
        identity: ground.identity.clone()?,
        authority: ground.input_authority()?,
        map: draft.map_id.clone(),
        terrain: active.definition.id.clone(),
        feature,
        waterfall,
    })
}

pub(super) fn take(
    pending: &mut Option<ObjectWaterPointClick>,
    editor: &mut MapEditorState,
    session: &EditorAuthoringSession,
    ground: Option<&TerrainGroundCursor>,
) -> Option<Vec3> {
    let frozen = pending.as_ref()?;
    let Some(draft) = session.draft.as_ref() else {
        *pending = None;
        return None;
    };
    let current = editor.open
        && !editor.close_requested
        && !session.saving
        && editor.selected_map_id.as_ref() == Some(&frozen.map)
        && editor.terrain_canvas_target.as_ref() == Some(&frozen.terrain)
        && draft.map_id == frozen.map
        && !draft.terrain_documents.loading
        && draft
            .terrain_documents
            .active
            .as_ref()
            .is_some_and(|active| active.definition.id == frozen.terrain)
        && session.revision == frozen.identity.revision
        && session.terrain_revision == frozen.identity.terrain_revision
        && draft
            .water
            .as_ref()
            .and_then(|water| active_feature_identity(water, editor))
            == Some((frozen.feature.clone(), frozen.waterfall))
        && ground.is_some_and(|ground| {
            ground.identity.as_ref() == Some(&frozen.identity)
                && ground.input_authority() == Some(frozen.authority)
        });
    if !current {
        *pending = None;
        return None;
    }
    let point = editor.ground_click_world.take()?;
    // 其它重叠网格不能成为当前作者画布的水网来源；通过后才转换到该画布的源坐标。
    let same_terrain =
        ground.is_some_and(|ground| ground.click_terrain.as_ref() == Some(&frozen.terrain));
    *pending = None;
    same_terrain.then(|| draft.terrain_point(point))
}
