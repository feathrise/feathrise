//! 植被连续参数的纯作者值转换测试。

use super::*; // 直接验证私有业务边界，无需启动渲染App。
use feathrise_scene_content::{
    SceneVegetationAuthorGroup, SceneVegetationAuthorLayer, SceneVegetationOutputMergeMode,
}; // 构造最小合法层组验证百分比与I32语义。

fn authoring(mode: SceneVegetationAuthorGroupMode) -> SceneVegetationAuthoring {
    SceneVegetationAuthoring {
        groups: vec![SceneVegetationAuthorGroup {
            id: "group".to_owned(),
            label: "测试组".to_owned(),
            mode,
            output_merge: SceneVegetationOutputMergeMode::Maximum,
            capacity: 1.0,
            claim_threshold: 0.0,
            selection_seed: 1,
            selection_cell_meters: 8.0,
            enabled: true,
        }],
        layers: vec![SceneVegetationAuthorLayer {
            id: "layer".to_owned(),
            label: "测试层".to_owned(),
            group_id: "group".to_owned(),
            output_layer_id: "grass".to_owned(),
            density_source_path: "scenes/test/layer.r8.png".to_owned(),
            resolution: [1, 1],
            enabled: true,
            locked: false,
            opacity: 1.0,
            priority: -3,
            weight: 1.0,
        }],
    }
}

fn target(kind: VegetationNumericKind) -> VegetationNumericTarget {
    VegetationNumericTarget {
        scene_id: SceneId::parse("MAP-test").expect("测试SceneId合法"),
        layer_id: "layer".to_owned(),
        group_id: "group".to_owned(),
        kind,
        authoring_revision: 0,
    }
}

#[test]
fn priority_uses_real_signed_integer_and_percentages_round_trip_to_author_values() {
    let mut value = authoring(SceneVegetationAuthorGroupMode::WeightedPartition);
    assert!(
        set_authoring_value(
            &mut value,
            &target(VegetationNumericKind::Priority),
            NumericValue::I32(-12),
        )
        .unwrap()
    );
    assert!(
        set_authoring_value(
            &mut value,
            &target(VegetationNumericKind::OpacityPercent),
            NumericValue::F32(35.0),
        )
        .unwrap()
    );
    assert!(
        set_authoring_value(
            &mut value,
            &target(VegetationNumericKind::GroupCapacityPercent),
            NumericValue::F32(80.0),
        )
        .unwrap()
    );
    assert_eq!(value.layers[0].priority, -12);
    assert_eq!(value.layers[0].opacity, 0.35);
    assert_eq!(value.groups[0].capacity, 0.8);
}

#[test]
fn group_specific_fields_reject_irrelevant_rules() {
    let mut additive = authoring(SceneVegetationAuthorGroupMode::Additive);
    assert!(
        set_authoring_value(
            &mut additive,
            &target(VegetationNumericKind::GroupCapacityPercent),
            NumericValue::F32(50.0),
        )
        .is_err()
    );
    let mut override_group = authoring(SceneVegetationAuthorGroupMode::PriorityOverride);
    assert!(
        set_authoring_value(
            &mut override_group,
            &target(VegetationNumericKind::ClaimThresholdPercent),
            NumericValue::F32(25.0),
        )
        .unwrap()
    );
    assert_eq!(override_group.groups[0].claim_threshold, 0.25);
}

#[test]
fn display_origin_restores_all_float_fields_from_exact_raw_snapshot() {
    let cases = [
        (
            VegetationNumericKind::Weight,
            SceneVegetationAuthorGroupMode::WeightedPartition,
            NumericValue::F32(1.23),
        ), // 权重显示两位，但作者raw可保留更多尾数。
        (
            VegetationNumericKind::OpacityPercent,
            SceneVegetationAuthorGroupMode::WeightedPartition,
            NumericValue::F32(12.0),
        ), // 透明度显示整数百分比。
        (
            VegetationNumericKind::GroupCapacityPercent,
            SceneVegetationAuthorGroupMode::WeightedPartition,
            NumericValue::F32(67.0),
        ), // 共享容量同样不能量化归一化raw值。
        (
            VegetationNumericKind::ClaimThresholdPercent,
            SceneVegetationAuthorGroupMode::PriorityOverride,
            NumericValue::F32(34.0),
        ), // 覆盖阈值使用独立组规则。
    ];
    for (kind, mode, latest_value) in cases {
        let mut before = authoring(mode); // 每个字段从独立完整作者快照开始。
        before.layers[0].weight = 1.234_567; // 权重raw高于两位显示精度。
        before.layers[0].opacity = 0.123_456_7; // 透明度raw高于整数百分比显示精度。
        before.groups[0].capacity = 0.678_912_3; // 组容量raw保留额外尾数。
        before.groups[0].claim_threshold = 0.345_678_9; // 覆盖阈值raw保留额外尾数。
        let selected_before_value =
            value_from_authoring(&before, &target(kind)).expect("测试稳定字段必须存在"); // 事务冻结业务显示值，同时完整before保留真正raw。
        let mut current = before.clone(); // 模拟左拖离开后又回到按下时的显示档。
        match kind {
            VegetationNumericKind::Weight => current.layers[0].weight = 1.23,
            VegetationNumericKind::OpacityPercent => current.layers[0].opacity = 0.12,
            VegetationNumericKind::GroupCapacityPercent => current.groups[0].capacity = 0.67,
            VegetationNumericKind::ClaimThresholdPercent => {
                current.groups[0].claim_threshold = 0.34;
            }
            VegetationNumericKind::Priority => unreachable!("raw尾数用例不包含I32"),
        } // 当前字段值与共享 UI 控件最后发布的吸附档保持一致。
        current.layers[0].enabled = false; // 同一作者描述上的后来非目标字段必须保留。
        let transaction = VegetationNumericScrub {
            entity: Entity::PLACEHOLDER,         // 纯作者恢复测试不依赖真实UI根。
            target: target(kind),                // 稳定场景、层、组和字段身份。
            before: before.clone(),              // 完整raw起点是恢复权威。
            before_densities: BTreeMap::new(),   // 数值恢复不触及任何R8 Arc。
            before_value: selected_before_value, // 冻结快照一致性证据。
            latest_value,                        // 共享 UI 控件回到按下时的显示档并报告changed=false。
        };
        assert!(
            restore_owned_scrub_value(&mut current, &transaction)
                .expect("仍由事务拥有的显示起点必须恢复raw")
        );
        match kind {
            VegetationNumericKind::Weight => assert_eq!(
                current.layers[0].weight.to_bits(),
                before.layers[0].weight.to_bits(),
            ),
            VegetationNumericKind::OpacityPercent => assert_eq!(
                current.layers[0].opacity.to_bits(),
                before.layers[0].opacity.to_bits(),
            ),
            VegetationNumericKind::GroupCapacityPercent => assert_eq!(
                current.groups[0].capacity.to_bits(),
                before.groups[0].capacity.to_bits(),
            ),
            VegetationNumericKind::ClaimThresholdPercent => assert_eq!(
                current.groups[0].claim_threshold.to_bits(),
                before.groups[0].claim_threshold.to_bits(),
            ),
            VegetationNumericKind::Priority => unreachable!("raw尾数用例不包含I32"),
        } // 位级比较证明没有经过百分比往返量化。
        assert!(!current.layers[0].enabled); // owned-field恢复不得覆盖后来非目标值。
    }
}

#[test]
fn aborted_scrub_latch_survives_late_changes_until_matching_release() {
    let root = Entity::from_bits(52); // 模拟首个Changed已经冻结稳定作者目标的公共根。
    let mut state = VegetationNumericInputState::default(); // 外部revision介入前没有拒绝门。
    state.abort_scrub_until_release(root, VegetationNumericKind::Weight); // 第二个Changed触发owned-field回滚并锁存物理拖动。
    assert!(state.scrub_is_aborted()); // 第三个Changed由系统入口直接丢弃，不可重新begin。
    assert!(!state.release_aborted_scrub(root, VegetationNumericKind::OpacityPercent)); // 错字段松键不能清权重门。
    assert!(state.scrub_is_aborted()); // 错字段消息后仍拒绝后续Changed。
    assert!(state.release_aborted_scrub(root, VegetationNumericKind::Weight)); // 对应release结束本次物理拖动。
    assert!(!state.scrub_is_aborted()); // 下一次独立按下才能重新开始植被事务。
}
