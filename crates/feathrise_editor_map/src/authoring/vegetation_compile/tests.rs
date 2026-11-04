//! 植被作者图层纯编译回归。

use std::collections::BTreeMap; // 测试密度按稳定作者层ID提供。
use std::sync::Arc; // 与正式后台任务相同地共享R8输入。

use feathrise_map_content::{
    MapId, MapVegetationAsset, MapVegetationAuthorGroup, MapVegetationAuthorGroupMode,
    MapVegetationAuthorLayer, MapVegetationAuthoring, MapVegetationKind, MapVegetationLayer,
    MapVegetationOutputMergeMode,
}; // 构造南北两组与两个固定正式profile。

use super::compiler::compile_vegetation_authoring; // 直接验证纯函数，不启动异步池。

#[test]
fn compiling_while_held_accepts_complete_pixels_but_source_change_or_undo_rejects_late_results() {
    use bevy::{prelude::*, ecs::system::RunSystemOnce};
    use crate::{authoring::{EditorAuthoringSession, VegetationBrushStroke, test_heightfield_draft}, resources::MapEditorState};
    use super::{VegetationCompileState, start_vegetation_compile, poll_vegetation_compile};
    let _app = App::new().add_plugins(MinimalPlugins);
    for case in 0..3 {
        let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.vegetation = Some(MapVegetationAsset { schema_version: 1, map_id: draft.map_id.clone(),
            layers: vec![output("grass")], authoring: Some(MapVegetationAuthoring {
                groups: vec![group("base", MapVegetationAuthorGroupMode::Additive)],
                layers: vec![layer("source", "base", "grass", 0)],
            }) });
        draft.vegetation_author_densities.insert("source".into(), Arc::new(vec![33; 16]));
        let mut world = World::new();
        world.insert_resource(EditorAuthoringSession { draft: Some(draft), vegetation_revision: 1, ..default() });
        world.insert_resource(MapEditorState::default());
        world.insert_resource(VegetationBrushStroke { active: true, ..default() });
        world.insert_resource(VegetationCompileState::default());
        world.run_system_once(start_vegetation_compile).unwrap();
        assert!(world.resource::<VegetationCompileState>().task.is_some(), "按住也必须提交单飞编译");
        {
            let mut session = world.resource_mut::<EditorAuthoringSession>();
            session.vegetation_revision = 2;
            session.draft.as_mut().unwrap().vegetation_author_densities.insert("source".into(), Arc::new(vec![200; 16]));
            if case == 1 { session.vegetation_structure_revision += 1; }
        }
        if case == 2 { world.resource_mut::<VegetationBrushStroke>().active = false; } // 松笔后的撤销不允许采用旧像素。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while world.resource::<VegetationCompileState>().task.is_some() {
            world.run_system_once(poll_vegetation_compile).unwrap();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let session = world.resource::<EditorAuthoringSession>();
        assert_eq!(session.vegetation_compiled_revision, u64::from(case == 0));
        if case == 0 { assert_eq!(session.draft.as_ref().unwrap().vegetation_densities["grass"].as_slice(), &[33; 16]); }
        assert_eq!(session.draft.as_ref().unwrap().vegetation_author_densities["source"].as_slice(), &[200; 16], "正式预览不得回写当前作者像素");
    }
}

#[test] // 重叠候选必须排除被组覆盖的作者层，并与正式池化结果一致。
fn picking_respects_priority_cross_group_replace_and_resample() {
    let mut vegetation = MapVegetationAsset {
        schema_version: 1,
        map_id: MapId::parse("MAP-test").unwrap(),
        layers: vec![output("grass")],
        authoring: Some(MapVegetationAuthoring {
            groups: vec![group(
                "base",
                MapVegetationAuthorGroupMode::PriorityOverride,
            )],
            layers: vec![
                layer("low", "base", "grass", 0),
                layer("high", "base", "grass", 1),
            ],
        }),
    }; // 两张同预设图必须保持独立身份。
    let mut densities = BTreeMap::from([
        ("low".into(), Arc::new(vec![255; 16])),
        ("high".into(), Arc::new(vec![255; 16])),
    ]); // 完全重叠。
    let pick = |v: &MapVegetationAsset, d: &BTreeMap<String, Arc<Vec<u8>>>| {
        super::author_layers_at_world(v, d, [0.0, 0.0], [4.0, 4.0], bevy::prelude::Vec2::ZERO)
    }; // 精确首纹素。
    assert_eq!(pick(&vegetation, &densities), vec!["high"]); // 只命中优先层。
    let authoring = vegetation.authoring.as_mut().unwrap(); // 增加跨组局部覆盖。
    let mut patch = group("patch", MapVegetationAuthorGroupMode::Additive);
    patch.output_merge = MapVegetationOutputMergeMode::ReplaceNonZero; // 后组替换。
    authoring.groups.push(patch);
    authoring.layers.push(layer("fine", "patch", "grass", 0)); // 独立画布。
    let mut pixels = vec![0; 16];
    pixels[5] = 173;
    densities.insert("fine".into(), Arc::new(pixels)); // 来源点不在单点最近采样位置。
    vegetation.layers[0].resolution = [2, 2]; // 正式最大池化必须保留细画布来源。
    assert_eq!(pick(&vegetation, &densities), vec!["fine"]); // 不能误返回被覆盖的前组。
    vegetation.authoring.as_mut().unwrap().layers[2].enabled = false; // 隐藏修补层。
    assert_eq!(pick(&vegetation, &densities), vec!["high"]); // 回到真实可见原层。
}

fn output(id: &str) -> MapVegetationLayer {
    MapVegetationLayer {
        id: id.to_owned(),                                               // 稳定正式输出ID。
        label: id.to_owned(),                                            // 测试不依赖显示名。
        kind: MapVegetationKind::Blade, // 两个输出均走现有Blade后端。
        runtime_profile: Some(format!("profile-{id}")), // 保持正式profile身份。
        runtime_layer_id: Some(if id == "grass" { 1 } else { 2 }), // 两个不同GPU层身份。
        prototype_id: None,             // Blade无需Mesh原型。
        asset_path: format!("{id}.glb"), // 来源说明不参与编译。
        density_source_path: format!("maps/test/vegetation/{id}.r8"), // 正式输出作者源。
        density_texture_path: format!("maps/test/vegetation/{id}.ktx2"), // 正式运行时KTX2。
        resolution: [4, 4],             // 小型逐像素断言。
        spacing_meters: 1.0,            // 运行时规格不由作者编译器改变。
        scale_range: [1.0, 1.0],        // 保持合法规格。
        max_instances: 16,              // 保持合法规格。
        seed: 1,                        // 保持稳定规格。
    }
}

fn layer(id: &str, group: &str, output: &str, priority: i32) -> MapVegetationAuthorLayer {
    MapVegetationAuthorLayer {
        id: id.to_owned(),                  // 独立作者画布身份。
        label: id.to_owned(),               // 测试显示名。
        group_id: group.to_owned(),         // 北部或南部规则组。
        output_layer_id: output.to_owned(), // 草或麦正式输出。
        density_source_path: format!("maps/test/vegetation/author/{id}.r8"), // 唯一作者路径。
        resolution: [4, 4],                 // 与测试正式输出一致。
        enabled: true,                      // 参与编译。
        locked: false,                      // 锁定不影响编译但保持普通作者状态。
        opacity: 1.0,                       // 完整密度。
        priority,                           // 北麦高于北草，南部同级。
        weight: 1.0,                        // 南部一比一共享。
    }
}

fn group(id: &str, mode: MapVegetationAuthorGroupMode) -> MapVegetationAuthorGroup {
    MapVegetationAuthorGroup {
        id: id.to_owned(),                                   // 稳定作者组ID。
        label: id.to_owned(),                                // 测试显示名。
        mode,                                                // 当前断言的组内规则。
        output_merge: MapVegetationOutputMergeMode::Maximum, // 南北结果按像素取最大。
        capacity: 1.0,                                       // 南部草麦共同共享一份完整容量。
        claim_threshold: 0.0,                                // 北部任意非零麦密度即可覆盖。
        selection_seed: 9,                                   // 同级选择稳定种子。
        selection_cell_meters: 4.0,                          // 小地图仍有稳定世界格。
        enabled: true,                                       // 参与编译。
    }
}

#[test]
fn deleting_every_author_layer_publishes_zero_for_every_formal_output() {
    let vegetation = MapVegetationAsset {
        schema_version: 1, // 删除作者层不会删除固定正式profile。
        map_id: MapId::parse("MAP-test").expect("测试地图ID有效"), // 稳定地图身份。
        layers: vec![output("grass"), output("wheat")], // 运行时输出集合保持不变。
        authoring: Some(MapVegetationAuthoring {
            groups: vec![group("ground", MapVegetationAuthorGroupMode::Additive)], // 空组允许作者稍后继续添加图层。
            layers: Vec::new(), // 精确模拟删除最后一张作者图后的结构。
        }),
    };
    let result = compile_vegetation_authoring(
        &vegetation,
        &BTreeMap::new(), // 最后一张作者图的内存画布也已经原子移除。
        [-2.0, -2.0],
        [2.0, 2.0],
    )
    .expect("空作者图集仍应成功编译固定正式输出");

    assert_eq!(result.outputs.len(), 2); // 不因作者图为空而删除正式LOD/风场身份。
    assert!(result.nonzero_output_ids.is_empty()); // 主线程可直接走零输出清理路径。
    assert!(result.output_stats.values().all(|stats| {
        stats.total_pixels == 16
            && stats.nonzero_pixels == 0
            && stats.density_sum == 0
            && stats.min_nonzero.is_none()
            && stats.max_value == 0
    })); // F4所需统计必须与同次全零正式输出一起发布。
    assert!(result.conflicts.is_empty()); // 空组不产生无意义整图诊断。
    assert!(
        result
            .outputs
            .values()
            .all(|pixels| { pixels.len() == 16 && pixels.iter().all(|value| *value == 0) })
    ); // 每个固定正式输出均以同一次完成修订归零。
}

#[test]
fn north_can_override_while_south_shares_one_to_one_without_new_runtime_layers() {
    let vegetation = MapVegetationAsset {
        schema_version: 1, // 兼容新增可选作者字段的当前格式。
        map_id: MapId::parse("MAP-test").expect("测试地图ID有效"), // 稳定地图身份。
        layers: vec![output("grass"), output("wheat")], // 运行时始终只有两个正式profile。
        authoring: Some(MapVegetationAuthoring {
            groups: vec![
                group("north", MapVegetationAuthorGroupMode::PriorityOverride), // 北部麦覆盖草。
                group("south", MapVegetationAuthorGroupMode::WeightedPartition), // 南部草麦共享。
            ],
            layers: vec![
                layer("north-grass", "north", "grass", 0), // 北草低优先级。
                layer("north-wheat", "north", "wheat", 10), // 北麦高优先级。
                layer("south-grass", "south", "grass", 0), // 南草同级一份。
                layer("south-wheat", "south", "wheat", 0), // 南麦同级一份。
            ],
        }),
    };
    let north = [255_u8; 8].into_iter().chain([0_u8; 8]).collect::<Vec<_>>(); // 上半图满密度、下半图为空。
    let south = [0_u8; 8].into_iter().chain([255_u8; 8]).collect::<Vec<_>>(); // 下半图满密度、上半图为空。
    let densities = BTreeMap::from([
        ("north-grass".to_owned(), Arc::new(north.clone())), // 北草与北麦完全重叠。
        ("north-wheat".to_owned(), Arc::new(north)),         // 高优先级麦取得北部所有权。
        ("south-grass".to_owned(), Arc::new(south.clone())), // 南草参与一比一共享。
        ("south-wheat".to_owned(), Arc::new(south)),         // 南麦参与一比一共享。
    ]);
    let result = compile_vegetation_authoring(&vegetation, &densities, [-2.0, -2.0], [2.0, 2.0])
        .expect("南北组应完整编译");
    assert_eq!(result.outputs.len(), 2); // 四张作者图仍只输出原有两张正式图。
    let grass = result.outputs.get("grass").expect("草正式输出存在");
    let wheat = result.outputs.get("wheat").expect("麦正式输出存在");
    assert!(grass[..8].iter().all(|value| *value == 0)); // 北草被高优先级北麦覆盖。
    assert!(wheat[..8].iter().all(|value| *value == 255)); // 北麦完整保留自身密度。
    assert!(grass[8..].iter().all(|value| *value == 128)); // 南草获得约一半容量。
    assert!(wheat[8..].iter().all(|value| *value == 128)); // 南麦获得同样一半容量。
    let grass_stats = result.output_stats.get("grass").expect("草统计存在"); // F4统计与正式草输出同批发布。
    assert_eq!(grass_stats.total_pixels, 16); // 正式四乘四输出共有十六个像素。
    assert_eq!(grass_stats.nonzero_pixels, 8); // 只有南部八像素保留草密度。
    assert_eq!(grass_stats.density_sum, 1_024); // 八个R8=128像素的精确和。
    assert_eq!(grass_stats.min_nonzero, Some(128)); // 非零区域只有单一密度值。
    assert_eq!(grass_stats.max_value, 128); // 最大值必须来自正式编译结果而非作者输入255。
    let wheat_stats = result.output_stats.get("wheat").expect("麦统计存在"); // 第二个输出拥有独立归约。
    assert_eq!(wheat_stats.total_pixels, 16); // 同分辨率保持相同总像素数。
    assert_eq!(wheat_stats.nonzero_pixels, 16); // 北部满密度和南部半密度均为非零。
    assert_eq!(wheat_stats.density_sum, 3_064); // 八个255与八个128的精确总和。
    assert_eq!(wheat_stats.min_nonzero, Some(128)); // 南部共享值形成非零下界。
    assert_eq!(wheat_stats.max_value, 255); // 北部覆盖层形成正式上界。
    assert!(
        result.conflicts["north"]
            .pixels
            .iter()
            .take(8)
            .all(|value| *value == 255)
    ); // 冲突预览明确显示北部重叠作者源。
}

#[test]
fn maximum_resample_preserves_a_single_high_resolution_author_pixel() {
    let mut formal = output("grass"); // 从普通四乘四正式层开始。
    formal.resolution = [2, 2]; // 模拟运行时输出低于作者图分辨率。
    let vegetation = MapVegetationAsset {
        schema_version: 1,                                         // 当前兼容格式。
        map_id: MapId::parse("MAP-test").expect("测试地图ID有效"), // 稳定地图身份。
        layers: vec![formal],                                      // 唯一正式草层。
        authoring: Some(MapVegetationAuthoring {
            groups: vec![group("base", MapVegetationAuthorGroupMode::Additive)], // 单层共存组。
            layers: vec![layer("fine", "base", "grass", 0)],                     // 四乘四作者图。
        }),
    };
    let mut pixels = vec![0_u8; 16]; // 其余作者像素为空。
    pixels[5] = 173; // 唯一细笔刷纹素位于首个目标分区。
    let result = compile_vegetation_authoring(
        &vegetation,
        &BTreeMap::from([("fine".to_owned(), Arc::new(pixels))]),
        [0.0, 0.0],
        [4.0, 4.0],
    )
    .expect("不同分辨率应按完整分区最大值编译");
    assert_eq!(result.outputs["grass"].as_slice(), &[173, 0, 0, 0]); // 孤立纹素没有被单点抽样吞掉。
}
