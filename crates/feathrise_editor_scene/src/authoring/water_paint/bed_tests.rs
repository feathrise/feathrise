//! 静水覆盖只改R8、保留全部GLB顶点及分帧检查点的回归。

use std::sync::Arc;

use super::*;
use feathrise_scene_content::{SceneStillWaterKind, SceneWaterLayer};

fn layer() -> SceneWaterLayer {
    SceneWaterLayer {
        id: "WATER-TEST".to_owned(),
        label: "测试静水".to_owned(),
        mask_source_path: "scenes/EDGE-TEST/water/test.r8".to_owned(),
        mask_texture_path: "scenes/EDGE-TEST/water/test.ktx2".to_owned(),
        resolution: [11, 11],
        canvas_bounds: None,
        water_level: 999.0,
        still_kind: SceneStillWaterKind::Lake,
        preset_id: String::new(),
        flow_direction: [1.0, 0.0],
        flow_speed: 1.0,
        base_color_srgba: [0.05, 0.2, 0.3, 0.7],
        perceptual_roughness: 0.06,
    }
}

fn fixture(mask_value: u8) -> (EditorSceneDraft, SceneWaterLayer) {
    let mut draft =
        crate::authoring::terrain::tests::test_heightfield_draft(Arc::new(vec![1_000; 121]));
    let layer = layer();
    draft
        .water_masks
        .insert(layer.id.clone(), Arc::new(vec![mask_value; 121]));
    (draft, layer)
}

fn completed(outcome: WaterSegmentOutcome) -> WaterDabChanges {
    let WaterSegmentOutcome::Progressed {
        changed,
        complete,
        checkpoint_required,
        ..
    } = outcome
    else {
        panic!("测试没有后台Arc读者，不应Deferred");
    };
    assert!(complete);
    assert!(!checkpoint_required);
    changed
}

#[test]
fn transformed_water_uses_its_source_bounds_and_world_radius_without_moving_pixels() {
    use crate::authoring::terrain_documents::TerrainDocumentIdentity;
    let (mut draft, layer) = fixture(0);
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.transform = feathrise_scene_content::TerrainObjectTransform {
        translation: [20.0, 6.0, -30.0],
        yaw_degrees: 37.0,
        scale: [2.0, 3.0, 0.5],
        ..Default::default()
    };
    let matrix = object.transform.as_transform().to_matrix();
    let source = object.source_manifest(&draft.manifest);
    draft.manifest.world_min = [-200.0, -300.0];
    draft.manifest.world_max = [400.0, 500.0];
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(object),
        manifest: source,
        folder: draft.folder.clone(),
    });
    let center = Vec3::new(5.0, 0.0, 5.0);
    let mut stroke = WaterBrushStroke::default();
    assert!(
        completed(
            paint_water_segment(
                &mut draft,
                &layer,
                center,
                center,
                2.0,
                0.0,
                false,
                1.0,
                0,
                &mut stroke
            )
            .unwrap()
        )
        .mask_changed
    );
    for z in 0..11 {
        for x in 0..11 {
            let distance = matrix
                .transform_point3(Vec3::new(x as f32, 0.0, z as f32))
                .distance(matrix.transform_point3(center));
            let expected = ((1.0 - distance / 2.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            assert!(
                draft.water_masks[&layer.id][z * 11 + x].abs_diff(expected) <= 1,
                "世界圆刷水域 ({x},{z})"
            );
        }
    }
    assert!(stroke.height_before.is_empty());
}

// Regression: drawing, repeated drawing and erasing leave all terrain vertices intact.
#[test]
fn coverage_changes_never_modify_or_clone_shared_terrain() {
    let (mut draft, layer) = fixture(0); // Start with empty water coverage.
    let observer = draft.terrain_mesh.as_ref().unwrap().clone(); // Keep a real shared reader alive.
    let before_positions = observer.positions.clone(); // Preserve every vertex, including Y.
    let before_indices = observer.indices.clone(); // Preserve complete topology too.
    for erase in [false, false, true] {
        // Include repeated drawing before erasing.
        let mut stroke = WaterBrushStroke::default(); // Independent whole-stroke history.
        let changes = completed(
            paint_water_segment(
                &mut draft,
                &layer,
                Vec3::new(5.0, layer.water_level, 5.0),
                Vec3::new(5.0, layer.water_level, 5.0),
                2.0,
                layer.water_level,
                erase,
                0.5,
                0,
                &mut stroke,
            )
            .unwrap(),
        ); // Shared terrain must not defer or copy the mask operation.
        assert!(changes.mask_changed); // Every operation actually changes coverage.
        assert!(!changes.height_changed); // No terrain invalidation is needed.
        assert!(stroke.height_before.is_empty()); // History contains no terrain samples.
        let mesh = draft.terrain_mesh.as_ref().unwrap(); // Inspect the live author source.
        assert!(Arc::ptr_eq(mesh, &observer)); // No terrain copy-on-write occurred.
        assert_eq!(mesh.positions, before_positions); // Every vertex stays unchanged.
        assert_eq!(mesh.indices, before_indices); // Topology stays unchanged.
    }
}

#[test]
fn coverage_does_not_require_editable_terrain_or_height_range() {
    let (mut draft, layer) = fixture(0); // Valid R8 with deliberately unavailable terrain.
    draft.terrain_mesh = None; // GLB readiness is irrelevant to the mask writer.
    draft.terrain_editing_index = None; // No terrain sampling or spatial index is needed.
    draft.manifest.terrain.height_scale = 0.0; // Coverage does not derive a riverbed height.
    let mut stroke = WaterBrushStroke::default(); // R8-only history.
    let changes = completed(
        paint_water_segment(
            &mut draft,
            &layer,
            Vec3::new(5.0, 0.0, 5.0),
            Vec3::new(5.0, 0.0, 5.0),
            2.0,
            layer.water_level,
            false,
            1.0,
            0,
            &mut stroke,
        )
        .unwrap(),
    ); // No editable-terrain validation is allowed here.
    assert!(changes.mask_changed); // Prove that this was not an empty operation.
    assert!(draft.water_masks[&layer.id][60] > 0); // Center pixel receives coverage.
}

#[test]
fn shared_mask_defers_without_copying_or_touching_terrain() {
    let (mut draft, layer) = fixture(0); // Standard empty mask.
    let observer = draft.water_masks[&layer.id].clone(); // Keep the actual write source shared.
    let mut stroke = WaterBrushStroke::default(); // History should stay empty when deferred.
    let outcome = paint_water_segment(
        &mut draft,
        &layer,
        Vec3::new(5.0, 0.0, 5.0),
        Vec3::new(5.0, 0.0, 5.0),
        2.0,
        layer.water_level,
        false,
        1.0,
        0,
        &mut stroke,
    )
    .unwrap(); // Shared R8 is the only relevant ownership gate.
    assert!(matches!(outcome, WaterSegmentOutcome::Deferred)); // Preserve nonblocking policy.
    assert!(Arc::ptr_eq(&draft.water_masks[&layer.id], &observer)); // Do not clone full R8.
    assert!(stroke.mask_before.is_empty()); // No partial changes before deferral.
}

#[test]
fn large_coverage_stroke_keeps_frame_budget_and_checkpoint_progress() {
    let (mut draft, mut layer) = fixture(0); // Use a larger R8 without enlarging terrain.
    layer.resolution = [257, 257]; // More pixels than one frame or temporary history block.
    draft
        .water_masks
        .insert(layer.id.clone(), Arc::new(vec![0; 257 * 257])); // Empty large mask.
    let observer = draft.terrain_mesh.as_ref().unwrap().clone(); // Shared terrain stays untouched.
    let mut stroke = WaterBrushStroke::default(); // Simulate checkpoint recycling only.
    let mut cursor = 0; // Frozen segment resumes at the next unvisited R8 pixel.
    let mut frames = 0; // Verify this cannot complete in one frame.
    let mut checkpoints = 0; // Verify temporary history limits still apply.
    loop {
        let WaterSegmentOutcome::Progressed {
            changed,
            next_cursor,
            complete,
            checkpoint_required,
        } = paint_water_segment(
            &mut draft,
            &layer,
            Vec3::new(5.0, 0.0, 5.0),
            Vec3::new(5.0, 0.0, 5.0),
            20.0,
            layer.water_level,
            false,
            1.0,
            cursor,
            &mut stroke,
        )
        .unwrap()
        else {
            panic!("shared terrain must not defer R8 painting")
        }; // Mask is uniquely owned.
        assert!(next_cursor - cursor <= MAX_WATER_GRID_VISITS_PER_FRAME); // Fixed visit budget.
        assert!(stroke.mask_before.len() <= MAX_WATER_STROKE_SAMPLES); // Fixed temporary history.
        assert!(!changed.height_changed); // No terrain edit in any partial frame.
        cursor = next_cursor; // Resume exactly where this frame stopped.
        frames += 1; // Count actual progress calls.
        if checkpoint_required {
            stroke.mask_before.clear(); // Production moves this into the whole-stroke snapshot.
            checkpoints += 1; // Count bounded temporary-block recycling.
        }
        if complete {
            break;
        } // Finish only when the frozen segment completes.
        assert!(frames < 16); // Fail instead of hanging if checkpoint progress regresses.
    }
    assert!(frames > 1); // Large coverage is sliced across frames.
    assert!(checkpoints > 0); // Large strokes still recycle bounded history.
    assert!(draft.water_masks[&layer.id].iter().all(|value| *value > 0)); // No pixels lost at boundaries.
    assert!(Arc::ptr_eq(draft.terrain_mesh.as_ref().unwrap(), &observer)); // No terrain copy.
    assert!(stroke.height_before.is_empty()); // No terrain history in any chunk.
}
