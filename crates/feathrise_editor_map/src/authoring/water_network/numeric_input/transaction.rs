//! 水网实时拖动的稳定点单字段所有权回滚门。

use feathrise_map_content::MapWaterAsset; // 纯函数只读写小型水网作者文档。

use super::{
    WaterNetworkNumericKind, WaterNetworkNumericScrub, WaterNetworkNumericTarget, point_mut_by_id,
    target_value,
}; // 稳定点身份与单字段读写共用生产路径。
use crate::authoring::resources::EditorAuthoringSession; // ECS包装只定位当前草稿。
use crate::authoring::water_network::connections::sync_water_network_connections; // 回滚主字段后按当前连接重算派生端点。

/// 只在同地图、同稳定点的当前字段仍等于latest时恢复before。
fn rollback_owned_water_field(
    water: &mut MapWaterAsset,          // 当前权威水网文档。
    target: &WaterNetworkNumericTarget, // 冻结地图、特征和点稳定ID。
    kind: WaterNetworkNumericKind,      // 高度、宽度、深度或流速单一字段。
    before: f32,                        // 本次拖动前的精确字段值。
    latest: f32,                        // 本事务最后实际拥有的字段值。
) -> bool {
    if water.map_id != target.map_id || target_value(water, target, kind) != Some(latest) {
        return false; // 换图、换点或外部已改同字段时立即失去回滚权。
    }
    let Some(point) = point_mut_by_id(
        water,              // 只在当前权威水网内定位。
        &target.feature_id, // 不使用可漂移数组下标。
        &target.point_id,   // 点ID必须仍存在。
        target.waterfall,   // 严格保持河流/瀑布集合身份。
    ) else {
        return false; // 结构换代后不猜测替代点。
    };
    kind.set_point_value(point, before); // 只撤销一个标量，保留连接和其他字段外部修改。
    sync_water_network_connections(water); // Height回滚后让引用该河端的瀑布端点同步恢复，其他字段也使用同一安全路径。
    true // 成功恢复该事务仍拥有的字段。
}

/// 对当前作者草稿尝试定点回滚一次水网拖动。
pub(super) fn rollback_water_network_scrub_if_owned(
    session: &mut EditorAuthoringSession, // 当前草稿可能已包含外部新分支。
    transaction: &WaterNetworkNumericScrub, // 持有before/latest与稳定目标的未入历史事务。
) -> bool {
    let Some(water) = session
        .draft
        .as_mut()
        .and_then(|draft| draft.water.as_mut())
    else {
        return false; // 当前草稿没有水域时无可回滚对象。
    };
    rollback_owned_water_field(
        water,                    // 只传入当前权威水网。
        &transaction.target,      // 使用冻结稳定身份。
        transaction.kind,         // 只恢复本数值框所有字段。
        transaction.before_value, // 恢复拖动前精确值。
        transaction.latest_value, // 只在当前值仍是本latest时写入。
    )
}

#[cfg(test)]
mod tests {
    use super::*; // 纯回滚门测试不启动水面重建。
    use feathrise_map_content::{
        MapWaterEndpoint, MapWaterRiver, MapWaterSegmentKind, MapWaterSplinePoint, MapWaterfall,
    }; // 构造单河单点与依赖河端的瀑布。

    #[test]
    fn rollback_requires_the_water_field_to_still_equal_owned_latest() {
        let mut water = MapWaterAsset::default(); // 默认地图ID可作为本纯函数稳定身份。
        water.rivers.push(MapWaterRiver {
            id: "river".to_owned(),     // 稳定特征ID。
            label: "测试河".to_owned(), // 作者显示名。
            source_layer_id: None,      // 自由首点避免高度派生约束。
            target_layer_id: None,      // 自由末点。
            points: vec![MapWaterSplinePoint {
                id: "point".to_owned(),               // 稳定点ID。
                position: [0.0, 3.0, 0.0],            // 本测试不修改高度。
                width: 6.12,                          // 模拟拖回两位显示起点的latest。
                depth: 1.5,                           // 其他字段保持合法。
                flow_speed: 2.0,                      // 其他字段保持合法。
                outgoing: MapWaterSegmentKind::River, // 普通河段类别。
            }],
            base_color_srgba: [0.1, 0.2, 0.3, 0.7], // 合法水材质颜色。
            perceptual_roughness: 0.2,              // 合法粗糙度。
        });
        let target = WaterNetworkNumericTarget {
            map_id: water.map_id.clone(),   // 冻结同一地图。
            feature_id: "river".to_owned(), // 指向测试河。
            point_id: "point".to_owned(),   // 指向测试点。
            waterfall: false,               // 使用河流集合。
            authoring_revision: 0,          // 纯函数不读修订。
        };
        assert!(rollback_owned_water_field(
            &mut water,
            &target,
            WaterNetworkNumericKind::Width,
            6.123_456,
            6.12
        )); // changed=false时恢复高精度raw。
        assert_eq!(water.rivers[0].points[0].width, 6.123_456); // 宽度恢复before。
        water.rivers[0].points[0].width = 10.0; // 外部已改同字段。
        assert!(!rollback_owned_water_field(
            &mut water,
            &target,
            WaterNetworkNumericKind::Width,
            6.123_456,
            6.12
        )); // 旧事务不再拥有字段。
        assert_eq!(water.rivers[0].points[0].width, 10.0); // 外部值绝不覆盖。
    }

    #[test]
    fn height_rollback_resynchronizes_waterfall_endpoint_from_current_connections() {
        let mut water = MapWaterAsset::default(); // 构造自由河首和引用它的瀑布。
        water.rivers.push(MapWaterRiver {
            id: "river".to_owned(),     // 被瀑布引用的稳定河ID。
            label: "自由河".to_owned(), // 测试显示名。
            source_layer_id: None,      // 河首Y由数值事务自由编辑。
            target_layer_id: None,      // 无静水端约束。
            points: vec![MapWaterSplinePoint {
                id: "river-start".to_owned(),         // 稳定河首ID。
                position: [1.0, 4.123_456, 2.0],      // 高精度raw起点。
                width: 6.0,                           // 合法宽度。
                depth: 1.5,                           // 合法深度。
                flow_speed: 2.0,                      // 合法流速。
                outgoing: MapWaterSegmentKind::River, // 普通河段。
            }],
            base_color_srgba: [0.1, 0.2, 0.3, 0.7], // 合法水材质。
            perceptual_roughness: 0.2,              // 合法粗糙度。
        });
        water.waterfalls.push(MapWaterfall {
            id: "fall".to_owned(),              // 稳定瀑布ID。
            label: "依赖河首的瀑布".to_owned(), // 测试显示名。
            source: Some(MapWaterEndpoint::RiverStart {
                river_id: "river".to_owned(),
            }), // 瀑布首点派生于河首完整XYZ。
            target: None,                       // 单点测试无下游约束。
            points: vec![MapWaterSplinePoint {
                id: "fall-start".to_owned(),          // 稳定瀑布点ID。
                position: [1.0, 4.123_456, 2.0],      // 初始与河首一致。
                width: 6.0,                           // 合法宽度。
                depth: 1.5,                           // 合法深度。
                flow_speed: 2.0,                      // 合法流速。
                outgoing: MapWaterSegmentKind::River, // 段类别与回滚无关。
            }],
            foam_strength: 1.0,                     // 合法泡沫强度。
            base_color_srgba: [0.1, 0.2, 0.3, 0.7], // 合法水材质。
            perceptual_roughness: 0.2,              // 合法粗糙度。
        });
        water.rivers[0].points[0].position[1] = 5.0; // 模拟Height Changed写入latest。
        sync_water_network_connections(&mut water); // 实时路径已把瀑布首点推到latest。
        let target = WaterNetworkNumericTarget {
            map_id: water.map_id.clone(),       // 冻结同一地图。
            feature_id: "river".to_owned(),     // 指向自由河。
            point_id: "river-start".to_owned(), // 指向河首。
            waterfall: false,                   // 使用河流集合。
            authoring_revision: 0,              // 纯函数不读修订。
        };
        assert!(rollback_owned_water_field(
            &mut water,
            &target,
            WaterNetworkNumericKind::Height,
            4.123_456,
            5.0
        )); // raw no-op或冲突时恢复主字段并重算派生端。
        assert_eq!(water.rivers[0].points[0].position[1], 4.123_456); // 河首恢复raw before。
        assert_eq!(water.waterfalls[0].points[0].position[1], 4.123_456); // 瀑布首点不遗留latest。
    }
}
