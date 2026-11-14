//! 无窗口真实同步系统回归：新建层不读取不存在的磁盘KTX2，关闭后仍完整发布。

use feathrise_editor_ui::UiInteraction;
use super::*; // 生产快照类型与任务状态。
use crate::authoring::live_density::{LiveBladeDensityState, sync_live_blade_density}; // 测试实际调度入口。
use crate::resources::SceneEditorState;
use bevy::asset::{AssetApp, AssetPlugin}; // 真实资产注册服务，用于断言没有文件加载请求。
use feathrise_scene_content::{
    ActiveDynamicScene, SCENE_SCHEMA_VERSION, SceneVegetationKind, SceneVegetationLayer,
}; // 与正式序列化一致的定义。 // 默认关闭，复现刷完立即返回游戏。

fn setup() -> (App, SceneId) {
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121])); // 小型真实动态场景草稿。
    let scene_id = draft.scene_id.clone(); // 所有完成值绑定同一场景。
    let layer = SceneVegetationLayer {
        id: "new-grass".to_owned(),     // 原磁盘不存在的新层。
        label: "新草地".to_owned(),     // 作者可读名称。
        kind: SceneVegetationKind::Blade, // 走GPU四级路径。
        runtime_profile: Some("hn01_ordinary_grass".to_owned()), // 正式预设身份。
        runtime_layer_id: Some(1),      // 运行时稳定层。
        prototype_id: None,             // Blade没有Mesh原型。
        asset_path: "grass.glb".to_owned(), // 本测试不加载模型。
        density_source_path: "scenes/missing/new-grass.r8".to_owned(), // 明确没有文件。
        density_texture_path: "scenes/missing/new-grass.ktx2".to_owned(), // 旧代码错误请求的路径。
        resolution: [2, 2],             // 四个纹素检查逐字节采用。
        spacing_meters: 1.0,            // 有效层参数。
        scale_range: [1.0, 1.0],        // 无随机缩放。
        max_instances: 4,               // 测试旧兼容字段。
        seed: 1,                        // 固定随机身份。
    };
    draft.vegetation = Some(SceneVegetationAsset {
        model_surfaces: Vec::new().into(),
        schema_version: SCENE_SCHEMA_VERSION, // 当前格式。
        scene_id: scene_id.clone(),             // 与清单一致。
        layers: vec![layer],                // 首个草层。
        authoring: None,                    // 测试直接提供已完整编译结果。
    });
    draft.vegetation_densities.insert(
        "new-grass".to_owned(),
        std::sync::Arc::new(vec![0, 255, 127, 0]),
    ); // 非零刷印。
    let mut app = App::new(); // 无窗口即可验证作者与运行时输入交接。
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>(); // 后台池与真实资产容器。
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),              // 未保存草稿。
        vegetation_revision: 1,          // 最新笔划。
        vegetation_compiled_revision: 1, // 后台编译已完成。
        ..default()                      // 不启动保存。
    });
    app.insert_resource(ActiveDynamicScene {
        scene_id: Some(scene_id.clone()),
    }); // 当前玩法图。
    app.init_resource::<LiveBladeDensityState>()
        .init_resource::<DynamicBladeDraft>()
        .init_resource::<SceneEditorState>(); // 默认编辑器关闭。
    app.add_systems(Update, sync_live_blade_density); // 与生产相同入口而非调用测试替身。
    (app, scene_id) // 返回可继续刷制和切换场景的会话。
}

fn wait_for(app: &mut App, predicate: impl Fn(&World) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5); // 测试有界等待，不属于帧路径。
    loop {
        app.update(); // 真实非阻塞系统驱动后台完成和分帧登记。
        if predicate(app.world()) {
            return;
        } // 满足目标即结束。
        assert!(
            std::time::Instant::now() < deadline,
            "草稿未在期限内完整发布"
        ); // 防止死循环测试。
        std::thread::sleep(std::time::Duration::from_millis(1)); // 仅测试让后台worker执行。
    }
}

#[test]
fn new_blade_without_ktx2_publishes_after_editor_closed_and_updates_clearance_source() {
    let (mut app, scene_id) = setup(); // 关闭F12且磁盘无密度文件。
    wait_for(&mut app, |world| {
        world
            .resource::<DynamicBladeDraft>()
            .for_scene(&scene_id)
            .is_some()
    }); // 等待完整快照。
    let old_handle = app
        .world()
        .resource::<DynamicBladeDraft>()
        .for_scene(&scene_id)
        .unwrap()
        .densities["new-grass"]
        .clone(); // 冻结第一份不可变源。
    assert_eq!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&old_handle)
            .unwrap()
            .data
            .as_deref(),
        Some(&[0, 255, 127, 0][..])
    ); // 精确笔刷像素。
    assert!(
        app.world()
            .resource::<AssetServer>()
            .get_handle::<Image>("scenes/missing/new-grass.ktx2")
            .is_none()
    ); // 关键回归：完全没有请求不存在的路径。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>(); // 模拟下一笔清空/删除结果。
        session.vegetation_revision = 2; // 使上一建筑净空源失效。
        session.vegetation_compiled_revision = 2; // 完整零密度完成值。
        session
            .draft
            .as_mut()
            .unwrap()
            .vegetation_densities
            .insert("new-grass".to_owned(), std::sync::Arc::new(vec![0; 4])); // 清空。
    }
    wait_for(&mut app, |world| {
        world
            .resource::<DynamicBladeDraft>()
            .for_scene(&scene_id)
            .unwrap()
            .densities["new-grass"]
            .id()
            != old_handle.id()
    }); // 新修订新句柄。
    let snapshot = app
        .world()
        .resource::<DynamicBladeDraft>()
        .for_scene(&scene_id)
        .unwrap(); // 当前完整源。
    assert_eq!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&snapshot.densities["new-grass"])
            .unwrap()
            .data
            .as_deref(),
        Some(&[0; 4][..])
    ); // 清空没有保留旧草。
    assert_eq!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&old_handle)
            .unwrap()
            .data
            .as_deref(),
        Some(&[0, 255, 127, 0][..])
    ); // 旧源不原位修改，迟到净空任务不可混用。
}

#[test]
fn clipmap_mesh_density_publishes_with_blade_in_one_immutable_draft() {
    let (mut app, scene_id) = setup();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().expect("测试草稿存在");
        draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
        draft.heights = None; // Mesh分页运行时不能依赖GLB整图高度。
        let tree = SceneVegetationLayer {
            id: "new-tree".to_owned(),
            label: "新树木".to_owned(),
            kind: SceneVegetationKind::Mesh,
            runtime_profile: Some("hn01_tree".to_owned()),
            runtime_layer_id: Some(101),
            prototype_id: Some(201),
            asset_path: "tree.glb".to_owned(),
            density_source_path: "scenes/missing/new-tree.r8".to_owned(),
            density_texture_path: "scenes/missing/new-tree.ktx2".to_owned(),
            resolution: [2, 2],
            spacing_meters: 4.0,
            scale_range: [0.8, 1.2],
            max_instances: 4,
            seed: 9,
        };
        draft
            .vegetation
            .as_mut()
            .expect("测试植被定义存在")
            .layers
            .push(tree);
        draft.vegetation_densities.insert(
            "new-tree".to_owned(),
            std::sync::Arc::new(vec![255, 0, 0, 0]),
        );
        session.vegetation_revision = 2;
        session.vegetation_compiled_revision = 2;
    }
    wait_for(&mut app, |world| {
        world
            .resource::<DynamicBladeDraft>()
            .for_scene(&scene_id)
            .is_some_and(|snapshot| snapshot.source_revision == 2)
    });
    let snapshot = app
        .world()
        .resource::<DynamicBladeDraft>()
        .for_scene(&scene_id)
        .expect("完整植被草稿应发布");
    assert_eq!(snapshot.definition.layers.len(), 2); // Blade与Mesh不得互相覆盖。
    assert!(snapshot.densities.contains_key("new-grass"));
    assert!(snapshot.densities.contains_key("new-tree"));
    assert!(snapshot.sources.contains_key("new-tree"));
    assert!(snapshot.nonzero_layers.contains("new-tree")); // 运行时无需在帧线程扫描R8。
    assert!(
        app.world()
            .resource::<AssetServer>()
            .get_handle::<Image>("scenes/missing/new-tree.ktx2")
            .is_none()
    ); // 未保存Mesh层与Blade一样不请求不存在的磁盘派生文件。
}

#[test]
fn three_formal_mesh_presets_publish_for_clipmap_without_dense_height() {
    let catalog: feathrise_scene_content::VegetationPresetCatalogAsset =
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/vegetation_presets.vegetation_presets.json"
        )))
        .expect("正式植被预设目录应可解析");
    for preset_id in ["hn01_tree", "hn01_shrub", "hn01_rock"] {
        let preset = catalog
            .presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .expect("树木、灌木和岩石正式预设必须存在");
        let (mut app, scene_id) = setup();
        let output_id;
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().expect("测试草稿存在");
            draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
            draft.heights = None;
            let mut layer = preset.create_scene_layer(&draft.folder);
            assert_eq!(layer.kind, SceneVegetationKind::Mesh);
            assert!(layer.prototype_id.is_some());
            layer.resolution = [2, 2];
            output_id = layer.id.clone();
            draft.vegetation = Some(SceneVegetationAsset {
                model_surfaces: Vec::new().into(),
                schema_version: SCENE_SCHEMA_VERSION,
                scene_id: scene_id.clone(),
                layers: vec![layer],
                authoring: None,
            });
            draft.vegetation_densities.clear();
            draft
                .vegetation_densities
                .insert(output_id.clone(), std::sync::Arc::new(vec![255; 4]));
            session.vegetation_revision = 2;
            session.vegetation_compiled_revision = 2;
        }
        wait_for(&mut app, |world| {
            world
                .resource::<DynamicBladeDraft>()
                .for_scene(&scene_id)
                .is_some_and(|snapshot| snapshot.source_revision == 2)
        });
        let snapshot = app
            .world()
            .resource::<DynamicBladeDraft>()
            .for_scene(&scene_id)
            .expect("Mesh草稿应完整发布");
        assert_eq!(snapshot.definition.layers[0].kind, SceneVegetationKind::Mesh);
        assert_eq!(
            snapshot.definition.layers[0].runtime_profile.as_deref(),
            Some(preset_id)
        );
        assert!(snapshot.nonzero_layers.contains(&output_id));
    }
}

#[test]
fn switching_scenes_rejects_in_flight_blade_draft() {
    let (mut app, old_scene) = setup(); // 启动真实后台任务。
    app.update(); // 捕获旧场景完成身份。
    app.world_mut().resource_mut::<ActiveDynamicScene>().scene_id =
        Some(SceneId::parse("MAP-other").unwrap()); // 在完成前切换场景。
    wait_for(&mut app, |world| {
        world
            .resource::<LiveBladeDensityState>()
            .dynamic
            .task
            .is_none()
    }); // 等待唯一旧槽归还。
    assert!(
        app.world()
            .resource::<DynamicBladeDraft>()
            .for_scene(&old_scene)
            .is_none()
    ); // 旧结果未发布。
}

#[test]
fn unfinished_compile_waits_then_empty_layer_list_and_undo_publish_completely() {
    let (mut app, scene_id) = setup(); // 模拟刷完立即关闭，而编译尚未完成。
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .vegetation_compiled_revision = 0; // 尚无当前完成值。
    app.update(); // 不能把旧像素当作这次笔划。
    assert!(
        app.world()
            .resource::<DynamicBladeDraft>()
            .for_scene(&scene_id)
            .is_none()
    ); // 没有半套发布。
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .vegetation_compiled_revision = 1; // 模拟后台完成。
    wait_for(&mut app, |world| {
        world
            .resource::<DynamicBladeDraft>()
            .for_scene(&scene_id)
            .is_some()
    }); // 关闭状态仍继续采用。
    let original = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .vegetation
        .clone(); // 撤销保存完整层定义。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>(); // 模拟删除最后输出。
        session
            .draft
            .as_mut()
            .unwrap()
            .vegetation
            .as_mut()
            .unwrap()
            .layers
            .clear(); // 合法空定义不能回退到旧磁盘层。
        session.vegetation_revision = 2; // 新身份。
        session.vegetation_compiled_revision = 2; // 已完成空编译。
    }
    wait_for(&mut app, |world| {
        world
            .resource::<DynamicBladeDraft>()
            .for_scene(&scene_id)
            .unwrap()
            .definition
            .layers
            .is_empty()
    }); // 空快照正常完成。
    assert!(
        app.world()
            .resource::<DynamicBladeDraft>()
            .for_scene(&scene_id)
            .unwrap()
            .densities
            .is_empty()
    ); // 旧强句柄没有混入。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>(); // 撤销删除。
        session.draft.as_mut().unwrap().vegetation = original; // 原始编译像素仍在草稿中。
        session.vegetation_revision = 3; // 撤销同样必须新建身份。
        session.vegetation_compiled_revision = 3; // 恢复完成。
    }
    wait_for(&mut app, |world| {
        world
            .resource::<DynamicBladeDraft>()
            .for_scene(&scene_id)
            .unwrap()
            .definition
            .layers
            .len()
            == 1
    }); // 正式运行时可重建四窗。
}

/// 为真实统一撤销系统提供空闲领域资源；植被测试不创建任何水域、摆放或门户实体。
fn install_clipmap_history(app: &mut App) {
    // 使用真实生产历史入口，不在测试中手动恢复快照。
    use crate::authoring::{
        EditorPreviewAssets, GlobalEnvironmentAuthoringState, SurfaceBrushStroke, WaterBrushStroke,
        WaterFileHandoff, WaterMaskMaterializeTask,
    }; // 统一历史系统的固定参数。
    app.insert_resource(EditorPreviewAssets {
        // 植被命令不会读取这些门户句柄，但系统参数必须完整。
        portal_base_mesh: default(),          // 无门户底盘。
        portal_arrow_head_mesh: default(),    // 无箭头头部。
        portal_arrow_shaft_mesh: default(),   // 无箭杆。
        portal_edge_beam_mesh: default(),     // 无门户光柱。
        portal_edge_ring_mesh: default(),     // 无门户圆环。
        portal_base_material: default(),      // 无底盘材质。
        portal_arrow_material: default(),     // 无箭头材质。
        portal_edge_beam_material: default(), // 无光柱材质。
        portal_edge_ring_material: default(), // 无圆环材质。
        portal_label_font: default(),         // 无门户文本。
    }); // 只建立轻量句柄，不加载资产。
    app.init_resource::<crate::authoring::courtyards::CourtyardAuthoringState>() // 院落没有在途整组操作。
        .init_resource::<GlobalEnvironmentAuthoringState>() // 全局环境没有活动历史。
        .init_resource::<WaterMaskMaterializeTask>() // 水域写侧为空闲。
        .init_resource::<crate::authoring::surface_layer_edit::SurfaceLayerEditTask>() // 地表写侧为空闲。
        .init_resource::<SurfaceBrushStroke>() // 无活动地表笔划。
        .init_resource::<WaterBrushStroke>() // 无活动水域笔划。
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>() // 无摆放根索引。
        .init_resource::<WaterFileHandoff>(); // 无水域交接阻塞。
} // 全部测试仍无窗口运行。

/// 等待当前完整编译修订进入实际Image，并核对作者预设稳定身份没有随操作变化。
fn assert_clipmap_density(
    app: &mut App,
    scene_id: &SceneId,
    output_id: &str,
    preset_id: &str,
    expected: u8,
) {
    // 辅助只驱动生产系统和读取结果。
    let revision = app
        .world()
        .resource::<EditorAuthoringSession>()
        .vegetation_revision; // 本次操作的真实修订。
    wait_for(app, |world| {
        world
            .resource::<DynamicBladeDraft>()
            .for_scene(scene_id)
            .is_some_and(|snapshot| snapshot.source_revision == revision)
    }); // 不接受旧修订图片。
    let snapshot = app
        .world()
        .resource::<DynamicBladeDraft>()
        .for_scene(scene_id)
        .unwrap(); // 取得完整当前快照。
    assert_eq!(snapshot.definition.layers.len(), 1); // 作者显隐不能改变正式预设数量。
    assert_eq!(snapshot.definition.layers[0].id, output_id); // 输出稳定ID不因撤销重做变化。
    assert_eq!(
        snapshot.definition.layers[0].runtime_profile.as_deref(),
        Some(preset_id)
    ); // 规范预设ID不退回硬编码普通草或别名。
    let pixels = app
        .world()
        .resource::<Assets<Image>>()
        .get(&snapshot.densities[output_id])
        .unwrap()
        .data
        .as_ref()
        .unwrap(); // 实际上传输入的CPU字节。
    assert_eq!(
        pixels[0], expected,
        "预设{preset_id}修订{revision}密度不匹配"
    ); // 画草、隐藏、擦除及历史恢复均逐字节验证。
} // 不依据UI文案猜测生效。

#[test] // 四种正式Blade目录项均执行真实笔刷、显隐、历史、后台编译和动态发布。
fn clipmap_brush_erase_compile_and_closed_editor_publish_same_density() {
    // 所有阶段均不提供GLB缓存。
    use crate::authoring::vegetation_compile::{
        VegetationCompileState, poll_vegetation_compile, start_vegetation_compile,
    }; // 真实后台编译。
    use crate::authoring::{VegetationBrushStroke, apply_undo_redo, apply_vegetation_brush}; // 与插件注册同一笔刷及历史入口。
    use crate::ui::vegetation_layers::{
        VegetationBlankLayerAllocation, handle_vegetation_authoring_buttons,
    }; // 显隐通过真实UI业务分派。
    let catalog: feathrise_scene_content::VegetationPresetCatalogAsset =
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/vegetation_presets.vegetation_presets.json"
        )))
        .unwrap(); // 读取随包正式目录，避免只改测试层字符串。
    for preset_id in [
        "hn01_ordinary_grass",
        "hn01_wheat",
        "hn01_mixed_grain",
        "hn02_water_grass",
    ] {
        // 明确锁定四个获批正式ID。
        let preset = catalog
            .presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .expect("正式Blade预设必须存在"); // 删除或误改ID会使回归失败。
        let (mut app, scene_id) = setup(); // 每个预设使用独立会话和资产容器。
        let output_id; // 后续断言跟随目录生成的真实场景输出身份。
        {
            // 构造无GLB的Clipmap作者会话。
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>(); // 当前唯一草稿。
            let draft = session.draft.as_mut().unwrap(); // setup已提供场景。
            draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap; // 稀疏高度路线。
            draft.heights = None; // 不提供整图高度。
            draft.terrain_mesh = None; // 不提供GLB顶点。
            let mut layer = preset.create_scene_layer(&draft.folder); // 使用真实生产目录到场景输出转换。
            assert_eq!(layer.kind, SceneVegetationKind::Blade); // 测试不能把Mesh假装成草。
            layer.resolution = [2, 2]; // 只缩小测试画布，不改变稳定ID、渲染类别、预设引用和散布参数。
            output_id = layer.id.clone(); // 冻结真实输出身份。
            draft.vegetation = Some(SceneVegetationAsset {
                model_surfaces: Vec::new().into(),
                schema_version: SCENE_SCHEMA_VERSION,
                scene_id: scene_id.clone(),
                layers: vec![layer],
                authoring: None,
            }); // 初始化一个有效正式输出。
            crate::authoring::vegetation_defaults::ensure_vegetation_authoring(
                draft.vegetation.as_mut().unwrap(),
                &draft.folder,
            ); // 使用正式作者一对一初始化。
            let author_id = draft
                .vegetation
                .as_ref()
                .unwrap()
                .authoring
                .as_ref()
                .unwrap()
                .layers[0]
                .id
                .clone(); // 从生产结构读取作者ID。
            draft
                .vegetation_author_densities
                .insert(author_id, std::sync::Arc::new(vec![0; 4])); // 独立空白作者R8。
            draft.vegetation_densities.clear(); // 不继承setup的普通草输出。
            draft
                .vegetation_densities
                .insert(output_id.clone(), std::sync::Arc::new(vec![0; 4])); // 完整空白正式基线。
        } // 草稿已可绘制。
        app.init_resource::<crate::viewport::vegetation_ground::VegetationGroundCursor>(); // 显式模拟精确曲面已完成命中。
        app.world_mut()
            .resource_mut::<crate::viewport::vegetation_ground::VegetationGroundCursor>()
            .paint_ready = true; // 回归笔刷必须通过生产地面证据门。
        install_clipmap_history(&mut app); // 提供真实撤销/重做系统的空闲领域资源。
        app.init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<VegetationBrushStroke>()
            .init_resource::<VegetationCompileState>(); // 正式输入与编译状态。
        app.init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
            .init_resource::<VegetationBlankLayerAllocation>(); // 按钮分派拥有完整资源。
        app.world_mut().spawn((
            crate::components::SceneEditorViewport,
            bevy::ui::RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::splat(0.5)),
            },
        )); // 合法视口hover。
        let toggle = app
            .world_mut()
            .spawn((
                UiInteraction::None,
                crate::components::SceneEditorActionButton(
                    crate::types::EditorAction::VegetationToggleEnabled,
                ),
            ))
            .id(); // 真实显隐按钮实体。
        app.add_systems(
            Update,
            (
                apply_vegetation_brush,
                handle_vegetation_authoring_buttons,
                apply_undo_redo,
                start_vegetation_compile,
                poll_vegetation_compile,
            )
                .chain()
                .before(sync_live_blade_density),
        ); // 作者操作先于编译与发布。
        app.add_plugins(crate::standalone::embedded_vegetation::VegetationPreviewPlugin);
        {
            let mut projection = app.world_mut().resource_mut::<crate::standalone::embedded_vegetation::EmbeddedVegetationState>();
            projection.enabled = true; projection.session = 1; projection.generation = 1;
        } // 同一生产笔刷的跨进程投影也必须在按住期间持续完成。
        for erase in [false, true] {
            // 每个预设分别画草和擦除。
            {
                // 真实笔刷用户上下文。
                let mut editor = app.world_mut().resource_mut::<SceneEditorState>(); // 每次重新打开同一会话。
                editor.open = true; // 输入只在F12打开时生效。
                editor.tool = crate::types::EditorTool::Vegetation; // 选择植被笔刷。
                editor.brush_cursor_world = Some(Vec3::ZERO); // 画布左上端点。
                editor.brush_radius = 2.0; // 不波及10米外另一端点。
                editor.brush_strength = 1.0; // 一次落笔达到精确目标值。
            } // 保持会话稳定身份。
            if erase {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::ShiftLeft);
            } // 正式Shift擦除。
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left); // 真实按下边沿。
            app.update(); // 笔刷写独立作者源。
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear_just_pressed(MouseButton::Left);
            let expected = if erase { 0 } else { 255 };
            assert_clipmap_density(&mut app, &scene_id, &output_id, preset_id, expected);
            let published = app.world().resource::<DynamicBladeDraft>().frozen_for_scene(&scene_id).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                app.update();
                let ready = app.world().resource::<crate::standalone::embedded_vegetation::EmbeddedVegetationState>().ready.clone();
                if let Some(ready) = ready {
                    let batch = feathrise_scene_content::embedded_preview::decode_vegetation_preview(&ready.bytes).unwrap();
                    let matched = batch.canvases.iter().any(|canvas| canvas.source_revision >= published.source_revision
                        && canvas.densities.get(&output_id) == published.sources.get(&output_id));
                    app.world_mut().resource_mut::<crate::standalone::embedded_vegetation::EmbeddedVegetationState>().acknowledged_sequence = ready.sequence;
                    if matched { break; }
                }
                assert!(std::time::Instant::now() < deadline, "真实按住笔刷的完成R8没有投影到通信批次");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(app.world().resource::<VegetationBrushStroke>().active);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left); // 真实松笔。
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .clear_just_pressed(MouseButton::Left); // 无InputPlugin时手动清理按下边沿。
            app.update(); // 撤销差分完成，后台开始编译。
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .clear(); // 不重复消费松笔。
            app.world_mut().resource_mut::<SceneEditorState>().open = false; // 编译中立即关闭F12。
            let expected = if erase { 0 } else { 255 }; // 画草与擦除目标。
            assert_clipmap_density(&mut app, &scene_id, &output_id, preset_id, expected); // 关闭后仍完整发布。
            for undo in [true, false] {
                // 笔刷撤销再重做，使用真实统一历史系统。
                let mut editor = app.world_mut().resource_mut::<SceneEditorState>(); // 请求保留至生产系统消费。
                editor.undo_requested = undo; // 首轮撤销当前笔划。
                editor.redo_requested = !undo; // 次轮重做同一稳定作者ID。
                drop(editor); // 释放World借用再更新。
                app.update(); // 生产历史恢复作者R8并递增修订。
                assert_clipmap_density(
                    &mut app,
                    &scene_id,
                    &output_id,
                    preset_id,
                    if undo { 255 - expected } else { expected },
                ); // 只接受新修订的实际Image。
            } // 当前再次回到原笔划完成状态。
            if !erase {
                // 在非空密度上验证显隐与对应历史恢复。
                app.world_mut().entity_mut(toggle).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered())); // 按真实显隐按钮。
                app.update(); // 正式业务事务写enabled并记录完整结构历史。
                *app.world_mut().get_mut::<UiInteraction>(toggle).unwrap() = UiInteraction::None; // 按钮松开。
                assert_clipmap_density(&mut app, &scene_id, &output_id, preset_id, 0); // 隐藏只归零正式输出，保留作者像素。
                for undo in [true, false] {
                    // 显隐结构命令同样执行真实撤销重做。
                    let mut editor = app.world_mut().resource_mut::<SceneEditorState>(); // 不直接篡改作者快照。
                    editor.undo_requested = undo; // 撤销隐藏恢复可见。
                    editor.redo_requested = !undo; // 重做恢复隐藏。
                    drop(editor); // 释放可变资源借用。
                    app.update(); // 生产结构恢复及编译换代。
                    assert_clipmap_density(
                        &mut app,
                        &scene_id,
                        &output_id,
                        preset_id,
                        if undo { 255 } else { 0 },
                    ); // 正式密度随显隐历史完整变化。
                } // 保留隐藏状态供下一次按钮恢复。
                app.world_mut().entity_mut(toggle).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered())); // 第二次显隐按钮恢复显示。
                app.update(); // 再次由真实业务层记录结构命令。
                *app.world_mut().get_mut::<UiInteraction>(toggle).unwrap() = UiInteraction::None; // 避免重复按钮消费。
                assert_clipmap_density(&mut app, &scene_id, &output_id, preset_id, 255); // 为下一轮擦除恢复非空画布。
            } // 显隐全过程不改变预设和输出ID。
        } // 四类预设都走完全相同的生产链。
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().undo.len(),
            4
        ); // 画草、隐藏、显示、擦除四条历史，撤销重做不增加重复命令。
    } // 每项测试只用2×2画布，无真实模型加载或GPU管线。
} // 目标硬件画面、正式文件重开和GPU贴地仍由独立验收覆盖。

#[test] // 真实笔刷等待、延迟连续采样及松手取消回归。
fn clipmap_ground_wait_press_release_and_delayed_segment() {
    // 不启动GPU即可验证不会迟到补画。
    use crate::authoring::{VegetationBrushStroke, apply_vegetation_brush}; // 使用生产系统。
    use crate::viewport::vegetation_ground::VegetationGroundCursor; // 仅模拟后台完成状态。
    let (mut app, _) = setup(); // 小型正式草稿。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap(); // 构建作者源。
        draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap; // 精确地面分支。
        crate::authoring::vegetation_defaults::ensure_vegetation_authoring(
            draft.vegetation.as_mut().unwrap(),
            &draft.folder,
        ); // 真实作者ID。
        let id = draft
            .vegetation
            .as_ref()
            .unwrap()
            .authoring
            .as_ref()
            .unwrap()
            .layers[0]
            .id
            .clone();
        draft
            .vegetation_author_densities
            .insert(id, std::sync::Arc::new(vec![0; 4])); // 空画布。
    } // 仅测输入不编译。
    app.init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<VegetationBrushStroke>()
        .init_resource::<VegetationGroundCursor>(); // 完整输入资源。
    app.world_mut().spawn((
        crate::components::SceneEditorViewport,
        bevy::ui::RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    )); // 合法按下区域。
    app.add_systems(
        Update,
        apply_vegetation_brush.before(sync_live_blade_density),
    ); // 与真实发布链顺序一致。
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.open = true;
        editor.tool = crate::types::EditorTool::Vegetation;
        editor.brush_strength = 1.;
        editor.brush_radius = 20.;
    } // 有效画草上下文。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update(); // press时曲面未就绪。
    assert!(!app.world().resource::<VegetationBrushStroke>().active);
    assert!(
        app.world()
            .resource::<VegetationBrushStroke>()
            .pending_ground
            .is_some()
    ); // 只保存意图。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear_just_pressed(MouseButton::Left); // 下一帧仍按住。
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .brush_cursor_world = Some(Vec3::ZERO);
    app.world_mut()
        .resource_mut::<VegetationGroundCursor>()
        .paint_ready = true;
    app.update(); // 真命中才开始。
    assert!(app.world().resource::<VegetationBrushStroke>().active);
    assert!(
        app.world()
            .resource::<VegetationBrushStroke>()
            .pending_ground
            .is_none()
    ); // 只起笔一次。
    {
        let mut ground = app.world_mut().resource_mut::<VegetationGroundCursor>();
        ground.paint_ready = false;
        ground.pending_same_context = true;
    } // 同代下一条射线计算需多帧。
    app.update();
    app.update();
    assert_eq!(
        app.world().resource::<VegetationBrushStroke>().last_world,
        Some(Vec3::ZERO)
    ); // 暂停而不丢连续段起点。
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .brush_cursor_world = Some(Vec3::X);
    app.world_mut()
        .resource_mut::<VegetationGroundCursor>()
        .paint_ready = true;
    app.update(); // 下一冻结射线完成。
    assert_eq!(
        app.world()
            .resource::<VegetationBrushStroke>()
            .segment_start,
        Some(Vec3::ZERO)
    ); // 连续涂抹补点从原点开始。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update(); // 正常结束。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<VegetationGroundCursor>()
        .paint_ready = false; // 第二笔等待。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update(); // 只有待开始意图。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear_just_pressed(MouseButton::Left);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update(); // 结果之前已松手。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<VegetationGroundCursor>()
        .paint_ready = true;
    app.update(); // 模拟迟到完成。
    assert!(!app.world().resource::<VegetationBrushStroke>().active);
    assert!(
        app.world()
            .resource::<VegetationBrushStroke>()
            .pending_ground
            .is_none()
    ); // 永不迟到起笔。
    // 体素工具已消费本次按下，移回普通地表时没有第二个just_pressed，仍必须接续草刷。
    let identity = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        (draft.scene_id.clone(), draft.vegetation.as_ref().unwrap().authoring.as_ref().unwrap().layers[0].id.clone())
    };
    app.world_mut().resource_mut::<VegetationBrushStroke>().volume_handoff = Some(identity);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear_just_pressed(MouseButton::Left);
    app.update();
    assert!(app.world().resource::<VegetationBrushStroke>().active,
        "同次按压越过体素边界后普通草刷不能停笔");
    assert!(app.world().resource::<VegetationBrushStroke>().volume_handoff.is_none());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    app.update();
    assert!(!app.world().resource::<VegetationBrushStroke>().active);
} // 原GLB流程不依赖该查询门。
