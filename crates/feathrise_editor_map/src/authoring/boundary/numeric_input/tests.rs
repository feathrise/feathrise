//! 墙厚稳定路径纯逻辑回归。

use super::*; // 测试可访问父模块私有稳定目标和写入函数。

#[test]
fn vertical_fields_share_validation_without_touching_other_fields() {
    let map = MapId::parse("MAP-wall-height").unwrap(); // 独立测试墙。
    let mut document =
        MapBoundariesAsset::rectangular_default(&map, [-20.0; 2], [20.0; 2], 0.0, 20.0).unwrap();
    let path = &mut document.paths[0];
    let original = path.clone();
    assert!(set_field(path, BoundaryField::Bottom, original.top_y).is_err()); // 禁止零高或负高。
    assert_eq!(*path, original); // 错误原子保持原值。
    assert!(set_field(path, BoundaryField::Top, f32::NAN).is_err());
    assert!(set_field(path, BoundaryField::Bottom, 1.0).unwrap());
    assert!(set_field(path, BoundaryField::Top, 4.0).unwrap());
    assert_eq!(path.thickness, original.thickness); // 高度不串写墙厚。
    assert_eq!(path.points, original.points); // 不修改几何或创建额外点。
}

#[test]
fn scalar_rollback_preserves_external_geometry_and_other_fields() {
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    let document =
        MapBoundariesAsset::rectangular_default(&draft.map_id, [0.0; 2], [10.0; 2], 0.0, 20.0)
            .unwrap();
    let before = document.paths[0].clone();
    let target = BoundaryThicknessTarget {
        map_id: draft.map_id.clone(),
        path_id: before.id.clone(),
        authoring_revision: 0,
    };
    draft.boundaries = Some(document);
    let mut session = EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    };
    let scrub = BoundaryScrub {
        entity: Entity::PLACEHOLDER,
        target,
        index: 0,
        field: BoundaryField::Top,
        before: before.clone(),
        latest: 80.0,
        aborted: false,
    };
    let path = &mut session
        .draft
        .as_mut()
        .unwrap()
        .boundaries
        .as_mut()
        .unwrap()
        .paths[0];
    path.top_y = 80.0; // 当前事务拥有的临时值。
    path.thickness = 6.0; // 其他事务修改独立字段。
    rollback(&mut session, &scrub);
    let path = &mut session
        .draft
        .as_mut()
        .unwrap()
        .boundaries
        .as_mut()
        .unwrap()
        .paths[0];
    assert_eq!(path.top_y, before.top_y); // 恢复本字段。
    assert_eq!(path.thickness, 6.0); // 保留外部字段。
    path.top_y = 90.0; // 外部修改同字段后失去回滚所有权。
    rollback(&mut session, &scrub);
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .boundaries
            .as_ref()
            .unwrap()
            .paths[0]
            .top_y,
        90.0
    );
}

#[test]
fn viewport_selected_secondary_path_is_the_numeric_target() {
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    let mut document =
        MapBoundariesAsset::rectangular_default(&draft.map_id, [0.0, 0.0], [10.0, 10.0], 0.0, 20.0)
            .unwrap();
    document.paths.push(document.paths[0].clone());
    document.paths[1].id = "inner".into();
    draft.boundaries = Some(document);
    let map = draft.map_id.clone();
    let mut session = EditorAuthoringSession {
        draft: Some(draft),
        selected_boundary_path: Some((map, "inner".into())),
        ..default()
    };
    let target = selected_target(&session).unwrap(); // 墙厚控件通过生产解析器获取次路径。
    assert_eq!(target.path_id, "inner");
    let document = session.draft.as_mut().unwrap().boundaries.as_mut().unwrap();
    assert!(set_target_value(document, &target, 4.0).unwrap());
    assert_eq!(document.paths[0].thickness, 2.0); // 没有误写首路径。
    assert_eq!(document.paths[1].thickness, 4.0);
}

#[test]
fn thickness_write_uses_stable_path_id_after_reorder() {
    let map_id = MapId::parse("MAP-boundary-numeric").expect("测试地图ID合法"); // 构造正式稳定身份。
    let mut document = MapBoundariesAsset::rectangular_default(
        &map_id,        // 边界所属地图。
        [-50.0, -50.0], // 合法世界最小角。
        [50.0, 50.0],   // 合法世界最大角。
        -20.0,          // 合法墙底。
        40.0,           // 合法墙高跨度。
    )
    .expect("默认边界应可构造");
    let target = BoundaryThicknessTarget {
        map_id,                                // 冻结测试地图。
        path_id: document.paths[0].id.clone(), // 冻结稳定路径ID。
        authoring_revision: 0,                 // 纯函数不读取修订。
    };
    document.paths.insert(0, document.paths[0].clone()); // 模拟数组前插并改成其他路径身份。
    document.paths[0].id = "other".to_owned(); // 旧下标现在指向其他路径。
    assert!(set_target_value(&mut document, &target, 3.5).expect("稳定目标仍存在")); // 写入必须按ID寻找原路径。
    assert_eq!(document.paths[0].thickness, 2.0); // 新首项不得被旧下标误写。
    assert_eq!(document.paths[1].thickness, 3.5); // 原稳定路径收到新墙厚。
}
