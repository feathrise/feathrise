use feathrise_scene_content::{
    ScenePlacementAssetTarget, ScenePlacementRecord, ScenePlacementWaterKind, ScenePlacementWaterSurface,
};

use super::*;
use crate::authoring::placement_structure_differs;

fn water_record(id: &str, asset_path: &str) -> ScenePlacementRecord {
    ScenePlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: id.to_owned(),
        asset_path: asset_path.to_owned(),
        asset_target: Default::default(), // Scene0需求测试基线保持可展开目标。
        building_preset_id: None,         // 浏览需求夹具不引用建筑目录。
        courtyard: None,                  // 独立摆放记录不属于任何院落副本。
        translation: [1.0, 2.0, 3.0],
        height_mode: feathrise_scene_content::ScenePlacementHeightMode::FixedHeight, // Scene0需求测试保持确定世界Y。
        terrain_offset: 0.0, // 固定高度不携带离地偏移。
        yaw_degrees: 20.0,
        scale: [1.0, 1.0, 1.0],
        water_surfaces: vec![ScenePlacementWaterSurface {
            mesh_index: 2,
            node_path: vec!["河道".to_owned(), "水面".to_owned()],
            mesh_name: "WaterSurface".to_owned(),
            kind: ScenePlacementWaterKind::Sloped,
            flow_speed: 2.0,
            height_offset: 0.0,
        }],
    }
}

#[test]
fn numeric_and_transform_changes_do_not_change_browser_structure() {
    let before = water_record("river-1", "gltf/river.glb");
    let mut after = before.clone();
    after.translation = [10.0, 20.0, 30.0];
    after.height_mode = feathrise_scene_content::ScenePlacementHeightMode::FollowTerrain; // 高度语义只影响完整实例Transform。
    after.terrain_offset = 1.5; // 离地偏移同样不增删Scene0树行。
    after.yaw_degrees = 90.0;
    after.scale = [2.0, 2.0, 2.0];
    after.water_surfaces[0].flow_speed = 17.0;
    after.water_surfaces[0].height_offset = -30.0;

    assert_eq!(
        placement_structure_signature(&before),
        placement_structure_signature(&after),
    ); // 浏览器签名只保留根、来源和绑定诊断身份。
    assert!(!placement_structure_differs(&before, &after)); // 作者独立结构修订也必须遵守同一边界。
}

#[test]
fn binding_kind_or_asset_path_changes_browser_structure() {
    let before = water_record("river-1", "gltf/river.glb");
    let mut kind_changed = before.clone();
    kind_changed.water_surfaces[0].kind = ScenePlacementWaterKind::Waterfall;
    assert!(placement_structure_differs(&before, &kind_changed));
    assert_ne!(
        placement_structure_signature(&before),
        placement_structure_signature(&kind_changed),
    );

    let mut asset_changed = before.clone();
    asset_changed.asset_path = "gltf/river-v2.glb".to_owned();
    assert!(placement_structure_differs(&before, &asset_changed));

    let mut target_changed = before.clone();
    target_changed.water_surfaces.clear(); // primitive格式本身不允许Scene0层级水绑定。
    target_changed.asset_target = ScenePlacementAssetTarget::Primitive {
        mesh: 6,
        primitive: 0,
        material: Some(2),
        collision: false,
    };
    let mut scene_without_water = before.clone();
    scene_without_water.water_surfaces.clear();
    assert!(placement_structure_differs(
        &scene_without_water,
        &target_changed,
    ));
    assert_ne!(
        placement_structure_signature(&scene_without_water),
        placement_structure_signature(&target_changed),
    ); // 文件内目标变化必须重建根行，并撤销可能存在的Scene0展开需求。
}

#[test]
fn explicit_structure_revision_detects_same_scene_same_length_reload() {
    let previous = BrowserSessionObservation {
        scene_objects_revision: Default::default(),
        draft_available: true,
        draft_scene_id: Some("MAP-river".to_owned()),
        source_placement_count: 16,
        placement_structure_revision: 7,
    };
    let numeric_only = previous.clone(); // 全局作者revision不属于该轻量观察，流速/高度Changed完整短路。
    assert!(!session_change_requires_signature_refresh(
        &previous,
        &numeric_only,
    ));

    let reloaded = BrowserSessionObservation {
        placement_structure_revision: 8,
        ..previous.clone()
    };
    assert!(session_change_requires_signature_refresh(
        &previous, &reloaded,
    )); // 即使scene_id和长度完全相同，显式载入代次也确定触发重扫。
}
