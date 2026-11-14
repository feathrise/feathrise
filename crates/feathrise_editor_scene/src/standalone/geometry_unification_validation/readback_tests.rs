//! 证据口径回归：live作者值、隐式零页和保存后磁盘必须表达同一完整小网格。
use super::*;
use std::sync::Arc;

#[test]
fn dirty_height_proof_reads_live_pages_zero_undo_is_canonical_and_saved_disk_matches() {
    let root = std::env::temp_dir().join(format!("feathrise-geometry-readback-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let owner = draft.scene_id;
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    let spec = SparseHeightSpec::new([-2.; 2], [2.; 2], 0.125, -8., 16.).unwrap();
    let mut author = SparseHeightDocument::create_for_terrain(&root, owner.clone(), Some(object.id.clone()), spec.clone()).unwrap();
    author.save_dirty().unwrap();
    object.world_min = spec.world_min; object.world_max = spec.world_max;
    object.terrain.kind = SceneTerrainKind::SparseClipmap;
    object.terrain.height_path = Some(author.manifest().asset_path());
    object.terrain.display_lods = Some(vec![SceneTerrainDisplayLod { outer_half_extent_meters: None, spacing_multiplier: 1 }]);
    let blank = capture(&root, &owner, &object, None, None).unwrap();
    assert_eq!(blank.nonzero_samples, 0);
    let (key, _) = spec.tile_for_grid_point(16, 16).unwrap();
    author.set_grid_height(16, 16, 1.25).unwrap();
    let mut live = SparseHeightLiveOverride::default();
    live.retarget(Some(owner.clone()), 0);
    live.publish_tiles(&owner, 1, [author.tile_snapshot(key).unwrap()]).unwrap();
    let changed = capture(&root, &owner, &object, Some(live.snapshot()), None).unwrap();
    assert_eq!(changed.nonzero_samples, 1);
    assert_ne!(blank.sha256, changed.sha256);
    assert_eq!(capture(&root, &owner, &object, None, None).unwrap(), blank, "尚未保存的雕刻不能偷用磁盘旧页当证据");
    author.set_grid_height(16, 16, 0.).unwrap();
    live.publish_tiles(&owner, 2, [author.tile_snapshot(key).unwrap()]).unwrap();
    assert_eq!(capture(&root, &owner, &object, Some(live.snapshot()), None).unwrap(), blank,
        "撤销到显式零页与最初隐式平地必须逐格相同");
    author.set_grid_height(16, 16, 1.25).unwrap();
    author.save_dirty().unwrap();
    assert_eq!(capture(&root, &owner, &object, None, None).unwrap(), changed, "独立磁盘回读必须匹配未保存作者值");
    drop(author);
    std::fs::remove_dir_all(root).unwrap(); // 仅删除本测试在系统临时目录独占创建的已知夹具。
}
