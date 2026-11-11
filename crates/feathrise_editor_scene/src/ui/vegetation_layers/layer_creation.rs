//! 植被作者图层的新建、复制与唯一私有源身份。

use std::collections::BTreeMap; // 新作者层像素以稳定ID登记。
use std::sync::Arc; // 复制层共享只读像素直到首次落笔。
use std::time::{SystemTime, UNIX_EPOCH}; // 避开删除后仍保留在磁盘上的旧作者源文件名。

use feathrise_scene_content::{
    SceneVegetationAuthorGroup, SceneVegetationAuthorGroupMode, SceneVegetationAuthorLayer,
    SceneVegetationAuthoring, SceneVegetationLayer, SceneVegetationOutputMergeMode,
}; // 新描述必须使用公共作者格式和动态容器。

use super::operations::{LocalMutation, changed}; // 创建完成值仍由主事务统一校验和采用。

pub(super) fn add_blank_layer(
    selected: Option<usize>,
    new_layer_output_id: Option<&str>,
    folder: &str,
    outputs: &[SceneVegetationLayer],
    authoring: &mut SceneVegetationAuthoring,
    densities: &mut BTreeMap<String, Arc<Vec<u8>>>,
) -> Result<LocalMutation, String> {
    let count = blank_layer_pixel_count(selected, new_layer_output_id, outputs, authoring)?;
    add_blank_layer_with_pixels(
        selected,
        new_layer_output_id,
        folder,
        outputs,
        authoring,
        densities,
        vec![0; count], // 仅供纯函数测试与非交互调用；生产按钮始终传入后台完成值。
    )
}

/// 使用后台已经完整清零的 R8 缓冲原子建立作者层。
#[allow(clippy::too_many_arguments)]
pub(super) fn add_blank_layer_with_pixels(
    selected: Option<usize>,
    new_layer_output_id: Option<&str>,
    folder: &str,
    outputs: &[SceneVegetationLayer],
    authoring: &mut SceneVegetationAuthoring,
    densities: &mut BTreeMap<String, Arc<Vec<u8>>>,
    pixels: Vec<u8>,
) -> Result<LocalMutation, String> {
    if authoring.groups.is_empty() {
        authoring.groups.push(make_group(authoring.groups.len())); // 全空作者图仍可从一个有效组重新开始。
    }
    let output = selected_output(selected, new_layer_output_id, outputs, authoring)?;
    let group_id = selected
        .and_then(|index| authoring.layers.get(index))
        .map(|layer| layer.group_id.clone())
        .unwrap_or_else(|| authoring.groups[0].id.clone());
    let id = unique_layer_id(authoring, &output.id);
    let count = pixel_count(output.resolution)?;
    if pixels.len() != count {
        return Err(format!(
            "后台空白画布长度{}与预设{}分辨率像素数{}不一致",
            pixels.len(),
            output.id,
            count
        )); // 迟到或错误身份完成值不得形成描述与R8长度不一致的半套层。
    }
    let ordinal = authoring
        .layers
        .iter()
        .filter(|layer| layer.output_layer_id == output.id)
        .count()
        + 1;
    let layer = SceneVegetationAuthorLayer {
        id: id.clone(),
        label: format!("{} · 区域 {ordinal}", output.label), // 即使尚未提供重命名输入，列表也能明确区分多张区域图。
        group_id,
        output_layer_id: output.id.clone(), // 正式profile的四级LOD、风场和贴地规格保持原样。
        density_source_path: format!("{}/vegetation/author/{id}.r8", folder.trim_end_matches('/')),
        resolution: output.resolution,
        enabled: true,
        locked: false,
        opacity: 1.0,
        priority: 0,
        weight: 1.0,
    };
    let index = selected.map_or(authoring.layers.len(), |value| value + 1);
    authoring.layers.insert(index, layer); // 新层紧跟当前层，便于区域拆分后连续比较。
    densities.insert(id, Arc::new(pixels)); // 后台完整缓冲直接进入Arc，主线程不再清零整张画布。
    Ok(changed(
        Some(index),
        format!("已新建“{}”空白作者图层", output.label),
    ))
}

/// 返回当前新建动作真实目标输出的像素数；不分配画布也不修改作者结构。
pub(super) fn blank_layer_pixel_count(
    selected: Option<usize>,
    new_layer_output_id: Option<&str>,
    outputs: &[SceneVegetationLayer],
    authoring: &SceneVegetationAuthoring,
) -> Result<usize, String> {
    pixel_count(selected_output(selected, new_layer_output_id, outputs, authoring)?.resolution)
}

pub(super) fn selected_output<'a>(
    // 分配前与事务采用前共享同一真实目标解析，避免Mesh通过回退输出绕过Clipmap门。
    selected: Option<usize>,
    new_layer_output_id: Option<&str>,
    outputs: &'a [SceneVegetationLayer],
    authoring: &SceneVegetationAuthoring,
) -> Result<&'a SceneVegetationLayer, String> {
    if let Some(output_id) = new_layer_output_id {
        outputs
            .iter()
            .find(|output| output.id == output_id)
            .ok_or_else(|| format!("选择的新图层正式输出{output_id}已不存在"))
    } else {
        selected
            .and_then(|index| authoring.layers.get(index))
            .and_then(|layer| {
                outputs
                    .iter()
                    .find(|output| output.id == layer.output_layer_id)
            })
            .or_else(|| outputs.first())
            .ok_or_else(|| "当前场景没有可供作者层输出的正式植被profile".to_owned())
    } // 明确选择优先；未选择时才沿用当前作者层或第一个正式输出。
}

pub(super) fn duplicate_layer(
    index: usize,
    folder: &str,
    authoring: &mut SceneVegetationAuthoring,
    densities: &mut BTreeMap<String, Arc<Vec<u8>>>,
) -> Result<LocalMutation, String> {
    let source = authoring
        .layers
        .get(index)
        .ok_or_else(|| "选择的作者图层不存在".to_owned())?
        .clone();
    if source.locked {
        return Err(format!("作者图层“{}”已锁定，请先解锁", source.label));
    }
    let pixels = densities
        .get(&source.id)
        .cloned()
        .ok_or_else(|| format!("作者图层{}缺少R8画布", source.id))?;
    let id = unique_layer_id(authoring, &source.output_layer_id);
    let ordinal = authoring
        .layers
        .iter()
        .filter(|layer| layer.output_layer_id == source.output_layer_id)
        .count()
        + 1;
    let mut duplicate = source.clone();
    duplicate.id = id.clone();
    duplicate.label = format!(
        "{} · 区域 {ordinal}",
        source.label.split(" · ").next().unwrap_or(&source.label)
    );
    duplicate.density_source_path =
        format!("{}/vegetation/author/{id}.r8", folder.trim_end_matches('/'));
    authoring.layers.insert(index + 1, duplicate);
    densities.insert(id, pixels); // 此刻共享像素；任一层首次落笔会由Arc::make_mut自动分离。
    Ok(changed(
        Some(index + 1),
        format!("已复制作者图层“{}”", source.label),
    ))
}

pub(super) fn make_group(index: usize) -> SceneVegetationAuthorGroup {
    let stamp = unique_stamp();
    SceneVegetationAuthorGroup {
        id: format!("group-{stamp:x}"),
        label: format!("图层组 {}", index + 1),
        mode: SceneVegetationAuthorGroupMode::Additive,
        output_merge: SceneVegetationOutputMergeMode::Maximum,
        capacity: 1.0,
        claim_threshold: 0.0,
        selection_seed: stamp as u64 ^ 0x4D41_505F_4752_4F55,
        selection_cell_meters: 8.0,
        enabled: true,
    }
}

fn unique_layer_id(authoring: &SceneVegetationAuthoring, output_id: &str) -> String {
    let stamp = unique_stamp();
    let mut suffix = 0_u32;
    loop {
        let id = if suffix == 0 {
            format!("author-{output_id}-{stamp:x}")
        } else {
            format!("author-{output_id}-{stamp:x}-{suffix}")
        };
        if !authoring.layers.iter().any(|layer| layer.id == id) {
            return id; // 纳秒时间戳加本地探测避免同帧连续点击或已加载作者ID冲突。
        }
        suffix = suffix.saturating_add(1);
    }
}

fn unique_stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn pixel_count(resolution: [u32; 2]) -> Result<usize, String> {
    (resolution[0] as usize)
        .checked_mul(resolution[1] as usize)
        .ok_or_else(|| "植被作者层分辨率乘法溢出".to_owned())
}
