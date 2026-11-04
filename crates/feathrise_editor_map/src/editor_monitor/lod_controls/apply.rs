//! 把 F4 六个植被页的共享 LOD 开关应用到当前地图运行时与 F12 正式预览。
//! 【功能设计】GPU叶片关闭后退出Compute、绘制和阴影；Mesh节点使用Hidden退出主视图、预通道和阴影。
//! 状态变化帧同步已有节点，后续只处理Added节点，稳定帧不扫描地图实体或作者像素。

use bevy::prelude::*; // 引入变更检测、父子关系、延迟命令和可见性组件。
use feathrise_environment::vegetation::blade::{
    GpuBladeLodDebugDisabled, GpuBladePatch,
}; // Blade关闭标记会阻止对应窗口被提取到RenderApp。
use feathrise_environment::vegetation::{
    VegetationBladeLayer, VegetationMeshBatch, VegetationMeshInstance, VegetationMeshLodNode,
}; // 动态运行时与F12预览共享通用Mesh层、原型和零至三档身份。
use feathrise_fps_display::{LOD_LEVEL_COUNT, LodControlContent, LodControlState}; // F3/F4共享唯一非持久化逐档状态。
use feathrise_map_content::MapVegetationKind; // 作者正式层提供受检profile及Mesh双重身份。
use feathrise_fps_display::LodControlPresetBindings;
use super::super::vegetation_target::target_layer;
use crate::resources::MapEditorState;

use crate::authoring::EditorAuthoringSession; // 当前已进入草稿提供精确层、原型与旧地图身份。
use crate::constants::EDITOR_VEGETATION_PREVIEW_LAYER_BIT; // F4与视口拾取共用F12预览层最高位身份。

type BladeLodData = (
    Entity,                        // 延迟命令写入关闭标记需要实体身份。
    &'static GpuBladePatch,        // profile槽和零至三档来自草窗本身。
    Option<&'static VegetationBladeLayer>, // 内容来自正式层身份，局部槽可由多种内容复用。
    Has<GpuBladeLodDebugDisabled>, // 避免对一致状态重复写命令。
); // 完成GPU叶片查询数据。
type CommonLodData = (
    &'static VegetationMeshLodNode, // 读取动态Mesh零至三档身份。
    &'static ChildOf,               // 通过直接父根核对层与原型。
    &'static mut Visibility,        // Hidden同时退出主视图、预通道与阴影。
); // 完成通用Mesh查询数据。
type CommonLodFilter = ();
type NewCommonLodFilter = Added<VegetationMeshLodNode>;

/// 当前地图一个正式Mesh输出的轻量双重身份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::editor_monitor) struct CurrentMeshTarget {
    content: LodControlContent, // 树木、灌木或岩石对应的共享状态槽。
    runtime_layer_id: u64,      // 运行时根原值；F12预览只额外增加高位。
    prototype_id: u64,          // 同层原型不匹配时禁止误关其他内容。
} // 完成单个Mesh目标。

/// 一次系统运行共享的六类当前地图目标。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::editor_monitor) struct CurrentVegetationTargets {
    ordinary_grass: Option<u64>,                   // 当前草稿是否登记普通草正式Blade输出。
    wheat: Option<u64>,                            // 当前草稿是否登记麦田正式Blade输出。
    mixed_grain: Option<u64>,                      // 当前草稿是否登记杂粮正式Blade输出。
    meshes: [Option<CurrentMeshTarget>; 3], // 树木、灌木、岩石的精确层与原型。
} // 完成六类目标快照。

/// 状态真实变化时同步当前地图已经存在的全部六类植被LOD。
#[allow(clippy::too_many_arguments)] // 四种后端查询必须在同一状态快照下应用，拆开会重复扫描作者身份。
pub(in crate::editor_monitor) fn apply_changed_vegetation_lods(
    state: Res<LodControlState>, // 只有共享资源ChangeTick变化才允许进入已有节点扫描。
    editor: Res<MapEditorState>,
    bindings: Option<Res<LodControlPresetBindings>>,
    presets: Option<Res<feathrise_map_content::VegetationPresetRegistry>>,
    session: Res<EditorAuthoringSession>, // 当前草稿决定六类正式输出身份。
    mut previous_targets: Local<Option<CurrentVegetationTargets>>, // 草稿载入或切图后让既有节点补应用当前状态。
    mut previous_selection: Local<Option<usize>>, // 鼠标更新也会改变编辑器资源，只观察实际层选择。
    mut had_bindings: Local<bool>, // Option<Res>变成None不产生is_changed，必须显式识别移除边沿。
    mut previous_preset_revision: Local<Option<u64>>,
    mut commands: Commands,                // 为GPU叶片窗口插入或移除关闭标记。
    blades: Query<BladeLodData>,           // 状态变化帧同步当前已有草窗。
    roots: Query<&VegetationMeshInstance>, // 逐实例根提供层与原型。
    batch_roots: Query<&VegetationMeshBatch>, // 空间批次根提供相同身份。
    mut common_nodes: Query<CommonLodData, CommonLodFilter>, // 同步三类动态Mesh节点。
) {
    let selection_changed = *previous_selection != Some(editor.selected_vegetation_layer);
    let preset_revision = presets.as_deref().map(|presets| presets.revision);
    let presets_changed = *previous_preset_revision != preset_revision
        || presets.as_ref().is_some_and(|presets| presets.is_changed());
    *previous_preset_revision = preset_revision;
    let bindings_removed = *had_bindings && bindings.is_none();
    let bindings_changed = bindings_removed || bindings.as_ref().is_some_and(|bindings| bindings.is_changed());
    *had_bindings = bindings.is_some();
    *previous_selection = Some(editor.selected_vegetation_layer);
    if !state.is_changed() && !session.is_changed() && !selection_changed && !bindings_removed && !presets_changed
        && bindings.as_ref().is_none_or(|bindings| !bindings.is_changed()) {
        return; // 稳定状态和稳定草稿帧禁止读取作者层或扫描常驻节点。
    } // 完成两类轻量变化门。
    let targets = current_vegetation_targets(&session, &editor, bindings.as_deref()); // 只在状态或会话变化时读取六个固定正式层。
    let targets_changed = previous_targets.as_ref() != targets.as_ref(); // 比较常数规模身份，不比较密度或实例。
    if !state.is_changed() && !targets_changed && !bindings_changed && !presets_changed {
        return; // 普通作者数值变化未改变LOD目标时不得扫描现有节点。
    } // 完成目标身份差异门。
    let previous = targets_changed.then_some(*previous_targets).flatten();
    *previous_targets = targets; // 保存最新目标，使无关作者变化不扫描节点。
    let targets = targets.unwrap_or_default();
    // F3有九槽、F4只有六槽。旧通用层转交湿地等F3专用页时，SDK已经先采用新掩码，
    // 这里不能再用“归还旧目标”覆盖它；每个边沿只解析固定九项，节点循环不查目录。
    let mut shared_layers = [None; 9];
    let mut shared_prototypes = [None; 9];
    if let (Some(bindings), Some(presets)) = (bindings.as_deref(), presets.as_deref()) {
        for (index, (_, id)) in bindings.iter().enumerate() {
            if let Some(preset) = presets.resolve(id) {
                shared_layers[index] = Some(preset.resolved_runtime_layer_id());
                shared_prototypes[index] = preset.resolved_prototype_id();
            }
        }
    }
    let restored = LodControlState::default(); // 选中目标变化先归还旧层，不能让隐藏状态遗留。
    for (entity, patch, layer, disabled) in &blades {
        let was_owned = previous.as_ref().is_some_and(|old| layer.is_some_and(|layer| blade_content_for_layer(layer.0.0, old).is_some()));
        let restore = was_owned && !layer.is_some_and(|layer| shared_layers.contains(&Some(layer.0.0)));
        if restore && disabled && (patch.lod_level() as usize) < LOD_LEVEL_COUNT {
            commands.entity(entity).remove::<GpuBladeLodDebugDisabled>();
        }
        sync_blade_patch(&state, &targets, &mut commands, entity, patch, layer, disabled && !restore); // 三种Blade分别读取自身共享掩码。
    } // 完成已有GPU叶片窗口同步。
    for (lod, child_of, mut visibility) in &mut common_nodes {
        let prototype = roots.get(child_of.parent()).ok().map(|root| root.prototype.0)
            .or_else(|| batch_roots.get(child_of.parent()).ok().map(|root| root.prototype.0));
        let shared_owns = prototype.is_some_and(|prototype| shared_prototypes.contains(&Some(prototype)));
        if let Some(previous) = previous.as_ref() && !shared_owns {
            sync_common_mesh_node(&restored, previous, &roots, &batch_roots, lod, child_of, &mut visibility);
        }
        sync_common_mesh_node(
            &state,
            &targets,
            &roots,
            &batch_roots,
            lod,
            child_of,
            &mut visibility,
        ); // 三种Mesh按父根双重身份读取各自共享掩码。
    } // 完成已有通用Mesh节点同步。
} // 完成状态变化全量应用系统。

/// 分帧安装、空间常驻或切图新生成的节点在首帧继承当前共享开关。
#[allow(clippy::too_many_arguments)] // Added查询按后端分离才能同时保持可变Visibility互斥与稳定帧零扫描。
pub(in crate::editor_monitor) fn apply_vegetation_lods_to_new_nodes(
    state: Res<LodControlState>, // 新节点读取本次进程内既有共享选择。
    editor: Res<MapEditorState>,
    bindings: Option<Res<LodControlPresetBindings>>,
    session: Res<EditorAuthoringSession>, // 当前草稿提供六类精确正式身份。
    mut commands: Commands,      // 为新增GPU叶片窗口补齐关闭标记。
    blades: Query<BladeLodData, Added<GpuBladePatch>>, // 只访问新增草窗。
    roots: Query<&VegetationMeshInstance>, // 新通用节点通过父根取得层与原型。
    batch_roots: Query<&VegetationMeshBatch>, // 新批次节点通过父根取得层与原型。
    mut common_nodes: Query<CommonLodData, NewCommonLodFilter>, // 只访问新增通用LOD节点。
) {
    if blades.is_empty()
        && common_nodes.is_empty()
    {
        return; // 稳定帧在读取作者层前完成O(1)快速返回。
    } // 完成全部Added集合空门。
    let Some(targets) = current_vegetation_targets(&session, &editor, bindings.as_deref()) else {
        return; // 未进入地图时新增的无关节点保持正式状态。
    }; // 只在确有新增节点时解析六个固定作者目标。
    for (entity, patch, layer, disabled) in &blades {
        sync_blade_patch(&state, &targets, &mut commands, entity, patch, layer, disabled); // 新草窗首帧继承当前开关。
    } // 完成新增Blade同步。
    for (lod, child_of, mut visibility) in &mut common_nodes {
        sync_common_mesh_node(
            &state,
            &targets,
            &roots,
            &batch_roots,
            lod,
            child_of,
            &mut visibility,
        ); // 新通用节点首帧按内容应用Hidden或Inherited。
    } // 完成新增通用Mesh同步。
} // 完成新增节点增量应用系统。

/// 同步一个GPU叶片窗口的RenderApp提取关闭标记。
fn sync_blade_patch(
    state: &LodControlState,            // 读取当前内容独立掩码。
    targets: &CurrentVegetationTargets, // 限定当前草稿实际登记的三种Blade输出。
    commands: &mut Commands,            // 插入或移除主世界调试标记。
    entity: Entity,                     // 当前草窗实体。
    patch: &GpuBladePatch,              // 当前LOD档位。
    layer: Option<&VegetationBladeLayer>, // 未绑定的窗口不猜测内容。
    disabled: bool,                     // 当前是否已经带关闭标记。
) {
    let Some(content) = layer.and_then(|layer| blade_content_for_layer(layer.0.0, targets)) else {
        return; // 复用槽或未登记内容不由F4动态植被页猜测控制。
    }; // 取得当前草窗唯一共享内容身份。
    let level = patch.lod_level() as usize;
    if level >= LOD_LEVEL_COUNT {
        return; // 四档控件不覆盖数据预设的后续扩展档。
    }
    let should_disable = !state.is_enabled(content, level); // 关闭一档不影响同内容其他窗口。
    if should_disable && !disabled {
        commands.entity(entity).insert(GpuBladeLodDebugDisabled); // 下一次提取移除对应GpuBladePatch并停止全部GPU阶段。
    } else if !should_disable && disabled {
        commands.entity(entity).remove::<GpuBladeLodDebugDisabled>(); // 恢复后重新提取并服从正式距离范围。
    } // 一致状态不产生延迟命令。
} // 完成单个Blade窗口同步。

/// 把正式Blade层映射到当前地图实际登记的F4内容。
fn blade_content_for_layer(
    layer: u64,
    targets: &CurrentVegetationTargets,
) -> Option<LodControlContent> {
    if targets.ordinary_grass == Some(layer) {
        Some(LodControlContent::OrdinaryGrass)
    } else if targets.wheat == Some(layer) {
        Some(LodControlContent::Wheat)
    } else if targets.mixed_grain == Some(layer) {
        Some(LodControlContent::MixedGrain)
    } else {
        None // 不接管当前草稿未登记的其他叶片层。
    }
}

/// 同步一个动态运行时或F12预览的通用Mesh LOD节点。
fn sync_common_mesh_node(
    state: &LodControlState,                   // 读取树木、灌木或岩石各自掩码。
    targets: &CurrentVegetationTargets,        // 当前地图三类Mesh双重身份。
    roots: &Query<&VegetationMeshInstance>,    // 通过父实体读取逐实例根。
    batch_roots: &Query<&VegetationMeshBatch>, // 或读取空间批次根。
    lod: &VegetationMeshLodNode,               // 当前通用零至三档身份。
    child_of: &ChildOf,                        // 当前节点直接父根。
    visibility: &mut Visibility,               // 差异写回目标可见性。
) {
    let parent = child_of.parent(); // 父根是层与原型的唯一内容证据。
    let content = roots
        .get(parent)
        .ok()
        .and_then(|root| mesh_content_for_identity(root.layer.0, root.prototype.0, targets))
        .or_else(|| {
            batch_roots
                .get(parent)
                .ok()
                .and_then(|root| mesh_content_for_identity(root.layer.0, root.prototype.0, targets))
        }); // 逐实例和批次后端使用完全相同的身份规则。
    let Some(content) = content else {
        return; // 其他地图层、原型和普通场景摆放保持原状态。
    }; // 取得当前节点唯一植被内容。
    let level = usize::from(lod.level.0); // 通用Mesh以零至三表示四档。
    if level >= LOD_LEVEL_COUNT {
        return; // 未来新增档位在F4契约升级前保持正式可见性。
    } // 完成档位范围保护。
    write_lod_visibility(state.is_enabled(content, level), visibility); // 关闭后真实退出各渲染阶段。
} // 完成单个通用Mesh节点同步。

/// 根据层与原型找到当前地图唯一Mesh内容，同时接受F12预览高位镜像。
fn mesh_content_for_identity(
    layer_id: u64,                      // 运行时或F12预览层身份。
    prototype_id: u64,                  // 当前根正式原型身份。
    targets: &CurrentVegetationTargets, // 当前地图三个受检Mesh目标。
) -> Option<LodControlContent> {
    let runtime_layer_id = layer_id & !EDITOR_VEGETATION_PREVIEW_LAYER_BIT; // 只清除F12所有权位，不改变正式低位身份。
    targets.meshes.into_iter().flatten().find_map(|target| {
        (target.runtime_layer_id == runtime_layer_id && target.prototype_id == prototype_id)
            .then_some(target.content) // 层与原型同时匹配才允许F4修改节点。
    }) // 三个固定目标为常数规模。
} // 完成通用Mesh身份映射。

/// 只在目标状态不同时修改Mesh节点Visibility。
fn write_lod_visibility(enabled: bool, visibility: &mut Visibility) {
    let target = if enabled {
        Visibility::Inherited // 开启后继续由父级与VisibilityRange决定正式可见范围。
    } else {
        Visibility::Hidden // 关闭后同时退出主视图、预通道和阴影。
    }; // 完成目标状态选择。
    if *visibility != target {
        *visibility = target; // 差异写回避免污染稳定帧变更检测。
    } // 完成可见性同步。
} // 完成Mesh可见性写入。

/// 从当前作者草稿解析六个正式植被输出的常数规模身份快照。
fn current_vegetation_targets(
    session: &EditorAuthoringSession,
    editor: &MapEditorState,
    bindings: Option<&LodControlPresetBindings>,
) -> Option<CurrentVegetationTargets> {
    let draft = session.draft.as_ref()?;
    let blade = |content| target_layer(draft, editor, bindings, content, MapVegetationKind::Blade)
        .and_then(|layer| layer.runtime_layer_id);
    let mesh = |content| {
        let layer = target_layer(draft, editor, bindings, content, MapVegetationKind::Mesh)?;
        Some(CurrentMeshTarget { content, runtime_layer_id: layer.runtime_layer_id?, prototype_id: layer.prototype_id? })
    };
    Some(CurrentVegetationTargets {
        ordinary_grass: blade(LodControlContent::OrdinaryGrass),
        wheat: blade(LodControlContent::Wheat),
        mixed_grain: blade(LodControlContent::MixedGrain),
        meshes: [mesh(LodControlContent::Trees), mesh(LodControlContent::Shrubs), mesh(LodControlContent::Rocks)],
    })
}

#[cfg(test)] // 纯身份和最小ECS测试不启动窗口或渲染器。
mod tests {
    use super::*; // 直接验证生产身份映射与系统参数互斥。
    use bevy::ecs::system::RunSystemOnce; // 一次性运行真实系统以捕获B0001查询冲突。
    use feathrise_environment::vegetation::blade::{GpuBladeConfig, GpuBladeProfileSlot}; // 构造三种profile槽的真实草窗组件。
    use feathrise_environment::vegetation::{
        VegetationLayerId, VegetationMeshLodLevel, VegetationPrototypeId,
    }; // 构造通用根和零至三档子节点身份。

    /// 构造覆盖三种Blade与三种Mesh的固定测试目标。
    fn all_targets() -> CurrentVegetationTargets {
        CurrentVegetationTargets {
            ordinary_grass: Some(11), // 普通草profile存在。
            wheat: Some(12),          // 麦田profile存在。
            mixed_grain: Some(13),    // 杂粮profile存在。
            meshes: [
                Some(CurrentMeshTarget {
                    content: LodControlContent::Trees, // 树木内容。
                    runtime_layer_id: 41,              // 树木层。
                    prototype_id: 51,                  // 树木原型。
                }),
                Some(CurrentMeshTarget {
                    content: LodControlContent::Shrubs, // 灌木内容。
                    runtime_layer_id: 42,               // 灌木层。
                    prototype_id: 52,                   // 灌木原型。
                }),
                Some(CurrentMeshTarget {
                    content: LodControlContent::Rocks, // 岩石内容。
                    runtime_layer_id: 43,              // 岩石层。
                    prototype_id: 53,                  // 岩石原型。
                }),
            ], // 完成三个Mesh目标。
        } // 返回完整目标。
    } // 完成测试目标构造器。

    /// 使用生产同步函数把固定测试目标应用到全部GPU叶片窗口。
    fn apply_test_blade_targets(
        state: Res<LodControlState>, // 读取测试中按内容设置的共享掩码。
        mut commands: Commands,      // 真实插入或移除关闭标记。
        blades: Query<BladeLodData>, // 遍历测试草窗。
    ) {
        let targets = all_targets(); // 三种正式Blade内容均登记。
        for (entity, patch, layer, disabled) in &blades {
            sync_blade_patch(&state, &targets, &mut commands, entity, patch, layer, disabled); // 复用完整生产标记逻辑。
        } // 完成测试草窗同步。
    } // 完成Blade测试系统。

    /// 使用生产同步函数把固定测试目标应用到全部通用Mesh节点。
    fn apply_test_mesh_targets(
        state: Res<LodControlState>,           // 读取树木、灌木和岩石独立掩码。
        roots: Query<&VegetationMeshInstance>, // 读取逐实例父根身份。
        batch_roots: Query<&VegetationMeshBatch>, // 读取分块父根身份。
        mut nodes: Query<CommonLodData, CommonLodFilter>, // 写回当前测试节点Visibility。
    ) {
        let targets = all_targets(); // 使用三类隔离层和原型。
        for (lod, child_of, mut visibility) in &mut nodes {
            sync_common_mesh_node(
                &state,
                &targets,
                &roots,
                &batch_roots,
                lod,
                child_of,
                &mut visibility,
            ); // 复用完整生产父根过滤与差异写回。
        } // 完成测试Mesh节点同步。
    } // 完成Mesh测试系统。

    /// 构造一个指定profile槽和档位的最小真实GPU叶片窗口。
    fn blade_patch(slot: GpuBladeProfileSlot, level: u32) -> GpuBladePatch {
        GpuBladePatch::square(Vec3::ZERO, 16.0, 4, level + 1)
            .with_lod_level(level)
            .with_vegetation_config(GpuBladeConfig::new(slot, 1.0, 0.5, 0.8, 0.03)) // 包围值不参与逐档判断，但通过正式公开配置接口写入槽号。
    } // 完成测试草窗构造器。

    #[test] // 三种Blade槽必须各自命中F4共享内容且未登记槽不得误控。
    fn blade_layers_map_to_their_own_registered_contents() {
        let mut targets = all_targets(); // 从三种Blade均登记开始。
        assert_eq!(
            blade_content_for_layer(11, &targets),
            Some(LodControlContent::OrdinaryGrass)
        ); // 普通草命中零号共享内容。
        assert_eq!(
            blade_content_for_layer(12, &targets),
            Some(LodControlContent::Wheat)
        ); // 麦田命中一号共享内容。
        assert_eq!(
            blade_content_for_layer(13, &targets),
            Some(LodControlContent::MixedGrain)
        ); // 杂粮命中三号共享内容。
        targets.wheat = None; // 模拟当前地图没有麦田正式层。
        assert_eq!(blade_content_for_layer(12, &targets), None); // 复用槽上的陌生内容不得被F4麦田页误关。
    } // 完成Blade精确映射回归。

    #[test] // 关闭与恢复三种Blade档位必须真实增删RenderApp提取关闭标记。
    fn blade_lod_state_inserts_and_removes_the_debug_disabled_marker() {
        let mut world = World::new(); // 创建不含渲染器的最小主世界。
        let mut state = LodControlState::default(); // 三种内容初始四档全开。
        state.solo(LodControlContent::OrdinaryGrass, 0); // 普通草二档将被关闭。
        state.solo(LodControlContent::Wheat, 2); // 麦田二档保持开启。
        state.solo(LodControlContent::MixedGrain, 3); // 杂粮一档将被关闭。
        world.insert_resource(state); // 插入共享状态供测试系统读取。
        let ordinary = world.spawn((blade_patch(GpuBladeProfileSlot::SLOT_0, 2), VegetationBladeLayer(VegetationLayerId(11)))).id(); // 关闭目标。
        let wheat = world.spawn((blade_patch(GpuBladeProfileSlot::SLOT_0, 2), VegetationBladeLayer(VegetationLayerId(12)))).id(); // 开启目标。
        let mixed = world.spawn((blade_patch(GpuBladeProfileSlot::SLOT_0, 1), VegetationBladeLayer(VegetationLayerId(13)))).id(); // 关闭目标。
        world
            .run_system_once(apply_test_blade_targets)
            .expect("三种Blade逐档标记同步应成功运行"); // 应用生产关闭逻辑和延迟命令。
        assert!(world.get::<GpuBladeLodDebugDisabled>(ordinary).is_some()); // 普通草关闭档停止提取。
        assert!(world.get::<GpuBladeLodDebugDisabled>(wheat).is_none()); // 麦田开启档保持提取。
        assert!(world.get::<GpuBladeLodDebugDisabled>(mixed).is_some()); // 杂粮关闭档停止提取。
        world
            .resource_mut::<LodControlState>()
            .enable_all(LodControlContent::OrdinaryGrass); // 恢复普通草全部四档。
        world
            .run_system_once(apply_test_blade_targets)
            .expect("Blade逐档恢复同步应成功运行"); // 应用生产移除标记逻辑。
        assert!(world.get::<GpuBladeLodDebugDisabled>(ordinary).is_none()); // 恢复后重新允许RenderApp提取。
        assert!(world.get::<GpuBladeLodDebugDisabled>(mixed).is_some()); // 普通草恢复不得串改杂粮。
    } // 完成Blade关闭、恢复与内容隔离回归。

    #[test] // 三类Mesh及F12高位镜像必须命中自身目标，层或原型任一不同都拒绝。
    fn runtime_and_editor_preview_mesh_roots_keep_three_contents_independent() {
        let targets = all_targets(); // 使用三种隔离层与原型。
        assert_eq!(
            mesh_content_for_identity(41, 51, &targets),
            Some(LodControlContent::Trees)
        ); // 运行时树根命中树木。
        assert_eq!(
            mesh_content_for_identity(42 | EDITOR_VEGETATION_PREVIEW_LAYER_BIT, 52, &targets),
            Some(LodControlContent::Shrubs)
        ); // F12预览灌木只增加高位仍命中灌木。
        assert_eq!(
            mesh_content_for_identity(43, 53, &targets),
            Some(LodControlContent::Rocks)
        ); // 运行时岩石命中岩石。
        assert_eq!(mesh_content_for_identity(41, 52, &targets), None); // 树层配灌木原型不得误命中。
        assert_eq!(mesh_content_for_identity(44, 51, &targets), None); // 其他层配树原型同样拒绝。
    } // 完成三类Mesh身份隔离回归。

    #[test] // 三类通用Mesh节点必须按各自状态写Hidden或Inherited并支持恢复。
    fn mesh_lod_state_changes_real_runtime_and_preview_node_visibility() {
        let mut world = World::new(); // 创建不含渲染器的最小主世界。
        let mut state = LodControlState::default(); // 三类Mesh初始四档全开。
        state.solo(LodControlContent::Trees, 1); // 树木二档关闭。
        state.solo(LodControlContent::Shrubs, 0); // 灌木高模开启。
        state.solo(LodControlContent::Rocks, 3); // 岩石超低模开启。
        world.insert_resource(state); // 插入共享状态供测试系统读取。
        let tree_root = world
            .spawn(VegetationMeshInstance {
                layer: VegetationLayerId(41),         // 正式运行时树木层。
                prototype: VegetationPrototypeId(51), // 正式树木原型。
                instance_id: 1,                       // 实例编号不参与逐档内容匹配。
            })
            .id(); // 完成运行时树根。
        let tree_lod = world
            .spawn((
                VegetationMeshLodNode {
                    level: VegetationMeshLodLevel(2), // 当前树木低模被只看中模关闭。
                    part_id: 0,                       // 部件不影响逐档开关。
                    billboard: false,                 // 当前不是公告板。
                },
                Visibility::Inherited, // 初始使用正式可见性。
                ChildOf(tree_root),    // 连接正式运行时父根。
            ))
            .id(); // 保存树木节点供断言。
        let shrub_root = world
            .spawn(VegetationMeshBatch {
                layer: VegetationLayerId(42 | EDITOR_VEGETATION_PREVIEW_LAYER_BIT), // F12预览只增加所有权高位。
                prototype: VegetationPrototypeId(52), // 正式灌木原型。
                batch_id: 2,                          // 批次编号不参与内容匹配。
                instance_count: 64,                   // 数量不参与逐档内容匹配。
            })
            .id(); // 完成F12预览灌木批次根。
        let shrub_lod = world
            .spawn((
                VegetationMeshLodNode {
                    level: VegetationMeshLodLevel(0), // 灌木高模保持开启。
                    part_id: 0,                       // 单部件fixture。
                    billboard: false,                 // 当前不是公告板。
                },
                Visibility::Inherited, // 初始使用正式可见性。
                ChildOf(shrub_root),   // 连接F12预览批次父根。
            ))
            .id(); // 保存灌木节点供断言。
        let rock_root = world
            .spawn(VegetationMeshInstance {
                layer: VegetationLayerId(43),         // 正式运行时岩石层。
                prototype: VegetationPrototypeId(53), // 正式岩石原型。
                instance_id: 3,                       // 实例编号不参与逐档内容匹配。
            })
            .id(); // 完成运行时岩石根。
        let rock_lod = world
            .spawn((
                VegetationMeshLodNode {
                    level: VegetationMeshLodLevel(3), // 岩石超低模保持开启。
                    part_id: 0,                       // 单部件fixture。
                    billboard: false,                 // 岩石最远档仍可为普通PBR网格。
                },
                Visibility::Inherited, // 初始使用正式可见性。
                ChildOf(rock_root),    // 连接正式运行时父根。
            ))
            .id(); // 保存岩石节点供断言。
        world
            .run_system_once(apply_test_mesh_targets)
            .expect("三类Mesh逐档可见性同步应成功运行"); // 应用生产双重身份与Hidden逻辑。
        assert_eq!(world.get::<Visibility>(tree_lod), Some(&Visibility::Hidden)); // 树木关闭档退出渲染。
        assert_eq!(
            world.get::<Visibility>(shrub_lod),
            Some(&Visibility::Inherited)
        ); // F12预览灌木开启档保持正式范围。
        assert_eq!(
            world.get::<Visibility>(rock_lod),
            Some(&Visibility::Inherited)
        ); // 运行时岩石开启档保持正式范围。
        world
            .resource_mut::<LodControlState>()
            .enable_all(LodControlContent::Trees); // 恢复树木四档。
        world
            .run_system_once(apply_test_mesh_targets)
            .expect("Mesh逐档恢复同步应成功运行"); // 应用生产Inherited恢复逻辑。
        assert_eq!(
            world.get::<Visibility>(tree_lod),
            Some(&Visibility::Inherited)
        ); // 恢复后重新服从父级与正式距离。
    } // 完成运行时/F12预览Mesh实际显隐回归。

    #[test] // 三份可变Visibility查询必须通过Without过滤保持Bevy参数互斥。
    fn vegetation_lod_apply_systems_initialize_without_query_conflicts() {
        let mut world = World::new(); // 创建不含窗口和渲染器的最小ECS世界。
        world.init_resource::<LodControlState>(); // 两个生产系统要求共享状态资源。
        world.init_resource::<MapEditorState>();
        world.init_resource::<EditorAuthoringSession>(); // 空会话让身份解析安全返回None。
        world
            .run_system_once(apply_changed_vegetation_lods)
            .expect("F4六类全量LOD系统不应触发Visibility查询B0001"); // 验证真实系统参数初始化。
        world
            .run_system_once(apply_vegetation_lods_to_new_nodes)
            .expect("F4六类增量LOD系统不应触发Visibility查询B0001"); // 同时锁定Added路径互斥。
    } // 完成生产查询参数回归。

    #[test]
    fn custom_selected_layers_obey_lod_and_switching_restores_the_previous_layer() {
        let mut app = App::new();
        app.init_resource::<MapEditorState>()
            .init_resource::<LodControlState>()
            .insert_resource(super::super::super::vegetation_target::custom_session())
            .add_systems(Update, apply_changed_vegetation_lods);
        let blade = app.world_mut().spawn((blade_patch(GpuBladeProfileSlot::SLOT_0, 2), VegetationBladeLayer(VegetationLayerId(11)))).id();
        let root = app.world_mut().spawn(VegetationMeshInstance { layer: VegetationLayerId(42), prototype: VegetationPrototypeId(52), instance_id: 1 }).id();
        let mesh = app.world_mut().spawn((VegetationMeshLodNode { level: VegetationMeshLodLevel(2), part_id: 0, billboard: false }, Visibility::Inherited, ChildOf(root))).id();
        app.world_mut().resource_mut::<LodControlState>().solo(LodControlContent::OrdinaryGrass, 0);
        app.world_mut().resource_mut::<LodControlState>().solo(LodControlContent::Trees, 0);
        app.update();
        assert!(app.world().get::<GpuBladeLodDebugDisabled>(blade).is_some());
        assert_eq!(app.world().get::<Visibility>(mesh), Some(&Visibility::Inherited));
        app.world_mut().resource_mut::<MapEditorState>().selected_vegetation_layer = 1;
        app.update();
        assert!(app.world().get::<GpuBladeLodDebugDisabled>(blade).is_none());
        assert_eq!(app.world().get::<Visibility>(mesh), Some(&Visibility::Hidden));
        app.world_mut().resource_mut::<MapEditorState>().selected_vegetation_layer = 0;
        app.update();
        assert!(app.world().get::<GpuBladeLodDebugDisabled>(blade).is_some());
        assert_eq!(app.world().get::<Visibility>(mesh), Some(&Visibility::Inherited));
    }

    fn diagnostic_registry(prototype: u64) -> feathrise_map_content::VegetationPresetRegistry {
        use feathrise_map_content::*;
        let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/vegetation_presets.vegetation_presets.json"))).unwrap();
        let mut blade = catalog.presets.iter().find(|preset| preset.kind() == MapVegetationKind::Blade).unwrap().clone();
        blade.id = "project_meadow".into(); blade.aliases.clear(); blade.runtime_layer_id = Some(11);
        let mut mesh = catalog.presets.iter().find(|preset| preset.kind() == MapVegetationKind::Mesh).unwrap().clone();
        mesh.id = "project_crystal".into(); mesh.aliases.clear(); mesh.runtime_layer_id = Some(42); mesh.prototype_id = Some(prototype);
        let mut other = mesh.clone(); other.id = "project_other".into(); other.output_id = "other_output".into();
        other.runtime_layer_id = Some(43); other.prototype_id = Some(53);
        let presets = if prototype == 53 { vec![blade, mesh] } else { vec![blade, mesh, other] };
        VegetationPresetRegistry { catalog: Default::default(), revision: prototype,
            resolved: ResolvedVegetationPresetCatalog::build(&VegetationPresetCatalogAsset {
                schema_version: VEGETATION_PRESET_SCHEMA_VERSION, presets }).unwrap(), last_error: None }
    }

    fn diagnostic_mesh(world: &mut World, layer: u64, prototype: u64) -> Entity {
        use feathrise_environment::vegetation::VegetationMeshRenderIdentity;
        let root = world.spawn(VegetationMeshInstance { layer: VegetationLayerId(layer),
            prototype: VegetationPrototypeId(prototype), instance_id: 1 }).id();
        world.spawn((VegetationMeshLodNode { level: VegetationMeshLodLevel(2), part_id: 0, billboard: false },
            VegetationMeshRenderIdentity { layer: VegetationLayerId(layer), prototype: VegetationPrototypeId(prototype),
                level: VegetationMeshLodLevel(2), part_id: 0, billboard: false }, Visibility::Inherited, ChildOf(root))).id()
    }

    fn diagnostic_app() -> App {
        let mut app = App::new();
        app.add_plugins(feathrise_fps_display::FpsDisplayPlugin)
            .insert_resource(MapEditorState { selected_vegetation_layer: 1, ..Default::default() })
            .insert_resource(super::super::super::vegetation_target::custom_session())
            .insert_resource(diagnostic_registry(52));
        // 安装真正的SDK与作者PostUpdate链，只跑该调度，不创建窗口或启动无关资源采样。
        super::super::super::install_lod_application(&mut app);
        for content in [LodControlContent::OrdinaryGrass, LodControlContent::Trees, LodControlContent::Shrubs] {
            app.world_mut().resource_mut::<LodControlState>().solo(content, 0);
        }
        app
    }

    #[test]
    fn binding_redirect_and_removal_restore_old_targets_before_current_author_lod() {
        let mut app = diagnostic_app();
        let mut bindings = LodControlPresetBindings::default();
        bindings.bind(LodControlContent::OrdinaryGrass, "project_meadow");
        bindings.bind(LodControlContent::Shrubs, "project_crystal");
        app.insert_resource(bindings);
        let blade = app.world_mut().spawn((blade_patch(GpuBladeProfileSlot::SLOT_0, 2), VegetationBladeLayer(VegetationLayerId(11)))).id();
        let current = diagnostic_mesh(app.world_mut(), 42, 52);
        let other = diagnostic_mesh(app.world_mut(), 43, 53);
        let unrelated = diagnostic_mesh(app.world_mut(), 44, 54);
        *app.world_mut().get_mut::<Visibility>(unrelated).unwrap() = Visibility::Hidden;
        let extended = diagnostic_mesh(app.world_mut(), 42, 52);
        app.world_mut().get_mut::<feathrise_environment::vegetation::VegetationMeshRenderIdentity>(extended).unwrap().level = VegetationMeshLodLevel(4);
        app.world_mut().get_mut::<VegetationMeshLodNode>(extended).unwrap().level = VegetationMeshLodLevel(4);
        *app.world_mut().get_mut::<Visibility>(extended).unwrap() = Visibility::Hidden;
        app.world_mut().run_schedule(PostUpdate);
        assert!(app.world().get::<GpuBladeLodDebugDisabled>(blade).is_some());
        assert_eq!(app.world().get::<Visibility>(current), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<Visibility>(other), Some(&Visibility::Inherited));
        app.world_mut().resource_mut::<LodControlPresetBindings>().bind(LodControlContent::Shrubs, "project_other");
        app.world_mut().run_schedule(PostUpdate);
        assert_eq!(app.world().get::<Visibility>(current), Some(&Visibility::Hidden)); // 已转成通用选中层，不能被SDK晚到归还覆盖。
        assert_eq!(app.world().get::<Visibility>(other), Some(&Visibility::Hidden));
        app.world_mut().remove_resource::<LodControlPresetBindings>();
        app.world_mut().run_schedule(PostUpdate);
        assert!(app.world().get::<GpuBladeLodDebugDisabled>(blade).is_none());
        assert_eq!(app.world().get::<Visibility>(current), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<Visibility>(other), Some(&Visibility::Inherited));
        assert_eq!(app.world().get::<Visibility>(unrelated), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<Visibility>(extended), Some(&Visibility::Hidden)); // 四档控件不接管扩展档，包括解绑恢复。
        app.world_mut().resource_mut::<MapEditorState>().selected_vegetation_layer = 0;
        app.world_mut().run_schedule(PostUpdate);
        assert!(app.world().get::<GpuBladeLodDebugDisabled>(blade).is_some());
        assert_eq!(app.world().get::<Visibility>(current), Some(&Visibility::Inherited));
    }

    #[test]
    fn catalog_hot_reload_does_not_reveal_previous_author_preview() {
        let mut app = diagnostic_app();
        let mut bindings = LodControlPresetBindings::default();
        bindings.bind(LodControlContent::Trees, "project_crystal");
        app.insert_resource(bindings);
        let current = diagnostic_mesh(app.world_mut(), 42, 52);
        app.world_mut().run_schedule(PostUpdate);
        assert_eq!(app.world().get::<Visibility>(current), Some(&Visibility::Hidden));
        app.insert_resource(diagnostic_registry(53)); // 新原型就绪前，作者保留原正式预览与原逐档设置。
        app.world_mut().run_schedule(PostUpdate);
        assert_eq!(app.world().get::<Visibility>(current), Some(&Visibility::Hidden));
        app.world_mut().remove_resource::<feathrise_map_content::VegetationPresetRegistry>();
        app.world_mut().run_schedule(PostUpdate);
        assert_eq!(app.world().get::<Visibility>(current), Some(&Visibility::Hidden));
    }

    #[test]
    fn f3_only_binding_takes_over_generic_author_target_without_being_restored() {
        let mut app = diagnostic_app();
        app.world_mut().resource_mut::<MapEditorState>().selected_vegetation_layer = 0;
        let blade = app.world_mut().spawn((blade_patch(GpuBladeProfileSlot::SLOT_0, 2), VegetationBladeLayer(VegetationLayerId(11)))).id();
        app.world_mut().run_schedule(PostUpdate);
        assert!(app.world().get::<GpuBladeLodDebugDisabled>(blade).is_some());

        let mut bindings = LodControlPresetBindings::default();
        bindings.bind(LodControlContent::WetlandGrass, "project_meadow");
        app.insert_resource(bindings);
        app.world_mut().resource_mut::<LodControlState>().solo(LodControlContent::WetlandGrass, 1);
        app.world_mut().run_schedule(PostUpdate);
        assert!(app.world().get::<GpuBladeLodDebugDisabled>(blade).is_some()); // F4退出旧目标不能覆盖F3专用页的本帧结果。
        app.world_mut().resource_mut::<LodControlState>().enable_all(LodControlContent::WetlandGrass);
        app.world_mut().run_schedule(PostUpdate);
        assert!(app.world().get::<GpuBladeLodDebugDisabled>(blade).is_none()); // F4旧通用掩码不能重新关闭F3接管的目标。
        app.world_mut().remove_resource::<LodControlPresetBindings>();
        app.world_mut().run_schedule(PostUpdate);
        assert!(app.world().get::<GpuBladeLodDebugDisabled>(blade).is_some()); // 解绑后重新由当前通用作者页接管。
    }

    #[test] // 通用身份新类型构造仍应保持层与原型原值可供上述映射使用。
    fn common_root_identity_fields_remain_compatible() {
        let root = VegetationMeshInstance {
            layer: VegetationLayerId(41),         // 正式树木层。
            prototype: VegetationPrototypeId(51), // 正式树木原型。
            instance_id: 7,                       // 实例编号不参与逐档内容匹配。
        }; // 构造最小通用根。
        assert_eq!(
            mesh_content_for_identity(root.layer.0, root.prototype.0, &all_targets()),
            Some(LodControlContent::Trees)
        ); // 防止公共根字段语义变化后F4静默失效。
    } // 完成通用根兼容回归。
} // 结束F4六类LOD应用测试。
