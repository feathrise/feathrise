//! GLB作者水面只替代自己来源的正式水；独立根水、Clipmap和兄弟实例不借场景ID共享隐藏权限。

use bevy::prelude::*;
use feathrise_scene_content::SceneTerrainKind;
use feathrise_scene::{DynamicWaterStatus, PaintedWaterPurpose, PaintedWaterSurface, TerrainObjectResource};
use feathrise_scene_core::residency::SceneInstanceOwner;

use super::super::resources::{
    EditorAuthoringSession, WaterPreviewRuntimeTarget, WaterPreviewSource, WaterPreviewState,
};

/// 属性焦点只用于判断已采用来源是否仍为当前画布；真正的后台任务另行冻结此身份。
pub(super) fn current_source(session: &EditorAuthoringSession) -> Option<WaterPreviewSource> {
    let draft = session.draft.as_ref()?;
    if draft.terrain_documents.loading
        || !matches!(draft.terrain_manifest().terrain.kind, SceneTerrainKind::Gltf | SceneTerrainKind::Heightfield)
    {
        return None;
    }
    let terrain_id = match draft.terrain_documents.active.as_ref() {
        Some(canvas) => Some(canvas.definition.id.clone()),
        None if draft.manifest.terrains.is_none() => None,
        None => return None, // 容器尚未交接到具体GLB，不能把它当成旧版无owner单图。
    };
    Some(WaterPreviewSource { scene_id: draft.scene_id.clone(), terrain_id })
}

/// 定点查完整活动路径；同图同地形源的子场景和未采用候选均不能成为替代对象。
pub(super) fn runtime_target(
    source: &WaterPreviewSource,
    status: &TerrainObjectResource<DynamicWaterStatus>,
) -> Option<WaterPreviewRuntimeTarget> {
    match source.terrain_id.as_ref() {
        Some(id) => status.for_saved_object(&source.scene_id, id)
            .map(|(entity, _)| WaterPreviewRuntimeTarget::Instance(entity)),
        None => status.for_root(&source.scene_id).map(|_| WaterPreviewRuntimeTarget::Root),
    }
}

/// 首次采用与后续Added热更共用同一个owner门，避免只有初次预览正确、保存后又误隐藏。
pub(super) fn hide_replaced_surface(
    commands: &mut Commands,
    preview: &mut WaterPreviewState,
    source: &WaterPreviewSource,
    target: WaterPreviewRuntimeTarget,
    (entity, surface, visibility, owner): (Entity, &PaintedWaterSurface, &Visibility, Option<&SceneInstanceOwner>),
) {
    let same_owner = match target {
        WaterPreviewRuntimeTarget::Root => owner.is_none(),
        WaterPreviewRuntimeTarget::Instance(instance) => owner.is_some_and(|owner| owner.0 == instance),
    };
    if !same_owner || surface.purpose != PaintedWaterPurpose::Runtime || surface.scene_id != source.scene_id
        || preview.hidden_runtime_entities.iter().any(|(hidden, _)| *hidden == entity)
    {
        return;
    }
    preview.hidden_runtime_entities.push((entity, *visibility));
    commands.entity(entity).insert(Visibility::Hidden);
}
