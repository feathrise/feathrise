//! 建筑点放模型幽灵的单根缓存、F12专层隔离和固定预算Scene0准备。
//!
//! 【功能设计】选择模型变化时才创建或替换根；光标移动只更新根变换。Scene0在
//! `WorldInstanceReady`后保持隐藏，每帧最多处理固定数量节点，全部材质和渲染层安全
//! 切换后下一帧才显示，避免玩法相机闪现源模型或在稳定帧重复遍历层级。

use bevy::asset::{DependencyLoadState, LoadState, RecursiveDependencyLoadState}; // primitive全部依赖就绪后才生成渲染实体。
use bevy::camera::visibility::RenderLayers; // 幽灵网格只进入F12辅助渲染层。
use bevy::gltf::GltfAssetLabel; // Scene0句柄与正式摆放使用同一GLTF标签。
use bevy::prelude::*; // ECS、PBR材质、Transform和层级查询。
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady}; // 完整Scene0生成后才启动隔离扫描。
use feathrise_map_content::{BuildingPresetRegistry, MapPlacementAssetTarget}; // 当前目录提供模型来源和精确子目标。
use feathrise_scene::direct_primitive_requires_skinning; // 与正式摆放共用蒙皮primitive失败关闭规则。

use super::preview::resolve_building_preview; // 模型、占地和箭头共用唯一候选解析。
use super::resources::{BuildingAuthoringState, BuildingGhostMaterials, BuildingGhostPreviewState}; // 状态颜色、共享材质和唯一根生命周期。
use super::types::{
    BuildingGhostAssetKey, BuildingGhostPhase, BuildingGhostPrimitiveLoad, BuildingGhostScan,
    BuildingPreviewTone,
}; // 小型稳定键与有限准备阶段。
use crate::authoring::EditorAuthoringSession; // 当前地图草稿与校验结果共享身份。
use crate::components::{
    EditorBuildingGhost, EditorBuildingGhostPrimitive, MapEditorCamera, MapEditorOwnedEntity,
}; // 根、已隔离网格、F12相机和作者所有权标记。
use crate::resources::MapEditorState; // 当前工具、选择和真实地形光标。
use crate::types::EditorTool; // 切离建筑工具时立即回收幽灵根。
use crate::ui::building_presets::BuildingPlacementControls; // 网格和九十度旋转驱动根变换。
use crate::viewport::MAP_EDITOR_BRUSH_RENDER_LAYER; // 与占地Gizmo相同的F12专用层。

const GHOST_DESCENDANTS_PER_FRAME: usize = 128; // 大Scene0隔离不得在单帧完整遍历。

/// 创建一份不读取源纹理、双面且不受天气光照影响的半透明幽灵材质。
fn ghost_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,            // 统一色相和Alpha直接表达候选状态。
        alpha_mode: AlphaMode::Blend, // 地形和既有建筑仍可透过模型观察。
        double_sided: true,           // 薄墙与屋面从内外观察都保持轮廓。
        cull_mode: None,              // 显式兑现双面显示，不依赖源材质设置。
        unlit: true,                  // F12天气与昼夜不会改变红绿琥珀辨识。
        fog_enabled: false,           // 远景雾不应吞没作者正在操作的候选。
        ..default()                   // 不复制源纹理、法线或昂贵材质扩展。
    }
}

/// 启动时建立三份固定材质；后续切状态只替换轻量句柄。
pub(super) fn setup_building_ghost_materials(
    mut commands: Commands, // 固定句柄以资源形式供全部系统共享。
    mut materials: ResMut<Assets<StandardMaterial>>, // 每种候选状态只登记一份材质资产。
) {
    commands.insert_resource(BuildingGhostMaterials {
        pending: materials.add(ghost_material(Color::srgba(1.0, 0.58, 0.08, 0.30))), // 未校验琥珀色。
        valid: materials.add(ghost_material(Color::srgba(0.10, 1.0, 0.34, 0.28))), // 合法候选绿色。
        invalid: materials.add(ghost_material(Color::srgba(1.0, 0.10, 0.07, 0.32))), // 非法候选红色。
    });
}

/// 返回当前候选状态的共享材质句柄。
fn material_for(
    materials: &BuildingGhostMaterials, // 启动期建立的三份固定资产。
    tone: BuildingPreviewTone,          // 与Gizmo一致的轻量三态值。
) -> Handle<StandardMaterial> {
    materials.handle(tone) // 统一入口避免Scene0扫描与primitive生成选错颜色。
}

/// 回收当前唯一根并清除所有迟到扫描身份。
fn retire_ghost(commands: &mut Commands, state: &mut BuildingGhostPreviewState) {
    if let Some(root) = state.root {
        commands.entity(root).try_despawn(); // ChildOf关系随根一起回收完整临时模型。
    }
    state.clear(); // WorldInstanceReady或旧队列到达后不再拥有采用资格。
}

/// 按当前预设模型创建一个隐藏根；Scene0等待完成事件，primitive等待安全网格。
fn spawn_ghost_root(
    commands: &mut Commands,    // 只在所选模型真实变化时生成实体。
    asset_server: &AssetServer, // GLTF Scene0或精确Mesh句柄均由正式加载器管理。
    preset: &feathrise_map_content::BuildingPresetDescriptor, // 当前已采用目录中的完整模型身份。
    transform: Transform,       // 模型根与Gizmo共享位置和旋转。
) -> (
    Entity,                             // 新建的唯一隐藏模型根。
    BuildingGhostPhase,                 // Scene0与primitive进入各自准备阶段。
    Option<BuildingGhostPrimitiveLoad>, // 仅静态primitive需要等待精确Mesh。
) {
    let root = commands
        .spawn((
            Name::new(format!("建筑预览_{}", preset.label)), // 诊断树中可直接识别当前模型。
            EditorBuildingGhost,                                // 唯一幽灵根生命周期标记。
            MapEditorOwnedEntity,                               // 纳入F12作者实体审计。
            RenderLayers::layer(MAP_EDITOR_BRUSH_RENDER_LAYER), // 根本身也不进入玩法相机。
            transform,                                          // 首帧即冻结当前模型姿态。
            Visibility::Hidden,                                 // 所有渲染组件安全隔离前绝不显示。
        ))
        .id();
    match preset.asset_target {
        MapPlacementAssetTarget::Cuboid { .. } | MapPlacementAssetTarget::EditableCuboid { .. } | MapPlacementAssetTarget::EditableMesh { .. } => {
            // 内置形状由专用添加入口管理；建筑目录校验拒绝此来源，守住意外热更新的失败边界。
            commands.entity(root).try_despawn();
            (root, BuildingGhostPhase::Loading, None)
        }
        MapPlacementAssetTarget::Scene0 => {
            let scene = asset_server.load(
                GltfAssetLabel::Scene(0).from_asset(preset.asset_path.clone()), // 与正式placement完全相同的Scene0来源。
            );
            commands.entity(root).insert(WorldAssetRoot(scene)); // World实例完成事件启动后代扫描。
            (root, BuildingGhostPhase::Loading, None) // 根保持隐藏直到完整层级隔离完成。
        }
        MapPlacementAssetTarget::Primitive {
            mesh, primitive, ..
        } => {
            let mesh_reference = preset
                .asset_target
                .mesh_reference(&preset.asset_path)
                .expect("primitive建筑目标必须生成精确Mesh引用"); // 目录已经完成目标格式校验。
            let load = BuildingGhostPrimitiveLoad {
                mesh: asset_server.load::<Mesh>(mesh_reference), // 强句柄异步请求原始几何，不复制顶点。
                mesh_index: mesh,                                // 完成时恢复精确父网格编号。
                primitive_index: primitive,                      // 完成时恢复精确单体编号。
                reported_failure: None,                          // 首次错误应报告给作者。
            };
            (root, BuildingGhostPhase::PrimitiveLoading, Some(load)) // 真实Mesh安全后才生成渲染子实体。
        }
    }
}

/// 返回静态primitive任一加载层级的确定失败；等待态不伪造成错误。
fn primitive_load_failure(
    asset_server: &AssetServer, // Bevy异步资产状态的唯一事实来源。
    mesh: &Handle<Mesh>,        // 当前建筑目标的精确Mesh子资产。
) -> Option<String> {
    let (direct, dependencies, recursive) = asset_server.get_load_states(mesh.id())?; // 刚请求的句柄允许尚无状态。
    match direct {
        LoadState::Failed(error) => Some(format!("建筑幽灵primitive网格加载失败：{error}")), // 保留加载器原始原因。
        _ if matches!(dependencies, DependencyLoadState::Failed(_)) => {
            Some("建筑幽灵primitive的直接依赖加载失败".to_owned()) // 缓冲依赖损坏不能继续生成Mesh3d。
        }
        _ if matches!(recursive, RecursiveDependencyLoadState::Failed(_)) => {
            Some("建筑幽灵primitive的递归依赖加载失败".to_owned()) // 深层依赖同样失败关闭。
        }
        _ => None, // 仍在加载或已经完整就绪。
    }
}

/// 同一primitive失败只报告一次，稳定失败帧不反复改写状态栏或日志。
fn report_primitive_failure(
    load: &mut BuildingGhostPrimitiveLoad, // 保存最近一次已公开失败文本。
    editor: &mut MapEditorState,           // 作者应看到模型未显示的真实原因。
    message: String,                       // 本次加载或蒙皮安全错误。
) {
    if load.reported_failure.as_deref() == Some(message.as_str()) {
        return; // 完全相同的失败保持稳定只读。
    }
    warn!("{message}"); // 日志保留可搜索诊断。
    editor.status = message.clone(); // F12状态栏直接反馈，不要求作者查看控制台。
    load.reported_failure = Some(message); // 后续稳定帧不重复发布。
}

/// 等待精确Mesh进入主世界并复用正式摆放的蒙皮安全门，再生成唯一渲染子实体。
pub(super) fn advance_building_ghost_primitive(
    mut commands: Commands,                       // 完整就绪时一次生成子实体。
    asset_server: Res<AssetServer>,               // 非阻塞读取加载与依赖状态。
    meshes: Res<Assets<Mesh>>,                    // 只读取当前一个精确网格的顶点布局。
    materials: Res<BuildingGhostMaterials>,       // 单体直接使用当前共享幽灵材质。
    mut editor: ResMut<MapEditorState>,           // 首个确定失败进入F12状态栏。
    mut state: ResMut<BuildingGhostPreviewState>, // 唯一加载句柄和根身份。
    roots: Query<(), With<EditorBuildingGhost>>,  // 外部回收后不得向悬空根添加子项。
) {
    if state.phase != BuildingGhostPhase::PrimitiveLoading {
        return; // Scene0、完成态和空态不轮询primitive资产。
    }
    let Some(root) = state.root else {
        state.clear(); // 加载阶段缺根时失败关闭整份状态。
        return;
    };
    if !roots.contains(root) {
        state.clear(); // 根已被外部F12生命周期回收，下一帧按当前选择重建。
        return;
    }
    let Some(mut load) = state.primitive_load.take() else {
        state.clear(); // PrimitiveLoading必须拥有精确强句柄。
        return;
    };
    if let Some(error) = primitive_load_failure(&asset_server, &load.mesh) {
        report_primitive_failure(&mut load, &mut editor, error); // 失败保持隐藏且允许同句柄热更后恢复。
        state.primitive_load = Some(load); // 保留唯一强句柄，不启动重复请求。
        return;
    }
    if !asset_server.is_loaded_with_dependencies(load.mesh.id()) {
        state.primitive_load = Some(load); // 异步等待期间不阻塞主线程。
        return;
    }
    let Some(mesh) = meshes.get(&load.mesh) else {
        state.primitive_load = Some(load); // Loaded到主世界Assets之间允许一帧采用窗口。
        return;
    };
    if direct_primitive_requires_skinning(mesh) {
        report_primitive_failure(
            &mut load,   // 同一错误只发布一次。
            &mut editor, // 状态栏说明应改用完整Scene0。
            "建筑幽灵不能直接显示蒙皮primitive，请把建筑预设模型目标改为Scene0".to_owned(), // 与正式摆放失败关闭原因一致。
        );
        state.primitive_load = Some(load); // 保留句柄，使GLB热改为静态网格后可以自动恢复。
        return;
    }
    let recovered = load.reported_failure.is_some(); // 成功前记住是否需要撤销旧失败反馈。
    let material = material_for(&materials, state.tone); // 采用本帧统一候选状态颜色。
    commands.entity(root).with_child((
        Name::new(format!(
            "建筑预览_Mesh{}_Primitive{}", // 精确子目标便于场景诊断。
            load.mesh_index,                  // 父网格正式编号。
            load.primitive_index,             // 父网格内单体正式编号。
        )),
        EditorBuildingGhostPrimitive, // 颜色变化只定向更新这一小型集合。
        Mesh3d(load.mesh),            // 已确认静态安全的精确网格强句柄。
        MeshMaterial3d(material),     // 原材质不进入幽灵，避免纹理和Alpha语义分叉。
        RenderLayers::layer(MAP_EDITOR_BRUSH_RENDER_LAYER), // 玩法相机永远不可见。
        Transform::IDENTITY,          // primitive继续使用GLTF网格自身局部坐标。
        Visibility::Inherited,        // 只服从隐藏根的最终显隐门。
    ));
    state.primitive_load = None; // 强句柄已经移交Mesh3d组件持有。
    state.phase = BuildingGhostPhase::RevealPending; // 下一帧确认子实体命令落地后才显示根。
    if recovered {
        editor.status = "建筑模型幽灵已恢复并完成准备".to_owned(); // 清除旧失败状态栏，明确热更恢复成功。
    }
}

/// 同步当前模型根、姿态、显隐和状态颜色；稳定帧不扫描Scene0。
#[allow(clippy::too_many_arguments)] // 每项输入分别拥有选择、几何、校验、资产和ECS生命周期职责。
pub(super) fn sync_building_ghost(
    mut commands: Commands,                        // 模型变化时替换唯一根。
    asset_server: Res<AssetServer>,                // 新根按正式标签请求异步资产。
    editor: Res<MapEditorState>,                   // 工具、选择和光标决定当前候选。
    session: Res<EditorAuthoringSession>,          // 红绿结果必须仍属于当前地图草稿。
    registry: Option<Res<BuildingPresetRegistry>>, // 目录损坏时不借用悬空模型。
    controls: Res<BuildingPlacementControls>,      // 网格和旋转与Gizmo一致。
    preferences: Option<Res<feathrise_editor_core::EditorSettings>>,
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    authoring: Res<BuildingAuthoringState>,        // 最近后台结果提供状态颜色和精确Y。
    materials: Res<BuildingGhostMaterials>,        // 三态颜色只替换共享句柄。
    mut state: ResMut<BuildingGhostPreviewState>,  // 唯一根、扫描和准备阶段。
    mut roots: Query<(&mut Transform, &mut Visibility), With<EditorBuildingGhost>>, // 根更新不遍历后代。
    mut primitives: Query<
        &mut MeshMaterial3d<StandardMaterial>,
        With<EditorBuildingGhostPrimitive>,
    >, // 只有状态真实变化才遍历当前幽灵网格。
) {
    let active_preset = registry.as_deref().and_then(|registry| {
        (editor.open && editor.tool == EditorTool::Building)
            .then_some(editor.building_selected_preset_id.as_deref())
            .flatten()
            .and_then(|selected| registry.resolve(selected))
    }); // 关闭、切工具、目录卸载或悬空选择都必须释放临时World实例。
    let Some(preset) = active_preset else {
        retire_ghost(&mut commands, &mut state); // 不让幽灵泄漏到玩法画面或其他作者工具。
        return;
    };
    let preview = registry.as_deref().and_then(|registry| {
        let snap = preferences.as_ref().map(|settings| controls.snap_policy(settings.editor_snapping, keyboard.as_ref().is_some_and(|keys| keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])), [0.0; 3]));
        resolve_building_preview(&editor, &session, registry, &controls, &authoring, snap)
    }); // 缺光标只隐藏并缓存同一根，避免鼠标经过侧栏时重复加载GLB。
    let same_asset = state.key.as_ref().is_some_and(|key| key.matches(preset)); // 借用比较不会在鼠标移动帧分配字符串。
    let root_alive = state.root.is_some_and(|root| roots.contains(root)); // 外部清理后允许安全重建。
    if !same_asset || !root_alive {
        retire_ghost(&mut commands, &mut state); // 旧根、热加载扫描和显示阶段同批失效。
        let Some(preview) = preview.as_ref() else {
            return; // 没有地形命中时等待下一次有效光标再创建模型。
        };
        let transform = Transform::from_translation(preview.root)
            .with_rotation(Quat::from_rotation_y(preview.yaw_degrees.to_radians())); // 模型与箭头共享唯一Y轴角。
        let (root, phase, primitive_load) =
            spawn_ghost_root(&mut commands, &asset_server, preview.preset, transform);
        state.root = Some(root); // 新WorldInstanceReady只接受这一个根。
        state.key = Some(BuildingGhostAssetKey::from_preset(preview.preset)); // 后续稳定帧只借用比较。
        state.phase = phase; // Scene0等待事件，primitive等待一帧显示。
        state.scan = None; // 新根尚无Scene0扫描工作。
        state.primitive_load = primitive_load; // 仅静态单体保留一个精确Mesh强句柄。
        state.tone = preview.tone; // 新节点和未来扫描使用当前颜色。
        state.desired_visible = true; // 当前真实地形命中允许准备完成后显示。
        return;
    }
    let Some(root) = state.root else {
        return; // 上方存活门已经防御该状态，仅保留无panic退路。
    };
    let Some(preview) = preview else {
        state.desired_visible = false; // 鼠标离开视口时仍保留已加载模型缓存。
        if let Ok((_, mut visibility)) = roots.get_mut(root) {
            *visibility = Visibility::Hidden; // 同帧与Gizmo一起隐藏，不留上一个落点残影。
        }
        return;
    };
    state.desired_visible = true; // 当前模型、选择和真实落点全部有效。
    if state.tone != preview.tone {
        state.tone = preview.tone; // 后续Scene扫描节点也采用新状态。
        let material = material_for(&materials, preview.tone); // 只克隆一个轻量强句柄。
        for mut current in &mut primitives {
            if current.0 != material {
                current.0 = material.clone(); // 单个幽灵的少量primitive同批切换红绿琥珀。
            }
        }
    }
    let Ok((mut transform, mut visibility)) = roots.get_mut(root) else {
        state.clear(); // 根在存活检查后被其他系统回收时让下一帧完整重建。
        return;
    };
    let desired_transform = Transform::from_translation(preview.root)
        .with_rotation(Quat::from_rotation_y(preview.yaw_degrees.to_radians())); // 与Gizmo同一解析结果。
    if *transform != desired_transform {
        *transform = desired_transform; // 光标或九十度旋转真实变化时才推进Transform检测。
    }
    let desired_visibility = if state.phase == BuildingGhostPhase::Ready {
        Visibility::Inherited // 完整隔离后才允许F12相机绘制。
    } else {
        Visibility::Hidden // Loading、Scanning与RevealPending都不能闪现源层。
    };
    if *visibility != desired_visibility {
        *visibility = desired_visibility; // 稳定Ready帧不重复写显隐组件。
    }
}

/// Scene0首次加载或热更完成时隐藏根并重建固定预算隔离队列。
pub(super) fn observe_building_ghost_world_ready(
    ready: On<WorldInstanceReady>, // 事件只指向本次完整生成的WorldAssetRoot。
    mut state: ResMut<BuildingGhostPreviewState>, // 当前根身份拒绝旧模型迟到事件。
    mut roots: Query<&mut Visibility, With<EditorBuildingGhost>>, // 热更后先关闭可见性。
) {
    if state.root != Some(ready.entity) {
        return; // 正式地图、普通placement和已经替换的旧幽灵都不属于本观察者。
    }
    let Ok(mut visibility) = roots.get_mut(ready.entity) else {
        return; // 根已回收时迟到事件没有任何副作用。
    };
    *visibility = Visibility::Hidden; // 新后代仍带默认层，隔离完成前绝不显示。
    state.phase = BuildingGhostPhase::Scanning; // 热更与首次加载统一走完整扫描。
    state.scan = Some(BuildingGhostScan::new(ready.entity)); // 队列只保存本代实体ID。
}

/// 每帧隔离固定数量Scene0节点；全部命令落地一帧后再显示根。
pub(super) fn advance_building_ghost_scan(
    mut commands: Commands,                 // 为本帧有限节点安装专层和幽灵材质。
    materials: Res<BuildingGhostMaterials>, // 新遇到的网格使用当前三态共享句柄。
    mut state: ResMut<BuildingGhostPreviewState>, // 唯一扫描队列与显示阶段。
    children: Query<&Children>,             // 只遍历当前隐藏根的层级。
    standard_meshes: Query<(), (With<Mesh3d>, With<MeshMaterial3d<StandardMaterial>>)>, // 只接管正式标准材质网格。
    mut embedded_cameras: Query<&mut Camera, Without<MapEditorCamera>>, // 防御外部GLB残留活动相机。
    mut roots: Query<&mut Visibility, With<EditorBuildingGhost>>, // RevealPending下一帧才解除隐藏。
) {
    if state.phase == BuildingGhostPhase::RevealPending {
        let Some(root) = state.root else {
            state.clear(); // 根身份缺失时不能发布Ready。
            return;
        };
        let Ok(mut visibility) = roots.get_mut(root) else {
            state.clear(); // 根已回收时丢弃完整准备结果。
            return;
        };
        *visibility = if state.desired_visible {
            Visibility::Inherited // 当前仍有真实地形命中才显示。
        } else {
            Visibility::Hidden // 鼠标离开期间只缓存已准备根。
        };
        state.phase = BuildingGhostPhase::Ready; // 稳定帧不再扫描层级。
        return;
    }
    if state.phase != BuildingGhostPhase::Scanning {
        return; // Empty、Loading和Ready均没有主线程层级工作。
    }
    let Some(mut scan) = state.scan.take() else {
        state.clear(); // Scanning必须拥有完整队列，异常状态失败关闭。
        return;
    };
    if state.root != Some(scan.root) || !roots.contains(scan.root) {
        state.clear(); // 换选择或根删除后旧队列不得修改其他实体。
        return;
    }
    let material = material_for(&materials, state.tone); // 本预算批次只克隆同一共享句柄。
    for _ in 0..GHOST_DESCENDANTS_PER_FRAME {
        let Some(entity) = scan.pending.pop_front() else {
            break; // 当前代全部节点已经处理完成。
        };
        scan.visited = scan.visited.saturating_add(1); // 诊断计数不允许异常溢出。
        commands
            .entity(entity)
            .insert(RenderLayers::layer(MAP_EDITOR_BRUSH_RENDER_LAYER)); // 覆盖源层，玩法相机永远不可见。
        if standard_meshes.contains(entity) {
            commands.entity(entity).insert((
                EditorBuildingGhostPrimitive,     // 颜色变化定向更新已隔离网格。
                MeshMaterial3d(material.clone()), // 不克隆源材质或纹理资产。
            ));
            scan.mesh_count = scan.mesh_count.saturating_add(1); // 完成时可诊断空Scene0。
        }
        if let Ok(mut camera) = embedded_cameras.get_mut(entity) {
            camera.is_active = false; // 幽灵资源绝不能创建第二台世界相机。
        }
        if let Ok(entity_children) = children.get(entity) {
            scan.pending.extend(entity_children.iter()); // 只追加实体ID，下一预算帧继续。
        }
    }
    if !scan.pending.is_empty() {
        state.scan = Some(scan); // 未完成队列保持唯一，不复制或启动第二份扫描。
        return;
    }
    if scan.mesh_count == 0 {
        warn!(
            "建筑预览Scene0完成{}个节点隔离，但未发现StandardMaterial网格",
            scan.visited,
        ); // 坏资产保持空模型但不影响占地Gizmo和作者操作。
    }
    state.scan = None; // 完整层级已经提交全部专层和材质命令。
    state.phase = BuildingGhostPhase::RevealPending; // 下一帧再解除根隐藏，避免一帧源层闪现。
}

#[cfg(test)]
mod tests {
    use super::*; // ECS回归直接运行固定预算扫描和显隐阶段。

    /// 建立三个可区分的测试材质句柄。
    fn test_materials(world: &mut World) -> BuildingGhostMaterials {
        let mut assets = world.resource_mut::<Assets<StandardMaterial>>(); // 使用真实Assets分配稳定句柄。
        BuildingGhostMaterials {
            pending: assets.add(StandardMaterial::default()), // 待校验句柄。
            valid: assets.add(StandardMaterial::default()),   // 合法句柄。
            invalid: assets.add(StandardMaterial::default()), // 非法句柄。
        }
    }

    #[test]
    fn scene_ghost_stays_hidden_until_bounded_isolation_finishes() {
        let mut app = App::new(); // 无窗口ECS足以验证层级预算、材质和显隐门。
        app.init_resource::<Assets<StandardMaterial>>()
            .init_resource::<BuildingGhostPreviewState>()
            .add_systems(Update, advance_building_ghost_scan); // 使用生产扫描系统。
        let materials = test_materials(app.world_mut()); // 先创建句柄再安装资源。
        let valid_material = materials.valid.clone(); // 保存预期句柄供完成后比较。
        app.world_mut().insert_resource(materials); // 扫描系统读取完整三态资源。
        let root = app
            .world_mut()
            .spawn((EditorBuildingGhost, Visibility::Hidden))
            .id(); // 模拟已经收到WorldInstanceReady的隐藏根。
        let embedded_camera = app
            .world_mut()
            .spawn((
                Camera::default(), // 外部GLB残留相机默认可能处于活动状态。
                ChildOf(root),     // 相机属于当前待隔离Scene0层级。
            ))
            .id(); // 扫描必须在显示模型前停用它。
        let mut first_mesh = None; // 保存任一真实网格以检查最终专层和材质。
        for index in 0..GHOST_DESCENDANTS_PER_FRAME {
            let entity = app
                .world_mut()
                .spawn((
                    Mesh3d(Handle::<Mesh>::default()), // 只需满足标准网格查询形态。
                    MeshMaterial3d(Handle::<StandardMaterial>::default()), // 模拟GLTF原材质。
                    ChildOf(root),                     // 全部节点属于同一隐藏Scene0根。
                ))
                .id();
            if index == 0 {
                first_mesh = Some(entity); // 首项必在第一预算帧处理。
            }
        }
        {
            let mut state = app.world_mut().resource_mut::<BuildingGhostPreviewState>(); // 构造生产扫描起点。
            state.root = Some(root); // 当前根身份与队列一致。
            state.phase = BuildingGhostPhase::Scanning; // 模拟Ready观察者已经触发。
            state.scan = Some(BuildingGhostScan::new(root)); // 根加128个子节点超过单帧预算。
            state.tone = BuildingPreviewTone::Valid; // 新网格应使用绿色共享句柄。
            state.desired_visible = true; // 完成后允许显示。
        }
        app.update(); // 第一帧严格只处理128个节点，至少剩一个子节点。
        let state = app.world().resource::<BuildingGhostPreviewState>(); // 读取仍在途的唯一队列。
        assert_eq!(state.phase, BuildingGhostPhase::Scanning); // 超预算Scene0不能提前Ready。
        assert_eq!(state.scan.as_ref().map(|scan| scan.visited), Some(128)); // 固定预算没有越界。
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Hidden)
        ); // 部分隔离绝不显示。
        app.update(); // 第二帧处理剩余节点并进入RevealPending。
        assert_eq!(
            app.world().resource::<BuildingGhostPreviewState>().phase,
            BuildingGhostPhase::RevealPending,
        ); // 命令刚落地的帧仍不显示。
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Hidden)
        ); // 防止默认层闪现。
        let first_mesh = first_mesh.expect("测试必须保存一个网格实体"); // 上方固定循环保证存在。
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(first_mesh)
                .map(|material| material.0.clone()),
            Some(valid_material),
        ); // 原GLTF材质已经替换为共享绿色幽灵材质。
        assert_eq!(
            app.world().get::<RenderLayers>(first_mesh),
            Some(&RenderLayers::layer(MAP_EDITOR_BRUSH_RENDER_LAYER)),
        ); // 网格只进入F12专用层。
        assert_eq!(
            app.world()
                .get::<Camera>(embedded_camera)
                .map(|camera| camera.is_active),
            Some(false),
        ); // 资产内相机不得在准备期或完成后创建额外世界视图。
        assert_eq!(
            app.world().get::<RenderLayers>(embedded_camera),
            Some(&RenderLayers::layer(MAP_EDITOR_BRUSH_RENDER_LAYER)),
        ); // 相机节点本身也完成专层隔离。
        assert!(
            app.world()
                .get::<EditorBuildingGhostPrimitive>(first_mesh)
                .is_some(),
        ); // 后续颜色变化可定向更新，不再遍历层级。
        app.update(); // 第三帧确认所有隔离命令已采用后才显示根。
        assert_eq!(
            app.world().resource::<BuildingGhostPreviewState>().phase,
            BuildingGhostPhase::Ready,
        ); // 稳定阶段不再保留扫描队列。
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Inherited),
        ); // 完整模型现在可以只被F12相机看到。
    }

    #[test]
    fn prepared_ghost_remains_hidden_when_cursor_left_the_viewport() {
        let mut app = App::new(); // 无窗口ECS验证RevealPending读取最终显示意图。
        app.init_resource::<Assets<StandardMaterial>>()
            .init_resource::<BuildingGhostPreviewState>()
            .add_systems(Update, advance_building_ghost_scan); // 使用生产显隐采用门。
        let materials = test_materials(app.world_mut()); // 系统参数仍要求完整共享材质资源。
        app.world_mut().insert_resource(materials); // 本测试不读取具体句柄。
        let root = app
            .world_mut()
            .spawn((EditorBuildingGhost, Visibility::Hidden))
            .id(); // 模拟全部隔离命令已经落地的根。
        {
            let mut state = app.world_mut().resource_mut::<BuildingGhostPreviewState>(); // 构造光标已经离开视口的完成前状态。
            state.root = Some(root); // 显示门只处理当前唯一根。
            state.phase = BuildingGhostPhase::RevealPending; // 下一系统帧将发布Ready。
            state.desired_visible = false; // 离开中央视口后不得恢复上一个落点残影。
        }

        app.update(); // 完成准备但保持缓存隐藏。

        assert_eq!(
            app.world().resource::<BuildingGhostPreviewState>().phase,
            BuildingGhostPhase::Ready,
        ); // 隐藏不妨碍准备状态完成，光标回来可直接复用。
        assert_eq!(
            app.world().get::<Visibility>(root),
            Some(&Visibility::Hidden),
        ); // 没有当前地形命中时绝不显示旧位置模型。
    }
}
