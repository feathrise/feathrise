//! 普通编辑器的外部域夹具，只测实际端口与身份；不导入角色或游戏文档格式。

use super::*;
use crate::authoring::domain_documents::{DomainSaveGuard, MapDomainDocument, MapDomainDocumentOps};

#[derive(Clone)]
struct FixtureDocument { hash: String }
static OPS: MapDomainDocumentOps = MapDomainDocumentOps {
    dirty: |_| false,
    hash: |value| &value.downcast_ref::<FixtureDocument>().unwrap().hash,
    asset_path: |_| "",
    registration_required: |_| false,
    guard: |_, _| Ok(DomainSaveGuard::default()),
    prepare: |_, _| Ok(None),
    register: |_, _| Ok(()),
    saved: |value, _| Arc::new(value.downcast_ref::<FixtureDocument>().unwrap().clone()),
    follow_terrain: |value, _| Ok(Arc::new(value.downcast_ref::<FixtureDocument>().unwrap().clone())),
};

pub(crate) fn document(hash: &str) -> MapDomainDocument {
    MapDomainDocument { domain: "test.domain", value: Arc::new(FixtureDocument { hash: hash.into() }), ops: &OPS }
}

pub(crate) fn snapshot(hash: &str, count: usize) -> Arc<MapInteractionSnapshot> {
    Arc::new(MapInteractionSnapshot {
        map: feathrise_map_content::MapId("EDGE-TEST".into()), document_domain: "test.domain",
        domain: "test.domain", label: "测试扩展", hash: hash.into(),
        layer_label: "测试扩展层", object_label: "测试扩展对象", layer_count: 1, object_count: count,
        pick: Arc::new(|_, _, _| vec![("fixture-domain-layer".into(), "测试扩展层".into(), 1.0)]),
        row: Arc::new(move |kind, index| match kind {
            DomainItemKind::Layer if index == 0 => Some(DomainTreeRow {
                id: "fixture-domain-layer".into(), name: "测试扩展层".into(), label: format!("测试扩展层 · {count}个来源"),
                hidden: false, locked: false, actions: 0b11_1111, copy_locked: true,
            }),
            DomainItemKind::Object if index < count => Some(DomainTreeRow {
                id: format!("fixture-source-{index}"), name: format!("测试对象{index}"), label: format!("测试对象{index}"),
                hidden: false, locked: false, actions: 0b00_1111, copy_locked: false,
            }),
            _ => None,
        }),
        selection: Arc::new(move |kind, id| {
            let valid = match kind {
                DomainItemKind::Layer => id == "fixture-domain-layer",
                DomainItemKind::Object => id.strip_prefix("fixture-source-").and_then(|id| id.parse::<usize>().ok()).is_some_and(|index| index < count),
            };
            valid.then(|| DomainSelectionInfo { layer_id: Some("fixture-domain-layer".into()), layer_index: Some(0), visible: true, status: "已选择测试扩展".into() })
        }),
        bounds: Arc::new(|_, _| Some((Vec3::new(-2., 0., -2.), Vec3::new(11., 8., 7.)))),
    })
}

pub(crate) fn state(hash: &str, count: usize) -> MapInteractionState {
    MapInteractionState { enabled: true, snapshot: Some(snapshot(hash, count)), ..default() }
}

pub(crate) fn ground_target(id: &str, continuous: bool, owner: &'static str) -> crate::viewport::vegetation_ground::GroundCursorTarget {
    crate::viewport::vegetation_ground::GroundCursorTarget::Domain(DomainGroundTarget {
        domain: owner, identity: id.into(), continuous, click_qualified: !continuous, payload: Arc::new(()),
    })
}
