//! 稀疏作者接口回归：只使用临时场景，不触碰旧 F12 或 HN 场景资产。

use std::path::PathBuf; // 每项测试独占临时资产根。
use std::sync::atomic::{AtomicU64, Ordering}; // 并行测试目录不复用。

use feathrise_scene_content::{SceneId, SparseHeightDocument, SparseHeightSpec}; // 用生产清单创建空白场景。

use super::{
    MAX_SPARSE_AUTHOR_CLEAN_TILES, MAX_SPARSE_BRUSH_CANDIDATES, SparseTerrainAuthorSession,
    SparseTerrainBrushInput, SparseTerrainBrushKind,
}; // 通过外部编辑接口验证行为。

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0); // 当前进程中的测试目录流水号。

pub(super) fn test_scene() -> (PathBuf, SceneId) {
    let serial = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "bevy-editor-sparse-terrain-{}-{serial}",
        std::process::id()
    ));
    std::fs::create_dir(&root).expect("测试应独占自己创建的资产根");
    let scene_id = SceneId::parse("MAP-sparse-brush-test").unwrap();
    let spec = SparseHeightSpec::new([-32.0, -32.0], [32.0, 32.0], 0.125, -80.0, 160.0).unwrap();
    let mut author = SparseHeightDocument::create_empty(&root, scene_id.clone(), spec).unwrap();
    author.save_dirty().expect("空白测试场景只写小清单");
    (root, scene_id)
}

fn brush(
    kind: SparseTerrainBrushKind,
    center_xz: [f64; 2],
    target: Option<f32>,
) -> SparseTerrainBrushInput {
    SparseTerrainBrushInput {
        kind,                          // 测试明确选择现有三种基本地形工具之一。
        center_xz,                     // 跨块用场景原点，普通刷印也保留世界固定坐标。
        radius_meters: 0.5,            // 只触及局部数十个格点。
        strength: 1.0,                 // 最大作者强度使结果容易判定。
        speed_meters_per_second: 20.0, // 0.05 秒刷印刚好抬升中心约 1 米。
        delta_seconds: 0.05,           // 与 F12 输入帧上限相同。
        flatten_target_meters: target, // 压平目标在任务启动前冻结。
    }
}

#[test]
fn raise_lower_flatten_and_reopen_keep_world_height() {
    let (root, scene_id) = test_scene();
    let mut author = SparseTerrainAuthorSession::load(&root, &scene_id).unwrap();
    assert_eq!(author.sample_world_height(0.0, 0.0).unwrap(), Some(0.0)); // 空白地形世界 Y 精确为零。
    let raise = brush(SparseTerrainBrushKind::Raise, [0.0, 0.0], None);
    assert!(author.prefetch_brush_tiles(raise).unwrap() <= 4); // IO 阶段只读取受影响块。
    let raised = author.apply_brush_loaded(raise).unwrap();
    assert!(!raised.changes.is_empty());
    assert!(raised.dirty_tiles <= 4); // 原点位于四块交界，绝不建立整图 4 个以外的块。
    let high = author.sample_world_height(0.0, 0.0).unwrap().unwrap();
    assert!(high > 0.95 && high < 1.05); // 抬升保持米制语义。
    author
        .apply_brush(brush(SparseTerrainBrushKind::Lower, [0.0, 0.0], None))
        .unwrap();
    let low = author.sample_world_height(0.0, 0.0).unwrap().unwrap();
    assert!(low.abs() < 0.01); // 等量压低回到零附近。
    author
        .apply_brush(brush(
            SparseTerrainBrushKind::Flatten,
            [0.0, 0.0],
            Some(4.0),
        ))
        .unwrap();
    let flattened = author.sample_world_height(0.0, 0.0).unwrap().unwrap();
    assert!(flattened > 0.9 && flattened < 1.1); // 既有压平为渐近 0.25 因子。
    assert!(author.is_dirty());
    let saved_tiles = author.save_dirty().unwrap();
    assert!(saved_tiles <= 4 && saved_tiles > 0); // 只提交实际刷过的块。
    assert!(!author.is_dirty());
    let mut reopened = SparseTerrainAuthorSession::load(&root, &scene_id).unwrap();
    assert!(reopened.apply_brush_loaded(raise).is_err()); // 真实非平地块未知时计算阶段不会同步读盘或覆盖。
    assert!(reopened.prefetch_world_sample_tiles(0.0, 0.0).unwrap() <= 4); // IO阶段只读双线性采样周围块。
    let persisted = reopened.sample_world_height(0.0, 0.0).unwrap().unwrap();
    assert!((persisted - flattened).abs() < 0.002); // 离开再进使用的是持久作者块，而非旧会话缓存。
    std::fs::remove_dir_all(root).expect("只清理当前测试创建的临时场景");
}

#[test]
fn cross_tile_undo_and_redo_are_all_or_nothing() {
    let (root, scene_id) = test_scene();
    let mut author = SparseTerrainAuthorSession::load(&root, &scene_id).unwrap();
    let raised = author
        .apply_brush(brush(SparseTerrainBrushKind::Raise, [0.0, 0.0], None))
        .unwrap();
    assert!(raised.changes.iter().any(|change| change.x < 256)); // 笔刷跨过全局 X=256 边界。
    assert!(raised.changes.iter().any(|change| change.x >= 256)); // 右侧块由同一命令覆盖。
    author.replay_changes(&raised.changes, false).unwrap();
    assert_eq!(author.sample_world_height(0.0, 0.0).unwrap(), Some(0.0)); // 一次撤销还原全部触点。
    author.replay_changes(&raised.changes, true).unwrap();
    assert!(author.sample_world_height(0.0, 0.0).unwrap().unwrap() > 0.95);
    let mut bad = raised.changes.clone();
    bad[0].after = bad[0].after.wrapping_add(1); // 故意制造跨块命令与当前代次不一致。
    let before = author.sample_world_height(0.0, 0.0).unwrap();
    assert!(author.replay_changes(&bad, false).is_err());
    assert_eq!(author.sample_world_height(0.0, 0.0).unwrap(), before); // 前置验证失败后绝无半笔修改。
    std::fs::remove_dir_all(root).expect("只清理当前测试创建的临时场景");
}

#[test]
fn perspective_ray_moves_xz_to_the_loaded_height_surface() {
    let (root, scene_id) = test_scene();
    let mut author = SparseTerrainAuthorSession::load(&root, &scene_id).unwrap();
    author
        .apply_brush(brush(SparseTerrainBrushKind::Raise, [0.0, 0.0], None))
        .unwrap(); // 原点附近形成约一米高的连续小丘。
    let hit = author
        .intersect_loaded_height_ray([1.0, 20.0, 0.0], [-0.05, -1.0, 0.0])
        .expect("已驻留小丘应得到真实透视命中");
    assert!(hit[1] > 0.5); // 命中真实作者高度，而不是世界零平面。
    assert!(hit[0] > 0.01 && hit[0] < 0.1); // 高处更早命中，因此 X 不能沿用零平面的恰好零值。
    author
        .save_dirty()
        .expect("射线夹具必须先发布内容寻址高度页"); // 重开后验证光标主动预取所依赖的等待语义。
    let mut reopened = SparseTerrainAuthorSession::load(&root, &scene_id).unwrap(); // 新会话只读清单，不提前读取真实块。
    assert!(
        reopened
            .intersect_loaded_height_ray([1.0, 20.0, 0.0], [-0.05, -1.0, 0.0])
            .is_none()
    ); // 未驻留非平地页必须明确等待，不能把它猜成世界零平面。
    let fallback_key = reopened.world_sample_tile_key(0.0, 0.0).unwrap(); // 托底命中确定需要预取的主块。
    assert_eq!(fallback_key.x, 1); // 原点由右侧共享边所有者块负责。
    assert_eq!(fallback_key.z, 1); // Z 轴同样遵守唯一共享点所有权。
    assert!(reopened.prefetch_world_sample_tiles(0.0, 0.0).unwrap() <= 4); // 光标一次只读取双线性邻域。
    let prefetched_hit = reopened
        .intersect_loaded_height_ray([1.0, 20.0, 0.0], [-0.05, -1.0, 0.0])
        .expect("预取后必须恢复真实透视高度命中");
    assert!((prefetched_hit[1] - hit[1]).abs() < 0.002); // 后台预取不能改变同一作者曲面。
    std::fs::remove_dir_all(root).expect("只清理当前测试创建的临时场景");
}

#[test]
fn oversized_brush_rejects_before_allocating_scene_tiles() {
    let (root, scene_id) = test_scene();
    let mut author = SparseTerrainAuthorSession::load(&root, &scene_id).unwrap();
    let mut request = brush(SparseTerrainBrushKind::Raise, [0.0, 0.0], None);
    request.radius_meters = 64.0; // 最小图上的 513² 候选超过固定预算。
    assert!(
        author
            .apply_brush(request)
            .unwrap_err()
            .contains(&MAX_SPARSE_BRUSH_CANDIDATES.to_string())
    );
    assert_eq!(author.loaded_tile_count(), 0); // 错误发生在任何作者块载入或分配前。
    assert!(!author.is_dirty()); // 保存门也不会误认为这是一笔有效修改。
    std::fs::remove_dir_all(root).expect("只清理当前测试创建的临时场景");
}

#[test]
fn smoothing_reads_cross_tile_grid_neighbors_before_writing() {
    let (root, scene_id) = test_scene(); // 原点恰好位于四个作者块共享边。
    let mut author = SparseTerrainAuthorSession::load(&root, &scene_id).unwrap();
    assert_eq!(author.spec().cells, [512, 512]); // 共享核心公开只读规格，不暴露内部持久文档。
    let mut spike = brush(SparseTerrainBrushKind::Raise, [0.0, 0.0], None); // 用正式笔刷建立中心尖峰。
    spike.radius_meters = 0.1; // 小于格距，只修改四块共享边上的权威中心格点。
    spike.speed_meters_per_second = 200.0; // 0.05 秒积分得到约十米高度。
    author
        .apply_brush(spike)
        .expect("中心尖峰应建立共享边权威块");
    let before = author.sample_world_height(0.0, 0.0).unwrap().unwrap();
    assert!((before - 10.0).abs() < 0.002); // 邻点仍为隐式零平地。
    let mut input = brush(SparseTerrainBrushKind::Smooth, [0.0, 0.0], None);
    input.radius_meters = 0.1; // 只修改中心，四邻域仅作为冻结输入。
    author.apply_brush(input).expect("跨块四邻域应先完整预取");
    let after = author.sample_world_height(0.0, 0.0).unwrap().unwrap();
    assert!(after < before && after > 7.9); // 20%渐近到包含自身的五点平均值二米。
    std::fs::remove_dir_all(root).expect("只清理当前测试创建的临时场景");
}

#[test]
fn cursor_prefetch_keeps_a_bounded_clean_author_cache() {
    let serial = TEST_COUNTER.fetch_add(1, Ordering::Relaxed); // 与其他并行测试目录保持隔离。
    let root = std::env::temp_dir().join(format!(
        "bevy-editor-sparse-cache-{}-{serial}",
        std::process::id()
    )); // 此夹具需要横向至少六十五个真实作者块。
    std::fs::create_dir(&root).expect("测试应独占自己创建的资产根");
    let scene_id = SceneId::parse("MAP-sparse-cache-test").unwrap(); // 只存在临时资产根内。
    let spec = SparseHeightSpec::new([0.0, 0.0], [8192.0, 64.0], 0.125, -80.0, 160.0)
        .expect("合法最大轴场景规格");
    let mut document = SparseHeightDocument::create_empty(&root, scene_id.clone(), spec).unwrap();
    for tile_x in 0_u64..=64 {
        document
            .set_grid_height(tile_x * 256 + 1, 1, 1.0)
            .expect("每块一个非零点应建立真实内容页");
    }
    document.save_dirty().expect("夹具应发布全部内容地址页");
    let mut author = SparseTerrainAuthorSession::load(&root, &scene_id).unwrap(); // 重开后从零驻留开始。
    for tile_x in 0_u64..=64 {
        let world_x = (tile_x * 256 + 1) as f64 * 0.125; // 点严格位于目标块内部。
        author
            .prefetch_world_sample_tiles(world_x, 0.125)
            .expect("每次光标预取只读取当前双线性邻域");
    }
    assert_eq!(
        author.clean_loaded_tile_count(),
        MAX_SPARSE_AUTHOR_CLEAN_TILES
    ); // 浏览第六十五页后最旧干净页必须被回收。
    assert_eq!(author.loaded_tile_count(), MAX_SPARSE_AUTHOR_CLEAN_TILES); // 无脏页时总驻留同样受固定预算约束。
    std::fs::remove_dir_all(root).expect("只清理当前测试创建的临时场景");
}
