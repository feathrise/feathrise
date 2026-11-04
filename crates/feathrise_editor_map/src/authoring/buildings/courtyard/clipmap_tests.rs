//! 院落建筑五点、普通装饰偏移与一栋失败整组拒绝回归。
use super::*;
use std::collections::BTreeMap;

fn record(id: &str, xz: f32, preset: Option<&BuildingPresetDescriptor>) -> MapPlacementRecord {
    MapPlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: id.into(),
        asset_path: "gltf/house.glb".into(),
        asset_target: Default::default(),
        building_preset_id: preset.map(|preset| preset.id.clone()),
        courtyard: None,
        translation: [xz, 900., xz],
        height_mode: if preset.is_some() {
            MapPlacementHeightMode::FixedHeight
        } else {
            MapPlacementHeightMode::FollowTerrain
        },
        terrain_offset: if preset.is_some() { 0. } else { 2.5 },
        yaw_degrees: 90.,
        scale: [1.; 3],
        water_surfaces: Vec::new(),
    }
}

fn fixture() -> (
    CourtyardBuildingValidator,
    Vec<(MapPlacementRecord, Option<BuildingPresetDescriptor>)>,
) {
    let mut preset = BuildingPresetDescriptor::from_asset(
        "house".into(),
        "院落民居".into(),
        "gltf/house.glb".into(),
        Default::default(),
    );
    preset.foundation.model_base_y = 2.;
    preset.foundation.max_ground_height_delta = 10.;
    let candidates = vec![
        (record("house-a", 20., Some(&preset)), Some(preset.clone())),
        (record("house-b", 50., Some(&preset)), Some(preset)),
        (record("lamp", 80., None), None),
    ];
    let mut heights = BTreeMap::new();
    for (ordinal, (record, preset)) in candidates.iter().enumerate() {
        if let Some(preset) = preset {
            for (index, point) in preset
                .world_footprint(record.translation, record.yaw_degrees, record.scale)
                .foundation_samples()
                .into_iter()
                .enumerate()
            {
                heights.insert(
                    (point.x.to_bits(), point.y.to_bits()),
                    10. * ordinal as f32 + index as f32,
                );
            }
        } else {
            heights.insert((80f32.to_bits(), 80f32.to_bits()), 30.);
        }
    }
    (
        CourtyardBuildingValidator {
            index: Arc::new(BuildingSpatialIndex::default()),
            sampler: Some(PlacementTerrainSampler::from_clipmap_samples(
                [7; 7], heights,
            )),
            map_id: MapId("EDGE-TEST".into()),
            bounds: Rect::from_corners(Vec2::ZERO, Vec2::splat(100.)),
        },
        candidates,
    )
}

#[test]
fn clipmap_courtyard_keeps_each_building_terrace_and_decoration_offset() {
    let (mut validator, candidates) = fixture();
    let mut query = None;
    assert!(validator.prepare_ground(&candidates, &mut query).unwrap());
    let result = validate_courtyard_batch(&validator, candidates, &BTreeSet::new()).unwrap();
    assert_eq!(result[0].translation[1], 2.);
    assert_eq!(result[1].translation[1], 12.); // 两栋分别按五点解析，院落没有联合整平。
    assert_eq!(result[2].translation[1], 32.5);
    assert_eq!(result[0].height_mode, MapPlacementHeightMode::FixedHeight);
    assert_eq!(result[2].terrain_offset, 2.5);
}

#[test]
fn clipmap_courtyard_rejects_the_whole_batch_with_the_failing_member_id() {
    let (validator, mut candidates) = fixture();
    candidates[1]
        .1
        .as_mut()
        .unwrap()
        .foundation
        .max_ground_height_delta = 0.5;
    let source = candidates
        .iter()
        .map(|(record, _)| record.clone())
        .collect::<Vec<_>>();
    let error =
        validate_courtyard_batch(&validator, candidates.clone(), &BTreeSet::new()).unwrap_err();
    assert!(error.contains("house-b"));
    assert_eq!(
        source,
        candidates
            .into_iter()
            .map(|(record, _)| record)
            .collect::<Vec<_>>()
    ); // 校验未写回任何成员。
}

#[test]
fn clipmap_courtyard_sampling_failure_identifies_the_member_before_adoption() {
    let (mut validator, mut candidates) = fixture();
    candidates[1].0.translation[0] += 1.; // 第二栋未在此冻结地形来源中取得完整五点。
    let before = candidates.clone();
    let error = validator
        .prepare_ground(&candidates, &mut None)
        .unwrap_err();
    assert!(error.contains("house-b"));
    assert!(error.contains("采样失败"));
    assert_eq!(candidates, before); // 失败阶段不能采用已经完成的第一栋。
}

#[test]
fn clipmap_courtyard_occupied_ground_still_requires_excluding_the_original_group() {
    let (mut validator, candidates) = fixture();
    let (record, preset) = &candidates[0];
    let preset = preset.as_ref().unwrap();
    let footprint = preset.world_footprint(record.translation, record.yaw_degrees, record.scale);
    let entrance =
        preset.world_entrance_clearance(record.translation, record.yaw_degrees, record.scale);
    validator.index = Arc::new(
        BuildingSpatialIndex::build(vec![super::super::types::IndexedBuilding {
            terrain_bound: false,
            id: record.id.clone(),
            preset_label: preset.label.clone(),
            footprint,
            entrance_clearance: entrance,
            vegetation_clearance: footprint,
            connections: Vec::new(),
            bounds: union_rect(footprint.aabb(), entrance.aabb()),
            translation: record.translation,
            yaw_degrees: record.yaw_degrees,
            scale: record.scale,
            height_mode: MapPlacementHeightMode::FixedHeight,
            foundation: preset.foundation,
            asset_matches_preset: true,
        }])
        .unwrap(),
    );
    assert!(
        validate_courtyard_batch(&validator, candidates.clone(), &BTreeSet::new())
            .unwrap_err()
            .contains("冲突")
    );
    assert!(
        validate_courtyard_batch(&validator, candidates, &BTreeSet::from(["house-a".into()]))
            .is_ok()
    );
}
