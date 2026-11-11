//! GLB/GLTF子资产目录与单primitive预览加载；所有规模化工作都使用固定帧预算。

use bevy::asset::{
    // 根、子资产及递归依赖状态共同决定是否可安全采用。
    DependencyLoadState,          // 直接依赖失败必须立即诊断。
    LoadState,                    // 根资产仍采用Bevy正式异步状态。
    RecursiveDependencyLoadState, // 间接纹理或缓冲失败不能假装一直加载。
    UntypedAssetId,               // Mesh与StandardMaterial共用一个只读状态检查入口。
};
use bevy::camera::primitives::{Aabb, MeshAabb}; // 单体复用预计算边界或固定预算累计范围。
use bevy::camera::visibility::{NoAutoAabb, RenderLayers}; // 禁止同步重扫顶点并隔离F12预览相机。
use bevy::gltf::{Gltf, GltfMesh}; // 根与网格元数据提供真实primitive及材质关联。
use bevy::mesh::VertexAttributeValues; // 只接受GLTF正式Float32x3位置格式。
use bevy::prelude::*; // ECS、Assets、Mesh3d与StandardMaterial。
use feathrise_editor_core::constants::render_layers::MAP_EDITOR_ASSET_PREVIEW_LAYER; // 与Scene0共用唯一隔离层。
use feathrise_scene::direct_primitive_requires_skinning; // 与正式摆放共用Bevy蒙皮顶点属性安全判定。

use super::resources::AssetPreviewNavigation; // 热更必须清除旧几何的观察偏移。
use super::types::*; // 当前模型会话、目录游标和单体加载状态。
use crate::components::MapEditorOwnedEntity; // 编辑器关闭审计可统一识别临时单体实体。

pub(super) const PRIMITIVES_PER_PAGE: usize = 5; // 固定五槽行池兼顾300px右栏与预览画布高度。
const CATALOG_WORK_PER_FRAME: usize = 64; // 材质索引、空网格和primitive共享同一元数据预算。
const PRIMITIVE_VERTICES_PER_FRAME: usize = 16_384; // Scene0外孤立primitive也不会单帧遍历任意大网格。
const HOT_RELOAD_EVENTS_PER_FRAME: usize = 128; // 超出事件预算时保守整代失效，不延迟重放旧编号。

/// Scene0也是明确的Bevy标签；多Scene文件不能把它描述成“全部场景”。
pub(super) fn scene_reference(source: &str) -> String {
    format!("{source}#Scene0") // 只生成当前默认预览的第零个Scene引用。
}

/// primitive主引用严格使用Bevy 0.19正式标签格式。
pub(super) fn primitive_reference(source: &str, id: AssetPreviewPrimitiveId) -> String {
    format!("{source}#Mesh{}/Primitive{}", id.mesh, id.primitive) // 编号来自GltfPrimitive事实。
}

/// 原关联材质使用PBR扩展发布的StandardMaterial派生标签。
pub(super) fn material_reference(source: &str, material: Option<usize>) -> String {
    match material {
        Some(index) => format!("{source}#Material{index}/std"), // 显式GLTF材质。
        None => format!("{source}#DefaultMaterial/std"), // primitive省略材质时使用加载器默认材质。
    }
}

/// 同时检查根、直接依赖和递归依赖，调用方继续非阻塞轮询等待态。
fn asset_ready(
    server: &AssetServer, // 当前唯一AssetServer。
    id: UntypedAssetId,   // 具体Gltf、Mesh或StandardMaterial身份。
    description: &str,    // 错误文案中的作者可读对象。
) -> Result<bool, String> {
    let Some((root, dependencies, recursive)) = server.get_load_states(id) else {
        return Ok(false); // 刚登记句柄时尚无状态，不执行同步读取。
    };
    match root {
        LoadState::Failed(error) => Err(format!("{description}加载失败：{error}")), // 保留真实加载器原因。
        _ if matches!(dependencies, DependencyLoadState::Failed(_)) => {
            Err(format!("{description}的直接依赖加载失败")) // 缓冲或纹理错误立即停止本次采用。
        }
        _ if matches!(recursive, RecursiveDependencyLoadState::Failed(_)) => {
            Err(format!("{description}的递归依赖加载失败")) // 深层依赖同样不等待超时。
        }
        _ => Ok(server.is_loaded_with_dependencies(id)), // 只有完整依赖就绪才向后推进。
    }
}

/// 在一个固定预算内继续建立材质O(1)索引并枚举全部真实primitive。
fn collect_catalog(
    catalog: &mut AssetPreviewModelCatalog, // 当前文件唯一未完成目录。
    gltf: &Gltf,                            // 已完整加载的根元数据。
    meshes: &Assets<GltfMesh>,              // 每次只读取当前一个GltfMesh。
    primitive_meshes: &Assets<Mesh>,        // 只做常数次顶点属性存在性查询，不扫描primitive顶点。
) -> Result<bool, String> {
    for _ in 0..CATALOG_WORK_PER_FRAME {
        if let Some(material) = gltf.materials.get(catalog.cursor.material) {
            catalog // 材质句柄与MaterialN索引只建立一次。
                .material_indices // 后续每个primitive保持O(1)关联查找。
                .insert(material.id(), catalog.cursor.material); // 编号等于GLTF根正式数组下标。
            catalog.cursor.material += 1; // 下一预算单元继续下一材质。
            continue; // 材质与primitive不会在同一个预算单元里叠加。
        }
        let Some(mesh_handle) = gltf.meshes.get(catalog.cursor.mesh) else {
            return Ok(true); // 全部网格完成后才允许原子公开候选。
        };
        catalog.mesh_ids.insert(mesh_handle.id()); // 热更只失效属于当前根的真实GltfMesh。
        let mesh = meshes
            .get(mesh_handle) // 根依赖已Ready时当前GltfMesh必须存在。
            .ok_or_else(|| "GLTF网格元数据在枚举期间失效，请等待热更新重试".to_owned())?;
        if let Some(primitive) = mesh.primitives.get(catalog.cursor.primitive) {
            let Some(primitive_mesh) = primitive_meshes.get(&primitive.mesh) else {
                // Loaded与主世界Assets采用之间允许短暂窗口。
                return Ok(false); // 不发布缺少蒙皮分类的半套候选，下帧从同一游标继续。
            }; // 根依赖完整后这里只会短暂等待，不同步读取或复制网格。
            let material = primitive
                .material // None保持默认材质语义。
                .as_ref() // 不克隆材质资产。
                .map(|handle| {
                    catalog // 显式材质必须来自刚建立的根索引。
                        .material_indices
                        .get(&handle.id())
                        .copied()
                        .ok_or_else(|| "primitive关联了不属于当前GLTF根的材质".to_owned())
                })
                .transpose()?; // 坏关联使整份候选失败关闭，不猜Material0。
            catalog.choices.push(AssetPreviewPrimitiveChoice {
                id: AssetPreviewPrimitiveId {
                    mesh: primitive.parent_mesh_index, // 使用真实父网格编号。
                    primitive: primitive.index,        // 使用真实网格内primitive编号。
                },
                material, // 保留这个primitive自己的原材质。
                requires_skinning: direct_primitive_requires_skinning(primitive_mesh), // 蒙皮单体只能回到完整Scene0取得Skin与关节层级。
            }); // 候选只保存POD身份，不复制Mesh或名称。
            catalog.cursor.primitive += 1; // 下一帧预算从下一primitive继续。
        } else {
            catalog.cursor.mesh += 1; // 空网格也消耗一个预算单元。
            catalog.cursor.primitive = 0; // 新网格从首primitive开始。
        }
    }
    Ok(false) // 预算耗尽时保留游标，稳定帧绝不重头扫描。
}

/// 根元数据加载后按固定预算建立完整子资产目录，Ready前不展示部分结果。
pub(super) fn advance_model_catalog(
    server: Res<AssetServer>,             // 仅非阻塞检查已有异步任务。
    gltfs: Res<Assets<Gltf>>,             // 当前根完整数据。
    meshes: Res<Assets<GltfMesh>>,        // primitive真实编号和材质关联。
    primitive_meshes: Res<Assets<Mesh>>,  // 根依赖采用后常数次读取每个primitive的顶点布局。
    mut state: ResMut<AssetPreviewState>, // 只有推进时才修改当前模型会话。
) {
    if state.external_constructed {
        return;
    } // 应用组合根的完整依赖已由来源后台验证，不重复枚举GLB子资产。
    let Some((phase, handle)) = state
        .model // 图片和数据文件没有子资产目录。
        .as_ref()
        .map(|model| (model.catalog.phase, model.catalog.gltf.clone()))
    else {
        return; // 当前不是模型预览。
    };
    if matches!(
        phase,
        AssetPreviewCatalogPhase::Ready | AssetPreviewCatalogPhase::Failed
    ) {
        return; // 稳定帧不再访问根、网格或候选Vec。
    }
    if phase == AssetPreviewCatalogPhase::Loading {
        match asset_ready(&server, handle.id().untyped(), "GLTF子资产目录") {
            Err(error) => {
                let model = state.model.as_mut().expect("已确认存在模型会话"); // 同帧身份未改变。
                model.catalog.phase = AssetPreviewCatalogPhase::Failed; // 不公开半套候选。
                model.catalog.status = error; // Scene0画布拥有独立状态，可继续报告自身结果。
                return; // 失败等待真实热更事件重新建代。
            }
            Ok(false) => return, // AssetServer仍在工作，本帧立刻让出。
            Ok(true) => {}       // 完整根已可读取。
        }
        let Some(gltf) = gltfs.get(&handle) else {
            return; // Loaded与主世界Assets采用之间允许短暂窗口。
        };
        let model = state.model.as_mut().expect("已确认存在模型会话"); // 当前文件仍是同一根句柄。
        model.catalog.scene_count = gltf.scenes.len(); // 一次发布真实Scene总数。
        model.catalog.phase = AssetPreviewCatalogPhase::Collecting; // 后续帧从材质零号开始。
        model.catalog.status = "正在分帧枚举GLTF材质与primitive".to_owned(); // 不声称候选已经完整。
    }
    let Some(gltf) = gltfs.get(&handle) else {
        return; // 热更替换窗口中保留当前游标，不读取失效引用。
    };
    let model = state.model.as_mut().expect("已确认存在模型会话"); // 上方只处理当前唯一模型。
    match collect_catalog(&mut model.catalog, gltf, &meshes, &primitive_meshes) {
        // 同一固定预算内同时冻结蒙皮安全分类。
        Ok(false) => {
            model.catalog.status = format!(
                "正在枚举子资产 · 已完成{}个primitive",
                model.catalog.choices.len()
            ); // 进度只读已完成计数，不扫描候选内容。
        }
        Ok(true) => {
            model.catalog.phase = AssetPreviewCatalogPhase::Ready; // 完整列表一次对UI可见。
            model.catalog.page = 0; // 第一组与新目录同时发布。
            model.catalog.status = format!(
                "Scene共{}个 · 默认预览Scene0 · primitive共{}个",
                model.catalog.scene_count,
                model.catalog.choices.len()
            ); // 多Scene文件明确说明当前不是全部Scene。
        }
        Err(error) => {
            model.catalog.phase = AssetPreviewCatalogPhase::Failed; // 旧半成品候选不可点击。
            model.catalog.status = format!("子资产枚举失败：{error}"); // 保留可操作诊断。
        }
    }
}

/// 当前GLTF根或任一GltfMesh换代时，整份数字候选和单体预览共同失效。
pub(super) fn invalidate_model_hot_reload(
    mut commands: Commands, // 精确回收当前最多一个手工primitive实体。
    mut roots: feathrise_scene_core::residency::ResidentAssetEventReader<Gltf>, // 根换代可能重排Scene、Mesh和Material编号。
    mut meshes: MessageReader<AssetEvent<GltfMesh>>, // 局部网格换代同样可能重排primitive。
    mut state: ResMut<AssetPreviewState>,            // 当前文件会话与画布阶段。
    mut navigation: ResMut<AssetPreviewNavigation>,  // 新几何不能继承旧中心和缩放。
    mut scene_roots: Query<&mut Visibility, With<AssetPreviewModelRoot>>, // 换代过程禁止显示混合内容。
) {
    if state.external_constructed {
        roots.clear();
        meshes.clear();
        return;
    } // 应用组合根热更新由其完整Ready回执重新启动联合扫描。
    let Some((root_id, phase, hot_reload_armed)) = state
        .model // 非模型选择仍要清空自己的消息游标。
        .as_ref()
        .map(|model| {
            (
                model.catalog.gltf.id(), // 只匹配当前GLTF根。
                model.catalog.phase,     // 首次目录完整前只排空初始化事件。
                model.hot_reload_armed,  // 基线建立前的Modified可能只是同源标签补载。
            )
        })
    else {
        roots.clear(); // 不把历史事件积累到下一次模型选择。
        meshes.clear(); // Mesh事件同样只作用于当帧当前会话。
        return;
    };
    if !hot_reload_armed {
        roots.clear(); // 首次请求同源Gltf、Scene与primitive时，AssetServer可能用Modified补齐既有缓存。
        meshes.clear(); // 这些消息描述当前首次完整结果，不能清除刚收到的WorldInstanceReady。
        if phase != AssetPreviewCatalogPhase::Loading {
            state
                .model
                .as_mut()
                .expect("已确认存在模型会话")
                .hot_reload_armed = true; // 目录开始枚举或已失败都说明初载已结算；下一帧识别真实修改并允许失败恢复。
        }
        return;
    }
    let root_overflow = roots.len() > HOT_RELOAD_EVENTS_PER_FRAME; // 根事件超量时不能证明当前Scene仍是同一代。
    let mesh_overflow = meshes.len() > HOT_RELOAD_EVENTS_PER_FRAME; // 网格事件超量时保守重建当前候选但可复用Scene实体。
    let root_changed = roots
        .read()
        .take(HOT_RELOAD_EVENTS_PER_FRAME)
        .any(|event| match event {
            AssetEvent::Modified { id } | AssetEvent::Removed { id } => *id == root_id, // 只匹配当前根。
            _ => false, // Added、Unused与LoadedWithDependencies不会使已公开编号过期。
        });
    let mesh_changed = {
        let model = state.model.as_ref().expect("已确认存在模型会话"); // 候选增量持有已见网格身份。
        meshes
            .read()
            .take(HOT_RELOAD_EVENTS_PER_FRAME)
            .any(|event| match event {
                AssetEvent::Modified { id } | AssetEvent::Removed { id } => {
                    model.catalog.mesh_ids.contains(id) // 不让无关GLTF热更重置当前预览。
                }
                _ => false, // Added、Unused与LoadedWithDependencies不改变已公开网格内容。
            })
    };
    roots.clear(); // 剩余消息不跨帧重复触发同一次失效。
    meshes.clear(); // 超预算已经由overflow转换为一次整代重建。
    if !root_overflow && !mesh_overflow && !root_changed && !mesh_changed {
        return; // 当前模型身份没有变化。
    }
    let scene_root = state.scene_root(); // 在可变借用模型会话前冻结固定实体身份。
    let (gltf, primitive_root, scene_was_ready) = {
        let model = state.model.as_ref().expect("已确认存在模型会话"); // 当前模型仍在本系统内。
        (
            model.catalog.gltf.clone(), // 句柄沿用AssetServer热更身份。
            model.primitive_root,       // 旧单体立即退出。
            model.scene_ready,          // 仅网格依赖换代时现有Scene实体仍可重新扫描。
        )
    };
    if let Some(root) = primitive_root {
        commands.entity(root).try_despawn(); // 旧Mesh或材质不能继续出现在离屏画面。
    }
    if let Some(mut root) = scene_root.and_then(|root| scene_roots.get_mut(root).ok()) {
        *root = Visibility::Hidden; // 新Scene0后代重新隔离前绝不解除隐藏。
    }
    let rebuild_scene = root_overflow || root_changed;
    if rebuild_scene {
        let path = state.selected_path.clone().unwrap_or_default();
        if let AssetPreviewContent::Model { handle, root, .. } = &mut state.content {
            // Gltf 根的 Modified 也可能只是元数据补载。旧 WorldAsset 实例这时不会再发送
            // Ready；换成同一强句柄的新隐藏实例，保证后续只接受当前根的完整通知。
            commands.entity(*root).try_despawn();
            *root = super::model::spawn_scene0(&mut commands, &path, handle.clone());
        }
    }
    let generation = state.allocate_model_generation(); // 所有旧单体扫描立即成为迟到结果。
    let rescan_root = scene_root.filter(|_| scene_was_ready && !rebuild_scene); // 纯GltfMesh换代继续复用现有Scene0。
    let mut replacement = AssetPreviewModelSession::new(generation, gltf); // 清空候选、页码、边界和目标。
    replacement.scene_ready = rescan_root.is_some(); // 根换代必须等待新实例，网格换代可重扫现有层级。
    state.model = Some(replacement); // 原位采用同一路径新会话。
    state.bounds_scan = rescan_root.map(AssetPreviewBoundsScan::new);
    state.model_bounds = None; // 旧几何范围不能用于新代取景。
    navigation.reset_for_model_target(); // 热更回到完整Scene0视角，保留当前文件的吸附偏好。
    if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
        *phase = if rescan_root.is_some() {
            AssetPreviewPhase::Scanning // Mesh依赖更新后按固定节点预算重新取景。
        } else {
            AssetPreviewPhase::Loading // 根更新等待新的WorldInstanceReady成功门。
        };
    }
    state.status = if rescan_root.is_some() {
        "模型网格已热更新，正在重新验证子资产并计算Scene0取景".to_owned() // 不等待不会重建的WorldAsset根。
    } else {
        "模型资源已热更新，正在重新验证子资产并加载Scene0".to_owned() // 根换代保持隐藏直到Ready。
    };
}

/// 将局部Aabb转换为自动取景使用的最小/最大值。
fn aabb_extents(aabb: &Aabb) -> (Vec3, Vec3) {
    let center = Vec3::from(aabb.center); // Aabb内部使用Vec3A，先转为普通Vec3。
    let half = Vec3::from(aabb.half_extents); // 半尺寸保持模型局部坐标。
    (center - half, center + half) // 单体Transform为单位矩阵，无需额外世界变换。
}

/// 按固定顶点切片推进单体边界；Some只在完整范围可原子采用时返回。
fn advance_primitive_bounds(
    load: &mut AssetPreviewPrimitiveLoad, // 跨帧保留下一个顶点游标和累计范围。
    mesh: &Mesh,                          // 当前精确primitive的只读网格。
) -> Result<Option<(Vec3, Vec3)>, String> {
    if mesh.final_aabb.is_some() {
        let aabb = mesh
            .get_aabb() // RenderWorld已提取顶点时复用预计算结果，为常数成本。
            .ok_or_else(|| "primitive预计算边界不可用".to_owned())?;
        return Ok(Some(aabb_extents(&aabb))); // 不再访问顶点数组。
    }
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Err("primitive缺少Float32x3位置属性，无法安全计算预览范围".to_owned()); // 不伪造单位方盒。
    };
    if positions.is_empty() {
        return Err("primitive没有位置顶点，无法显示".to_owned()); // 空网格不会生成无意义实体。
    }
    let end = load
        .bounds
        .cursor
        .saturating_add(PRIMITIVE_VERTICES_PER_FRAME)
        .min(positions.len()); // 单帧工作量有明确上限。
    for position in &positions[load.bounds.cursor..end] {
        let position = Vec3::from(*position); // 一个顶点只做常数次比较。
        if !position.is_finite() {
            return Err("primitive包含NaN或无穷位置，无法自动取景".to_owned()); // 坏数据不能污染相机矩阵。
        }
        load.bounds.minimum = load.bounds.minimum.min(position); // 累计局部下界。
        load.bounds.maximum = load.bounds.maximum.max(position); // 累计局部上界。
    }
    load.bounds.cursor = end; // 下一帧从未处理位置继续。
    Ok((end == positions.len()).then_some((load.bounds.minimum, load.bounds.maximum))) // 完整顶点完成后才发布。
}

/// 非阻塞加载并显示当前唯一primitive，原材质和完整边界同时就绪后才生成实体。
pub(super) fn advance_primitive_preview(
    mut commands: Commands,                   // 最终完整采用时只生成一个预览实体。
    server: Res<AssetServer>,                 // Mesh与StandardMaterial加载状态。
    meshes: Res<Assets<Mesh>>,                // 固定预算读取位置数组。
    materials: Res<Assets<StandardMaterial>>, // 确认/std真实存在再渲染。
    mut state: ResMut<AssetPreviewState>,     // 当前目标、代号和画布阶段。
) {
    if !state
        .model
        .as_ref()
        .is_some_and(|model| model.primitive_load.is_some())
    {
        return; // 稳定Scene0或单体帧只读退出，不把资源误标为Changed并重复刷新固定UI。
    }
    let Some(mut load) = state
        .model // 图片、Scene0模式和稳定单体帧立即退出。
        .as_mut()
        .and_then(|model| model.primitive_load.take())
    else {
        return;
    };
    let current = state.model.as_ref().is_some_and(|model| {
        model.generation == load.generation // 必须仍是请求时文件代。
            && model.selected_primitive == Some(load.choice) // 必须仍是作者明确选择项。
            && model.target == AssetPreviewModelTarget::Primitive(load.choice.id) // 展示目标不能已经切回Scene0。
    });
    if !current {
        return; // 迟到句柄或扫描直接释放，不产生实体或状态副作用。
    }
    for (id, description) in [
        (load.mesh.id().untyped(), "primitive网格"), // 精确Mesh子资产。
        (load.material.id().untyped(), "primitive关联材质"), // 精确StandardMaterial派生资产。
    ] {
        match asset_ready(&server, id, description) {
            Ok(true) => {} // 继续验证主世界Assets。
            Ok(false) => {
                state.model.as_mut().expect("当前模型仍存在").primitive_load = Some(load); // 保留唯一在途状态。
                return; // 本帧不阻塞等待IO。
            }
            Err(error) => {
                if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
                    *phase = AssetPreviewPhase::Failed; // 当前单体不可显示，但Scene0按钮仍可恢复。
                }
                state.model_bounds = None; // 不使用上一个目标范围。
                state.status = error; // 显示真实Mesh或材质失败。
                return; // 失败不生成半材质实体。
            }
        }
    }
    if !materials.contains(load.material.id()) {
        state.model.as_mut().expect("当前模型仍存在").primitive_load = Some(load); // Loaded到Assets采用允许一帧窗口。
        return; // 不用默认白材质掩盖关联材质尚未到达。
    }
    let Some(mesh) = meshes.get(&load.mesh) else {
        state.model.as_mut().expect("当前模型仍存在").primitive_load = Some(load); // 等待主世界采用真实Mesh。
        return; // 不同步重载或复制网格。
    };
    if direct_primitive_requires_skinning(mesh) {
        // 最终生成前复验真实顶点布局，防止热更或异常状态绕过目录分类。
        if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
            // 保留Scene0恢复按钮和当前文件会话。
            *phase = AssetPreviewPhase::Failed; // 蒙皮Mesh绝不进入缺少SkinnedMesh组件的渲染管线。
        } // 失败态只阻止当前单体，不销毁已加载的完整Scene0。
        state.model_bounds = None; // 不把Scene0或上一个静态单体范围冒充当前结果。
        state.status = "该primitive是蒙皮网格，必须选择完整场景Scene0预览".to_owned(); // 给作者明确恢复路径而不是触发GPU校验崩溃。
        return; // 不生成裸Mesh3d，因此RenderApp不会取得不兼容的model-only绑定组。
    } // 静态primitive继续使用原句柄和固定预算边界路径。
    let cached = state
        .model
        .as_ref()
        .and_then(|model| model.local_bounds.get(&load.mesh.id()))
        .map(aabb_extents); // Scene0已经分帧见过的primitive直接复用加载器Aabb。
    let bounds = match cached {
        Some(bounds) => Some(bounds), // 常见Scene0成员无需再次读取顶点。
        None => match advance_primitive_bounds(&mut load, mesh) {
            Ok(bounds) => bounds, // Scene0外孤立primitive按固定切片推进。
            Err(error) => {
                if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
                    *phase = AssetPreviewPhase::Failed; // 数据格式失败保持Scene0恢复入口。
                }
                state.model_bounds = None; // 坏范围不进入相机。
                state.status = error; // 精确说明失败原因。
                return; // 不创建不可取景实体。
            }
        },
    };
    let Some((minimum, maximum)) = bounds else {
        let cursor = load.bounds.cursor; // 移回状态前读取小型进度值。
        state.model.as_mut().expect("当前模型仍存在").primitive_load = Some(load); // 跨帧继续同一Mesh。
        state.status = format!("正在分帧计算primitive取景范围 · 已检查{cursor}个顶点"); // 不扫描总目录。
        return; // 固定预算耗尽立即让出主线程。
    };
    if !minimum.is_finite() || !maximum.is_finite() || minimum.cmpgt(maximum).any() {
        if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
            *phase = AssetPreviewPhase::Failed; // 非法范围不能进入投影数学。
        }
        state.model_bounds = None; // 保留显式失败而非旧画面。
        state.status = "primitive边界无效，无法自动取景".to_owned(); // 作者可回Blender检查网格。
        return; // 不创建实体。
    }
    let aabb = Aabb::from_min_max(minimum, maximum); // 已完整计算的局部范围一次采用。
    let root = commands
        .spawn((
            Name::new(format!(
                "子资产预览_Mesh{}_Primitive{}",
                load.choice.id.mesh, load.choice.id.primitive
            )), // 可在ECS诊断中识别真实编号。
            AssetPreviewPrimitiveRoot, // 与Scene0根互斥显示的唯一单体marker。
            MapEditorOwnedEntity,      // F12统一生命周期审计。
            Mesh3d(load.mesh),         // 精确primitive Mesh句柄，不创建顶点副本。
            MeshMaterial3d(load.material), // 精确原关联StandardMaterial。
            Transform::IDENTITY,       // 单体使用网格局部坐标并由相机居中，不继承Scene节点变换。
            aabb,                      // 固定预算得到的完整局部边界。
            NoAutoAabb,                // 热更由本状态机换代，禁止Bevy同步重扫全部顶点。
            Visibility::Hidden,        // 相机和实体命令完整落地一帧后才显示。
            RenderLayers::layer(MAP_EDITOR_ASSET_PREVIEW_LAYER), // 不进入地图或玩法相机。
        ))
        .id(); // 同时最多保留一个手工单体实体。
    let size = maximum - minimum; // 状态栏只读取已采用范围。
    let model = state.model.as_mut().expect("当前模型仍存在"); // 同一代成功采用。
    if let Some(old_root) = model.primitive_root.replace(root) {
        commands.entity(old_root).try_despawn(); // 防御重复选择同帧，只保留最后一个实体。
    }
    model.primitive_load = None; // 句柄已移交实体组件持有。
    state.model_bounds = Some(AssetPreviewModelBounds { minimum, maximum }); // 导航从新单体中心重新取景。
    if let AssetPreviewContent::Model { phase, .. } = &mut state.content {
        *phase = AssetPreviewPhase::RevealPending; // PostUpdate下一次只解除当前root隐藏。
    }
    state.status = format!(
        "单体预览 · Mesh{}/Primitive{} · 尺寸 {:.2} × {:.2} × {:.2} m",
        load.choice.id.mesh, load.choice.id.primitive, size.x, size.y, size.z
    ); // 不把单体冒充Scene0完整网格数。
}

#[cfg(test)] // 纯引用、预算枚举与材质关联不启动窗口或GPU。
#[path = "subassets_tests.rs"] // 大型夹具与生产系统职责分离。
mod tests; // 覆盖真实编号、分页基础与固定顶点游标。
