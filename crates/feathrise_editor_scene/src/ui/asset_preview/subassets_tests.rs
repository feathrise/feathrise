//! 子资产目录和单体边界纯内存回归；不启动窗口、渲染器或系统剪贴板。

use super::*; // 直接验证生产枚举预算与边界游标。
use bevy::asset::AssetPlugin; // 真实夹具使用正式异步AssetServer。
use bevy::asset::RenderAssetUsages; // 测试Mesh同时保留主世界位置数据。
use bevy::asset::uuid::Uuid; // 构造不依赖AssetServer注册的稳定测试句柄。
use bevy::gltf::{GltfMaterial, GltfPlugin, GltfPrimitive}; // 构造元数据并安装正式GLTF加载器。
use bevy::mesh::PrimitiveTopology; // 最小三角网格类型。
use bevy::world_serialization::WorldAsset; // 正式GLTF插件发布的Scene子资产类型。
use std::collections::HashSet; // 与生产会话字段保持相同的标准库集合类型。
use std::time::{Duration, Instant}; // 真实异步夹具只用有界测试截止时间。

/// 创建不含隐式候选的最小真实Gltf根。
fn gltf() -> Gltf {
    Gltf {
        scenes: Vec::new(),          // 测试不实例化WorldAsset。
        named_scenes: default(),     // 不按名称猜Scene编号。
        meshes: Vec::new(),          // 用例显式注入网格。
        named_meshes: default(),     // 作者名不决定路径。
        materials: Vec::new(),       // 用例显式注入材质。
        named_materials: default(),  // 不按名称匹配材质。
        nodes: Vec::new(),           // 单体候选不依赖节点树。
        named_nodes: default(),      // 无命名节点。
        skins: Vec::new(),           // 不涉及蒙皮。
        named_skins: default(),      // 无命名蒙皮。
        default_scene: None,         // Scene0数量由scenes真实长度决定。
        animations: Vec::new(),      // 测试模型没有动画。
        named_animations: default(), // 无动画名称。
        source: None,                // 不复制原始JSON文档。
    }
}

#[test] // 多primitive跨预算完整枚举，并保留各自真实材质关联。
fn catalog_enumeration_is_bounded_and_preserves_materials() {
    let mut material_assets = Assets::<GltfMaterial>::default(); // 只生成稳定类型句柄。
    let material = material_assets.add(GltfMaterial::default()); // Material0真实身份。
    let mut primitive_mesh_assets = Assets::<Mesh>::default(); // 目录分类读取真实Mesh资产。
    let primitive_mesh = primitive_mesh_assets.add(Mesh::new(
        // 所有候选复用同一个静态顶点布局即可。
        PrimitiveTopology::TriangleList, // 静态测试网格不声明骨骼属性。
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD, // 与正式GLTF Mesh保留策略一致。
    )); // 句柄由Assets分配稳定测试身份。
    let count = CATALOG_WORK_PER_FRAME + 3; // 材质项占一个预算后必然跨帧。
    let primitives = (0..count)
        .map(|index| GltfPrimitive {
            index: index + 4,                  // 刻意不从零开始，防止用Vec下标冒充标签。
            parent_mesh_index: 7,              // 根数组位置也不等于真实Mesh编号。
            name: format!("Primitive{index}"), // 名称只作显示辅助。
            mesh: primitive_mesh.clone(),      // 每项只查询同一静态布局的属性表。
            material: (index % 2 == 0).then(|| material.clone()), // 偶数项显式Material0，奇数项默认材质。
            extras: None,                                         // 无primitive扩展。
            material_extras: None,                                // 无材质扩展。
        })
        .collect::<Vec<_>>(); // 测试同步创建固定小型夹具。
    let mut mesh_assets = Assets::<GltfMesh>::default(); // 真实元数据容器。
    let mesh = mesh_assets.add(GltfMesh {
        index: 7,                    // 真实Mesh编号。
        name: "多部件树".to_owned(), // 中文作者名称不影响选择身份。
        primitives,                  // 超过一帧预算的真实数组。
        extras: None,                // 无网格扩展。
    });
    let mut root = gltf(); // 最小根。
    root.materials.push(material); // 建立Material0根索引。
    root.meshes.push(mesh); // 根数组只有一个真实Mesh7。
    let mut catalog = AssetPreviewModelCatalog::new(Handle::default()); // 不需要AssetServer。
    catalog.phase = AssetPreviewCatalogPhase::Collecting; // 直接测试枚举阶段。
    assert!(!collect_catalog(&mut catalog, &root, &mesh_assets, &primitive_mesh_assets).unwrap()); // 首帧预算耗尽不能声称完成。
    assert!(catalog.choices.len() < count); // 部分结果仍只在内部草稿。
    while !collect_catalog(&mut catalog, &root, &mesh_assets, &primitive_mesh_assets).unwrap() {} // 测试夹具同步推进到完整结果。
    assert_eq!(catalog.choices.len(), count); // 固定帧预算不是产品总量上限。
    assert_eq!(
        catalog.choices[0],
        AssetPreviewPrimitiveChoice {
            id: AssetPreviewPrimitiveId {
                mesh: 7,      // 使用真实父网格编号。
                primitive: 4, // 使用真实primitive编号。
            },
            material: Some(0),        // 显式关联Material0。
            requires_skinning: false, // 静态顶点布局允许直接预览和摆放。
        }
    );
    assert_eq!(catalog.choices[1].material, None); // 省略材质保持DefaultMaterial语义。
}

#[test] // 目录必须从真实顶点布局识别蒙皮primitive，供UI在生成实体前阻止选择。
fn catalog_marks_joint_weight_mesh_as_requiring_skinning() {
    // 覆盖截图中Mixamo身体网格的关键属性组合。
    let mut primitive_mesh_assets = Assets::<Mesh>::default(); // 建立主世界Mesh容器。
    let mut skinned_mesh = Mesh::new(
        // 创建不需要GPU的最小蒙皮顶点布局。
        PrimitiveTopology::TriangleList, // 拓扑不参与蒙皮分类。
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD, // 保留属性供目录读取。
    ); // 初始网格尚未声明任何骨骼输入。
    skinned_mesh.insert_attribute(
        // 加入Bevy正式关节索引属性。
        Mesh::ATTRIBUTE_JOINT_INDEX, // 此属性单独不足，必须与权重联合识别。
        VertexAttributeValues::Uint16x4(vec![[0_u16, 0_u16, 0_u16, 0_u16]]), // 显式匹配Bevy关节索引格式。
    ); // 不扫描数组内容，只查询属性表。
    skinned_mesh.insert_attribute(
        // 加入Bevy正式关节权重属性。
        Mesh::ATTRIBUTE_JOINT_WEIGHT, // 与关节索引齐全时PBR选择skinned_mesh_layout。
        VertexAttributeValues::Float32x4(vec![[1.0_f32, 0.0_f32, 0.0_f32, 0.0_f32]]), // 显式匹配Bevy权重格式。
    ); // 完成截图资产同类的JOINTS与WEIGHTS组合。
    let skinned_handle = primitive_mesh_assets.add(skinned_mesh); // 冻结真实Mesh资产身份。
    let mut gltf_mesh_assets = Assets::<GltfMesh>::default(); // 建立GLTF网格元数据容器。
    let gltf_mesh = gltf_mesh_assets.add(GltfMesh {
        // Mesh0只包含当前蒙皮primitive。
        index: 0,                      // 使用正式根数组编号。
        name: "MixamoBody".to_owned(), // 作者名只用于UI识别。
        primitives: vec![GltfPrimitive {
            // 目录枚举一个真实子资产。
            index: 0,                // Primitive0正式编号。
            parent_mesh_index: 0,    // 父网格为Mesh0。
            name: "Body".to_owned(), // 名称不参与安全判定。
            mesh: skinned_handle,    // 指向含关节和权重属性的真实Mesh。
            material: None,          // 默认材质不影响蒙皮能力。
            extras: None,            // 不需要GLTF扩展数据。
            material_extras: None,   // 不需要材质扩展数据。
        }], // 当前网格只有一个primitive。
        extras: None,                  // 不需要网格扩展数据。
    }); // 完成根可枚举元数据。
    let mut root = gltf(); // 建立最小GLTF根。
    root.meshes.push(gltf_mesh); // Mesh0进入正式根顺序。
    let mut catalog = AssetPreviewModelCatalog::new(Handle::default()); // 不需要磁盘AssetServer。
    catalog.phase = AssetPreviewCatalogPhase::Collecting; // 直接进入固定预算枚举阶段。

    assert!(
        // 单个primitive应在一个生产预算内完整发布。
        collect_catalog(
            // 使用与正式系统完全相同的属性分类入口。
            &mut catalog,           // 写入唯一目录草稿。
            &root,                  // 读取GLTF根顺序。
            &gltf_mesh_assets,      // 读取当前GltfMesh元数据。
            &primitive_mesh_assets, // 读取对应Mesh属性表。
        ) // 返回值只在完整枚举后为true。
        .expect("蒙皮目录枚举不应失败") // 资产均已同步放入测试容器。
    ); // 完成一次原子目录构建。
    assert_eq!(catalog.choices.len(), 1); // 仍保留蒙皮行供作者理解文件结构。
    assert!(catalog.choices[0].requires_skinning); // 关键结果：该行不能生成裸Mesh3d。
} // 测试没有实例化Scene或启动渲染器。

#[test] // 官方加载器端到端证明真实Mesh与Material索引能形成完整目录。
fn real_gltf_loader_catalog_preserves_mesh_and_material_indices() {
    let mut app = App::new(); // 无窗口、无GPU的资产加载测试应用。
    app.add_plugins((
        MinimalPlugins, // 只提供异步任务和时间等最小运行基础。
        AssetPlugin {
            file_path: concat!(
                env!("CARGO_MANIFEST_DIR"),        // 从当前crate稳定定位测试夹具。
                "/tests/fixtures/asset_reference"  // 不读取作者现场assets目录。
            )
            .to_owned(), // AssetPlugin需要拥有根路径字符串。
            ..default() // 沿用正式异步IO与缓存策略。
        },
        GltfPlugin::default(), // 使用项目当前Bevy版本的官方GLTF解析器与标签规则。
    ))
    .init_asset::<WorldAsset>() // Scene子资产登记与正式插件环境一致。
    .init_asset::<Mesh>() // primitive依赖类型必须可由加载器发布。
    .init_asset::<Image>() // 材质纹理依赖类型即使夹具为空也保持正式契约。
    .init_asset::<StandardMaterial>() // `/std`派生材质进入主世界Assets。
    .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>() // GLTF蒙皮依赖类型登记。
    .add_systems(Update, advance_model_catalog); // 直接推进生产固定预算目录系统。
    let root = app
        .world()
        .resource::<AssetServer>()
        .load::<Gltf>("reference.gltf"); // 夹具包含两个网格及两个各自关联材质。
    app.insert_resource(AssetPreviewState {
        model: Some(AssetPreviewModelSession::new(1, root)), // 默认Scene0会话并行建立目录。
        ..default() // 本测试不生成WorldAsset实例、相机或UI。
    });
    app.finish(); // GltfPlugin在finish阶段登记真实加载器。
    app.cleanup(); // 完成与正式App相同的插件生命周期。
    let deadline = Instant::now() + Duration::from_secs(10); // 损坏夹具应快速失败而非挂住测试。
    loop {
        app.update(); // 推进真实AssetServer与生产目录状态机一帧。
        let model = app
            .world()
            .resource::<AssetPreviewState>()
            .model
            .as_ref()
            .expect("测试模型会话必须保持存在"); // 只读当前完整阶段。
        if matches!(
            model.catalog.phase,
            AssetPreviewCatalogPhase::Ready | AssetPreviewCatalogPhase::Failed
        ) {
            break; // 只在成功或确定失败时结束真实异步轮询。
        }
        assert!(
            Instant::now() < deadline,
            "实际GLTF目录加载未完成：{}",
            model.catalog.status
        ); // 截止时保留生产诊断。
        std::thread::sleep(Duration::from_millis(1)); // 仅测试让出CPU，不进入生产帧路径。
    }
    let state = app.world().resource::<AssetPreviewState>(); // 检查一次原子发布结果。
    let catalog = &state.model.as_ref().unwrap().catalog; // 当前唯一模型目录。
    assert_eq!(
        catalog.phase,
        AssetPreviewCatalogPhase::Ready,
        "{}",
        catalog.status
    ); // 真实加载失败直接显示生产诊断。
    assert_eq!(catalog.scene_count, 1); // 夹具明确只有Scene0。
    assert_eq!(
        catalog.choices,
        vec![
            AssetPreviewPrimitiveChoice {
                id: AssetPreviewPrimitiveId {
                    mesh: 0,      // 第一真实Mesh编号。
                    primitive: 0, // 第一网格唯一primitive。
                },
                material: Some(0),        // 真实关联Material0。
                requires_skinning: false, // 夹具第一网格不包含蒙皮属性。
            },
            AssetPreviewPrimitiveChoice {
                id: AssetPreviewPrimitiveId {
                    mesh: 1,      // 第二真实Mesh编号。
                    primitive: 0, // 第二网格唯一primitive。
                },
                material: Some(1),        // 真实关联Material1，不能沿用前一项。
                requires_skinning: false, // 夹具第二网格同样是静态primitive。
            },
        ]
    ); // 候选顺序、数字标签和逐primitive材质全部来自官方加载结果。
}

#[test] // Scene0外孤立primitive按固定顶点切片完成，不在首帧遍历全部位置。
fn primitive_bounds_progresses_with_a_fixed_vertex_budget() {
    let count = PRIMITIVE_VERTICES_PER_FRAME + 3; // 强制至少两个生产预算帧。
    let positions = (0..count)
        .map(|index| [index as f32, -(index as f32), 2.0]) // 可精确断言的有限范围。
        .collect::<Vec<_>>(); // 测试内存输入。
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList, // 拓扑不影响位置边界。
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD, // 保留CPU属性供分帧读取。
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions); // 正式Float32x3位置格式。
    let choice = AssetPreviewPrimitiveChoice {
        id: AssetPreviewPrimitiveId {
            mesh: 2,      // 任意真实编号。
            primitive: 1, // 任意真实编号。
        },
        material: None,           // 边界计算与材质无关。
        requires_skinning: false, // 本用例只验证静态单体的分帧取景。
    };
    let mut load = AssetPreviewPrimitiveLoad {
        generation: 1,                                      // 有效测试代。
        choice,                                             // 完整数字身份。
        mesh: Handle::default(),                            // 纯函数不读取句柄。
        material: Handle::default(),                        // 纯函数不读取材质。
        bounds: AssetPreviewPrimitiveBoundsScan::default(), // 从首顶点开始。
    };
    assert!(
        advance_primitive_bounds(&mut load, &mesh)
            .unwrap()
            .is_none()
    ); // 第一预算不能完成超量网格。
    assert_eq!(load.bounds.cursor, PRIMITIVE_VERTICES_PER_FRAME); // 严格停在固定上限。
    let (minimum, maximum) = advance_primitive_bounds(&mut load, &mesh)
        .unwrap()
        .expect("第二预算必须完成剩余三个顶点");
    assert_eq!(minimum, Vec3::new(0.0, -((count - 1) as f32), 2.0)); // 累计下界覆盖两帧。
    assert_eq!(maximum, Vec3::new((count - 1) as f32, 0.0, 2.0)); // 累计上界覆盖两帧。
}

#[test] // 无关GltfMesh事件不能重置当前文件，相关网格换代则安全回到Scene0并保留可重扫根。
fn mesh_hot_reload_is_scoped_to_the_current_catalog() {
    let mut app = App::new(); // 纯ECS消息测试。
    app.add_message::<AssetEvent<Gltf>>() // 根事件读者所需正式消息资源。
        .add_message::<AssetEvent<GltfMesh>>() // 网格事件读者所需正式消息资源。
        .init_resource::<AssetPreviewNavigation>() // 换代重置观察状态。
        .add_systems(Update, invalidate_model_hot_reload); // 生产热更新系统。
    let scene_root = app
        .world_mut()
        .spawn((AssetPreviewModelRoot, Visibility::Inherited))
        .id(); // 已完整生成的Scene0根。
    let primitive_root = app.world_mut().spawn(AssetPreviewPrimitiveRoot).id(); // 当前显示单体。
    let root_handle = Handle::<Gltf>::from(Uuid::from_u128(1)); // 当前GLTF根稳定身份。
    let current_mesh_id = AssetId::<GltfMesh>::Uuid {
        uuid: Uuid::from_u128(2), // 当前根真实网格身份。
    };
    let unrelated_mesh_id = AssetId::<GltfMesh>::Uuid {
        uuid: Uuid::from_u128(3), // 另一GLTF的网格身份。
    };
    let choice = AssetPreviewPrimitiveChoice {
        id: AssetPreviewPrimitiveId {
            mesh: 0,      // 当前单体网格编号。
            primitive: 0, // 当前单体primitive编号。
        },
        material: None,           // 默认材质不影响热更新身份。
        requires_skinning: false, // 热更新用例保持正常静态单体语义。
    };
    let mut model = AssetPreviewModelSession::new(1, root_handle); // 当前模型会话。
    model.hot_reload_armed = true; // 本用例直接从已经完成首次加载的稳定会话开始。
    model.catalog.phase = AssetPreviewCatalogPhase::Ready; // 旧候选已公开。
    model.catalog.mesh_ids = HashSet::from([current_mesh_id]); // 只登记本根网格。
    model.target = AssetPreviewModelTarget::Primitive(choice.id); // 当前显示单体。
    model.selected_primitive = Some(choice); // 材质身份已冻结。
    model.primitive_root = Some(primitive_root); // 唯一单体实体。
    model.scene_ready = true; // Scene根仍存在，可在依赖换代后重新扫描。
    app.insert_resource(AssetPreviewState {
        selected_path: Some("models/tree.glb".to_owned()), // 当前文件路径。
        content: AssetPreviewContent::Model {
            handle: Handle::default(),       // 本测试不读取WorldAsset。
            root: scene_root,                // 真实测试Scene根。
            phase: AssetPreviewPhase::Ready, // 单体当前可见。
        },
        model: Some(model), // 发布完整会话。
        ..default()         // 相机与RTT无关。
    });
    app.world_mut()
        .write_message(AssetEvent::<GltfMesh>::Modified {
            id: unrelated_mesh_id, // 另一文件热更。
        });
    app.update(); // 消费无关事件。
    assert_eq!(
        app.world()
            .resource::<AssetPreviewState>()
            .model
            .as_ref()
            .unwrap()
            .target,
        AssetPreviewModelTarget::Primitive(choice.id)
    ); // 当前选择不能被无关文件重置。
    assert!(app.world().get_entity(primitive_root).is_ok()); // 单体仍存在。
    app.world_mut()
        .write_message(AssetEvent::<GltfMesh>::Modified {
            id: current_mesh_id, // 当前文件真实网格换代。
        });
    app.update(); // 执行整代失效并应用Commands。
    let state = app.world().resource::<AssetPreviewState>(); // 检查新安全代。
    let model = state.model.as_ref().unwrap(); // 当前模型会话继续存在。
    assert_eq!(model.target, AssetPreviewModelTarget::Scene0); // 热更默认回到完整Scene0。
    assert!(model.primitive_root.is_none()); // 旧单体身份已释放。
    assert!(model.scene_ready); // 纯网格换代不等待不会发生的WorldReady。
    assert_eq!(model.catalog.phase, AssetPreviewCatalogPhase::Loading); // 子资产编号重新验证。
    assert!(state.bounds_scan.is_some()); // 现有Scene实体按固定节点预算重新取景。
    assert!(matches!(
        state.content,
        AssetPreviewContent::Model {
            phase: AssetPreviewPhase::Scanning,
            ..
        }
    )); // 根保持隐藏到新边界完整采用。
    assert!(app.world().get_entity(primitive_root).is_err()); // 旧单体实体精确回收。
    assert_eq!(
        app.world().entity(scene_root).get::<Visibility>(),
        Some(&Visibility::Hidden)
    ); // 新Aabb扫描完成前Scene0不闪现。
}

#[test] // 同源GLB已被运行时预载部分标签时，首次补载Modified不得清掉刚完成的Scene0。
fn initial_modified_events_are_drained_before_hot_reload_is_armed() {
    let mut app = App::new(); // 纯ECS消息测试，不启动窗口、GPU或磁盘加载。
    app.add_message::<AssetEvent<Gltf>>() // 根初始化Modified与真实热更共用正式消息类型。
        .add_message::<AssetEvent<GltfMesh>>() // 网格标签补载也必须先建立基线。
        .init_resource::<AssetPreviewNavigation>() // 真正热更仍需验证观察状态重置。
        .add_systems(Update, invalidate_model_hot_reload); // 直接执行生产失效门。
    let scene_root = app
        .world_mut()
        .spawn((AssetPreviewModelRoot, Visibility::Inherited))
        .id(); // 模拟本帧刚收到WorldInstanceReady的完整Scene0根。
    let root_handle = Handle::<Gltf>::from(Uuid::from_u128(11)); // 当前GLTF根稳定身份。
    let current_mesh_id = AssetId::<GltfMesh>::Uuid {
        uuid: Uuid::from_u128(12), // 当前根已枚举的真实网格身份。
    };
    let choice = AssetPreviewPrimitiveChoice {
        id: AssetPreviewPrimitiveId {
            mesh: 7,      // 使用非零编号证明选择身份原样保留。
            primitive: 0, // 当前网格首primitive。
        },
        material: Some(1),        // 保留原材质身份，初始化事件不能重置选择。
        requires_skinning: false, // 本用例只验证初始化消息，不触发蒙皮限制。
    };
    let mut model = AssetPreviewModelSession::new(5, root_handle.clone()); // 首次会话尚未启用热更新。
    model.catalog.phase = AssetPreviewCatalogPhase::Ready; // 子资产目录已经原子完成。
    model.catalog.mesh_ids = HashSet::from([current_mesh_id]); // 当前文件真实网格集合。
    model.catalog.choices.push(choice); // 已公开候选不得被初始化Modified清空。
    model.target = AssetPreviewModelTarget::Primitive(choice.id); // 模拟作者已切到单体。
    model.selected_primitive = Some(choice); // 单体材质身份已经冻结。
    model.scene_ready = true; // Scene0也在同一帧刚完成实例化。
    app.insert_resource(AssetPreviewState {
        selected_path: Some("gltf/项目自有植被/灌木/开封自有灌木高模.glb".to_owned()), // 复现自有灌木同源预载资产语义。
        content: AssetPreviewContent::Model {
            handle: Handle::default(),       // 消息门不读取WorldAsset内容。
            root: scene_root,                // 当前唯一Scene0根。
            phase: AssetPreviewPhase::Ready, // 当前完整结果已经可用。
        },
        model: Some(model),       // 发布尚未建立热更新基线的首次会话。
        next_model_generation: 6, // 后续真实热更新必须得到下一代身份。
        ..default()               // 相机、灯光与RTT不参与消息时序。
    });
    app.world_mut().write_message(AssetEvent::<Gltf>::Modified {
        id: root_handle.id(), // AssetServer补齐Scene0或根标签时可能覆盖同源槽位。
    });
    app.world_mut()
        .write_message(AssetEvent::<GltfMesh>::Modified {
            id: current_mesh_id, // 已预载primitive对应元数据也可能收到初始化Modified。
        });
    app.update(); // 首帧只排空消息并原子建立后续热更新基线。
    {
        let state = app.world().resource::<AssetPreviewState>(); // 检查刚完成的Scene0没有被倒退。
        let model = state.model.as_ref().unwrap(); // 当前会话仍是原始第五代。
        assert_eq!(model.generation, 5); // 初始化事件不能伪造新代。
        assert!(model.hot_reload_armed); // 消息排空后从下一帧起监听真正修改。
        assert!(model.scene_ready); // 关键回归：不得重新等待不会再发送的Ready。
        assert_eq!(model.target, AssetPreviewModelTarget::Primitive(choice.id)); // 作者目标保持不变。
        assert_eq!(model.catalog.choices, vec![choice]); // 已完整目录保持原子可用。
    }
    app.world_mut()
        .write_message(AssetEvent::<GltfMesh>::Modified {
            id: current_mesh_id, // 基线后的同一消息是真正热更新。
        });
    app.update(); // 第二帧必须沿用正式失效重建行为。
    let state = app.world().resource::<AssetPreviewState>(); // 核对真正热更新没有被永久忽略。
    let model = state.model.as_ref().unwrap(); // 当前已采用第六代会话。
    assert_eq!(model.generation, 6); // 真正修改仍推进代号。
    assert_eq!(model.target, AssetPreviewModelTarget::Scene0); // 安全回到完整场景。
    assert!(model.scene_ready); // 纯网格换代复用现有World实例。
    assert_eq!(model.catalog.phase, AssetPreviewCatalogPhase::Loading); // 新目录重新验证编号。
}

#[test]
fn late_gltf_metadata_event_gets_a_fresh_scene_ready_without_a_world_asset_change() {
    use bevy::world_serialization::{WorldAssetRoot, WorldSerializationPlugin};

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), WorldSerializationPlugin))
        .add_message::<AssetEvent<Gltf>>()
        .add_message::<AssetEvent<GltfMesh>>()
        .init_resource::<AssetPreviewNavigation>()
        .add_systems(Update, invalidate_model_hot_reload)
        .add_observer(super::super::model::observe_asset_preview_world_ready);
    app.finish();
    app.cleanup();

    let scene = app
        .world_mut()
        .resource_mut::<Assets<WorldAsset>>()
        .add(WorldAsset::new(World::new()));
    let old_root = app
        .world_mut()
        .spawn((
            AssetPreviewModelRoot,
            WorldAssetRoot(scene.clone()),
            Visibility::Hidden,
        ))
        .id();
    let gltf = Handle::<Gltf>::from(Uuid::from_u128(21));
    let mut model = AssetPreviewModelSession::new(1, gltf.clone());
    model.catalog.phase = AssetPreviewCatalogPhase::Ready;
    app.insert_resource(AssetPreviewState {
        selected_path: Some("models/humanoid.glb".to_owned()),
        content: AssetPreviewContent::Model {
            handle: scene.clone(),
            root: old_root,
            phase: AssetPreviewPhase::Loading,
        },
        model: Some(model),
        next_model_generation: 2,
        ..default()
    });
    app.update();
    assert!(app.world().resource::<AssetPreviewState>().model.as_ref().unwrap().scene_ready);

    // 只发布 GLTF 根元数据事件，保留 Scene0 资产原字节和句柄，不伪造第二个 Ready。
    app.world_mut().write_message(AssetEvent::<Gltf>::Modified { id: gltf.id() });
    app.update();

    let world = app.world();
    let state = world.resource::<AssetPreviewState>();
    let fresh_root = state.scene_root().unwrap();
    let model = state.model.as_ref().unwrap();
    assert_ne!(fresh_root, old_root);
    assert!(world.get_entity(old_root).is_err());
    assert_eq!(world.get::<WorldAssetRoot>(fresh_root).unwrap().id(), scene.id());
    assert_eq!(world.get::<Visibility>(fresh_root), Some(&Visibility::Hidden));
    assert_eq!(model.generation, 2);
    assert!(model.scene_ready);
    assert_eq!(state.bounds_scan.as_ref().map(|scan| scan.root), Some(fresh_root));
}
