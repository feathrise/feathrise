//! 静水、河流与瀑布稳定端点的选择、显示和几何连续性派生。

use feathrise_map_content::{MapWaterAsset, MapWaterEndpoint, MapWaterSplinePoint}; // 冻结水网结构是连接同步的唯一输入和输出。

/// 读取静水层或河流首尾端点的世界位置；静水只提供Y，XZ由调用方保留。
fn endpoint_position(water: &MapWaterAsset, endpoint: &MapWaterEndpoint) -> Option<[f32; 3]> {
    match endpoint {
        MapWaterEndpoint::StillWater { layer_id } => water
            .layers
            .iter()
            .find(|layer| &layer.id == layer_id)
            .map(|layer| [0.0, layer.water_level, 0.0]), // 静水没有单一平面锚点，零XZ只是明确占位。
        MapWaterEndpoint::RiverStart { river_id } => water
            .rivers
            .iter()
            .find(|river| &river.id == river_id)
            .and_then(|river| river.points.first())
            .map(|point| point.position),
        MapWaterEndpoint::RiverEnd { river_id } => water
            .rivers
            .iter()
            .find(|river| &river.id == river_id)
            .and_then(|river| river.points.last())
            .map(|point| point.position),
    }
}

/// 读取静水层或河流首尾端点的世界Y；空路径没有可吸附高度。
pub(super) fn endpoint_height(water: &MapWaterAsset, endpoint: &MapWaterEndpoint) -> Option<f32> {
    endpoint_position(water, endpoint).map(|position| position[1])
}

/// 把瀑布控制点同步到连接端；静水只约束Y，河流端点约束完整XYZ以维持几何连续。
fn sync_waterfall_point(
    point: &mut MapWaterSplinePoint,
    endpoint: &MapWaterEndpoint,
    endpoint_position: [f32; 3],
) {
    match endpoint {
        MapWaterEndpoint::StillWater { .. } => {
            point.position[1] = endpoint_position[1]; // 湖泊或池塘没有唯一XZ锚点，只同步绝对水位。
        }
        MapWaterEndpoint::RiverStart { .. } | MapWaterEndpoint::RiverEnd { .. } => {
            point.position = endpoint_position; // 河流首尾是唯一三维锚点，瀑布必须无缝跟随完整位置。
        }
    }
}

/// 统一维护静水、河流与瀑布连接连续性；所有作者事务在校验和快照提交前调用。
pub(crate) fn sync_water_network_connections(water: &mut MapWaterAsset) {
    let layers = &water.layers; // 河流只引用静水层，可与河流数组进行不相交字段借用。
    for river in &mut water.rivers {
        if river
            .source_layer_id
            .as_ref()
            .is_some_and(|layer_id| !layers.iter().any(|layer| &layer.id == layer_id))
        {
            river.source_layer_id = None; // 删除或损坏的静水源必须显式断开，不能留下悬空ID。
        }
        if river
            .target_layer_id
            .as_ref()
            .is_some_and(|layer_id| !layers.iter().any(|layer| &layer.id == layer_id))
        {
            river.target_layer_id = None; // 下游静水目标与上游使用相同悬空清理规则。
        }
        let source_height = river.source_layer_id.as_ref().and_then(|layer_id| {
            layers
                .iter()
                .find(|layer| &layer.id == layer_id)
                .map(|layer| layer.water_level)
        });
        let target_height = river.target_layer_id.as_ref().and_then(|layer_id| {
            layers
                .iter()
                .find(|layer| &layer.id == layer_id)
                .map(|layer| layer.water_level)
        });
        match river.points.as_mut_slice() {
            [] => {} // 空河流保留合法编辑中间态，未来首点会在加入时立即同步。
            [point] => {
                if let Some(height) = source_height {
                    point.position[1] = height; // 单点同时有两端时源连接优先，避免同一点被目标二次覆盖。
                } else if let Some(height) = target_height {
                    point.position[1] = height; // 只有目标连接时单点仍跟随该静水位。
                }
            }
            points => {
                if let Some(height) = source_height {
                    points[0].position[1] = height; // 多点河流首点持续跟随上游静水位。
                }
                if let Some(height) = target_height {
                    let last = points.len() - 1;
                    points[last].position[1] = height; // 多点河流末点持续跟随下游静水位。
                }
            }
        }
    }

    for index in 0..water.waterfalls.len() {
        let source = water.waterfalls[index].source.clone(); // 先冻结端点，随后才能可变借用当前瀑布点数组。
        let target = water.waterfalls[index].target.clone();
        let source_position = source
            .as_ref()
            .and_then(|endpoint| endpoint_position(water, endpoint));
        let target_position = target
            .as_ref()
            .and_then(|endpoint| endpoint_position(water, endpoint));
        let waterfall = &mut water.waterfalls[index];
        if source.is_some() && source_position.is_none() {
            waterfall.source = None; // 被删空河流或失效静水端点必须在同一事务内显式断开。
        }
        if target.is_some() && target_position.is_none() {
            waterfall.target = None; // 目标端点同样不能保留到宽松草稿或正式保存链路中。
        }
        match waterfall.points.as_mut_slice() {
            [] => {} // 空瀑布可以先选择端点，首点加入时再同步。
            [point] => {
                if let (Some(endpoint), Some(position)) = (source.as_ref(), source_position) {
                    sync_waterfall_point(point, endpoint, position); // 单点两端并存时优先上游瀑布口。
                } else if let (Some(endpoint), Some(position)) = (target.as_ref(), target_position)
                {
                    sync_waterfall_point(point, endpoint, position); // 只有目标时单点跟随落水端。
                }
            }
            points => {
                if let (Some(endpoint), Some(position)) = (source.as_ref(), source_position) {
                    sync_waterfall_point(&mut points[0], endpoint, position); // 多点瀑布首点持续连接上游。
                }
                if let (Some(endpoint), Some(position)) = (target.as_ref(), target_position) {
                    let last = points.len() - 1;
                    sync_waterfall_point(&mut points[last], endpoint, position); // 末点持续连接下游完整端点或静水位。
                }
            }
        }
    }
}

/// 返回空连接、全部静水体和全部非空河流首尾端点的固定循环序列。
pub(super) fn waterfall_endpoint_choices(water: &MapWaterAsset) -> Vec<Option<MapWaterEndpoint>> {
    let mut choices = vec![None]; // 第一项明确表示断开连接。
    choices.extend(water.layers.iter().map(|layer| {
        Some(MapWaterEndpoint::StillWater {
            layer_id: layer.id.clone(),
        })
    }));
    for river in water.rivers.iter().filter(|river| !river.points.is_empty()) {
        choices.push(Some(MapWaterEndpoint::RiverStart {
            river_id: river.id.clone(),
        }));
        choices.push(Some(MapWaterEndpoint::RiverEnd {
            river_id: river.id.clone(),
        }));
    }
    choices
}

/// 从固定选项中返回当前值后一项；旧坏引用会安全回到第一项。
#[cfg(test)]
pub(super) fn next_endpoint_choice(
    choices: &[Option<MapWaterEndpoint>],
    current: &Option<MapWaterEndpoint>,
) -> Option<MapWaterEndpoint> {
    let index = choices.iter().position(|choice| choice == current);
    choices
        .get(index.map_or(0, |index| (index + 1) % choices.len()))
        .cloned()
        .flatten()
}

/// 返回端点紧凑中文名称，状态栏不暴露内部枚举调试格式。
pub(super) fn endpoint_label(water: &MapWaterAsset, endpoint: &Option<MapWaterEndpoint>) -> String {
    let Some(endpoint) = endpoint else {
        return "未连接".to_owned();
    };
    match endpoint {
        MapWaterEndpoint::StillWater { layer_id } => water
            .layers
            .iter()
            .find(|layer| &layer.id == layer_id)
            .map_or_else(
                || format!("失效静水:{layer_id}"),
                |layer| format!("静水:{}", layer.label),
            ),
        MapWaterEndpoint::RiverStart { river_id } => water
            .rivers
            .iter()
            .find(|river| &river.id == river_id)
            .map_or_else(
                || format!("失效河首:{river_id}"),
                |river| format!("河首:{}", river.label),
            ),
        MapWaterEndpoint::RiverEnd { river_id } => water
            .rivers
            .iter()
            .find(|river| &river.id == river_id)
            .map_or_else(
                || format!("失效河尾:{river_id}"),
                |river| format!("河尾:{}", river.label),
            ),
    }
}

/// 判断瀑布端点是否引用待删除河流。
pub(super) fn endpoint_references_river(
    endpoint: &Option<MapWaterEndpoint>,
    river_id: &str,
) -> bool {
    matches!(
        endpoint,
        Some(MapWaterEndpoint::RiverStart { river_id: value })
            | Some(MapWaterEndpoint::RiverEnd { river_id: value }) if value == river_id
    )
}
