//! 植被密度覆盖图纯函数回归。

use super::*; // 子模块可直接验证生产降采样与综合色函数。
use feathrise_map_content::MapVegetationKind; // 构造最小正式Blade层颜色身份。

#[test]
fn late_density_task_cannot_adopt_into_another_terrain_with_the_same_map_layer_and_revision() {
    use crate::authoring::terrain_documents::TerrainDocumentIdentity;
    use bevy::ecs::system::RunSystemOnce;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&draft.map_id, &draft.folder);
    water.layers[0].resolution = [2, 2];
    draft
        .water_masks
        .insert(water.layers[0].id.clone(), Arc::new(vec![255; 4]));
    draft.water = Some(water);
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("overlay-a").unwrap();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(object.clone()),
        manifest: draft.manifest.clone(),
        folder: draft.folder.clone(),
    });
    app.insert_resource(MapEditorState {
        open: true,
        tool: EditorTool::Water,
        terrain_canvas_target: Some(object.id.clone()),
        selected_map_id: Some(draft.map_id.clone()),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .init_resource::<LayerCandidateState>()
    .init_resource::<VegetationBrushStroke>()
    .init_resource::<WaterBrushStroke>()
    .init_resource::<VegetationDensityOverlayState>()
    .init_resource::<Assets<Image>>()
    .init_resource::<Assets<StandardMaterial>>();
    app.update();
    app.world_mut()
        .run_system_once(start_density_overlay_rebuild)
        .unwrap();
    assert!(
        app.world()
            .resource::<VegetationDensityOverlayState>()
            .task
            .is_some()
    );
    object.id = TerrainId::parse("overlay-b").unwrap();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .terrain_canvas_target = Some(object.id.clone());
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .terrain_documents
        .active
        .as_mut()
        .unwrap()
        .definition = Arc::new(object);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app
        .world()
        .resource::<VegetationDensityOverlayState>()
        .task
        .is_some()
    {
        app.world_mut()
            .run_system_once(poll_density_overlay_rebuild)
            .unwrap();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        app.world()
            .resource::<VegetationDensityOverlayState>()
            .image
            .is_none()
    );
    assert!(app.world().resource::<Assets<Image>>().is_empty());
}

fn layer() -> MapVegetationLayer {
    MapVegetationLayer {
        id: "grass".to_owned(),                                  // 稳定正式层ID。
        label: "草".to_owned(),                                  // 测试显示名。
        kind: MapVegetationKind::Blade, // 颜色映射使用正式profile而非作者层类别。
        runtime_profile: Some("hn01_ordinary_grass".to_owned()), // 普通草综合色。
        runtime_layer_id: Some(1),      // 完整正式身份。
        prototype_id: None,             // Blade无需Mesh原型。
        asset_path: "grass.glb".to_owned(), // 颜色测试不读取模型。
        density_source_path: "grass.png".to_owned(), // 颜色测试不读取文件。
        density_texture_path: "grass.png".to_owned(), // 颜色测试不读取文件。
        resolution: [2, 2],             // 四像素输入。
        spacing_meters: 1.0,            // 完整正式规格。
        scale_range: [1.0, 1.0],        // 完整正式规格。
        max_instances: 4,               // 完整正式规格。
        seed: 1,                        // 完整正式规格。
    }
}

#[test]
fn custom_profile_color_is_stable_across_layer_rename_and_kind() {
    let mut first = layer();
    first.runtime_profile = Some("project_meadow".into());
    let color = layer_color(&first);
    first.id = "renamed-output".into();
    first.label = "工程显示名".into();
    first.kind = MapVegetationKind::Mesh;
    assert_eq!(layer_color(&first), color);
    first.runtime_profile = None;
    let unbound = layer_color(&first);
    first.label = "另一个显示名".into();
    assert_eq!(layer_color(&first), unbound);
}

#[test]
fn overlay_keeps_zero_transparent_and_nonzero_visible() {
    let pixels = build_overlay(&[0, 1, 128, 255], [2, 2], [2, 2], layer_color(&layer())).unwrap(); // 四个强度逐值生成RGBA。
    assert_eq!(pixels[3], 0); // 零密度必须完全透明。
    assert!(pixels[7] > 0); // 最小非零密度也留下可见提示。
    assert!(pixels[15] > pixels[11]); // 更高密度具有更高透明度。
}

#[test]
fn large_density_uses_fixed_overlay_budget() {
    assert_eq!(overlay_resolution([2048, 2048]), [512, 512]); // HN-01大图不生成第二张2048² RGBA覆盖。
    assert_eq!(overlay_resolution([1024, 512]), [512, 256]); // 非方形输入保持宽高比。
}

#[test]
fn one_source_pixel_cannot_disappear_when_overlay_is_downsampled() {
    let mut density = vec![0; 8 * 8]; // 构造八乘八作者R8。
    density[3 * 8 + 6] = 173; // 放在旧单点抽样可能跳过的分区内部。
    let pixels = build_overlay(&density, [8, 8], [2, 2], [10, 20, 30]).unwrap(); // 汇聚为二乘二覆盖。
    assert_eq!(pixels[(1 * 4) + 3], rgba_for_density(173, [10, 20, 30])[3]); // 完整源分区最大值保留孤立纹素。
}

#[test] // 选择作者层必须区分共享正式输出，悬停不写侧栏状态。
fn inspect_overlay_uses_author_source_and_restores_after_hover() {
    use feathrise_map_content::{MapVegetationAsset, MapVegetationAuthorLayer, MapVegetationAuthoring}; // 最小独立画布夹具。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 同地图高度场。
    let authors = ["first", "second"].map(|id| MapVegetationAuthorLayer {
        // 同一草预设的两个作者层。
        id: id.into(),                   // 精确作者身份。
        label: id.into(),                // 显示名不参与解析。
        group_id: "group".into(),        // 源图选择无需扫描组规则。
        output_layer_id: "grass".into(), // 共享正式输出。
        density_source_path: id.into(),  // 不执行文件IO。
        resolution: [2, 2],              // 固定小输入。
        enabled: true,                   // 可参与点选。
        locked: false,                   // 锁定与否不改变高亮语义。
        opacity: 1.0,                    // 完整作者强度。
        priority: 0,                     // 非编译测试。
        weight: 1.0,                     // 非编译测试。
    }); // 两份作者密度必须各自解析。
    draft.vegetation = Some(MapVegetationAsset {
        // 独立作者图夹具。
        schema_version: 1,            // 当前格式。
        map_id: draft.map_id.clone(), // 与编辑器身份一致。
        layers: vec![layer()],        // 同一正式输出。
        authoring: Some(MapVegetationAuthoring {
            groups: vec![],
            layers: authors.into(),
        }), // 不通过合并图解析。
    }); // 解析只需常数作者索引。
    draft
        .vegetation_author_densities
        .insert("first".into(), Arc::new(vec![1; 4])); // 第一作者源。
    draft
        .vegetation_author_densities
        .insert("second".into(), Arc::new(vec![2; 4])); // 第二作者源。
    draft
        .vegetation_densities
        .insert("grass".into(), Arc::new(vec![255; 4])); // 正式合并结果不能用于选层。
    let editor = MapEditorState {
        open: true,
        tool: EditorTool::Inspect,
        selected_map_id: Some(draft.map_id.clone()),
        vegetation_preview_mode: VegetationPreviewMode::Compiled,
        ..default()
    }; // 进入选择前允许处于最终图模式。
    let session = EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    }; // 共享不可变作者快照。
    let mut candidates = LayerCandidateState::default(); // 真实生产状态。
    candidates.selected = Some(LayerTarget::Vegetation("first".into())); // 正式第一层。
    candidates.selected_author_index = Some(0); // 后台常数索引。
    candidates.hover = Some(LayerTarget::Vegetation("second".into())); // 临时第二层。
    candidates.hover_author_index = Some(1); // 精确作者位置。
    assert_eq!(
        selected_overlay(&editor, &session, &candidates)
            .unwrap()
            .density
            .as_slice(),
        &[2; 4]
    ); // 不得使用255的正式输出。
    assert_eq!(overlay_mode(&editor), VegetationPreviewMode::Source); // 任务身份强制原图。
    candidates.hover = None; // 移开恢复。
    assert_eq!(
        selected_overlay(&editor, &session, &candidates)
            .unwrap()
            .density
            .as_slice(),
        &[1; 4]
    ); // 回到正式作者层。
    assert_eq!(
        editor.vegetation_preview_mode,
        VegetationPreviewMode::Compiled
    ); // 不篡改编辑模式参数。
    candidates.hover = Some(LayerTarget::Surface("ground".into())); // 跨类候选。
    assert!(selected_overlay(&editor, &session, &candidates).is_none()); // 不保留旧植被覆盖。
}
