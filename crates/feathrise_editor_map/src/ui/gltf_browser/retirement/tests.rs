//! GLB浏览器大型旧代容器的分帧退役与背压回归。

use bevy::prelude::Handle; // 纯资源测试使用默认WorldAsset强句柄。
use bevy::world_serialization::WorldAsset; // 关闭缓存的Scene0资产类型。

use super::*; // 测试直接观察私有代次计数和固定预算推进。
use crate::ui::gltf_browser::{
    CachedGltfAssetPhase, EditorGltfBrowserState, MAX_GLTF_BROWSER_PLACEMENTS,
}; // 活动资源和浏览硬门定义在退役模块父级。

fn signature(index: usize) -> PlacementSignature {
    PlacementSignature {
        editor: Default::default(),
        placement_id: format!("placement-{index}"), // 每项拥有独立稳定实例ID。
        asset_path: format!("gltf/model-{index}.glb"), // 每项拥有独立资产路径。
        asset_target: Default::default(),           // 退役成本夹具保留Scene0固定大小目标。
        water_bindings: Vec::new(),                 // 本测试锁定8192记录壳，而非公共256水绑定门。
    }
}

#[test]
fn closing_8192_signatures_and_caches_retires_over_multiple_frames() {
    let mut state = EditorGltfBrowserState {
        initialized: true, // 模拟F12已经建立完整活动会话。
        signature: (0..MAX_GLTF_BROWSER_PLACEMENTS).map(signature).collect(), // 最大合法浏览签名。
        ..Default::default()
    };
    for index in 0..MAX_GLTF_BROWSER_PLACEMENTS {
        state.placement_asset_paths.insert(
            format!("placement-{index}"),
            format!("gltf/model-{index}.glb"),
        ); // 关闭还必须逐项释放同规模O(1)点击索引。
    }
    for index in 0..64usize {
        let path = format!("gltf/shared-{index}.glb");
        state.assets.insert(
            path,
            CachedGltfAsset {
                handle: Handle::<WorldAsset>::default(), // 无AssetServer也能验证强句柄所有权转移。
                meshes: vec![CachedGltfMesh {
                    node_path: vec!["河道".to_owned(), "水面".to_owned()], // 深字段不能随BTreeMap整批Drop。
                    mesh_name: format!("WaterSurface{index}"), // 每缓存拥有独立名称分配。
                    mesh_index: Some(index as u32),            // 诊断索引不影响退役预算。
                    valid: true,                               // 完整Ready缓存语义。
                    ambiguous: false,                          // 退役测试不构造歧义身份。
                }],
                active_ready: true,                 // 关闭前属于公开可用缓存。
                phase: CachedGltfAssetPhase::Ready, // 无加载轮询状态。
                truncated: false,                   // 测试内容未触及Scene上限。
                content_revision: 1,                // 单一稳定修订。
            },
        );
    }

    let generation = take_browser_session_generation(&mut state); // 活动字段只移动所有权。
    state.retirement.enqueue_close(generation); // 使用预留关闭槽。
    assert!(state.signature.is_empty()); // 对外活动签名同帧即为空。
    assert!(state.assets.is_empty()); // 对外强句柄缓存索引同帧即为空。
    assert_eq!(state.retirement.generation_count(), 1); // 全部关闭字段合成一代而非按容器堆代次。

    state.retirement.advance_frame(); // 一帧最多释放128个逻辑元素。
    assert_eq!(state.retirement.generation_count(), 1); // 8192记录绝不在关闭帧或首个清理帧整批Drop。
    let mut frames = 1usize;
    while !state.retirement.is_empty() {
        state.retirement.advance_frame();
        frames += 1;
        assert!(frames < 300, "固定预算应在有限帧内清空有界浏览数据");
    }
    assert!(frames > 2); // 明确锁定跨多帧退役而非一次释放。
}

#[test]
fn normal_generation_backpressure_never_accumulates_new_generations() {
    let mut retirement = BrowserRetirement::default();
    let mut first = BrowserRetiredGeneration::default();
    first.signatures((0..512).map(signature).collect()); // 建立需要多帧的第一代。
    retirement.enqueue_normal_reserved(first); // 正常槽首次采用成功。
    assert_eq!(retirement.generation_count(), 1);

    for _ in 0..128 {
        assert!(!retirement.can_enqueue_normal()); // 连续热更只应保留调用方最新草稿。
        assert_eq!(retirement.generation_count(), 1); // 队列物理代次数不随请求增长。
    }
}

#[test]
fn one_oversized_retired_row_can_exclusively_finish_on_a_fresh_frame() {
    let mut generation = BrowserRetiredGeneration::default();
    generation.entries(vec![EditorGltfBrowserEntry {
        kind: super::super::EditorGltfBrowserEntryKind::Mesh, // 大项仍是一条普通网格行。
        display_name: "水".repeat(20_000),                    // UTF-8字节超过32KiB退役近似预算。
        depth: 1,                                             // 视觉深度不增加退役成本。
        selection: None,                                      // 单独用显示字符串复现超预算情形。
    }]);
    let mut retirement = BrowserRetirement::default();
    retirement.enqueue_normal_reserved(generation);

    retirement.advance_frame(); // 新鲜帧允许该单项独占预算。
    assert!(retirement.is_empty()); // 不会每帧Blocked而永久饿死。
}

#[test]
fn enqueue_side_effect_stays_outside_debug_assert_source_contract() {
    let source = include_str!("queue.rs"); // release构建会移除debug_assert，源码结构必须保证push仍执行。
    let normalized_source = source.replace("\r\n", "\n"); // Windows检出使用CRLF时仍验证同一Rust源码结构。
    let unconditional_push = ");\n        self.generations.push_back(generation)";

    assert_eq!(
        normalized_source.matches(unconditional_push).count(),
        2,
        "正常换代与F12关闭都必须在debug_assert结束后无条件入队",
    );
    assert!(!normalized_source.contains("debug_assert!(self.generations.push_back"));
}
