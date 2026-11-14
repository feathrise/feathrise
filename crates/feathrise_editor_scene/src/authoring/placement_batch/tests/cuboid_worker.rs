//! 纯立方体组以真实最低底面为锚点；空图与真实地形各自保持独立高度语义。

use super::*;
use feathrise_scene_content::ScenePlacementAssetTarget;

fn captured() -> Arc<ClipboardData> {
    let source = [(-1.0, 5.0, 4000, 1.5), (1.0, 8.0, 2000, 2.0)]
        .into_iter()
        .enumerate()
        .map(|(index, (x, y, height, scale))| {
            let mut record = record(&format!("cube-{index}"), x);
            record.asset_path.clear();
            record.asset_target = ScenePlacementAssetTarget::Cuboid {
                size_mm: [2000, height, 2000],
                color_srgb: [160; 3],
                roughness_milli: 700,
                collision: true,
            };
            record.translation[1] = y;
            record.scale[1] = scale;
            record.height_mode = ScenePlacementHeightMode::FollowTerrain;
            record.terrain_offset = y - 1.0;
            (index, record)
        })
        .collect::<Vec<_>>();
    let BatchOutput::Captured(Ok(clipboard), false) = worker::capture(Capture { model_vegetation: Arc::default(),
        identity: BatchIdentity {
            document: crate::authoring::placement_deletion::PlacementDeletionIntents::default().identity(),
            scene_id: map(),
            author: 0,
            structure: 0,
            terrain: 0,
            placement_derived_pose_revision: 0,
            content: None,
        },
        action: PlacementBatchAction::Copy,
        members: Arc::default(),
        records: source,
        bounds: bounds(),
        source_objects: Default::default(),
        length: 2,
        selection_revision: 0,
        selected_id: None,
    }) else {
        panic!("内置对象复制不应访问空资产路径")
    };
    clipboard
}

#[test]
fn empty_scene_cuboid_paste_uses_scaled_lowest_bottom_and_clears_follow_semantics() {
    let clipboard = captured();
    assert_eq!(clipboard.origin, Vec3::new(0.0, 2.0, 0.0));
    let ghosts = worker::ghost_records(&clipboard, "copy").unwrap();
    let BatchOutput::Paste(Ok(prepared)) = worker::run_paste(
        PasteWork { model_vegetation: None,
            source: ghosts,
            delta: Vec3::new(5.0, 0.0, 5.0) - clipboard.origin,
            records: Vec::new(),
            validator: None,
            presets: Arc::new(vec![None, None]),
            sampler: None,
            work_plane: true,
            patch: None,
            query: None,
            bounds: bounds(),
            scene_id: map(),
            building_revision: 0,
        },
        0,
    ) else {
        panic!("明确空图的纯立方体应在无采样器时完成")
    };
    assert_eq!(prepared.apply[0].translation, [4.0, 3.0, 5.0]);
    assert_eq!(prepared.apply[1].translation, [6.0, 6.0, 5.0]);
    for (index, record) in prepared.apply.iter().enumerate() {
        assert_eq!(record.scale, clipboard.records[index].scale);
        assert_eq!(record.asset_target, clipboard.records[index].asset_target);
        assert_eq!(record.terrain_offset, 0.0);
        assert_eq!(record.height_mode, ScenePlacementHeightMode::FixedHeight);
        assert!(record.terrain_binding.is_none());
    }
    assert_eq!(prepared.history.added, prepared.apply);
    assert_eq!(clipboard.records[0].translation[1], 5.0);
}

#[test]
fn grounded_cuboid_paste_keeps_center_offsets_without_adding_half_height_twice() {
    let clipboard = captured();
    let draft = crate::authoring::test_support::test_clipmap_draft(Arc::new(vec![4; 121]), 0);
    let sampler = crate::authoring::PlacementTerrainSampler::from_sources(&draft, None, 0).unwrap();
    let BatchOutput::Paste(Ok(prepared)) = worker::run_paste(
        PasteWork { model_vegetation: None,
            source: worker::ghost_records(&clipboard, "grounded-copy").unwrap(),
            delta: Vec3::new(5.0, 4.0, 5.0) - clipboard.origin,
            records: Vec::new(),
            validator: None,
            presets: Arc::new(vec![None, None]),
            sampler: Some(sampler),
            work_plane: false,
            patch: None,
            query: None,
            bounds: draft.manifest.world_bounds().unwrap(),
            scene_id: draft.scene_id,
            building_revision: 0,
        },
        0,
    ) else {
        panic!("真实地形只重算中心离地偏移")
    };
    assert_eq!(prepared.apply[0].translation, [4.0, 7.0, 5.0]);
    assert_eq!(prepared.apply[1].translation, [6.0, 10.0, 5.0]);
    assert_eq!(prepared.apply[0].terrain_offset, 3.0);
    assert_eq!(prepared.apply[1].terrain_offset, 6.0);
}

#[test]
fn work_plane_worker_rejects_external_or_mixed_asset_sources() {
    let BatchOutput::Paste(result) = worker::run_paste(
        PasteWork { model_vegetation: None,
            source: Arc::new(vec![record("external", 0.0)]),
            delta: Vec3::ZERO,
            records: Vec::new(),
            validator: None,
            presets: Arc::new(vec![None]),
            sampler: None,
            work_plane: true,
            patch: None,
            query: None,
            bounds: bounds(),
            scene_id: map(),
            building_revision: 0,
        },
        0,
    ) else {
        panic!("外部资产应明确拒收")
    };
    assert!(result.is_err());
}
