//! 视口植被整层选择的纯密度与候选逻辑。

use std::collections::BTreeMap; // 正式密度表沿用草稿的确定性层ID索引。
use std::sync::Arc; // 点击只借用共享R8，不复制任何完整像素。

use bevy::prelude::Vec2; // 世界X/Z与离屏物理像素位置都使用二维向量。
use feathrise_scene_content::{SceneId, SceneVegetationAuthoring, SceneVegetationLayer}; // 场景、正式输出和作者图层图共同决定可见层及UI锚点。

use crate::constants::EDITOR_VEGETATION_PREVIEW_LAYER_BIT; // F12逐株预览与正式运行时层只差最高隔离位。
use crate::resources::EditorViewportSelectionState; // 重复点击序列属于会话资源而不是场景作者数据。

const VEGETATION_REPEAT_MAX_SECONDS: f64 = 0.75; // 兼顾普通连续点击并避免迟到点击意外换层。
const VEGETATION_REPEAT_MAX_PIXELS_SQUARED: f32 = 6.0 * 6.0; // 与场景树拖拽阈值一致的物理像素容差。

/// 一个正式输出对应的作者面板锚点以及当前真正参与编译的同输出层数量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AuthorLayerAnchor {
    pub(super) index: usize,             // 面板最终高亮的作者层数组下标。
    pub(super) contributor_count: usize, // 大于一时只能如实显示完整编译结果而不能猜单一来源。
}

/// 在正式R8上按与作者笔刷相同的端点网格语义采样最近纹素。
pub(super) fn sample_density_at_world(
    density: &[u8],       // 当前正式输出的完整无Mip R8。
    resolution: [u32; 2], // X/Z纹素数。
    world_min: [f32; 2],  // 场景X/Z最小端点。
    world_max: [f32; 2],  // 场景X/Z最大端点。
    world: Vec2,          // 点击地形或Mesh命中的世界X/Z。
) -> Option<u8> {
    if resolution[0] == 0
        || resolution[1] == 0
        || !world.is_finite()
        || !world_min.iter().all(|value| value.is_finite())
        || !world_max.iter().all(|value| value.is_finite())
        || world_min[0] >= world_max[0]
        || world_min[1] >= world_max[1]
        || world.x < world_min[0]
        || world.x > world_max[0]
        || world.y < world_min[1]
        || world.y > world_max[1]
    {
        return None; // 损坏规格和场景外落点都不能被夹紧成边缘植被。
    }
    let width = resolution[0] as usize; // 后续受检乘法使用平台索引宽度。
    let height = resolution[1] as usize; // Z轴行数独立校验。
    if width.checked_mul(height) != Some(density.len()) {
        return None; // 描述与像素长度不一致时失败关闭，不能越界或读取前缀。
    }
    let normalized_x = (world.x - world_min[0]) / (world_max[0] - world_min[0]); // X端点映射到0..1。
    let normalized_z = (world.y - world_min[1]) / (world_max[1] - world_min[1]); // Vec2.y在本域表示世界Z。
    let x = (normalized_x * resolution[0].saturating_sub(1) as f32).round() as usize; // 与离散作者网格最近点一致。
    let z = (normalized_z * resolution[1].saturating_sub(1) as f32).round() as usize; // 最大世界端点命中最后一行。
    density.get(z.checked_mul(width)?.checked_add(x)?).copied() // 所有索引仍通过受检算术和切片边界。
}

/// 按正式层顺序收集点击位置的非零输出，并把真实Mesh命中的运行时层稳定置于首项。
pub(super) fn vegetation_layer_candidates(
    layers: &[SceneVegetationLayer], // 当前场景固定正式输出定义。
    densities: &BTreeMap<String, Arc<Vec<u8>>>, // 同修订原子采用的正式R8表。
    world_min: [f32; 2],           // 当前场景X/Z最小端点。
    world_max: [f32; 2],           // 当前场景X/Z最大端点。
    world: Vec2,                   // 本次点击的世界X/Z。
    preferred_runtime_layer_id: Option<u64>, // 逐株Mesh命中提供的强层身份。
) -> Vec<String> {
    let preferred_runtime_layer_id =
        preferred_runtime_layer_id.map(|layer_id| layer_id & !EDITOR_VEGETATION_PREVIEW_LAYER_BIT); // F12预览与正式层恢复到同一低位身份。
    let preferred_output_id = preferred_runtime_layer_id.and_then(|runtime_layer_id| {
        layers
            .iter()
            .find(|layer| layer.runtime_layer_id == Some(runtime_layer_id))
            .map(|layer| layer.id.as_str()) // 真实冠层命中即使落点纹素为零也优先选择其整层。
    });
    let mut candidates = Vec::with_capacity(layers.len().min(16)); // 正常六输出不重新扩容，异常规模仍只按点击分配。
    if let Some(output_id) = preferred_output_id {
        candidates.push(output_id.to_owned()); // Mesh强身份固定为第一次点击的首候选。
    }
    for layer in layers {
        if Some(layer.id.as_str()) == preferred_output_id {
            continue; // 强身份已经进入首项，不能在密度扫描中重复。
        }
        let visible_here = densities.get(&layer.id).is_some_and(|density| {
            sample_density_at_world(
                density.as_slice(),
                layer.resolution,
                world_min,
                world_max,
                world,
            )
            .is_some_and(|value| value > 0) // 编译完成的零值不属于可点击图层。
        });
        if visible_here && !candidates.iter().any(|candidate| candidate == &layer.id) {
            candidates.push(layer.id.clone()); // 重复坏ID只发布一次，后续作者校验仍会明确报错。
        }
    }
    candidates // 最多正式层数量，且只在左键边沿构造。
}

/// 为正式输出选择一个作者面板锚点；多张活动作者层共同输出时返回真实贡献数量。
pub(super) fn author_layer_anchor(
    authoring: &SceneVegetationAuthoring, // 当前完整作者图层图。
    output_id: &str,                    // 已选正式输出层ID。
    current_index: usize,               // 尽量保持作者当前明确选择。
) -> Option<AuthorLayerAnchor> {
    let active = authoring
        .layers
        .iter()
        .enumerate()
        .filter(|(_, layer)| {
            layer.output_layer_id == output_id
                && layer.enabled
                && authoring
                    .groups
                    .iter()
                    .find(|group| group.id == layer.group_id)
                    .is_some_and(|group| group.enabled) // 隐藏层或关闭组不参与当前正式编译结果。
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>(); // 作者层硬上限128，且只在点击边沿分配。
    let candidates = if active.is_empty() {
        authoring
            .layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| layer.output_layer_id == output_id)
            .map(|(index, _)| index)
            .collect::<Vec<_>>() // 损坏或全隐藏输出仍给作者一个可诊断锚点。
    } else {
        active // 正常情况只在真实参与编译的作者层中选择。
    };
    let contributor_count = candidates.len(); // 模式判断与面板锚点使用同一完整集合。
    let index = candidates
        .iter()
        .copied()
        .find(|index| *index == current_index)
        .or_else(|| candidates.first().copied())?; // 当前层不属于该输出时采用UI顺序首层。
    Some(AuthorLayerAnchor {
        index,             // 高亮真实作者记录。
        contributor_count, // 多层时视口必须显示Compiled覆盖。
    })
}

/// 在同一像素位置的重复点击或 Alt 点击中循环重叠正式层，否则从首候选开始。
pub(super) fn choose_vegetation_candidate(
    state: &mut EditorViewportSelectionState, // 保存跨点击的轻量候选序列。
    scene_id: &SceneId,                           // 重叠循环绝不能跨场景继承同名正式层。
    candidates: &[String],                    // 本次落点的完整正式层候选。
    click_position: Vec2,                     // 离屏视口物理像素。
    now_seconds: f64,                         // 不受玩法暂停影响的真实时间。
    force_cycle: bool,                        // Alt明确请求切到下一重叠层。
) -> Option<usize> {
    if candidates.is_empty() {
        state.clear_vegetation_selection(); // 空落点同时结束旧整层高亮与循环。
        return None;
    }
    let same_position = state
        .last_vegetation_click_position
        .is_some_and(|previous| {
            previous.distance_squared(click_position) <= VEGETATION_REPEAT_MAX_PIXELS_SQUARED
        }); // 手抖范围内仍视为同一落点。
    let same_scene = state.selected_vegetation_scene_id.as_ref() == Some(scene_id); // 切换场景后即使层ID和像素位置相同也必须重新从首项开始。
    let same_candidates =
        same_scene && same_position && state.last_vegetation_candidates == candidates; // 场景、层集合或顺序任一变化都重置循环。
    let repeated_in_time = same_candidates
        && now_seconds.is_finite()
        && state.last_vegetation_click_seconds.is_some_and(|previous| {
            previous.is_finite()
                && now_seconds >= previous
                && now_seconds - previous <= VEGETATION_REPEAT_MAX_SECONDS
        }); // 时钟倒退或超窗都不是重复点击。
    let index = if same_candidates && (force_cycle || repeated_in_time) {
        state
            .last_vegetation_candidate_index
            .saturating_add(1)
            .rem_euclid(candidates.len()) // 单层安全回到零，多层按确定顺序循环。
    } else {
        0 // 新落点、新候选集或普通迟到点击都选择视觉上最优先的首层。
    };
    state.last_vegetation_click_position = Some(click_position); // 下一次点击使用本次物理位置复核。
    state.last_vegetation_click_seconds = now_seconds.is_finite().then_some(now_seconds); // 异常时钟不建立永久窗口。
    state.last_vegetation_candidates.clear(); // 原位复用上一小型Vec容量。
    state
        .last_vegetation_candidates
        .extend(candidates.iter().cloned()); // 只复制最多正式层数量的稳定短ID。
    state.last_vegetation_candidate_index = index; // 与已发布候选身份同批更新。
    Some(index) // 调用方再把正式输出映射到真实作者层。
}
