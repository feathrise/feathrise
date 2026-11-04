//! 真实作者文件上的旋转平台、跨页、只抬不削、过渡与保存重开回归。
use super::*;
use feathrise_map_content::{BuildingPresetDescriptor, MapId, SparseHeightDocument};

fn fixture() -> (std::path::PathBuf, SparseTerrainAuthorSession) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "clipmap-building-platform-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    let id = MapId::parse("MAP-platform-test").unwrap();
    let spec = SparseHeightSpec::new([-32., -32.], [32., 32.], 0.125, -20., 80.).unwrap();
    let mut document = SparseHeightDocument::create_empty(&root, id.clone(), spec).unwrap();
    document.set_grid_height(254, 255, 5.).unwrap();
    document.save_dirty().unwrap();
    (
        root.clone(),
        SparseTerrainAuthorSession::load(&root, &id).unwrap(),
    )
}

fn footprint(yaw: f32) -> feathrise_map_content::BuildingWorldQuad {
    BuildingPresetDescriptor::from_asset(
        "house".into(),
        "民居".into(),
        "gltf/house.glb".into(),
        Default::default(),
    )
    .world_footprint([0.; 3], yaw, [1.; 3])
}

#[test]
fn rotated_platform_across_four_pages_is_raise_only_and_keeps_original_until_commit() {
    for yaw in [0., 90.] {
        let (root, mut author) = fixture();
        let quad = footprint(yaw);
        let (min, max, padding) = grid_bounds(author.spec(), quad).unwrap();
        author.prefetch_grid_rect(min, max).unwrap();
        let before = author.sample_world_height(0., 0.).unwrap().unwrap();
        let (edit, height) = build(&mut author, quad, min, max, padding).unwrap();
        assert!(height > 4.99);
        assert!(!author.is_dirty());
        assert_eq!(author.sample_world_height(0., 0.).unwrap(), Some(before));
        let adopted = author.install_prepared_edit(edit).unwrap();
        assert!(
            adopted
                .changes
                .iter()
                .all(|change| author.spec().decode_height(change.after) > author.spec().decode_height(change.before))
        );
        assert_eq!(adopted.live_tiles.len(), 4);
        for point in quad.foundation_samples() {
            assert!(
                (author
                    .sample_world_height(point.x as f64, point.y as f64)
                    .unwrap()
                    .unwrap()
                    - height)
                    .abs()
                    < 0.0001
            );
        }
        assert_eq!(author.sample_world_height(20., 20.).unwrap(), Some(0.));
        let id = author.map_id().clone();
        author.save_dirty().unwrap();
        let mut reopened = SparseTerrainAuthorSession::load(&root, &id).unwrap();
        reopened.prefetch_world_sample_tiles(0., 0.).unwrap();
        assert!((reopened.sample_world_height(0., 0.).unwrap().unwrap() - height).abs() < 0.0001);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn platform_transition_is_finite_and_large_footprint_fails_before_prefetch() {
    let (root, mut author) = fixture();
    let quad = footprint(0.);
    let (min, max, padding) = grid_bounds(author.spec(), quad).unwrap();
    author.prefetch_grid_rect(min, max).unwrap();
    let (edit, _) = build(&mut author, quad, min, max, padding).unwrap();
    let adopted = author.install_prepared_edit(edit).unwrap();
    assert!(adopted.changes.iter().all(|change| {
        let world = author.spec().grid_point_world(change.x, change.z).unwrap();
        super::super::super::leveling::distance_to_quad(
            Vec2::new(world[0] as f32, world[1] as f32),
            quad,
        ) <= padding * 2.
    }));
    assert!(
        adopted
            .changes
            .iter()
            .any(|change| author.spec().decode_height(change.after) < 5.)
    );
    let spec = SparseHeightSpec::new([0., 0.], [8192., 8192.], 0.125, -20., 80.).unwrap();
    let huge = feathrise_map_content::BuildingWorldQuad {
        corners: [
            Vec2::ZERO,
            Vec2::new(8000., 0.),
            Vec2::splat(8000.),
            Vec2::new(0., 8000.),
        ],
    };
    assert!(grid_bounds(&spec, huge).unwrap_err().contains("预算"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn signed_meter_platform_raises_negative_and_cross_zero_heights_with_exact_history() {
    for platform_height in [-30.5, 84.25] {
        let (root, mut author) = fixture();
        let quad = footprint(0.);
        let (min, max, padding) = grid_bounds(author.spec(), quad).unwrap();
        author.prefetch_grid_rect(min, max).unwrap();
        let spec = author.spec().clone();
        let mut initial = Vec::new();
        for z in min[1]..=max[1] {
            for x in min[0]..=max[0] {
                // 两种平台和背景均越过fixture旧-20～60m域，不能退回旧编码cap才能成立。
                let height = if [x, z] == [256, 256] { platform_height } else { -120.25 };
                initial.push(SparseTerrainHeightChange {
                    x,
                    z,
                    before: author.sample_grid_encoded(x, z).unwrap().unwrap(),
                    after: spec.encode_height(height).unwrap(),
                });
            }
        }
        let edit = author.prepare_changes_loaded(Arc::new(initial), true).unwrap();
        author.install_prepared_edit(edit).unwrap();
        author.save_dirty().unwrap();
        let (edit, height) = build(&mut author, quad, min, max, padding).unwrap();
        assert_eq!(height, platform_height, "负米制高度必须按高度而不是位模式选平台");
        assert!(!author.is_dirty(), "准备候选不能提前修改作者页");
        let applied = author.install_prepared_edit(edit).unwrap();
        assert!(!applied.changes.is_empty());
        assert!(applied.changes.iter().all(|change|
            spec.decode_height(change.after) > spec.decode_height(change.before)));
        if platform_height < 0. {
            assert!(applied.changes.iter().all(|change| change.after < change.before),
                "负浮点位模式与米制顺序相反，回归必须真实覆盖这条分支");
        }
        let changes = applied.changes;
        for redo in [false, true] {
            let edit = author.prepare_changes_loaded(changes.clone(), redo).unwrap();
            author.install_prepared_edit(edit).unwrap();
            for change in changes.iter() {
                assert_eq!(author.sample_grid_encoded(change.x, change.z).unwrap(),
                    Some(if redo { change.after } else { change.before }),
                    "撤销重做必须逐位恢复原始作者码");
            }
        }
        let map = author.map_id().clone();
        author.save_dirty().unwrap();
        let mut reopened = SparseTerrainAuthorSession::load(&root, &map).unwrap();
        reopened.prefetch_grid_rect(min, max).unwrap();
        for change in changes.iter() {
            assert_eq!(reopened.sample_grid_encoded(change.x, change.z).unwrap(), Some(change.after));
        }
        assert_eq!(reopened.sample_world_height(0., 0.).unwrap(), Some(platform_height));
        std::fs::remove_dir_all(root).unwrap();
    }
}
