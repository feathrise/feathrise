use std::collections::BTreeMap; // 正式密度候选测试复用生产索引结构。
use std::sync::Arc; // 测试像素与草稿一样使用共享R8。

use bevy::prelude::Vec2; // 世界和点击位置使用生产相同二维类型。
use feathrise_scene_content::{
    SceneVegetationAuthorGroup, SceneVegetationAuthorGroupMode, SceneVegetationAuthorLayer,
    SceneVegetationAuthoring, SceneVegetationKind, SceneVegetationLayer, SceneVegetationOutputMergeMode,
}; // 构造最小完整正式层与作者图层图。

use super::helpers::{
    author_layer_anchor, choose_vegetation_candidate, sample_density_at_world,
    vegetation_layer_candidates,
}; // 直接锁定无ECS纯逻辑。
use crate::constants::EDITOR_VEGETATION_PREVIEW_LAYER_BIT; // 预览高位归一化属于正式回归目标。
use crate::resources::EditorViewportSelectionState; // 候选循环使用真实会话资源。

fn formal_layer(id: &str, runtime_layer_id: u64) -> SceneVegetationLayer {
    SceneVegetationLayer {
        id: id.to_owned(),                                     // 稳定正式输出ID。
        label: id.to_owned(),                                  // 测试不依赖显示名。
        kind: SceneVegetationKind::Mesh,                         // 逐株Mesh可提供强层身份。
        runtime_profile: Some("test".to_owned()),              // 保持完整profile字段。
        runtime_layer_id: Some(runtime_layer_id),              // 本测试关注的通用层ID。
        prototype_id: Some(runtime_layer_id + 100),            // 合法但不实际加载原型。
        asset_path: "gltf/test.glb".to_owned(),                // 不执行资产IO。
        density_source_path: format!("vegetation/{id}.r8"),    // 每层独立作者源。
        density_texture_path: format!("vegetation/{id}.ktx2"), // 每层独立运行时派生。
        resolution: [3, 2],                                    // 非方形图锁定X/Z索引方向。
        spacing_meters: 1.0,                                   // 测试不执行散布。
        scale_range: [1.0, 1.0],                               // 合法固定缩放。
        max_instances: u32::MAX,                               // 作者驱动正式输出。
        seed: runtime_layer_id,                                // 稳定但不参与选择。
    }
}

fn author_group(id: &str, enabled: bool) -> SceneVegetationAuthorGroup {
    SceneVegetationAuthorGroup {
        id: id.to_owned(),                                   // 作者层稳定组引用。
        label: id.to_owned(),                                // 测试不依赖显示名。
        mode: SceneVegetationAuthorGroupMode::Additive,        // 共存规则足以测试活动门。
        output_merge: SceneVegetationOutputMergeMode::Maximum, // 跨组规则不参与锚点。
        capacity: 1.0,                                       // 合法默认容量。
        claim_threshold: 0.0,                                // 合法默认阈值。
        selection_seed: 1,                                   // 稳定选择种子。
        selection_cell_meters: 1.0,                          // 合法世界格距。
        enabled,                                             // 测试活动与关闭组分支。
    }
}

fn author_layer(
    id: &str,
    group_id: &str,
    output_id: &str,
    enabled: bool,
) -> SceneVegetationAuthorLayer {
    SceneVegetationAuthorLayer {
        id: id.to_owned(),                              // 稳定作者层ID。
        label: id.to_owned(),                           // 测试不依赖显示名。
        group_id: group_id.to_owned(),                  // 指向测试组。
        output_layer_id: output_id.to_owned(),          // 多层可共同写同一正式输出。
        density_source_path: format!("author/{id}.r8"), // 独立作者R8路径。
        resolution: [3, 2],                             // 与正式夹具保持一致。
        enabled,                                        // 测试贡献者过滤。
        locked: false,                                  // 锁定不影响编译贡献。
        opacity: 1.0,                                   // 完整贡献。
        priority: 0,                                    // 锚点不按优先级改UI顺序。
        weight: 1.0,                                    // 合法正权重。
    }
}

#[test]
fn density_sampling_uses_xz_endpoints_and_rejects_bad_lengths() {
    let pixels = [1, 2, 3, 4, 5, 6]; // 两行三列，第二行中点值为5。
    assert_eq!(
        sample_density_at_world(
            &pixels,
            [3, 2],
            [0.0, -10.0],
            [20.0, 10.0],
            Vec2::new(10.0, 10.0),
        ),
        Some(5),
    ); // 最大Z端点和中间X必须命中索引4。
    assert_eq!(
        sample_density_at_world(
            &pixels[..5],
            [3, 2],
            [0.0, -10.0],
            [20.0, 10.0],
            Vec2::new(10.0, 10.0),
        ),
        None,
    ); // 声明分辨率与字节数不一致时失败关闭。
}

#[test]
fn mesh_identity_precedes_density_and_preview_bit_scenes_to_formal_layer() {
    let layers = vec![formal_layer("grass", 1), formal_layer("tree", 2)]; // 正式定义顺序先草后树。
    let densities = BTreeMap::from([
        ("grass".to_owned(), Arc::new(vec![255; 6])), // 点击处草密度非零。
        ("tree".to_owned(), Arc::new(vec![0; 6])),    // 树纹素为零但真实冠层仍被命中。
    ]);
    let candidates = vegetation_layer_candidates(
        &layers,
        &densities,
        [0.0, 0.0],
        [2.0, 1.0],
        Vec2::new(1.0, 0.5),
        Some(2 | EDITOR_VEGETATION_PREVIEW_LAYER_BIT),
    );
    assert_eq!(candidates, ["tree", "grass"]); // F12预览树层恢复低位身份并稳定优先于地表密度。
}

#[test]
fn author_anchor_prefers_current_active_layer_and_reports_all_contributors() {
    let authoring = SceneVegetationAuthoring {
        groups: vec![author_group("active", true), author_group("hidden", false)], // 关闭组不参与当前编译。
        layers: vec![
            author_layer("north", "active", "tree", true),
            author_layer("south", "active", "tree", true),
            author_layer("old", "hidden", "tree", true),
        ],
    };
    assert_eq!(
        author_layer_anchor(&authoring, "tree", 1),
        Some(super::helpers::AuthorLayerAnchor {
            index: 1,
            contributor_count: 2,
        }),
    ); // 当前活动作者层保持锚点，Compiled模式依据两张真实贡献图。
}

#[test]
fn repeated_or_alt_click_cycles_only_within_same_pixel_candidate_context() {
    let mut state = EditorViewportSelectionState::default(); // 独立会话从首候选开始。
    let scene_id =
        feathrise_scene_content::SceneId::parse("MAP-content-selection").expect("测试场景ID合法"); // 循环上下文必须包含稳定场景身份。
    let candidates = vec!["grass".to_owned(), "tree".to_owned()]; // 同落点两个正式层。
    assert_eq!(
        choose_vegetation_candidate(
            &mut state,
            &scene_id,
            &candidates,
            Vec2::new(10.0, 20.0),
            1.0,
            false,
        ),
        Some(0),
    ); // 首次普通点击选择优先层。
    state.selected_vegetation_scene_id = Some(scene_id.clone()); // 生产系统在成功发布首候选后写入同一场景身份。
    assert_eq!(
        choose_vegetation_candidate(
            &mut state,
            &scene_id,
            &candidates,
            Vec2::new(12.0, 21.0),
            1.2,
            false,
        ),
        Some(1),
    ); // 六像素内快速重复点击切到下一层。
    assert_eq!(
        choose_vegetation_candidate(
            &mut state,
            &scene_id,
            &candidates,
            Vec2::new(12.0, 21.0),
            5.0,
            true,
        ),
        Some(0),
    ); // 同一候选上下文中Alt不受普通时间窗限制并继续循环。
    assert_eq!(
        choose_vegetation_candidate(
            &mut state,
            &scene_id,
            &candidates,
            Vec2::new(40.0, 21.0),
            5.1,
            true,
        ),
        Some(0),
    ); // 新像素位置即使按Alt也从视觉首层开始。
    let other_scene = feathrise_scene_content::SceneId::parse("MAP-content-selection-other")
        .expect("另一测试场景ID合法");
    assert_eq!(
        choose_vegetation_candidate(
            &mut state,
            &other_scene,
            &candidates,
            Vec2::new(40.0, 21.0),
            5.2,
            true,
        ),
        Some(0),
    ); // 同名候选也不能跨场景继承上一循环下标。
}
