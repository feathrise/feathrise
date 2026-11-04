//! 水网共享投影、射线和跨水体连接派生的纯逻辑回归。

use bevy::prelude::*; // 纯Vec投影与射线交点测试。
use feathrise_map_content::{
    DEFAULT_MAP_WATER_PERCEPTUAL_ROUGHNESS, MapId, MapWaterAsset, MapWaterEndpoint, MapWaterRiver,
    MapWaterSegmentKind, MapWaterSplinePoint, MapWaterfall,
}; // 构造最小冻结水网验证端点连续性。

use super::connections::{next_endpoint_choice, sync_water_network_connections};
use super::helpers::{
    DEFAULT_POINT_DEPTH, DEFAULT_POINT_SPEED, DEFAULT_POINT_WIDTH, allocate_water_id,
    horizontal_plane_intersection, nearest_projected_point,
}; // 纯函数回归不启动Bevy App或渲染世界。
use crate::authoring::EditorAuthoringSession; // 稳定ID回归模拟进程重开后序号从零开始。

fn point(id: &str, position: [f32; 3]) -> MapWaterSplinePoint {
    MapWaterSplinePoint {
        id: id.to_owned(),                    // 测试路径内稳定身份。
        position,                             // 当前用例指定的三维位置。
        width: DEFAULT_POINT_WIDTH,           // 复用正式作者默认完整宽度。
        depth: DEFAULT_POINT_DEPTH,           // 复用正式作者玩法深度。
        flow_speed: DEFAULT_POINT_SPEED,      // 复用正式作者流速。
        outgoing: MapWaterSegmentKind::River, // 测试不需要急流样式分支。
    }
}

fn connected_water() -> MapWaterAsset {
    let map_id = MapId::parse("MAP-water-network-sync-test").expect("测试地图ID应有效");
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&map_id, "maps/MAP-water-sync");
    water.layers[0].id = "upper".to_owned(); // 第一层作为十米高位静水源。
    water.layers[0].water_level = 10.0;
    let mut lower = water.layers[0].clone(); // 第二层沿用受检静水规格，只替换稳定身份和水位。
    lower.id = "lower".to_owned();
    lower.label = "低位静水".to_owned();
    lower.mask_source_path = "maps/MAP-water-sync/water/lower.r8".to_owned();
    lower.mask_texture_path = "maps/MAP-water-sync/water/lower.ktx2".to_owned();
    lower.water_level = 2.0;
    water.layers.push(lower);
    water.rivers.push(MapWaterRiver {
        id: "river".to_owned(),                    // 瀑布端点引用的稳定河流ID。
        label: "测试河流".to_owned(),              // 仅用于失败诊断。
        source_layer_id: Some("upper".to_owned()), // 首点跟随高位静水。
        target_layer_id: Some("lower".to_owned()), // 末点跟随低位静水。
        points: vec![point("river-start", [1.0, 0.0, 2.0])], // 先验证单点源优先。
        base_color_srgba: [0.055, 0.24, 0.29, 0.72], // 使用正式默认范围内材质。
        perceptual_roughness: DEFAULT_MAP_WATER_PERCEPTUAL_ROUGHNESS, // 使用清晰反射正式默认粗糙度。
    });
    water
}

#[test]
fn endpoint_cycle_starts_with_disconnect() {
    let choices = vec![
        None,
        Some(MapWaterEndpoint::StillWater {
            layer_id: "lake".to_owned(),
        }),
    ];
    assert_eq!(next_endpoint_choice(&choices, &None), choices[1]);
    assert_eq!(next_endpoint_choice(&choices, &choices[1]), None);
}

#[test]
fn stable_id_allocator_skips_existing_ids_after_a_map_is_reopened() {
    let mut water = connected_water();
    let mut session = EditorAuthoringSession {
        revision: 7,
        ..default()
    }; // 重开地图后会话序号归零，但作者资产继续保留旧稳定ID。
    water.rivers[0].id = "water-river-7-1".to_owned();
    assert_eq!(
        allocate_water_id(&mut session, &water, "water-river"),
        "water-river-7-2"
    ); // 首个候选碰撞时必须在一次按钮动作内直接取得下一个可用ID。
    water.rivers[0].points[0].id = "water-point-7-3".to_owned();
    assert_eq!(
        allocate_water_id(&mut session, &water, "water-point"),
        "water-point-7-4"
    ); // 控制点也不得因重启后的序号复用而需要作者重复落点。
}

#[test]
fn projected_pick_distinguishes_points_with_the_same_xz() {
    let points = vec![
        point("lower", [3.0, 2.0, 7.0]),  // 与另一点共享XZ但屏幕高度更低。
        point("upper", [3.0, 12.0, 7.0]), // 模拟透视中可见的高位瀑布口。
    ];
    let selected = nearest_projected_point(
        &points,
        Vec2::new(3.0, 12.0),
        1.0,
        |world| Some(Vec2::new(world.x, world.y)), // 纯闭包模拟确定相机投影，不启动渲染世界。
    );
    assert_eq!(selected, Some(1)); // 命中由实际投影决定，不再被相同地形XZ锁到首点。
}

#[test]
fn horizontal_plane_hit_supports_perspective_and_orthographic_rays() {
    let perspective = horizontal_plane_intersection(
        Vec3::new(0.0, 10.0, 10.0),
        Vec3::new(0.0, -1.0, -1.0).normalize(),
        4.0,
    )
    .expect("向下透视射线应命中控制点水平面");
    assert!(perspective.distance(Vec3::new(0.0, 4.0, 4.0)) < 0.0001);
    let orthographic = horizontal_plane_intersection(Vec3::new(3.0, 10.0, -2.0), Vec3::NEG_Y, 2.0)
        .expect("顶视正交射线应保持XZ并命中指定Y");
    assert_eq!(orthographic, Vec3::new(3.0, 2.0, -2.0));
    assert!(horizontal_plane_intersection(Vec3::ZERO, Vec3::X, 1.0).is_none()); // 平行射线必须拒绝。
}

#[test]
fn single_river_point_prefers_source_until_target_has_its_own_point() {
    let mut water = connected_water();
    sync_water_network_connections(&mut water);
    assert_eq!(water.rivers[0].points[0].position[1], 10.0); // 单点两端连接时源水位优先。
    water.rivers[0]
        .points
        .push(point("river-end", [5.0, 0.0, 6.0]));
    sync_water_network_connections(&mut water);
    assert_eq!(water.rivers[0].points[0].position[1], 10.0); // 多点首端继续跟随源。
    assert_eq!(water.rivers[0].points[1].position[1], 2.0); // 多点末端开始独立跟随目标。
}

#[test]
fn waterfall_copies_river_xyz_but_only_still_water_y() {
    let mut water = connected_water();
    water.rivers[0]
        .points
        .push(point("river-end", [5.0, 0.0, 6.0]));
    water.waterfalls.push(MapWaterfall {
        id: "waterfall".to_owned(),   // 独立瀑布稳定ID。
        label: "测试瀑布".to_owned(), // 仅用于诊断。
        source: Some(MapWaterEndpoint::RiverStart {
            river_id: "river".to_owned(), // 上游应复制河流首点完整XYZ。
        }),
        target: Some(MapWaterEndpoint::StillWater {
            layer_id: "lower".to_owned(), // 下游静水只提供绝对Y。
        }),
        points: vec![
            point("fall-start", [100.0, 50.0, 200.0]),
            point("fall-end", [300.0, 40.0, 400.0]),
        ],
        foam_strength: 1.0,                          // 使用正式允许范围。
        base_color_srgba: [0.055, 0.24, 0.29, 0.72], // 使用正式默认范围内材质。
        perceptual_roughness: DEFAULT_MAP_WATER_PERCEPTUAL_ROUGHNESS, // 使用清晰反射正式默认粗糙度。
    });
    sync_water_network_connections(&mut water);
    assert_eq!(water.waterfalls[0].points[0].position, [1.0, 10.0, 2.0]); // 河端完整吸附。
    assert_eq!(water.waterfalls[0].points[1].position, [300.0, 2.0, 400.0]); // 静水保留XZ。
}

#[test]
fn waterfall_disconnects_when_referenced_river_becomes_empty() {
    let mut water = connected_water();
    water.waterfalls.push(MapWaterfall {
        id: "waterfall".to_owned(),   // 独立瀑布稳定ID。
        label: "测试瀑布".to_owned(), // 仅用于诊断。
        source: Some(MapWaterEndpoint::RiverStart {
            river_id: "river".to_owned(), // 清空河流后该端点无法继续维护。
        }),
        target: None,                                       // 目标保持编辑中间态。
        points: vec![point("fall-start", [1.0, 9.0, 2.0])], // 单点足以验证断连。
        foam_strength: 1.0,                                 // 使用正式允许范围。
        base_color_srgba: [0.055, 0.24, 0.29, 0.72],        // 使用正式默认范围内材质。
        perceptual_roughness: DEFAULT_MAP_WATER_PERCEPTUAL_ROUGHNESS, // 使用清晰反射正式默认粗糙度。
    });
    water.rivers[0].points.clear(); // 模拟删除最后一个被引用河流控制点。
    sync_water_network_connections(&mut water);
    assert!(water.waterfalls[0].source.is_none()); // 同一事务显式断开，不留下悬空河端引用。
}
