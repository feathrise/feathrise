//! 引用资产的非阻塞加载、完整候选发布和最终类型验证；失败保持原字段。

use super::super::types::{
    AssetReferenceRequest, ReferenceKind, ReferencePhase, SubAssetChoice,
    VegetationPresetEditorState,
}; // 唯一请求状态。
use super::enumeration::collect_choices; // 所有模型元数据枚举遵循64项预算。
use super::helpers::{apply_value, choice_path, target_is_current}; // 最终只有一个强类型落值入口。
use crate::resources::MapEditorState;
use bevy::asset::{DependencyLoadState, LoadState, RecursiveDependencyLoadState, UntypedAssetId}; // 同时报告根和递归依赖失败。
use bevy::gltf::{Gltf, GltfMesh, GltfNode}; // 候选基于加载器元数据，而非文件名猜测。
use bevy::prelude::*; // ECS、强句柄与真实目标资产集合。
use bevy::render::render_resource::{TextureDimension, TextureViewDimension}; // 基础色不能引用Cube/数组纹理。
use std::time::{Duration, Instant}; // 超时使用真实时间，不受F12暂停影响。 // 关闭F12立即取消未完成赋值。

const LOAD_TIMEOUT: Duration = Duration::from_secs(30); // 只限制加载/枚举，人工选择没有倒计时。
const EVENT_BUDGET: usize = 128; // 热更消息只允许固定数量检查。

/// 已开始枚举的候选遇到GLTF热更时失效，防止同一路径重新编号后选错Mesh。
pub(in super::super) fn invalidate_changed_assets(
    mut state: ResMut<VegetationPresetEditorState>, // 只影响临时选择，不改正式资产。
    mut roots: feathrise_scene_core::residency::ResidentAssetEventReader<Gltf>, // 根换代是整组候选换代。
    mut meshes: MessageReader<AssetEvent<GltfMesh>>, // Mesh局部热更也必须拒绝旧候选。
) {
    let Some(request) = state.asset_reference.as_ref() else {
        roots.clear(); // 没有请求时不积攒历史事件。
        meshes.clear(); // 清理游标为常数操作。
        return;
    };
    if matches!(request.phase, ReferencePhase::Loading) {
        roots.clear(); // 首次加载的Added事件不是旧候选失效。
        meshes.clear(); // 候选尚未建立，可直接采用最新完整根。
        return;
    }
    let root_id = request.gltf.as_ref().map(Handle::id); // 只比较本次根，不扫描全资源集合。
    let overflow = roots.len() > EVENT_BUDGET || meshes.len() > EVENT_BUDGET; // 超过可审计预算时保守取消旧选择。
    let root_changed = roots.read().take(EVENT_BUDGET).any(|event| match event {
        AssetEvent::Modified { id } | AssetEvent::Removed { id } => Some(*id) == root_id, // 真正换代才取消。
        _ => false, // 首次加载、Unused和LoadedWithDependencies不改编号。
    });
    let mesh_changed = meshes.read().take(EVENT_BUDGET).any(|event| {
        matches!(
            event,
            AssetEvent::Modified { .. } | AssetEvent::Removed { .. }
        )
    }); // 保守拒绝局部网格换代，绝不扫描任意长依赖列表。
    roots.clear(); // 消费剩余事件，不在下一帧重放同一次失效。
    meshes.clear(); // 保持固定检查预算。
    if root_id.is_some() && (overflow || root_changed || mesh_changed) {
        state.asset_reference = None; // POD候选一次释放，不逐个销毁UI或模型。
        state.status = "模型资源已热更新，旧子资产选择已取消；请重新点击使用选中".to_owned(); // 明确要求重新验证。
        state.touch_ui(); // 隐藏已失效的选择面板。
    }
}

/// 非阻塞推进当前唯一请求；临时结果完整且身份有效时才原子修改字段。
#[allow(clippy::too_many_arguments)] // 各Assets集合分别验证真实类型，不能用字符串替代。
pub(in super::super) fn poll_reference(
    mut state: ResMut<VegetationPresetEditorState>, // 请求和草稿由同一领域拥有。
    mut editor: ResMut<MapEditorState>,             // 全局底栏也显示验证失败。
    server: Res<AssetServer>,                       // 只登记加载与检查状态，不同步读取文件。
    gltfs: Res<Assets<Gltf>>,                       // 已完整加载的根。
    gltf_meshes: Res<Assets<GltfMesh>>,             // 原始子网格清单。
    nodes: Res<Assets<GltfNode>>,                   // Blade作者变换验证。
    meshes: Res<Assets<Mesh>>,                      // 最终网格必须真实存在。
    materials: Res<Assets<StandardMaterial>>,       // 必须是/std材质而不是GltfMaterial包装。
    images: Res<Assets<Image>>,                     // 图片需验证二维视图。
) {
    let Some(mut request) = state.asset_reference.take() else {
        return;
    }; // 稳定帧没有在途请求时立即返回。
    if !editor.open
        || editor.close_requested
        || editor.pending_map_transition.is_some()
        || !target_is_current(&state, &request.target)
    {
        state.status = "已离开目标标签、分区或LOD，本次资产选择已取消，原值保留".to_owned(); // 迟到结果不转交新焦点。
        editor.status = state.status.clone(); // 联合关闭和导航取消同样有明确回显。
        state.touch_ui(); // 清除旧面板。
        return;
    }
    if !matches!(request.phase, ReferencePhase::Choosing)
        && request.started.elapsed() > LOAD_TIMEOUT
    {
        state.status = "资产加载或验证超时，原字段保留；可以重新选择重试".to_owned(); // 错误不会清空路径。
        editor.status = state.status.clone(); // 提供可见错误。
        state.touch_ui(); // 解除忙碌门。
        return;
    }
    let was_choosing = matches!(request.phase, ReferencePhase::Choosing); // 只在阶段发生实质变化时重建有限UI。
    let result = advance(
        &mut request,
        &server,
        &gltfs,
        &gltf_meshes,
        &nodes,
        &meshes,
        &materials,
        &images,
    ); // 本帧工作固定有界。
    match result {
        Ok(Some(path)) => {
            if let Err(error) = apply_value(&mut state, &request.target, &path) {
                state.status = error; // 最后身份复验仍可能拒绝写入。
                state.touch_ui(); // 关闭失效的候选面板。
            }
            editor.status = state.status.clone(); // 显示实际采用的目标与路径。
        }
        Ok(None) => {
            if !was_choosing && matches!(request.phase, ReferencePhase::Choosing) {
                state.status = format!(
                    "{}有{}个可用子资产，请为{}明确选择",
                    request.source_path,
                    request.choices.len(),
                    request.target.description
                ); // 不猜第一个。
                editor.status = state.status.clone(); // 左右区域都能知道当前目标。
                state.touch_ui(); // 候选完整后一次发布选择面板。
            }
            state.asset_reference = Some(request); // 待加载或等待作者选择时保留唯一强句柄。
        }
        Err(error) => {
            state.status = format!("资产引用设置失败：{error}；原字段保留"); // 全部失败发生在落值前。
            editor.status = state.status.clone(); // 底栏明确诊断。
            state.touch_ui(); // 释放临时忙碌门，允许修复或重试。
        }
    }
}

/// 单帧状态推进；Some仅代表可以提交的一条完整路径。
#[allow(clippy::too_many_arguments)] // 将真实Assets验证和ECS落值边界分开以便回归。
fn advance(
    request: &mut AssetReferenceRequest,  // 当前唯一请求。
    server: &AssetServer,                 // 非阻塞加载仲裁。
    gltfs: &Assets<Gltf>,                 // 根事实。
    gltf_meshes: &Assets<GltfMesh>,       // 子网格元数据。
    nodes: &Assets<GltfNode>,             // Blade节点约束。
    meshes: &Assets<Mesh>,                // 最终真实网格。
    materials: &Assets<StandardMaterial>, // 最终真实材质。
    images: &Assets<Image>,               // 最终真实图片。
) -> Result<Option<String>, String> {
    if let ReferencePhase::Validating { path, handle } = &request.phase {
        if !loaded(server, handle.id())? {
            return Ok(None);
        } // 不阻塞等待GPU/IO依赖。
        let exists = match request.kind {
            ReferenceKind::Mesh | ReferenceKind::BladeMesh => {
                meshes.contains(handle.clone().typed::<Mesh>().id())
            } // 检查Mesh类型。
            ReferenceKind::Material => {
                materials.contains(handle.clone().typed::<StandardMaterial>().id())
            } // 检查Bevy派生标准材质。
            _ => false, // 图片与来源在根加载阶段验证。
        };
        return exists
            .then(|| Some(path.clone()))
            .ok_or_else(|| "子资产已报告就绪但没有对应类型的数据".to_owned()); // 不把仅有路径当成真实资产。
    }
    if matches!(request.phase, ReferencePhase::Choosing) {
        return Ok(None);
    } // 作者思考时不重新枚举任何数据。
    if let Some(handle) = &request.image {
        if !loaded(server, handle.id().untyped())? {
            return Ok(None);
        } // 等待Image解码而非仅校验后缀。
        let image = images
            .get(handle)
            .ok_or_else(|| "纹理就绪后没有Image数据".to_owned())?; // 真实图片类型。
        validate_image(image)?; // 禁止把天空Cube等填到二维叶片纹理。
        return Ok(Some(request.source_path.clone())); // 只有当前二维图集字段会采用该路径。
    }
    let handle = request
        .gltf
        .as_ref()
        .ok_or_else(|| "模型请求缺少根句柄".to_owned())?; // 不使用猜测或默认模型。
    if !loaded(server, handle.id().untyped())? {
        return Ok(None);
    } // 同时等待递归依赖。
    let gltf = gltfs
        .get(handle)
        .ok_or_else(|| "GLTF就绪后没有根数据".to_owned())?; // 根不存在不能开始枚举。
    request.phase = ReferencePhase::Collecting; // 后续热更必须撤销候选快照。
    let default_path = format!("{}#DefaultMaterial/std", request.source_path); // 只形成一个固定查询，不遍历材质表。
    let default_exists = server
        .get_handle::<StandardMaterial>(&default_path)
        .is_some_and(|handle| materials.contains(handle.id())); // 只有实际加载器生成才列出。
    if !collect_choices(request, gltf, gltf_meshes, nodes, default_exists)? {
        return Ok(None);
    } // 枚举超过预算留待后帧。
    match request.choices.as_slice() {
        [] => Err("这个GLB没有可供当前字段使用的网格或材质".to_owned()), // 不写空引用。
        [choice] => {
            begin_validation(request, *choice, server);
            Ok(None)
        } // 唯一候选仍需真实类型验证。
        _ => {
            request.phase = ReferencePhase::Choosing;
            Ok(None)
        } // 多候选禁止隐式取第一项。
    }
}

/// 人工选择或唯一候选共用最后的类型验证阶段。
pub(super) fn begin_validation(
    request: &mut AssetReferenceRequest,
    choice: SubAssetChoice,
    server: &AssetServer,
) {
    let path = choice_path(&request.source_path, choice); // 真正枚举过的GLTF编号。
    let handle = match choice {
        SubAssetChoice::Mesh { .. } => server.load::<Mesh>(path.clone()).untyped(), // 不能把Scene0放进Mesh字段。
        SubAssetChoice::Material(_) | SubAssetChoice::DefaultMaterial => {
            server.load::<StandardMaterial>(path.clone()).untyped()
        } // 不能误用GltfMaterial包装资产。
    };
    request.phase = ReferencePhase::Validating { path, handle }; // 在真正加载成功之前保持原值。
    request.started = Instant::now(); // 最终类型验证拥有独立超时门。
}

/// 同时处理直接与递归依赖失败，不把失败图片当成永远加载中。
fn loaded(server: &AssetServer, id: UntypedAssetId) -> Result<bool, String> {
    let Some((root, dependencies, recursive)) = server.get_load_states(id) else {
        return Ok(false);
    }; // 请求刚登记时尚无状态。
    match root {
        LoadState::Failed(error) => Err(error.to_string()), // 原始加载器错误保留。
        _ if matches!(dependencies, DependencyLoadState::Failed(_))
            || matches!(recursive, RecursiveDependencyLoadState::Failed(_)) =>
        {
            Err("模型或图片的依赖加载失败，请检查纹理和缓冲文件".to_owned())
        } // 不等待到超时才提示。
        _ => Ok(server.is_loaded_with_dependencies(id)),    // 只有完整Loaded才向下推进。
    }
}

/// 基础色引用只接受非空二维单层视图，与预览/运行时采样维度一致。
pub(super) fn validate_image(image: &Image) -> Result<(), String> {
    let descriptor = &image.texture_descriptor; // 只读取固定尺寸元数据。
    let view = image
        .texture_view_descriptor
        .as_ref()
        .and_then(|view| view.dimension); // 自定义视图也参与检查。
    if descriptor.dimension != TextureDimension::D2
        || descriptor.size.depth_or_array_layers != 1
        || descriptor.size.width == 0
        || descriptor.size.height == 0
        || !matches!(view, None | Some(TextureViewDimension::D2))
    {
        return Err("基础色需要普通二维图片，不能使用Cube、数组或空纹理".to_owned()); // 拒绝错误类型而不是渲染时崩溃。
    }
    Ok(()) // 不改变纹理内容、颜色空间或磁盘资源。
}
