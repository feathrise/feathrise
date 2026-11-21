//! 可见行直接按数组下标或已有角色归属索引取一项；旧图正式植被输出保留只读入口。

use super::{inventory::{LayerInventory, vegetation_source}, types::LayerList};
use crate::{
    authoring::EditorAuthoringSession,
    resources::VegetationEditorVisibility,
    ui::scene_context_menu::{SceneObjectKey, SceneObjectTarget},
};
use crate::game_authoring::scene_interaction::{SceneInteractionState, DomainItemKind};

pub(super) fn object(
    session: &EditorAuthoringSession,
    inventory: &LayerInventory,
    list: LayerList,
    index: usize,
    domain: Option<&SceneInteractionState>,
    visibility: &VegetationEditorVisibility,
) -> Option<(SceneObjectTarget, String)> {
    let draft = session.draft.as_ref()?;
    let source = |water| {
        let (terrain_index, index) = inventory.source(water, index)?;
        let terrain = match terrain_index {
            Some(index) => Some(draft.manifest.terrains.as_ref()?.get(index)?),
            None => None,
        };
        Some((terrain, index))
    };
    let (key, name, label, hidden, locked) = match list {
        LayerList::Water => {
            use crate::types::WaterAuthoringFeatureKind::*;
            let (terrain, ordinal) = source(true)?;
            let water = crate::authoring::terrain_documents::root_water::listed_source(draft, terrain.map(|object| &object.id))?;
            let (kind, index, id) = crate::ui::water_feature_list::water_feature_at(water, ordinal)?;
            let (name, kind_label) = match kind {
                StillWater => (&water.layers[index].label, "静水"),
                River => (&water.rivers[index].label, "河流"),
                Waterfall => (&water.waterfalls[index].label, "瀑布"),
            };
            (
                SceneObjectKey::Water {
                    kind,
                    index,
                    id: id.to_owned(),
                    terrain: terrain.map(|object| object.id.clone()),
                },
                name.clone(),
                format!("{} · {kind_label} {} · {name}", terrain.map_or("场景", |object| object.display_name.as_str()), index + 1),
                false,
                false,
            )
        }
        LayerList::Vegetation => {
            let (terrain, index) = source(false)?;
            let terrain_id = terrain.map(|object| &object.id);
            let vegetation = vegetation_source(draft, terrain_id)?;
            let label = |name: &str| match terrain {
                Some(object) => format!("{name} · {}", object.display_name),
                None => format!("{name} · 场景"),
            };
            if let Some(authoring) = &vegetation.authoring {
                let item = authoring.layers.get(index)?;
                (
                    SceneObjectKey::Vegetation {
                        index,
                        id: item.id.clone(),
                        output: false,
                        terrain: terrain_id.cloned(),
                    },
                    item.label.clone(),
                    label(&item.label),
                    visibility.hidden_in(&draft.scene_id, terrain_id)
                        .is_some_and(|hidden| hidden.contains(&item.id)),
                    item.locked,
                )
            } else {
                let item = vegetation.layers.get(index)?;
                (
                    SceneObjectKey::Vegetation {
                        index,
                        id: item.id.clone(),
                        output: true,
                        terrain: terrain_id.cloned(),
                    },
                    item.label.clone(),
                    format!("{} · 正式输出", label(&item.label)),
                    false,
                    false,
                )
            }
        }
        LayerList::DomainLayers | LayerList::DomainObjects => {
            let snapshot = domain?.snapshot_for(draft)?;
            let kind = if list == LayerList::DomainLayers { DomainItemKind::Layer } else { DomainItemKind::Object };
            let row = (snapshot.row)(kind, index)?;
            (SceneObjectKey::Domain { kind, index, id: row.id, hash: snapshot.hash.clone(), actions: row.actions, copy_locked: row.copy_locked },
                row.name, row.label, row.hidden, row.locked)
        }
    };
    let suffix = match (hidden, locked) {
        (true, true) => " · 隐藏 · 锁定",
        (true, false) => " · 隐藏",
        (false, true) => " · 锁定",
        _ => "",
    };
    Some((
        SceneObjectTarget {
            scene_id: draft.scene_id.clone(),
            revision: session.revision,
            key,
            name,
            hidden,
            locked,
            copyable: true,
        },
        format!("{label}{suffix}"),
    ))
}
