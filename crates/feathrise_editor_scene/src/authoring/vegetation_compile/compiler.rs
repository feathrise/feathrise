//! 作者植被图层到预设正式运行时密度层的纯CPU确定性编译器。
#[path = "picking.rs"] // 单点归属复用相同组规则，不复制整图编译。
mod picking;
pub(crate) use picking::author_layers_at_world; // 只允许后台候选任务调用。

use std::collections::{BTreeMap, BTreeSet}; // 输入、输出和当前组引用均使用确定性容器。
use std::sync::Arc; // 后台编译共享只读R8画布，不复制完整作者图集。

use feathrise_scene_content::{
    SceneVegetationAsset, SceneVegetationAuthorGroupMode, SceneVegetationAuthorLayer,
    SceneVegetationOutputMergeMode,
}; // 编译器只解释公共作者格式，不声明任何草木内容名称。

use super::budget::ensure_compile_working_set; // 坏图层组合不得制造无界临时整图分配。
use super::resources::{CompiledDensityImage, CompiledDensityStats, CompiledVegetation}; // 纯函数返回可原子采用的完整结果和密度统计。

/// 对最终正式R8执行一次后台线性归约，供F4稳定帧常数成本读取。
fn collect_density_stats(pixels: &[u8]) -> CompiledDensityStats {
    let mut stats = CompiledDensityStats {
        total_pixels: pixels.len() as u64, // 输入预算已限制在usize和u64可表达范围内。
        ..Default::default()               // 全零图自然保留零和None。
    };
    for &value in pixels {
        stats.density_sum = stats.density_sum.saturating_add(u64::from(value)); // 目录不限制画布边长，归约使用饱和加法防御极端总量。
        stats.max_value = stats.max_value.max(value); // 最大值直接表达当前画布最高密度。
        if value == 0 {
            continue; // 零像素不参与非零覆盖和最小值。
        }
        stats.nonzero_pixels = stats.nonzero_pixels.saturating_add(1); // 覆盖面积由正式完成图而非作者中间图决定。
        stats.min_nonzero = Some(
            stats
                .min_nonzero
                .map_or(value, |current| current.min(value)),
        ); // 首个非零值建立下界，后续只收紧。
    }
    stats // 返回与正式输出同一后台修订的完整归约。
}

/// 为大图申请可恢复失败的零值R8缓冲；系统资源不足时保留上一完整编译结果。
fn try_zeroed_u8(length: usize, label: &str) -> Result<Vec<u8>, String> {
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(length)
        .map_err(|error| format!("植被{label}申请{length}字节失败：{error}"))?;
    pixels.resize(length, 0);
    Ok(pixels)
}

/// 为逐像素组规则申请浮点工作区，避免取消内容上限后由分配失败直接终止进程。
fn try_zeroed_f32(length: usize, label: &str) -> Result<Vec<f32>, String> {
    let bytes = length
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(|| format!("植被{label}浮点工作区字节数溢出"))?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .map_err(|error| format!("植被{label}申请{bytes}字节失败：{error}"))?;
    values.resize(length, 0.0);
    Ok(values)
}

/// 把作者组逐组解析，并合成为预设对应的正式运行时层。
pub(crate) fn compile_vegetation_authoring(
    vegetation: &SceneVegetationAsset,
    densities: &BTreeMap<String, Arc<Vec<u8>>>,
    world_min: [f32; 2],
    world_max: [f32; 2],
) -> Result<CompiledVegetation, String> {
    let authoring = vegetation
        .authoring
        .as_ref()
        .ok_or_else(|| "植被文件缺少作者图层图".to_owned())?; // F12加载边界会先为旧文件补齐一对一作者层。
    authoring.validate(&vegetation.layers)?; // 任何坏引用或不可寻址尺寸必须在分配整图前拒绝。
    if !world_min
        .iter()
        .chain(world_max.iter())
        .all(|value| value.is_finite())
        || world_min[0] >= world_max[0]
        || world_min[1] >= world_max[1]
    {
        return Err("植被作者编译收到无效世界范围".to_owned()); // 世界选择格不能接受NaN或退化范围。
    }
    let output_indices = vegetation
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (layer.id.as_str(), index))
        .collect::<BTreeMap<_, _>>(); // 作者层输出引用已经通过公共校验。
    let output_sizes = vegetation
        .layers
        .iter()
        .map(|layer| {
            let count = checked_pixel_count(layer.resolution, &layer.id)?; // 正式输出同样受显式长度预算保护。
            Ok((layer.id.clone(), count))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let final_output_bytes = output_sizes.iter().try_fold(0_usize, |total, (_, count)| {
        total
            .checked_add(*count)
            .ok_or_else(|| "植被正式输出总字节数溢出".to_owned())
    })?;
    ensure_compile_working_set(final_output_bytes, 0, "正式输出")?; // 分配第一张整图前先拒绝整数溢出。
    let mut outputs = BTreeMap::new();
    for (id, count) in output_sizes {
        let pixels = try_zeroed_u8(count, &format!("正式输出{id}"))?;
        outputs.insert(id, pixels); // 每个实际引用的预设输出只建立一张最终R8图。
    }
    let mut conflicts = BTreeMap::new(); // 每个启用组最多产生一张诊断图。
    let mut retained_conflict_bytes = 0_usize; // 已完成组诊断随最终结果保留，必须计入后续峰值。

    for group in authoring.groups.iter().filter(|group| group.enabled) {
        let layers = authoring
            .layers
            .iter()
            .filter(|layer| layer.enabled && layer.group_id == group.id)
            .collect::<Vec<_>>(); // 动态集合只包含当前启用组的实际作者层。
        if layers.is_empty() {
            continue; // 空组保留UI顺序但不分配整图临时内存。
        }
        let resolution = layers.iter().fold([2_u32, 2_u32], |current, layer| {
            [
                current[0].max(layer.resolution[0]),
                current[1].max(layer.resolution[1]),
            ]
        }); // 同组在最高作者分辨率解析一次，再向各正式输出保细节降采样。
        let pixel_count = checked_pixel_count(resolution, &group.id)?; // 任意可寻址分辨率仍需检查usize乘法。
        let prepared = layers
            .iter()
            .map(|layer| prepare_layer(layer, densities, &output_indices))
            .collect::<Result<Vec<_>, String>>()?; // Arc只读像素在任务内共享，不克隆整图。
        let referenced_outputs = prepared
            .iter()
            .map(|layer| layer.output_index)
            .collect::<BTreeSet<_>>(); // 当前组没有引用的正式profile绝不分配临时整图。
        let group_buffer_count = referenced_outputs
            .len()
            .checked_add(1)
            .ok_or_else(|| format!("植被作者组{}临时缓冲数量溢出", group.id))?; // 加一张组内冲突诊断图。
        let current_group_bytes = pixel_count
            .checked_mul(group_buffer_count)
            .ok_or_else(|| format!("植被作者组{}临时内存计算溢出", group.id))?;
        let resample_bytes = prepared
            .iter()
            .map(|layer| vegetation.layers[layer.output_index].resolution)
            .try_fold(0_usize, |largest, resolution| {
                Ok::<_, String>(largest.max(checked_pixel_count(resolution, &group.id)?))
            })?; // 降采样目标逐个生成，但峰值仍多保留一张正式分辨率临时图。
        let temporary_bytes = retained_conflict_bytes
            .checked_add(current_group_bytes)
            .and_then(|bytes| bytes.checked_add(resample_bytes))
            .ok_or_else(|| format!("植被作者组{}累计工作集计算溢出", group.id))?;
        ensure_compile_working_set(final_output_bytes, temporary_bytes, &group.id)?;
        let mut group_outputs = BTreeMap::new();
        for output_index in referenced_outputs {
            let pixels = try_zeroed_u8(pixel_count, &format!("作者组{}临时输出", group.id))?;
            group_outputs.insert(output_index, pixels); // 组完成后立即释放当前真实引用集合。
        }
        let mut conflict = try_zeroed_u8(pixel_count, &format!("作者组{}冲突图", group.id))?;
        let mut demands = try_zeroed_f32(prepared.len(), &format!("作者组{}需求", group.id))?;
        let mut resolved = try_zeroed_f32(prepared.len(), &format!("作者组{}解析", group.id))?;

        for y in 0..resolution[1] {
            for x in 0..resolution[0] {
                let index = y as usize * resolution[0] as usize + x as usize; // 先扩到usize，超大合法画布不会在u32乘法处回绕。
                let mut active_count = 0_u32; // 只为诊断统计同时存在需求的作者层数。
                let mut active_peak = 0.0_f32; // 冲突覆盖亮度使用重叠层最大需求。
                for (layer_index, layer) in prepared.iter().enumerate() {
                    let density =
                        sample_nearest(layer.pixels.as_slice(), layer.resolution, resolution, x, y)
                            as f32
                            / 255.0; // 所有规则在归一化空间计算，最终只量化一次。
                    let demand = (density * layer.opacity).clamp(0.0, 1.0); // 作者不透明度不改变原R8源。
                    demands[layer_index] = demand; // 写入当前像素的固定工作区。
                    if demand > f32::EPSILON {
                        active_count += 1; // 任意两个非零画布即构成可见重叠。
                        active_peak = active_peak.max(demand); // 保留冲突强度而非二值锯齿。
                    }
                }
                conflict[index] = if active_count >= 2 {
                    quantize_density(active_peak) // 冲突模式可直接看见重叠范围和强度。
                } else {
                    0 // 单层或空白区域不显示冲突色。
                };
                resolve_group_pixel(
                    group.mode,
                    group.capacity,
                    group.claim_threshold,
                    group.selection_seed,
                    group.selection_cell_meters,
                    &prepared,
                    &demands,
                    &mut resolved,
                    resolution,
                    x,
                    y,
                    world_min,
                    world_max,
                ); // 纯函数不写ECS或正式图，当前像素全部解析完成后再聚合。
                for (layer_index, value) in resolved.iter().copied().enumerate() {
                    if value <= 0.0 {
                        continue; // 零贡献不触及正式输出临时图。
                    }
                    let output_index = prepared[layer_index].output_index; // 作者层稳定映射到既有正式层。
                    let pixel = &mut group_outputs
                        .get_mut(&output_index)
                        .expect("当前组引用集合必须包含所有准备层输出")[index]; // 同一输出的多个作者层在组内相加封顶。
                    *pixel = pixel.saturating_add(quantize_density(value)); // 只在R8边界量化并保持确定性。
                }
            }
        }

        conflicts.insert(
            group.id.clone(),
            CompiledDensityImage {
                resolution,                 // 冲突覆盖使用当前组最高作者分辨率。
                pixels: Arc::new(conflict), // 主线程采用时只移动Arc。
            },
        );
        retained_conflict_bytes = retained_conflict_bytes
            .checked_add(pixel_count)
            .ok_or_else(|| "植被作者冲突诊断累计字节数溢出".to_owned())?;
        for (output_index, group_output) in group_outputs {
            let output = &vegetation.layers[output_index]; // 取得正式目标分辨率和稳定ID。
            let group_density = resample_max(&group_output, resolution, output.resolution)?; // 最大池化保证细笔刷和孤立纹素不会在编译降采样时消失。
            let destination = outputs
                .get_mut(&output.id)
                .expect("正式输出索引与结果表必须同源"); // 前方从同一个layers数组构造。
            merge_group_output(destination, &group_density, group.output_merge); // 跨组规则只作用于同profile最终图。
        }
    }

    let outputs = outputs
        .into_iter()
        .map(|(id, pixels)| (id, Arc::new(pixels)))
        .collect::<BTreeMap<_, _>>(); // 完整成功后一次把所有正式层移交主线程。
    let output_stats = outputs
        .iter()
        .map(|(id, pixels)| (id.clone(), collect_density_stats(pixels)))
        .collect::<BTreeMap<_, _>>(); // 最终图只在后台归约一次，F4刷新不再触碰整张R8。
    let nonzero_output_ids = output_stats
        .iter()
        .filter(|(_, stats)| stats.nonzero_pixels != 0)
        .map(|(id, _)| id.clone())
        .collect(); // 非零索引与更完整统计严格来自同一份最终像素。
    Ok(CompiledVegetation {
        outputs,            // 全部正式R8与非零身份来自同一原子修订。
        nonzero_output_ids, // 零输出可直接清空旧预览而无需重新扫描整图或加载Mesh原型。
        output_stats,       // F4以常数成本展示覆盖、均值和范围。
        conflicts,          // 诊断图与正式输出共享同一输入修订。
    })
}

/// 后台内部准备的只读作者层。
struct PreparedLayer {
    pixels: Arc<Vec<u8>>, // 共享当前作者修订的完整R8画布。
    resolution: [u32; 2], // 最近采样使用的源宽高。
    output_index: usize,  // 对应正式层数组下标。
    opacity: f32,         // 已受检归一化不透明度。
    priority: i32,        // 数值越大越优先。
    weight: f32,          // 已受检有限正权重。
}

fn prepare_layer(
    layer: &SceneVegetationAuthorLayer,
    densities: &BTreeMap<String, Arc<Vec<u8>>>,
    output_indices: &BTreeMap<&str, usize>,
) -> Result<PreparedLayer, String> {
    let pixels = densities
        .get(&layer.id)
        .cloned()
        .ok_or_else(|| format!("植被作者层{}缺少R8画布", layer.id))?; // 不创建零图掩盖坏作者文件。
    let expected = checked_pixel_count(layer.resolution, &layer.id)?; // 先检查乘法再比较长度。
    if pixels.len() != expected {
        return Err(format!(
            "植被作者层{}为{}字节，期望{}字节",
            layer.id,
            pixels.len(),
            expected
        ));
    }
    let output_index = *output_indices
        .get(layer.output_layer_id.as_str())
        .ok_or_else(|| format!("植被作者层{}正式输出失效", layer.id))?; // 公共校验后的防御性身份门。
    Ok(PreparedLayer {
        pixels,                       // Arc移动到任务准备值。
        resolution: layer.resolution, // 保持作者源声明方向。
        output_index,                 // 后续不再做字符串查找。
        opacity: layer.opacity,       // 公共校验已保证零至一。
        priority: layer.priority,     // i32全范围均有确定顺序。
        weight: layer.weight,         // 公共校验已保证有限正数。
    })
}

fn checked_pixel_count(resolution: [u32; 2], label: &str) -> Result<usize, String> {
    (resolution[0] as usize)
        .checked_mul(resolution[1] as usize)
        .ok_or_else(|| format!("密度图{label}分辨率乘法溢出")) // 损坏输入不能触发绕回小分配。
}

fn sample_nearest(pixels: &[u8], source: [u32; 2], target: [u32; 2], x: u32, y: u32) -> u8 {
    let source_x = ((x as u64 * source[0] as u64 + target[0] as u64 / 2) / target[0] as u64)
        .min(source[0].saturating_sub(1) as u64) as u32; // 中心最近采样且钳制最后一列。
    let source_y = ((y as u64 * source[1] as u64 + target[1] as u64 / 2) / target[1] as u64)
        .min(source[1].saturating_sub(1) as u64) as u32; // Z方向保持源行序，不上下翻转。
    pixels[source_y as usize * source[0] as usize + source_x as usize] // 输入长度已经严格验证。
}

#[allow(clippy::too_many_arguments)] // 参数明确表达纯像素解析身份，打包反而隐藏组规则与世界选择格。
fn resolve_group_pixel(
    mode: SceneVegetationAuthorGroupMode,
    capacity: f32,
    claim_threshold: f32,
    seed: u64,
    selection_cell_meters: f32,
    layers: &[PreparedLayer],
    demands: &[f32],
    output: &mut [f32],
    resolution: [u32; 2],
    x: u32,
    y: u32,
    world_min: [f32; 2],
    world_max: [f32; 2],
) {
    output.fill(0.0); // 每个像素必须覆盖上一像素的工作区完成值。
    match mode {
        SceneVegetationAuthorGroupMode::Additive => output.copy_from_slice(demands), // 各作者层保留自身密度。
        SceneVegetationAuthorGroupMode::WeightedPartition => {
            resolve_weighted_partition(capacity, layers, demands, output); // 高优先级先占、同级水填充。
        }
        SceneVegetationAuthorGroupMode::PriorityOverride => {
            resolve_priority_override(
                claim_threshold,
                seed,
                selection_cell_meters,
                layers,
                demands,
                output,
                resolution,
                x,
                y,
                world_min,
                world_max,
            ); // 同级使用世界稳定选择值，窗口和重启不会洗牌。
        }
    }
}

fn resolve_weighted_partition(
    capacity: f32,
    layers: &[PreparedLayer],
    demands: &[f32],
    output: &mut [f32],
) {
    let mut remaining = capacity.clamp(0.0, 1.0); // 公共校验后的防御性钳制。
    let mut priorities = layers
        .iter()
        .map(|layer| layer.priority)
        .collect::<Vec<_>>(); // 组级小数组只复制优先级，不复制像素。
    priorities.sort_unstable_by(|left, right| right.cmp(left)); // 数值较大的优先级先消费容量。
    priorities.dedup(); // 每个优先级档只解析一次。
    for priority in priorities {
        if remaining <= f32::EPSILON {
            break; // 容量耗尽后低优先级保持零。
        }
        let tier = layers
            .iter()
            .enumerate()
            .filter_map(|(index, layer)| {
                (layer.priority == priority && demands[index] > 0.0).then_some(index)
            })
            .collect::<Vec<_>>(); // 同级层数很小，水填充需显式活动集合。
        let total = tier.iter().map(|&index| demands[index]).sum::<f32>(); // 原需求总量决定是否需要分配。
        if total <= remaining + f32::EPSILON {
            for index in tier {
                output[index] = demands[index]; // 需求不足时禁止放大密度填满容量。
            }
            remaining = (remaining - total).max(0.0); // 把未使用容量留给下一优先级。
            continue;
        }
        allocate_weighted_tier(remaining, &tier, layers, demands, output); // 只在超容量时按权重水填充。
        break; // 当前档已耗尽全部剩余容量。
    }
}

fn allocate_weighted_tier(
    capacity: f32,
    tier: &[usize],
    layers: &[PreparedLayer],
    demands: &[f32],
    output: &mut [f32],
) {
    let mut active = tier.to_vec(); // 达到自身需求上限的层会从后续再分配中移除。
    let mut remaining = capacity; // 当前优先级可消费的剩余容量。
    for _ in 0..=tier.len() {
        if active.is_empty() || remaining <= f32::EPSILON {
            break; // 全部层封顶或容量耗尽。
        }
        let total_weight = active
            .iter()
            .map(|&index| demands[index] * layers[index].weight)
            .sum::<f32>(); // 密度与作者权重共同决定同级份额。
        if total_weight <= f32::EPSILON {
            break; // 防御性避免除零；合法正权重下仅全零需求会到此。
        }
        let mut consumed = 0.0_f32; // 本轮真正采用的容量。
        let mut next = Vec::with_capacity(active.len()); // 仍未达到原需求上限的层。
        for &index in &active {
            let share = remaining * demands[index] * layers[index].weight / total_weight; // 同级比例份额。
            let room = (demands[index] - output[index]).max(0.0); // 绝不放大超过原作者需求。
            let assigned = share.min(room); // 已封顶层把多余容量留给下一轮。
            output[index] += assigned; // 完成值保持归一化浮点直到最终R8量化。
            consumed += assigned; // 计算下一轮剩余容量。
            if output[index] + f32::EPSILON < demands[index] {
                next.push(index); // 尚有需求的层继续参加再分配。
            }
        }
        remaining = (remaining - consumed).max(0.0); // 浮点误差不允许产生负容量。
        if consumed <= f32::EPSILON {
            break; // 无进展时终止，避免退化输入循环。
        }
        active = next; // 下一轮只处理未封顶层。
    }
}

#[allow(clippy::too_many_arguments)] // 世界稳定选择所需参数均为纯值，保持调用侧可审计。
fn resolve_priority_override(
    claim_threshold: f32,
    seed: u64,
    selection_cell_meters: f32,
    layers: &[PreparedLayer],
    demands: &[f32],
    output: &mut [f32],
    resolution: [u32; 2],
    x: u32,
    y: u32,
    world_min: [f32; 2],
    world_max: [f32; 2],
) {
    let winning_priority = layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| (demands[index] > claim_threshold).then_some(layer.priority))
        .max(); // 覆盖只比较达到声明阈值的层。
    let Some(winning_priority) = winning_priority else {
        return; // 没有任何层认领当前像素。
    };
    let candidates = layers
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| {
            (layer.priority == winning_priority && demands[index] > claim_threshold)
                .then_some(index)
        })
        .collect::<Vec<_>>(); // 同级候选通常仅两三层，但不依赖产品数量上限。
    if candidates.len() == 1 {
        let winner = candidates[0]; // 唯一最高优先级无需哈希。
        output[winner] = demands[winner]; // 所有权不会把实际密度膨胀到一。
        return;
    }
    let world_x = world_min[0]
        + (x as f32 / resolution[0].saturating_sub(1).max(1) as f32)
            * (world_max[0] - world_min[0]); // 与笔刷端点到世界范围的方向一致。
    let world_z = world_min[1]
        + (y as f32 / resolution[1].saturating_sub(1).max(1) as f32)
            * (world_max[1] - world_min[1]); // R8首行对应world_min Z。
    let cell_x = (world_x / selection_cell_meters).floor() as i64; // 负坐标必须向下取整保持跨原点稳定。
    let cell_z = (world_z / selection_cell_meters).floor() as i64; // X/Z共同确定固定世界选择格。
    let selector = stable_unit(seed, cell_x, cell_z); // 同一世界格、种子与重启得到相同选择值。
    let total = candidates
        .iter()
        .map(|&index| demands[index] * layers[index].weight)
        .sum::<f32>(); // 同级需求和权重共同决定选择区间。
    let mut cursor = selector * total; // 把零至一选择值映射到完整权重区间。
    let mut winner = *candidates.last().expect("多候选集合非空"); // 浮点尾差回退到稳定最后层。
    for &index in &candidates {
        let span = demands[index] * layers[index].weight; // 当前候选区间长度。
        if cursor < span {
            winner = index; // 命中稳定区间后结束。
            break;
        }
        cursor -= span; // 继续检查下一稳定作者顺序候选。
    }
    output[winner] = demands[winner]; // 取得所有权仍只输出作者自身实际密度。
}

fn stable_unit(seed: u64, cell_x: i64, cell_z: i64) -> f32 {
    let mut value = seed
        ^ (cell_x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (cell_z as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9); // 有符号格坐标按二进制稳定混入。
    value ^= value >> 30; // SplitMix64终结器第一步。
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9); // 扩散相邻格低位相关性。
    value ^= value >> 27; // SplitMix64终结器第二步。
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB); // 完成高低位雪崩。
    value ^= value >> 31; // 最终稳定64位哈希。
    ((value >> 40) as f32) / ((1_u32 << 24) as f32) // 使用可精确表示的24位尾数得到[0,1)。
}

fn quantize_density(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8 // 每条贡献在R8边界只量化一次。
}

fn resample_max(source: &[u8], from: [u32; 2], to: [u32; 2]) -> Result<Vec<u8>, String> {
    if source.len() != checked_pixel_count(from, "组临时输出")? {
        return Err("植被组临时输出长度与分辨率不一致".to_owned()); // 内部不变量仍保持显式错误。
    }
    let mut output = try_zeroed_u8(checked_pixel_count(to, "正式输出")?, "正式输出降采样")?;
    for target_y in 0..to[1] {
        for target_x in 0..to[0] {
            let start_x = target_x as u64 * from[0] as u64 / to[0] as u64; // 当前目标像素覆盖源区间起点。
            let end_x = (((target_x + 1) as u64 * from[0] as u64 + to[0] as u64 - 1)
                / to[0] as u64)
                .max(start_x + 1)
                .min(from[0] as u64); // 至少汇聚一个源像素且不越界。
            let start_y = target_y as u64 * from[1] as u64 / to[1] as u64; // Z方向同样按完整覆盖区间。
            let end_y = (((target_y + 1) as u64 * from[1] as u64 + to[1] as u64 - 1)
                / to[1] as u64)
                .max(start_y + 1)
                .min(from[1] as u64); // 上采样时多个目标像素可稳定引用同一源像素。
            let mut value = 0_u8; // 最大池化保留任意细线和孤立纹素。
            for source_y in start_y..end_y {
                for source_x in start_x..end_x {
                    value = value.max(source[(source_y * from[0] as u64 + source_x) as usize]); // 完整源分区聚合。
                }
            }
            output[target_y as usize * to[0] as usize + target_x as usize] = value; // 行主序写入正式目标。
        }
    }
    Ok(output) // 完整构建后返回，不暴露部分图。
}

#[cfg(test)]
mod allocation_tests {
    use super::{try_zeroed_f32, try_zeroed_u8};

    #[test]
    fn impossible_allocations_return_errors_instead_of_panicking() {
        assert!(try_zeroed_u8(usize::MAX, "测试R8").is_err());
        assert!(try_zeroed_f32(usize::MAX, "测试浮点").is_err());
    }
}

fn merge_group_output(destination: &mut [u8], source: &[u8], mode: SceneVegetationOutputMergeMode) {
    for (destination, source) in destination.iter_mut().zip(source.iter().copied()) {
        *destination = match mode {
            SceneVegetationOutputMergeMode::Maximum => (*destination).max(source), // 相离或重叠区域保留更强结果。
            SceneVegetationOutputMergeMode::AddClamped => destination.saturating_add(source), // 多组密度累积但不溢出。
            SceneVegetationOutputMergeMode::ReplaceNonZero if source > 0 => source, // 非零局部修补覆盖前组。
            SceneVegetationOutputMergeMode::ReplaceNonZero => *destination, // 零值保持前组而不是清空整图。
        };
    }
}
