//! 已呈现树行冻结完整实例、精确网格或组，失效诊断行不冒充对象。

use super::*;
use crate::{
    authoring::EditorAuthoringSession,
    ui::scene_context_menu::{SceneObjectKey, SceneObjectRow, SceneObjectTarget},
};

pub(crate) fn bind_scene_menu_rows(
    mut commands: Commands,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    objects: Option<Res<crate::ui::scene_objects::SceneObjectState>>,
    rows: Query<(Entity, &SceneEditorGltfBrowserRow, Option<&SceneObjectRow>)>,
) {
    let draft = session.draft.as_ref();
    for (entity, row, previous) in &rows {
        let target = (|| {
            if browser.signature_build.is_some() || browser.entries_build.is_some() {
                return None;
            }
            let index = row.entry_index.filter(|_| row.entries_revision == browser.published_entries_revision)?;
            let entry = browser.entries.get(index)?;
            let draft = draft?;
            if let EditorGltfBrowserEntryKind::Group { name, first_member, .. } = &entry.kind {
                // 整组只提供读操作。首成员加组名校验当前目录身份，展开与收起均使用同一目标。
                let record = draft.placements.placements.get(*first_member)?;
                let signature = browser.signature.get(*first_member)?;
                if !browser.placement_index_ready(session.placement_structure_revision)
                    || record.id != signature.placement_id || record.editor.group != *name
                {
                    return None;
                }
                return Some(SceneObjectTarget {
                    scene_id: draft.scene_id.clone(),
                    revision: session.revision,
                    key: SceneObjectKey::Group { first_member: *first_member, id: record.id.clone() },
                    name: name.clone(),
                    hidden: false,
                    locked: false,
                    copyable: false,
                });
            }
            let (id, mesh) = match &entry.kind {
                EditorGltfBrowserEntryKind::Placement { placement_id, .. } => (placement_id, None),
                EditorGltfBrowserEntryKind::Mesh => {
                    let mesh = entry
                        .selection
                        .as_ref()
                        .filter(|mesh| mesh.valid && !mesh.orphaned_water_binding)?;
                    (&mesh.placement_id, Some(mesh.clone()))
                }
                _ => return None,
            };
            let index = browser.placement_record_index(id, session.placement_structure_revision)?;
            let record = draft
                .placements
                .placements
                .get(index)
                .filter(|record| record.id == *id)?;
            let metadata = crate::ui::scene_objects::effective_metadata(objects.as_deref(), &session, record);
            Some(SceneObjectTarget {
                scene_id: draft.scene_id.clone(),
                revision: session.revision,
                key: SceneObjectKey::Placement {
                    index,
                    id: id.clone(),
                    mesh,
                },
                name: metadata.name.clone(),
                hidden: metadata.hidden,
                locked: metadata.locked,
                copyable: !record
                    .terrain_patch
                    .as_ref()
                    .is_some_and(|patch| patch.sculpted_volume),
            })
        })();
        if previous.is_none_or(|previous| previous.0 != target) {
            commands.entity(entity).insert(SceneObjectRow(target));
        }
    }
}
