//! 精确五点的根高度、坡差、完整样本和保存审计回归；不依赖渲染或磁盘页。
use super::*;
use feathrise_map_content::{BuildingPresetDescriptor, MapId};
use std::collections::BTreeMap;
use std::sync::Arc;

fn request(yaw: f32) -> BuildingPlacementRequest {
    let mut preset = BuildingPresetDescriptor::from_asset(
        "house".into(),
        "地基测试民居".into(),
        "gltf/house.glb".into(),
        Default::default(),
    );
    preset.foundation.model_base_y = 2.;
    preset.foundation.max_ground_height_delta = 10.;
    BuildingPlacementRequest {
        terrain_frame: None,
        identity: BuildingIndexIdentity {
            map_id: MapId("EDGE-TEST".into()),
            author_revision: 1,
            terrain_revision: 1,
            structure_revision: 1,
            catalog_revision: 1,
        },
        ticket: 1,
        preset,
        world_xz: Vec2::splat(20.),
        yaw_degrees: yaw,
        scale: [1.; 3],
        exclude_id: None,
        operation: BuildingPlacementOperation::Create,
        level_terrain: false,
        terrain_leveling: None,
        ground_content: Some([7; 7]),
        input_controls: None,
    }
}

fn samples(request: &BuildingPlacementRequest) -> BTreeMap<(u32, u32), f32> {
    request
        .preset
        .world_footprint(
            [request.world_xz.x, 0., request.world_xz.y],
            request.yaw_degrees,
            request.scale,
        )
        .foundation_samples()
        .into_iter()
        .enumerate()
        .map(|(index, point)| ((point.x.to_bits(), point.y.to_bits()), 10. + index as f32))
        .collect()
}

fn bounds() -> Rect {
    Rect::from_corners(Vec2::ZERO, Vec2::splat(100.))
}

fn indexed(request: &BuildingPlacementRequest, y: f32) -> BuildingSpatialIndex {
    let translation = [request.world_xz.x, y, request.world_xz.y];
    let footprint = request
        .preset
        .world_footprint(translation, request.yaw_degrees, request.scale);
    let entrance_clearance =
        request
            .preset
            .world_entrance_clearance(translation, request.yaw_degrees, request.scale);
    BuildingSpatialIndex::build(vec![IndexedBuilding {
        terrain_bound: false,
        id: "house-1".into(),
        preset_label: request.preset.label.clone(),
        footprint,
        entrance_clearance,
        vegetation_clearance: footprint,
        connections: Vec::new(),
        bounds: union_rect(footprint.aabb(), entrance_clearance.aabb()),
        translation,
        yaw_degrees: request.yaw_degrees,
        scale: request.scale,
        height_mode: MapPlacementHeightMode::FixedHeight,
        foundation: request.preset.foundation,
        asset_matches_preset: true,
    }])
    .unwrap()
}

#[test]
fn clipmap_foundation_uses_highest_of_five_points_and_model_base_for_both_rotations() {
    for yaw in [0., 90.] {
        let request = request(yaw);
        let sampler = PlacementTerrainSampler::from_clipmap_samples([7; 7], samples(&request));
        let result = validate_with_ground(
            request,
            &BuildingSpatialIndex::default(),
            &sampler,
            bounds(),
            None,
        );
        let ready = result.outcome.unwrap();
        assert_eq!(ready.minimum_ground, 10.);
        assert_eq!(ready.maximum_ground, 14.);
        assert_eq!(ready.translation, Vec3::new(20., 12., 20.));
        assert!(result.ground_query.is_none());
    }
}

#[test]
fn clipmap_foundation_rejects_excess_slope_and_incomplete_corner_samples() {
    let mut request = request(0.);
    let mut heights = samples(&request);
    request.preset.foundation.max_ground_height_delta = 0.5;
    let sampler = PlacementTerrainSampler::from_clipmap_samples([7; 7], heights.clone());
    let invalid = validate_with_ground(
        request.clone(),
        &BuildingSpatialIndex::default(),
        &sampler,
        bounds(),
        None,
    );
    assert!(invalid.outcome.unwrap_err().contains("五点高差"));
    heights.pop_last();
    let incomplete = PlacementTerrainSampler::from_clipmap_samples([7; 7], heights);
    assert!(
        validate_with_ground(
            request,
            &BuildingSpatialIndex::default(),
            &incomplete,
            bounds(),
            None
        )
        .outcome
        .is_err()
    );
}

#[test]
fn clipmap_save_audit_checks_saved_root_against_the_same_complete_five_points() {
    let request = request(90.);
    let sampler = PlacementTerrainSampler::from_clipmap_samples([7; 7], samples(&request));
    assert!(audit_existing_buildings(&indexed(&request, 12.), &sampler, bounds()).is_none());
    assert!(
        audit_existing_buildings(&indexed(&request, 99.), &sampler, bounds())
            .unwrap()
            .contains("根高度")
    );
}

#[test]
fn first_building_waiting_for_height_blocks_save_even_before_a_record_exists() {
    let request = request(0.);
    let footprint = request.preset.world_footprint([20., 0., 20.], 0., [1.; 3]);
    let state = BuildingAuthoringState {
        validation_waiting: Some(BuildingValidationResult {
            request,
            footprint,
            entrance_clearance: footprint,
            outcome: Err("等待".into()),
            terrain_leveling: None,
            ground_query: Some(BuildingGroundQuery::default()),
            sparse_leveling: None,
        }),
        ..default()
    };
    let session = EditorAuthoringSession {
        draft: Some(crate::authoring::test_heightfield_draft(Arc::new(vec![
            0;
            121
        ]))),
        ..default()
    };
    assert!(matches!(
        super::super::save_readiness(&session, None, Some(&state), None),
        BuildingSaveReadiness::Waiting(_)
    ));
    assert!(session.undo.is_empty());
    assert!(
        session
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .is_empty()
    );
}
