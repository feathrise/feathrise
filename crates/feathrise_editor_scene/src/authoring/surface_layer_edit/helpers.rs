//! 地表删层的受检四槽重映射、可失败整图分配和原子历史恢复。

use super::super::resources::EditorSceneDraft; // 纯助手只修改已经独占借用的作者草稿。
use super::types::{SurfaceAuthoringChange, SurfaceAuthoringSnapshot}; // 引入地表作者层事务所需类型和纯助手。
use feathrise_scene_content::SceneSurfaceAsset; // 正式资产未实现PartialEq，按全部作者字段显式比较。
use std::sync::Arc; // 历史恢复只换共享句柄，不在帧内遍历像素。 // 定义与两张作者图始终保持一套。

pub(super) fn definitions_match(left: &SceneSurfaceAsset, right: &SceneSurfaceAsset) -> bool {
    // 地表作者操作在受检输入下执行当前步骤。
    // 仅在提交或历史操作时比较完整小型元数据，不进入稳定帧。
    left.schema_version == right.schema_version // 地表作者格式版本必须保持一致。
        && left.scene_id == right.scene_id // 稳定场景身份必须与当前草稿一致。
        && left.weight_source_path == right.weight_source_path // 作者源和派生路径共同构成完整地表定义。
        && left.weight_texture_path == right.weight_texture_path // 作者源和派生路径共同构成完整地表定义。
        && left.index_source_path == right.index_source_path // 作者源和派生路径共同构成完整地表定义。
        && left.index_texture_path == right.index_texture_path // 作者源和派生路径共同构成完整地表定义。
        && left.resolution == right.resolution // 依据完整作者条件选择有限处理分支。
        && left.layers == right.layers // 路径、分辨率和全部局部层顺序共同组成同一作者定义。
} // 完整地表定义比较结束。

pub(super) fn snapshot(draft: &EditorSceneDraft) -> Result<SurfaceAuthoringSnapshot, String> {
    // 地表作者操作在受检输入下执行当前步骤。
    // 启动帧仅复制定义与Arc句柄。
    Ok(SurfaceAuthoringSnapshot {
        // 保持当前地表操作的数据和原子采用边界。
        // 缺任一作者源就拒绝启动，不临时猜测空白画布。
        surface: draft // 保持当前地表操作的数据和原子采用边界。
            .surface // 地表局部层表是唯一作者索引依据。
            .clone() // 小型定义或Arc句柄复制不遍历整图像素。
            .ok_or_else(|| "当前场景没有地表作者层".to_owned())?, // 保留完整局部层顺序。
        weights: draft // 读取或传递同版本RGBA8权重作者源。
            .surface_weights // 读取或传递同版本RGBA8权重作者源。
            .clone() // 小型定义或Arc句柄复制不遍历整图像素。
            .ok_or_else(|| "当前场景缺少地表权重作者源".to_owned())?, // 浅克隆RGBA8。
        indices: draft // 读取或传递同版本小端RGBA32局部索引源。
            .surface_indices // 读取或传递同版本小端RGBA32局部索引源。
            .clone() // 小型定义或Arc句柄复制不遍历整图像素。
            .ok_or_else(|| "当前场景缺少地表索引作者源".to_owned())?, // 浅克隆RGBA32。
    }) // 所有输入同时存在才返回快照。
} // 输入冻结结束。

pub(super) fn validate_snapshot(value: &SurfaceAuthoringSnapshot) -> Result<usize, String> {
    // 地表作者操作在受检输入下执行当前步骤。
    // 这里只做固定次数长度与整数检查。
    let [width, height] = value.surface.resolution; // 索引和权重必须共享分辨率。
    let texels = (width as usize) // 保持当前地表操作的数据和原子采用边界。
        .checked_mul(height as usize) // 先验证尺寸乘法可表达，再使用字节或像素数量。
        .filter(|count| *count > 0) // 只继续处理存在且身份仍匹配的作者值。
        .ok_or_else(|| "地表分辨率为空或超出平台范围".to_owned())?; // 防止空图和乘法溢出。
    let weight_bytes = texels // 保持当前地表操作的数据和原子采用边界。
        .checked_mul(4) // 先验证尺寸乘法可表达，再使用字节或像素数量。
        .ok_or_else(|| "地表权重字节数超出平台范围".to_owned())?; // RGBA8每像素四字节。
    let index_bytes = texels // 保持当前地表操作的数据和原子采用边界。
        .checked_mul(16) // 先验证尺寸乘法可表达，再使用字节或像素数量。
        .ok_or_else(|| "地表索引字节数超出平台范围".to_owned())?; // RGBA32每像素十六字节。
    if value.weights.len() != weight_bytes // 空作者层表合法，仍要求两张画布完整配对。
        || value.indices.len() != index_bytes
    // 依据完整作者条件选择有限处理分支。
    {
        // 满足上述完整条件后才进入当前分支。
        // 完整配对是所有事务的前置条件。
        return Err("地表要求尺寸一致的完整索引/权重作者源".to_owned()); // 空层仍保留画布供撤销和重新绘制。
    } // 配对长度校验结束。
    Ok(texels) // 后台像素循环使用已经受检的总数。
} // 快照基础校验结束。

fn copy_bytes(source: &[u8]) -> Result<Vec<u8>, String> {
    // 地表作者操作在受检输入下执行当前步骤。
    // 仅允许在后台任务或纯测试中调用。
    let mut output = Vec::new(); // 先建立空容器以便显式报告真实分配失败。
    output // 完成当前地表作者事务的受检参数或值转换。
        .try_reserve_exact(source.len()) // 真实分配失败以可见错误返回，旧地表保持完整。
        .map_err(|error| format!("地表后台画布分配失败：{error}"))?; // 不对内容规模设置人为上限。
    output.extend_from_slice(source); // 分配已经成功，复制不会重新分配或阻塞主线程。
    Ok(output) // 返回完整独立写侧。
} // 可失败复制结束。

pub(super) fn materialize_snapshot(
    // 地表作者操作在受检输入下执行当前步骤。
    source: &SurfaceAuthoringSnapshot, // 只读输入保留操作前的完整作者版本。
) -> Result<SurfaceAuthoringSnapshot, String> {
    // 当前受检分支或参数组结束。
    // 两张图完整成功后才交还主世界。
    validate_snapshot(source)?; // 即使只是复制也不能把损坏配对交给笔刷。
    Ok(SurfaceAuthoringSnapshot {
        // 保持当前地表操作的数据和原子采用边界。
        // 任一分配失败时已完成临时图随错误释放。
        surface: source.surface.clone(), // 元数据语义保持不变。
        weights: Arc::new(copy_bytes(&source.weights)?), // 后台复制权重独立写侧。
        indices: Arc::new(copy_bytes(&source.indices)?), // 后台复制索引独立写侧。
    }) // 同一完成值携带两个唯一Arc。
} // 写侧物化结束。

pub(super) fn remap_texel(
    // 地表作者操作在受检输入下执行当前步骤。
    indices: [u32; 4], // 读取或传递同版本小端RGBA32局部索引源。
    weights: [u8; 4],  // 读取或传递同版本RGBA8权重作者源。
    removed: u32,      // 冻结场景局部层位置，防止删除或绘制其他作者层。
    layer_count: u32,  // 完成当前地表作者事务的受检参数或值转换。
) -> Result<([u32; 4], [u8; 4]), String> {
    // 当前受检分支或参数组结束。
    // 每像素最多四项，计算成本固定。
    if indices.iter().any(|index| *index >= layer_count) {
        // 依据完整作者条件选择有限处理分支。
        // 零权重槽也须满足正式索引验证契约。
        return Err("地表索引超出删除前的场景层表，已保留原作者源".to_owned()); // 损坏输入不静默修复为其他材质。
    } // 原局部索引校验结束。
    if layer_count == 1 || weights == [0; 4] {
        // 删除唯一层或原本没有覆盖时保持裸地，避免重新绘制后删其他层把空白区填满。
        return Ok(([0; 4], [0; 4])); // 不再凭空保留原材质或制造不存在的基础层。
    } // 空层画布转换结束。
    let mut next_indices = [0_u32; 4]; // 所有清空槽回到删除后的保底索引零。
    let mut contributions = [0_u32; 4]; // 整数累加重复局部层贡献，最大总量1020。
    let mut count = 0_usize; // 唯一剩余层数永不超过四槽。
    for (index, weight) in indices.into_iter().zip(weights) {
        // 依据完整作者条件选择有限处理分支。
        // 同时消费每个槽的身份和权重。
        if index == removed || weight == 0 {
            // 依据完整作者条件选择有限处理分支。
            // 删除目标贡献，空槽无需加入候选。
            continue; // 跳过目标后其覆盖由剩余层重新归一化接管。
        } // 无效贡献过滤结束。
        let index = index - u32::from(index > removed); // 大于删除位置的局部层下标整体左移一位。
        let slot = (0..count) // 保持当前地表操作的数据和原子采用边界。
            .find(|slot| next_indices[*slot] == index) // 只继续处理存在且身份仍匹配的作者值。
            .unwrap_or(count); // 重复身份合并，不制造重复有效槽。
        if slot == count {
            // 依据完整作者条件选择有限处理分支。
            // 第一次出现的剩余层保持原槽相对顺序。
            next_indices[slot] = index; // 写入已压缩的局部索引。
            count += 1; // 只在新增唯一贡献时扩展有效区。
        } // 唯一身份登记结束。
        contributions[slot] += u32::from(weight); // 整数权重避免浮点误差影响撤销和保存重现。
    } // 四槽聚合结束。
    let total = contributions.iter().sum::<u32>(); // 删除后所有剩余贡献的精确总量。
    if total == 0 {
        // 依据完整作者条件选择有限处理分支。
        // 纯被删层像素或原零像素需要明确保底。
        return Ok(([0; 4], [255, 0, 0, 0])); // 删除后的第一层完全填充该像素。
    } // 纯目标覆盖回退结束。
    let mut next_weights = [0_u8; 4]; // 归一化后四槽总和必须严格等于255。
    let mut remainders = [0_u32; 4]; // 最大余数法按原槽顺序稳定分配尾数。
    let mut assigned = 0_u32; // 已分配的整数权重。
    for slot in 0..count {
        // 依据完整作者条件选择有限处理分支。
        // 有效候选最多四项，不随全局层数增长。
        let scaled = contributions[slot] * 255; // 最大260100，u32安全容纳。
        next_weights[slot] = (scaled / total) as u8; // 先分配每项整数部分。
        remainders[slot] = scaled % total; // 保存精确余数，避免浮点排序抖动。
        assigned += u32::from(next_weights[slot]); // 汇总已分配量。
    } // 整数归一化结束。
    for _ in assigned..255 {
        // 依据完整作者条件选择有限处理分支。
        // 最多补三份余数，工作量固定。
        let winner = (0..count) // 保持当前地表操作的数据和原子采用边界。
            .max_by_key(|slot| (remainders[*slot], std::cmp::Reverse(*slot))) // 完成当前地表作者事务的受检参数或值转换。
            .expect("非零总量至少有一个贡献"); // 余数相同优先原槽。
        next_weights[winner] += 1; // 把一个剩余权重单位交给最大余数项。
        remainders[winner] = 0; // 每个候选最多取得一个余数单位。
    } // 权重255守恒补齐结束。
    Ok((next_indices, next_weights)) // 身份与权重作为同一个像素结果返回。
} // 固定四槽重映射结束。

pub(super) fn remove_layer(
    // 地表作者操作在受检输入下执行当前步骤。
    source: &SurfaceAuthoringSnapshot, // 只读输入保留操作前的完整作者版本。
    index: usize,                      // 冻结场景局部层位置，防止删除或绘制其他作者层。
) -> Result<SurfaceAuthoringSnapshot, String> {
    // 当前受检分支或参数组结束。
    // 整图算法只由AsyncComputeTaskPool调用。
    let texels = validate_snapshot(source)?; // 先确认整图配对与可表达长度。
    if index >= source.surface.layers.len() {
        // 依据完整作者条件选择有限处理分支。
        // 外部直接调用也复核原局部索引。
        return Err("待删除的地表作者层已失效，请重新选择".to_owned()); // 不允许越界删除其他层。
    } // 目标范围校验结束。
    let layer_count = u32::try_from(source.surface.layers.len()) // 保持当前地表操作的数据和原子采用边界。
        .map_err(|_| "地表层数量超出u32索引范围".to_owned())?; // 正式索引格式的真实能力门。
    let mut after = materialize_snapshot(source)?; // 后台复制独立结果，两图失败时不会污染输入。
    after.surface.layers.remove(index); // 删除定义并压缩其后所有场景局部下标。
    let output_weights = Arc::get_mut(&mut after.weights).expect("后台结果尚未共享"); // 唯一Arc取写侧，不触发任何隐式复制。
    let output_indices = Arc::get_mut(&mut after.indices).expect("后台结果尚未共享"); // 索引写侧与权重同生命周期。
    for texel in 0..texels {
        // 依据完整作者条件选择有限处理分支。
        // 完整像素扫描严格留在唯一后台CPU任务中。
        let weight_start = texel * 4; // 已校验乘法范围，按RGBA8定位。
        let index_start = texel * 16; // 已校验乘法范围，按RGBA32定位。
        let weights = source.weights[weight_start..weight_start + 4] // 保持当前地表操作的数据和原子采用边界。
            .try_into() // 将已经验证长度的源片段转为固定四槽值。
            .expect("完整四字节权重"); // 读取输入原始权重。
        let indices = std::array::from_fn(|slot| {
            // 保持当前地表操作的数据和原子采用边界。
            u32::from_le_bytes(
                // 完成当前地表作者事务的受检参数或值转换。
                source.indices[index_start + slot * 4..index_start + slot * 4 + 4] // 完成当前地表作者事务的受检参数或值转换。
                    .try_into() // 将已经验证长度的源片段转为固定四槽值。
                    .expect("完整四字节索引"), // 前置身份和长度检查已保证该测试或作者值存在。
            ) // 当前受检分支或参数组结束。
        }); // 按文件约定小端解析四槽。
        let (indices, weights) = remap_texel(indices, weights, index as u32, layer_count)?; // 任一坏像素使整套候选失败。
        output_weights[weight_start..weight_start + 4].copy_from_slice(&weights); // 只写后台完整候选权重。
        for (slot, index) in indices.into_iter().enumerate() {
            // 依据完整作者条件选择有限处理分支。
            // 对应四槽按同一顺序重新编码。
            output_indices[index_start + slot * 4..index_start + slot * 4 + 4] // 读取或传递同版本小端RGBA32局部索引源。
                .copy_from_slice(&index.to_le_bytes()); // 输出继续使用场景局部u32索引。
        } // 当前像素索引写入结束。
    } // 整图重映射完成。
    Ok(after) // 完整定义与两图全部成功后返回候选。
} // 删除候选生成结束。

pub(super) fn install_snapshot(draft: &mut EditorSceneDraft, snapshot: &SurfaceAuthoringSnapshot) {
    // 地表作者操作在受检输入下执行当前步骤。
    // 调用前必须完成全部校验。
    draft.surface = Some(snapshot.surface.clone()); // 恢复精确层顺序和所有元数据。
    draft.surface_weights = Some(Arc::clone(&snapshot.weights)); // 无整图复制地恢复精确RGBA8字节。
    draft.surface_indices = Some(Arc::clone(&snapshot.indices)); // 同一系统边界恢复精确RGBA32字节。
} // 原子快照安装结束。

pub(crate) fn apply_surface_authoring_change(
    // 地表作者操作在受检输入下执行当前步骤。
    draft: &mut EditorSceneDraft, // 完成当前地表作者事务的受检参数或值转换。
    change: &SurfaceAuthoringChange, // 完成当前地表作者事务的受检参数或值转换。
    undoing: bool,              // 完成当前地表作者事务的受检参数或值转换。
) -> Result<(), String> {
    // 当前受检分支或参数组结束。
    // 统一撤销系统调用，不单独修改历史栈。
    let (expected, target) = if undoing {
        // 保持当前地表操作的数据和原子采用边界。
        (&change.after, &change.before) // 完成当前地表作者事务的受检参数或值转换。
    } else {
        // 当前受检分支或参数组结束。
        (&change.before, &change.after) // 完成当前地表作者事务的受检参数或值转换。
    }; // 两个方向使用同一对完整作者快照。
    validate_snapshot(target)?; // 在写入任何草稿字段前复核目标两图完整性。
    if draft.scene_id != target.surface.scene_id // 稳定场景身份必须与当前草稿一致。
        || !draft // 依据完整作者条件选择有限处理分支。
            .surface // 地表局部层表是唯一作者索引依据。
            .as_ref() // 只读借用当前完整作者值。
            .is_some_and(|surface| definitions_match(surface, &expected.surface)) // 只继续处理存在且身份仍匹配的作者值。
        || draft.surface_weights.as_ref().map(|pixels| pixels.len()) != Some(expected.weights.len()) // 读取或传递同版本RGBA8权重作者源。
        || draft.surface_indices.as_ref().map(|pixels| pixels.len()) != Some(expected.indices.len())
    // 读取或传递同版本小端RGBA32局部索引源。
    {
        // 满足上述完整条件后才进入当前分支。
        // LIFO历史已保证像素来源，固定长度门避免帧内扫描整图。
        return Err("地表删除历史的场景、层表或画布尺寸已失效，未修改作者源".to_owned()); // 任一身份错误保持完整原版本。
    } // 历史来源校验结束。
    install_snapshot(draft, target); // 定义、索引和权重同次采用。
    draft.surface_dirty = true; // 统一保存继续写作者两图与配对KTX2。
    Ok(()) // 后续由既有撤销入口统一推进作者和地表修订。
} // 原子历史恢复结束。
