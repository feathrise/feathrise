//! 后台单点作者归属：复用正式竞争、最大池化和跨组合并规则。
use super::*; // 共用正式编译器私有像素函数，禁止另造近似组规则。
use bevy::prelude::Vec2; // 世界XZ采样坐标。

pub(crate) fn author_layers_at_world(
    vegetation: &SceneVegetationAsset,
    densities: &BTreeMap<String, Arc<Vec<u8>>>,
    world_min: [f32; 2],
    world_max: [f32; 2],
    world: Vec2,
) -> Vec<String> {
    let Some(authoring) = vegetation.authoring.as_ref() else {
        return vec![];
    }; // 旧数据由加载迁移处理。
    let min = Vec2::from(world_min);
    let max = Vec2::from(world_max); // 不接受场景外钳制命中。
    if !world.is_finite()
        || !min.is_finite()
        || !max.is_finite()
        || min.cmpge(max).any()
        || world.cmplt(min).any()
        || world.cmpgt(max).any()
    {
        return vec![];
    }
    if authoring.validate(&vegetation.layers).is_err() {
        return vec![];
    } // 损坏作者图失败关闭。
    let uv = (world - min) / (max - min); // 正式输出纹素的端点坐标。
    let indices = vegetation
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| (l.id.as_str(), i))
        .collect::<BTreeMap<_, _>>(); // 后台轻量索引。
    let mut outputs = vec![(0u8, BTreeSet::<String>::new()); vegetation.layers.len()]; // 每输出只保留一个点击纹素与贡献ID。
    for group in authoring.groups.iter().filter(|g| g.enabled) {
        let layers = authoring
            .layers
            .iter()
            .filter(|l| l.enabled && l.group_id == group.id)
            .collect::<Vec<_>>(); // 隐藏层不参与。
        let prepared = layers
            .iter()
            .map(|l| prepare_layer(l, densities, &indices))
            .collect::<Result<Vec<_>, _>>(); // 共享R8，不克隆图像。
        let Ok(prepared) = prepared else {
            continue;
        }; // 坏组不能制造候选。
        let from = layers.iter().fold([2, 2], |r, l| {
            [r[0].max(l.resolution[0]), r[1].max(l.resolution[1])]
        }); // 与正式组分辨率一致。
        let mut demands = vec![0.0; layers.len()];
        let mut resolved = demands.clone(); // 单像素工作区。
        let used = prepared
            .iter()
            .map(|p| p.output_index)
            .collect::<BTreeSet<_>>(); // 只解析实际引用输出。
        for output_index in used {
            let to = vegetation.layers[output_index].resolution; // 读取正式目标纹素规格。
            let at = [
                (uv.x * to[0].saturating_sub(1) as f32).round() as u64,
                (uv.y * to[1].saturating_sub(1) as f32).round() as u64,
            ]; // 匹配视口采样。
            let start = [
                at[0] * u64::from(from[0]) / u64::from(to[0]),
                at[1] * u64::from(from[1]) / u64::from(to[1]),
            ]; // 正式最大池化覆盖区间。
            let end = [
                ((at[0] + 1) * u64::from(from[0]))
                    .div_ceil(u64::from(to[0]))
                    .max(start[0] + 1)
                    .min(u64::from(from[0])),
                ((at[1] + 1) * u64::from(from[1]))
                    .div_ceil(u64::from(to[1]))
                    .max(start[1] + 1)
                    .min(u64::from(from[1])),
            ]; // 与resample_max边界一致。
            let mut value = 0u8;
            let mut contributors = BTreeSet::new(); // 当前组该输出的最终纹素。
            for y in start[1]..end[1] {
                for x in start[0]..end[0] {
                    for (i, p) in prepared.iter().enumerate() {
                        demands[i] = f32::from(sample_nearest(
                            &p.pixels,
                            p.resolution,
                            from,
                            x as u32,
                            y as u32,
                        )) / 255.0
                            * p.opacity;
                    } // 与正式原始需求一致。
                    resolve_group_pixel(
                        group.mode,
                        group.capacity,
                        group.claim_threshold,
                        group.selection_seed,
                        group.selection_cell_meters,
                        &prepared,
                        &demands,
                        &mut resolved,
                        from,
                        x as u32,
                        y as u32,
                        world_min,
                        world_max,
                    ); // 真实优先级和竞争结果。
                    let pixel = resolved
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| prepared[*i].output_index == output_index)
                        .fold(0u8, |v, (_, n)| v.saturating_add(quantize_density(*n))); // 同输出组内量化累加。
                    if pixel > value {
                        value = pixel;
                        contributors.clear();
                    } // 最大池化只保留获胜纹素来源。
                    if pixel > 0 && pixel == value {
                        for (i, n) in resolved.iter().enumerate() {
                            if prepared[i].output_index == output_index && quantize_density(*n) > 0
                            {
                                contributors.insert(layers[i].id.clone());
                            }
                        }
                    } // 同值贡献并列候选。
                }
            }
            if value == 0 {
                continue;
            } // 零组不覆盖之前组。
            let (previous, ids) = &mut outputs[output_index]; // 跨组按正式合并方式保留真实贡献。
            match group.output_merge {
                SceneVegetationOutputMergeMode::ReplaceNonZero => {
                    *previous = value;
                    *ids = contributors;
                } // 后组非零替换前组。
                SceneVegetationOutputMergeMode::Maximum if value > *previous => {
                    *previous = value;
                    *ids = contributors;
                } // 强组覆盖。
                SceneVegetationOutputMergeMode::Maximum if value == *previous => {
                    ids.extend(contributors)
                } // 同值来源均可检查。
                SceneVegetationOutputMergeMode::Maximum => {} // 弱组不贡献。
                SceneVegetationOutputMergeMode::AddClamped => {
                    *previous = previous.saturating_add(value);
                    ids.extend(contributors);
                } // 共存保留多个作者层。
            }
        }
    }
    let ids = outputs
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .collect::<BTreeSet<_>>(); // 去重后按作者目录顺序返回。
    authoring
        .layers
        .iter()
        .filter(|l| ids.contains(&l.id))
        .map(|l| l.id.clone())
        .collect() // 同预设不同层保留独立ID。
}
