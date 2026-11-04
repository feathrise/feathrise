//! 新旧清单转换必须连同完整作者槽一起恢复；删除引用仍保留可撤销的私有画布。
use super::super::EditorMapDraft;
use super::{TerrainCanvas, TerrainDocumentIdentity};
use feathrise_map_content::TerrainId;
use std::sync::Arc;

/// 域控件只能冻结当前同对象、同几何与同量程的作者网格；停放或载入中的旧槽不获得写权限。
pub(crate) fn height_domain_mesh<'a>(
    draft: &'a EditorMapDraft,
    object: &feathrise_map_content::MapTerrainObject,
) -> Option<&'a Arc<feathrise_map_terrain::TerrainMeshInput>> {
    if object.terrain.kind != feathrise_map_content::MapTerrainKind::Heightfield || draft.terrain_documents.loading {
        return None;
    }
    let current = if let Some(active) = &draft.terrain_documents.active {
        active.manifest.id == draft.map_id
            && super::load::same_heightfield_source(&active.definition, object)
            && active.definition.terrain.base_height == object.terrain.base_height
            && active.definition.terrain.height_scale == object.terrain.height_scale
    } else {
        draft.manifest.terrains.is_none() && object.id == TerrainId::legacy()
            && draft.manifest.terrain == object.terrain
    };
    current.then_some(draft.terrain_mesh.as_ref()).flatten()
}

/// 首次迁移先认领原作者槽，再把容器改为空权威；旧未保存数据不能先失去类型与路径。
pub(crate) fn ensure_primary_canvas(draft: &mut EditorMapDraft) {
    if draft.manifest.terrains.is_none() && draft.terrain_documents.active.is_none() {
        if let Some(object) = draft.manifest.terrain_objects_for_edit().first() {
            draft.terrain_documents.active = Some(TerrainDocumentIdentity {
                definition: Arc::new(object.clone()),
                manifest: object.source_manifest(&draft.manifest),
                folder: draft.folder.clone(),
            });
            draft.terrain_documents.scoped_through = 0;
        }
    }
}

pub(super) fn park(draft: &mut EditorMapDraft) {
    let mut previous = TerrainCanvas::default();
    previous.swap(draft);
    if let Some(id) = previous
        .identity
        .as_ref()
        .map(|identity| identity.definition.id.clone())
    {
        if previous.dirty() {
            // 保存后的活动槽可能继续绘制；saved只覆盖旧停放快照，不能清除本次新脏责任。
            draft.terrain_documents.saved.remove(&id);
            draft.terrain_documents.dirty_stored.insert(id.clone());
        }
        draft
            .terrain_documents
            .values
            .insert(id, Arc::new(previous));
    } else if draft.manifest.terrains.is_some() {
        super::root_water::park(draft, &mut previous);
    }
}

pub(crate) fn restore_legacy(draft: &mut EditorMapDraft) -> Result<(), String> {
    if draft.manifest.terrains.is_some() {
        return Ok(());
    }
    let Some(active) = &draft.terrain_documents.active else {
        return Ok(());
    };
    if active.definition.id != TerrainId::legacy() {
        let stored = draft
            .terrain_documents
            .values
            .remove(&TerrainId::legacy())
            .ok_or("旧主地形作者槽不存在，不能恢复旧清单")?;
        park(draft);
        let mut primary = Arc::try_unwrap(stored).unwrap_or_else(|shared| (*shared).clone());
        if draft.terrain_documents.saved.remove(&TerrainId::legacy()) {
            primary.clear_dirty();
        }
        draft
            .terrain_documents
            .dirty_stored
            .remove(&TerrainId::legacy());
        primary.swap(draft);
    }
    if let Some(digest) = draft
        .terrain_documents
        .saved_digests
        .get(&TerrainId::legacy())
    {
        draft.terrain_source_sha256 = Some(*digest);
        draft.terrain_overwrite_authorized_sha256 = None;
    }
    if let Some(TerrainDocumentIdentity { manifest, .. }) = draft.terrain_documents.active.take() {
        draft.manifest.puddles = manifest.puddles; // 原主画布新建/删除积水描述也属于恢复内容。
    }
    Ok(())
}
