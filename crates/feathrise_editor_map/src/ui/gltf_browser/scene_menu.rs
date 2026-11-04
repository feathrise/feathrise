//! 固定八行在正式呈现后冻结完整实例或精确网格，分组及失效诊断行不冒充对象。

use super::*;
use crate::{
    authoring::EditorAuthoringSession,
    ui::scene_context_menu::{SceneObjectKey, SceneObjectRow, SceneObjectTarget},
};

pub(crate) fn bind_scene_menu_rows(
    mut commands: Commands,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    rows: Query<(Entity, &MapEditorGltfBrowserRow, Option<&SceneObjectRow>)>,
) {
    let draft = session.draft.as_ref();
    for (entity, row, previous) in &rows {
        let target = (|| {
            if browser.signature_build.is_some() || browser.entries_build.is_some() {
                return None;
            }
            let index = row
                .entry_index
                .filter(|index| *index == browser.page * GLTF_BROWSER_ROW_COUNT + row.slot)?;
            let entry = browser.entries.get(index)?;
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
            let draft = draft?;
            let record = draft
                .placements
                .placements
                .get(index)
                .filter(|record| record.id == *id)?;
            Some(SceneObjectTarget {
                map: draft.map_id.clone(),
                revision: session.revision,
                key: SceneObjectKey::Placement {
                    index,
                    id: id.clone(),
                    mesh,
                },
                name: record.editor.name.clone(),
                hidden: record.editor.hidden,
                locked: record.editor.locked,
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
