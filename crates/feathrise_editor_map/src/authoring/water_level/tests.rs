//! 静水水位稳定身份、范围和拖动历史收口回归。

use super::*; // 子模块测试可访问生产纯函数与事务收口。

fn exact_request(
    map_id: &feathrise_map_content::MapId, // 请求所属稳定地图。
    layer_id: &str,                          // 请求所属稳定水层。
    water_level: f32,                        // 待应用精确水位。
) -> WaterLevelSetRequest {
    WaterLevelSetRequest {
        target: WaterLevelInputTarget {
            map_id: map_id.clone(),        // 冻结测试地图。
            layer_id: layer_id.to_owned(), // 冻结测试水层。
            authoring_revision: 0,         // 纯函数不使用修订。
        },
        water_level, // 将测试值交给生产范围门。
    }
}

#[test]
fn exact_signed_water_level_targets_the_stable_layer_id() {
    let map_id =
        feathrise_map_content::MapId::parse("MAP-water-level-test").expect("测试地图ID有效"); // 合法稳定地图。
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&map_id, "maps/MAP-water-level-test"); // 构造显式湖泊夹具。
    let mut pond = water.layers[0].clone(); // 复制第二层验证ID定位。
    pond.id = "pond".to_owned(); // 第二层稳定ID。
    pond.label = "高处水潭".to_owned(); // 第二层显示名。
    pond.water_level = 37.125; // 第二层必须保持。
    water.layers.push(pond); // 完成双层测试数据。
    let (label, before, after) =
        set_water_level(&mut water, &exact_request(&map_id, "lake", -2.75))
            .expect("负数精确水位应合法"); // 只修改lake。
    assert_eq!(label, "湖泊"); // 返回稳定层标签。
    assert_eq!(before, 0.0); // 记录原值。
    assert_eq!(after, -2.75); // 记录新值。
    assert_eq!(water.layers[0].water_level, -2.75); // 目标层已改。
    assert_eq!(water.layers[1].water_level, 37.125); // 其他层保持。
}

#[test]
fn invalid_exact_water_level_does_not_mutate_the_asset() {
    let map_id =
        feathrise_map_content::MapId::parse("MAP-water-level-test").expect("测试地图ID有效"); // 合法稳定地图。
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&map_id, "maps/MAP-water-level-test"); // 构造含静水体的水域夹具。
    for invalid in [f32::NAN, f32::INFINITY, MAX_WATER_LEVEL_METERS + 0.001] {
        let result = set_water_level(&mut water, &exact_request(&map_id, "lake", invalid)); // 依次验证异常值。
        assert!(result.is_err()); // 异常值全部拒绝。
        assert_eq!(water.layers[0].water_level, 0.0); // 资产保持原值。
    }
}

#[test]
fn same_layer_id_from_another_map_is_rejected_without_mutation() {
    let map_a = feathrise_map_content::MapId::parse("MAP-water-a").expect("测试地图ID有效"); // 请求地图。
    let map_b = feathrise_map_content::MapId::parse("MAP-water-b").expect("测试地图ID有效"); // 当前地图。
    let mut water = crate::authoring::water_asset_with_layer_for_tests(&map_b, "maps/MAP-water-b"); // 当前水域属于B且含同名层。
    let result = set_water_level(&mut water, &exact_request(&map_a, "lake", 2.5)); // 使用A请求。
    assert!(result.is_err()); // 跨图同层ID也拒绝。
    assert_eq!(water.layers[0].water_level, 0.0); // B地图不变。
}

#[test]
fn clipmap_cursor_level_request_waits_without_mutating_or_losing_identity() {
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.manifest.terrain.kind = MapTerrainKind::SparseClipmap;
    draft.water = Some(crate::authoring::water_asset_with_layer_for_tests(
        &draft.map_id,
        &draft.folder,
    ));
    let target = WaterLevelInputTarget {
        map_id: draft.map_id.clone(),
        layer_id: draft.water.as_ref().unwrap().layers[0].id.clone(),
        authoring_revision: 4,
    };
    let mut app = App::new();
    app.insert_resource(MapEditorState {
        open: true,
        tool: crate::types::EditorTool::Water,
        water_level_from_cursor_requested: Some(target.clone()),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        revision: 4,
        ..default()
    })
    .add_systems(Update, apply_water_level_requests);

    app.update();

    let editor = app.world().resource::<MapEditorState>();
    assert_eq!(
        editor.water_level_from_cursor_requested.as_ref(),
        Some(&target)
    ); // 尚无已采用地表快照时保留同一稳定身份，回到视口后继续等待。
    assert!(editor.status.contains("真实地形"));
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .water
            .as_ref()
            .unwrap()
            .layers[0]
            .water_level,
        0.0
    ); // 等待状态绝不把水平面或空值写成水位。
}

#[test]
fn one_ulp_water_level_difference_remains_a_real_author_change() {
    let map_id = feathrise_map_content::MapId::parse("MAP-water-ulp").expect("测试地图ID有效"); // 高精度测试地图。
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&map_id, "maps/MAP-water-ulp"); // 显式单层水域夹具。
    water.layers[0].water_level = 1.0; // 设定可表示起点。
    let next = f32::from_bits(1.0f32.to_bits() + 1); // 相邻一个ULP。
    let (_, before, after) = set_water_level(&mut water, &exact_request(&map_id, "lake", next))
        .expect("相邻可表示水位仍是有效作者修改"); // 验证不做近似同值。
    assert_ne!(before, after); // 一ULP仍是变更。
    assert_eq!(water.layers[0].water_level, next); // 保留精确值。
}

#[test]
fn many_live_water_levels_finish_as_one_undo_command() {
    let map_id = feathrise_map_content::MapId::parse("MAP-water-scrub").expect("测试地图ID有效"); // 拖动测试地图。
    let mut editor = MapEditorState {
        dirty: true,
        ..default()
    }; // 模拟实时Changed已标脏。
    let mut session = EditorAuthoringSession {
        revision: 7,
        water_revision: 7,
        ..default()
    }; // 多档位已推进预览修订。
    let mut scrub = WaterLevelScrubState {
        target: Some(WaterLevelInputTarget {
            map_id,
            layer_id: "river".to_owned(),
            authoring_revision: 3,
        }), // 冻结稳定层。
        before: 0.0,          // 唯一撤销起点。
        after: 4.25,          // 最终拥有值。
        expected_revision: 7, // 本事务最后修订。
        dirty_before: false,  // 进入拖动前未标脏。
        aborted: false,       // 正常收口。
    };
    finish_water_level_scrub(&mut editor, &mut session, &mut scrub, "河水"); // 整段拖动收口。
    assert_eq!(session.undo.len(), 1); // 只一条撤销。
    assert!(matches!(
        session.undo.first(),
        Some(EditorCommand::WaterLevelChanged {
            before: 0.0,
            after: 4.25,
            ..
        })
    )); // 撤销端点正确。
    assert!(scrub.target.is_none()); // 事务已清理。
}

#[test]
fn returning_to_exact_start_restores_previous_dirty_state_without_undo() {
    let map_id = feathrise_map_content::MapId::parse("MAP-water-scrub").expect("测试地图ID有效"); // 拖回起点地图。
    let mut editor = MapEditorState {
        dirty: true,
        ..default()
    }; // 实时阶段暂时标脏。
    let mut session = EditorAuthoringSession::default(); // 空历史起点。
    let mut scrub = WaterLevelScrubState {
        target: Some(WaterLevelInputTarget {
            map_id,
            layer_id: "river".to_owned(),
            authoring_revision: 0,
        }), // 冻结层。
        before: 2.125,        // 起点。
        after: 2.125,         // 拖回相同值。
        expected_revision: 2, // 中途可已推进预览。
        dirty_before: false,  // 进入前未脏。
        aborted: false,       // 正常收口。
    };
    finish_water_level_scrub(&mut editor, &mut session, &mut scrub, "河水"); // 收口无净变化事务。
    assert!(session.undo.is_empty()); // 不留空撤销。
    assert!(!editor.dirty); // 恢复进入前dirty。
}
