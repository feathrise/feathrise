//! 可见行直接按数组下标或已有角色归属索引取一项；旧图正式植被输出保留只读入口。

use super::types::LayerList;
use crate::{
    authoring::EditorAuthoringSession,
    ui::scene_context_menu::{SceneObjectKey, SceneObjectTarget},
};
use crate::game_authoring::scene_interaction::{SceneInteractionState, DomainItemKind};

pub(super) fn object(
    session: &EditorAuthoringSession,
    list: LayerList,
    index: usize,
    domain: Option<&SceneInteractionState>,
) -> Option<(SceneObjectTarget, String)> {
    let draft = session.draft.as_ref()?;
    let (key, name, label, hidden, locked) = match list {
        LayerList::Water => {
            use crate::types::WaterAuthoringFeatureKind::*;
            let (terrain, water, kind, index, id) = crate::ui::water_feature_list::listed_water_feature_at(draft, index)?;
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
                    terrain: terrain.cloned(),
                },
                name.clone(),
                format!("{} · {kind_label} {} · {name}", if terrain.is_some() { "网格" } else { "场景" }, index + 1),
                false,
                false,
            )
        }
        LayerList::Vegetation => {
            let vegetation = draft.vegetation.as_ref()?;
            if let Some(authoring) = &vegetation.authoring {
                let item = authoring.layers.get(index)?;
                (
                    SceneObjectKey::Vegetation {
                        index,
                        id: item.id.clone(),
                        output: false,
                    },
                    item.label.clone(),
                    item.label.clone(),
                    !item.enabled,
                    item.locked,
                )
            } else {
                let item = vegetation.layers.get(index)?;
                (
                    SceneObjectKey::Vegetation {
                        index,
                        id: item.id.clone(),
                        output: true,
                    },
                    item.label.clone(),
                    format!("{} · 正式输出", item.label),
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
