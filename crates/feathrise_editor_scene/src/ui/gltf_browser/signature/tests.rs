use feathrise_scene_content::MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_PLACEMENT;
use std::collections::BTreeSet;

use feathrise_scene_content::{
    ScenePlacementAssetTarget, ScenePlacementRecord, ScenePlacementWaterKind, ScenePlacementWaterSurface,
};

use super::*;
use crate::ui::gltf_browser::retirement::{BrowserRetiredGeneration, BrowserRetirement};

fn placement(id: impl Into<String>, asset_path: impl Into<String>) -> ScenePlacementRecord {
    ScenePlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: id.into(),
        asset_path: asset_path.into(),
        asset_target: Default::default(), // 浏览签名基线使用可展开Scene0。
        building_preset_id: None,         // 普通浏览签名不附带建筑身份。
        courtyard: None,                  // 独立摆放记录不属于任何院落副本。
        translation: [0.0; 3],
        height_mode: feathrise_scene_content::ScenePlacementHeightMode::FixedHeight, // 树签名测试只比较结构身份。
        terrain_offset: 0.0, // 固定高度不携带离地偏移。
        yaw_degrees: 0.0,
        scale: [1.0; 3],
        water_surfaces: Vec::new(),
    }
}

fn water_surface(
    index: u32,
    node_path: Vec<String>,
    mesh_name: String,
) -> ScenePlacementWaterSurface {
    ScenePlacementWaterSurface {
        mesh_index: index,
        node_path,
        mesh_name,
        kind: ScenePlacementWaterKind::Sloped,
        flow_speed: 1.0,
        height_offset: 0.0,
    }
}

#[test]
fn maximum_short_signature_uses_32_record_budget_frames() {
    let placements = (0..MAX_GLTF_BROWSER_PLACEMENTS)
        .map(|index| placement(format!("instance-{index}"), "gltf/shared.glb"))
        .collect::<Vec<_>>();
    let expanded = placements
        .iter()
        .map(|placement| placement.id.clone())
        .collect::<BTreeSet<_>>();
    let observation = BrowserSessionObservation {
        scene_objects_revision: Default::default(),
        draft_available: true,
        draft_scene_id: Some("MAP-budget".to_owned()),
        source_placement_count: placements.len(),
        placement_structure_revision: 7,
    };
    let mut build = BrowserSignatureBuild::new(observation);

    assert!(!advance_signature_build(&mut build, &placements, &expanded,));
    assert_eq!(
        build.next_placement,
        MAX_GLTF_SIGNATURE_PLACEMENTS_PER_FRAME
    );
    assert_eq!(
        build.signature.len(),
        MAX_GLTF_SIGNATURE_PLACEMENTS_PER_FRAME
    ); // 第一帧不复制8192项。

    let mut frames = 1usize;
    while !advance_signature_build(&mut build, &placements, &expanded) {
        frames += 1;
    }
    frames += 1; // 最后一次返回true的推进同样占一个Update预算。

    assert_eq!(frames, 32); // 8192 / 256严格分成32帧。
    assert_eq!(build.signature.len(), MAX_GLTF_BROWSER_PLACEMENTS);
    assert_eq!(
        build.asset_demand_counts["gltf/shared.glb"],
        placements.len()
    ); // 共享路径只保留一个强句柄需求键。
    assert_eq!(build.wanted_asset_paths.len(), 1);
    assert_eq!(
        build.placement_record_indices["instance-8191"],
        MAX_GLTF_BROWSER_PLACEMENTS - 1,
    ); // 最后一项也由分帧构建直接记录下标，后续数值拖动无需线性查找。
}

#[test]
fn duplicate_id_removes_expansion_and_all_provisional_asset_demand() {
    let placements = vec![
        placement("duplicate", "gltf/first.glb"),
        placement("duplicate", "gltf/second.glb"),
    ];
    let expanded = BTreeSet::from(["duplicate".to_owned()]);
    let mut build = BrowserSignatureBuild::new(BrowserSessionObservation {
        scene_objects_revision: Default::default(),
        draft_available: true,
        draft_scene_id: Some("MAP-duplicate".to_owned()),
        source_placement_count: placements.len(),
        placement_structure_revision: 1,
    });

    assert!(advance_signature_build(&mut build, &placements, &expanded,));

    assert!(build.duplicate_ids.contains("duplicate"));
    assert!(!build.placement_asset_paths.contains_key("duplicate"));
    assert!(!build.placement_record_indices.contains_key("duplicate")); // 重复ID不能获得任意一条写回下标。
    assert!(!build.expanded_live.contains("duplicate"));
    assert!(build.asset_demand_counts.is_empty());
    assert!(build.wanted_asset_paths.is_empty());
}

#[test]
fn primitive_target_never_restores_scene_expansion_or_asset_demand() {
    let mut primitive = placement("leaf", "gltf/plant.glb");
    primitive.asset_target = ScenePlacementAssetTarget::Primitive {
        mesh: 6,
        primitive: 0,
        material: Some(2),
    };
    let expanded = BTreeSet::from([primitive.id.clone()]); // 模拟该ID上一代曾经还是展开的Scene0。
    let mut build = BrowserSignatureBuild::new(BrowserSessionObservation {
        scene_objects_revision: Default::default(),
        draft_available: true,
        draft_scene_id: Some("MAP-primitive-browser".to_owned()),
        source_placement_count: 1,
        placement_structure_revision: 2,
    });

    assert!(advance_signature_build(&mut build, &[primitive], &expanded));
    assert!(!build.expanded_live.contains("leaf"));
    assert!(build.asset_demand_counts.is_empty());
    assert!(build.wanted_asset_paths.is_empty());
    assert_eq!(
        build.signature[0].asset_target,
        ScenePlacementAssetTarget::Primitive {
            mesh: 6,
            primitive: 0,
            material: Some(2),
        },
    ); // 子资产仍进入根和作者下标索引，但绝不为同路径加载完整Scene0树。
}

#[test]
fn records_after_browser_limit_are_not_read_into_signature() {
    let placements = (0..=MAX_GLTF_BROWSER_PLACEMENTS)
        .map(|index| placement(format!("instance-{index}"), format!("gltf/{index}.glb")))
        .collect::<Vec<_>>();
    let mut build = BrowserSignatureBuild::new(BrowserSessionObservation {
        scene_objects_revision: Default::default(),
        draft_available: true,
        draft_scene_id: Some("MAP-limit".to_owned()),
        source_placement_count: placements.len(),
        placement_structure_revision: 1,
    });
    let expanded = BTreeSet::new();

    while !advance_signature_build(&mut build, &placements, &expanded) {}

    assert_eq!(build.next_placement, MAX_GLTF_BROWSER_PLACEMENTS);
    assert_eq!(build.signature.len(), MAX_GLTF_BROWSER_PLACEMENTS);
    assert_eq!(
        build.signature.last().unwrap().placement_id,
        "instance-8191"
    );
}

#[test]
fn one_record_water_bindings_obey_path_segment_budget() {
    let mut record = placement("deep-water", "gltf/deep-water.glb");
    record.water_surfaces = (0..MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_PLACEMENT)
        .map(|index| {
            water_surface(
                index as u32,
                (0..64).map(|segment| format!("节点{segment}")).collect(),
                format!("水面{index}"),
            )
        })
        .collect();
    assert!(record.validate_water_surfaces().is_ok()); // 回归样本本身位于公共正式作者硬门内。
    let placements = vec![record];
    let mut build = BrowserSignatureBuild::new(BrowserSessionObservation {
        scene_objects_revision: Default::default(),
        draft_available: true,
        draft_scene_id: Some("MAP-deep-water".to_owned()),
        source_placement_count: 1,
        placement_structure_revision: 1,
    });

    assert!(!advance_signature_build(
        &mut build,
        &placements,
        &BTreeSet::new(),
    ));
    assert_eq!(build.next_placement, 0); // 单条记录尚未完整，不能提前进入活动签名前缀。
    assert_eq!(
        build
            .pending_signature
            .as_ref()
            .expect("深绑定记录应保留跨帧草稿")
            .water_bindings
            .len(),
        MAX_GLTF_SIGNATURE_PATH_SEGMENTS_PER_FRAME / 64,
    ); // 64层路径每帧只复制固定512段。

    let mut frames = 1usize;
    while !advance_signature_build(&mut build, &placements, &BTreeSet::new()) {
        frames += 1;
    }
    assert!(frames > 1);
    assert_eq!(build.signature[0].water_bindings.len(), 64);
}

#[test]
fn one_record_water_bindings_obey_string_byte_budget() {
    let mut record = placement("long-water", "gltf/long-water.glb");
    let long_segment = "水".repeat(256); // 16段合计4096个Unicode字符，精确位于公共合法上限。
    record.water_surfaces = (0..MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_PLACEMENT)
        .map(|index| {
            water_surface(
                index as u32,
                vec![long_segment.clone(); 16],
                format!("{}{:02}", "网".repeat(254), index), // 256字符内保持每项稳定身份唯一。
            )
        })
        .collect();
    assert!(record.validate_water_surfaces().is_ok()); // 字符、路径深度和唯一性都属于正式合法输入。
    let placements = vec![record];
    let mut build = BrowserSignatureBuild::new(BrowserSessionObservation {
        scene_objects_revision: Default::default(),
        draft_available: true,
        draft_scene_id: Some("MAP-long-water".to_owned()),
        source_placement_count: 1,
        placement_structure_revision: 1,
    });

    assert!(!advance_signature_build(
        &mut build,
        &placements,
        &BTreeSet::new(),
    ));
    let copied = build
        .pending_signature
        .as_ref()
        .expect("长字符串记录应保留跨帧草稿")
        .water_bindings
        .len();
    assert!(copied > 0); // 新鲜帧能推进合法单项，不会永久饿死。
    assert!(copied < MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_PLACEMENT); // 128KiB额度阻止一帧复制全部深字符串。
}

#[test]
fn cancelled_partial_water_signature_retires_over_multiple_frames() {
    let bindings = (0..MAX_SCENE_PLACEMENT_WATER_SURFACES_PER_PLACEMENT)
        .map(|index| {
            PlacementWaterBindingSignature::from(&water_surface(
                index as u32,
                (0..64).map(|segment| format!("节点{segment}")).collect(),
                format!("水面{index}"),
            ))
        })
        .collect();
    let mut build = BrowserSignatureBuild::new(BrowserSessionObservation::default());
    build.pending_signature = Some(PlacementSignature {
        editor: Default::default(),
        placement_id: "partial".to_owned(),
        asset_path: "gltf/partial.glb".to_owned(),
        asset_target: Default::default(), // 深绑定退役夹具仍代表Scene0。
        water_bindings: bindings,
    }); // 模拟结构热更恰好发生在单记录绑定复制途中。
    let mut generation = BrowserRetiredGeneration::default();
    generation.signature_build(Some(build));
    let mut retirement = BrowserRetirement::default();
    retirement.enqueue_normal_reserved(generation);

    retirement.advance_frame();
    assert!(!retirement.is_empty()); // 64项深路径受共享512段退役预算约束，取消帧不能整批Drop。
    while !retirement.is_empty() {
        retirement.advance_frame();
    }
}

#[test]
fn completed_signature_auxiliary_index_retires_across_frames() {
    let mut build = BrowserSignatureBuild::new(BrowserSessionObservation::default());
    build.seen_ids = (0..MAX_GLTF_BROWSER_PLACEMENTS)
        .map(|index| format!("completed-instance-{index}"))
        .collect(); // 最终签名发布后仍留在草稿内的唯一辅助索引。
    let mut generation = BrowserRetiredGeneration::default();
    generation.signature_build(Some(build));
    let mut retirement = BrowserRetirement::default();
    retirement.enqueue_normal_reserved(generation);

    retirement.advance_frame();
    assert!(!retirement.is_empty()); // 8192个seen_ids不能随成功发布在一个Update内全Drop。
    let mut frames = 1usize;
    while !retirement.is_empty() {
        retirement.advance_frame();
        frames += 1;
    }

    assert!(frames > 1); // 固定元素/字节预算逐帧清空完成草稿壳。
}
