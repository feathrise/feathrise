//! 网格水专业数值框的纯业务边界回归测试。

use feathrise_scene_content::ScenePlacementWaterKind; // 构造一条通过公共水绑定校验的完整作者记录。

use super::*; // 直接覆盖字段写入、稳定身份与独立灵敏度，不启动窗口或RenderApp。
use crate::authoring::{EditorAuthoringSession, EditorCommand}; // 验证整段拖动只压入一个正式作者命令。
use crate::resources::SceneEditorState; // 验证无净变化时恢复进入拖动前的dirty门。

/// 构造同一组测试共享的合法Blender整网格水绑定与稳定目标。
fn record_and_target() -> (ScenePlacementRecord, PlacementWaterNumericTarget) {
    let scene_id = SceneId::parse("MAP-numeric-water").expect("测试场景ID必须合法"); // 稳定场景身份用于模拟作者焦点冻结。
    let record = ScenePlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: "placement-water".to_owned(), // 数值事务按完整placement ID定位记录。
        asset_path: "gltf/river.glb".to_owned(), // 合法相对GLB路径仅作为完整快照内容。
        asset_target: Default::default(), // 水字段事务必须保持完整Scene0目标。
        building_preset_id: None,         // 水字段事务夹具不附带建筑目录身份。
        courtyard: None,                  // 独立摆放记录不属于任何院落副本。
        translation: [0.0, 3.0, 0.0],     // 摆放变换不应被任一水字段修改。
        height_mode: feathrise_scene_content::ScenePlacementHeightMode::FixedHeight, // 水字段测试锁定高度元数据不变。
        terrain_offset: 0.0,    // 固定高度不携带离地偏移。
        yaw_degrees: 15.0,      // 非零旋转帮助测试避免依赖默认记录。
        scale: [1.0, 1.0, 1.0], // 普通单位缩放保持测试聚焦水字段。
        water_surfaces: vec![ScenePlacementWaterSurface {
            mesh_index: 2, // 索引只作诊断，稳定匹配依赖路径和网格名。
            node_path: vec!["场景根".to_owned(), "河流对象".to_owned()], // Blender显式对象路径。
            mesh_name: "河流水面".to_owned(), // Blender显式Mesh datablock名称。
            kind: ScenePlacementWaterKind::Sloped, // 斜水面同时支持流速与世界Y偏移。
            flow_speed: 1.0, // 标准一倍起点。
            height_offset: 2.5, // 非零高度用于确认字段互不串写。
        }],
    };
    let target = PlacementWaterNumericTarget {
        scene_id,                          // 测试只验证记录内部纯修改，仍保留完整冻结身份。
        placement_id: record.id.clone(), // 与记录稳定ID一致。
        record_index: 0,                 // 测试记录位于唯一数组首项。
        surface_index: 0,                // 唯一水绑定位于记录首项，拖中直接按槽访问。
        node_path: record.water_surfaces[0].node_path.clone(), // 匹配显式对象路径。
        mesh_name: record.water_surfaces[0].mesh_name.clone(), // 匹配显式网格名。
        authoring_revision: 7,           // 任意非零修订证明值写入不依赖默认修订。
        placement_structure_revision: 0, // 纯记录测试不发生结构换代。
    };
    (record, target)
}

#[test]
fn numeric_fields_update_independently_and_reject_invalid_values() {
    let (mut record, target) = record_and_target(); // 从同时含两项非默认值的完整记录开始。
    assert!(
        set_record_numeric_value(
            &mut record,
            &target,
            PlacementWaterNumericKind::FlowSpeed,
            3.25,
        )
        .expect("合法流速必须通过公共水绑定校验")
    );
    assert_eq!(record.water_surfaces[0].flow_speed, 3.25); // 左拖或精确输入只改变流速。
    assert_eq!(record.water_surfaces[0].height_offset, 2.5); // 流速输入不得串写高度。

    assert!(
        set_record_numeric_value(
            &mut record,
            &target,
            PlacementWaterNumericKind::HeightOffset,
            -12.125,
        )
        .expect("合法世界Y偏移必须通过公共水绑定校验")
    );
    assert_eq!(record.water_surfaces[0].flow_speed, 3.25); // 高度输入不得串写流速。
    assert_eq!(record.water_surfaces[0].height_offset, -12.125); // 世界Y值按米原样保存。

    let before = record.water_surfaces[0].clone(); // 非法候选必须保持完整绑定原状。
    assert!(
        set_record_numeric_value(
            &mut record,
            &target,
            PlacementWaterNumericKind::FlowSpeed,
            f32::NAN,
        )
        .is_err()
    );
    assert_eq!(record.water_surfaces[0], before); // NaN不得部分写入作者草稿。
    assert!(
        set_record_numeric_value(
            &mut record,
            &target,
            PlacementWaterNumericKind::HeightOffset,
            MAX_SCENE_PLACEMENT_WATER_HEIGHT_OFFSET_METERS + 1.0,
        )
        .is_err()
    );
    assert_eq!(record.water_surfaces[0], before); // 越界高度同样保持记录完全不变。
}

#[test]
fn scrub_fast_path_uses_frozen_surface_slot_without_full_record_validation() {
    let (mut record, target) = record_and_target(); // 首项是冻结目标。
    let mut invalid_duplicate = record.water_surfaces[0].clone(); // 第二项故意复用同一稳定身份破坏完整记录唯一性。
    invalid_duplicate.flow_speed = 2.0; // 不同数值仍然是同一非法目标身份。
    record.water_surfaces.push(invalid_duplicate); // 公共全记录校验现在必然失败。
    assert!(record.validate_water_surfaces().is_err()); // 先证明夹带的非目标结构确实非法。
    assert!(
        set_record_numeric_value_during_scrub(
            &mut record,
            &target,
            PlacementWaterNumericKind::FlowSpeed,
            4.5,
        )
        .expect("拖中快路径只应复核冻结目标字段")
    );
    assert_eq!(record.water_surfaces[0].flow_speed, 4.5); // 目标槽按O(1)路径实时改变。
    assert_eq!(record.water_surfaces[1].flow_speed, 2.0); // 非目标非法项也不得被拖中扫描或修改。
    assert!(
        set_record_numeric_value(
            &mut record,
            &target,
            PlacementWaterNumericKind::FlowSpeed,
            5.0,
        )
        .is_err()
    ); // 精确输入仍须执行完整记录校验并拒绝非法结构。
    assert_eq!(record.water_surfaces[0].flow_speed, 4.5); // 精确提交失败恢复刚写字段。
}

#[test]
fn scrub_fast_path_rejects_reordered_target_instead_of_scanning_for_it() {
    let (mut record, mut target) = record_and_target(); // 原目标稳定身份仍位于首项。
    let mut other = record.water_surfaces[0].clone(); // 构造第二个合法独立绑定。
    other.node_path[1] = "另一河流对象".to_owned(); // 路径不同形成独立稳定身份。
    other.mesh_name = "另一河流水面".to_owned(); // 网格名也保持明确可读。
    record.water_surfaces.push(other); // 当前记录包含两个合法绑定。
    target.surface_index = 1; // 模拟结构未推进却把旧身份移动到另一槽的损坏状态。
    assert!(
        set_record_numeric_value_during_scrub(
            &mut record,
            &target,
            PlacementWaterNumericKind::HeightOffset,
            9.0,
        )
        .is_err()
    ); // 即使匹配身份仍存在于首项，也不得退回线性搜索后串用旧下标。
    assert_eq!(record.water_surfaces[0].height_offset, 2.5); // 真实目标保持原值。
    assert_eq!(record.water_surfaces[1].height_offset, 2.5); // 错槽同样没有被写入。
}

#[test]
fn scrub_sensitivity_is_persisted_per_field() {
    let mut settings = EditorSettings::default(); // 两个新增设置槽默认都使用一倍标准手感。
    assert!(PlacementWaterNumericKind::FlowSpeed.set_sensitivity(&mut settings, 2.0));
    assert_eq!(
        settings
            .scene_editor_scrub_sensitivities
            .mesh_water_flow_speed,
        2.0,
    ); // 右拖流速只持久化流速倍率。
    assert_eq!(
        settings
            .scene_editor_scrub_sensitivities
            .mesh_water_height_offset,
        1.0,
    ); // 高度倍率保持默认值。

    assert!(PlacementWaterNumericKind::HeightOffset.set_sensitivity(&mut settings, 0.5));
    assert_eq!(
        settings
            .scene_editor_scrub_sensitivities
            .mesh_water_flow_speed,
        2.0,
    ); // 高度右拖不得回写流速倍率。
    assert_eq!(
        settings
            .scene_editor_scrub_sensitivities
            .mesh_water_height_offset,
        0.5,
    ); // 高度倍率保存到自己的独立槽。
}

#[test]
fn left_scrub_finishes_as_one_bounded_history_entry() {
    let (before, target) = record_and_target(); // 完整起点保存所有非当前字段属性。
    let mut after = before.clone(); // 多帧实时Changed最终只保留最后一个完整快照。
    after.water_surfaces[0].flow_speed = 6.75; // 模拟松开左键时的最终流速。
    let mut session = EditorAuthoringSession {
        revision: 11, // 必须与事务连续推进后的期望修订一致。
        ..Default::default()
    };
    let mut editor = SceneEditorState {
        dirty: true, // 实时拖动已经让当前草稿进入未保存状态。
        ..Default::default()
    };
    let transaction = PlacementWaterNumericScrub {
        entity: Entity::PLACEHOLDER, // 纯收口不再读取输入根，只保留事务完整形状。
        kind: PlacementWaterNumericKind::FlowSpeed, // 本次比较唯一流速字段。
        target,
        before,
        initial_value: 1.0,    // 拖动前流速。
        latest_value: 6.75,    // 松键时当前草稿中的最终流速。
        expected_revision: 11, // 没有外部作者动作介入。
        dirty_before: false,   // 真实变化后应继续保持dirty而不是恢复该值。
    };
    let revision = session.revision; // 拆借两侧历史前冻结当前连续作者修订。
    let changed = super::transaction::finish_numeric_scrub_record(
        &mut editor,
        &mut session.undo,
        &mut session.redo,
        revision,
        transaction,
        &after,
    )
    .expect("连续的有效左拖必须能收口");
    assert!(changed); // 起终点不同必须形成作者操作。
    assert_eq!(session.undo.len(), 1); // 任意多实时Changed在松开时只产生一条历史。
    assert!(session.redo.is_empty()); // 新作者分支通过统一有界入口清空redo。
    assert!(matches!(
        session.undo.first(),
        Some(EditorCommand::PlacementChanged { .. })
    )); // 历史使用完整placement前后快照，而不是私有增量命令。
}

#[test]
fn scrub_returned_to_origin_preserves_history_and_dirty_state() {
    let (record, target) = record_and_target(); // 拖动终点回到相同字段值。
    let mut session = EditorAuthoringSession {
        revision: 4, // 事务身份仍连续，只是没有净数值变化。
        undo: vec![EditorCommand::PlacementAdded(record.clone())], // 已有历史必须完整保留。
        redo: vec![EditorCommand::PlacementAdded(record.clone())], // 无净变化不得切断redo链。
        ..Default::default()
    };
    let mut editor = SceneEditorState {
        dirty: true, // 实时变化途中曾临时设为dirty。
        ..Default::default()
    };
    let transaction = PlacementWaterNumericScrub {
        entity: Entity::PLACEHOLDER, // 收口纯函数不依赖实际ECS实体存在。
        kind: PlacementWaterNumericKind::HeightOffset, // 比较世界Y偏移字段。
        target,
        before: record.clone(), // 起点完整记录。
        initial_value: 2.5,     // 起点高度值。
        latest_value: 2.5,      // 最终字段回到同一高度。
        expected_revision: 4,   // 没有外部作者修改。
        dirty_before: false,    // 进入拖动前场景原本已保存。
    };
    let revision = session.revision; // 当前连续修订在拆借历史前复制为标量。
    let changed = super::transaction::finish_numeric_scrub_record(
        &mut editor,
        &mut session.undo,
        &mut session.redo,
        revision,
        transaction,
        &record,
    )
    .expect("拖回起点也应正常结束事务");
    assert!(!changed); // 字段无净变化不得形成作者命令。
    assert_eq!(session.undo.len(), 1); // 既有undo保持原样。
    assert_eq!(session.redo.len(), 1); // 既有redo链同样保留。
    assert!(!editor.dirty); // 恢复进入拖动前的保存状态。
}

#[test]
fn scrub_returned_to_display_origin_restores_hidden_raw_precision() {
    let (mut before, target) = record_and_target(); // 作者原值故意比流速框两位显示精度更细。
    before.water_surfaces[0].flow_speed = 1.234_567; // 该raw尾数必须在拖离再回显示起点后原样保留。
    let mut current = before.clone(); // 当前记录模拟拖动已经实时写入显示吸附档。
    current.water_surfaces[0].flow_speed = 1.23; // 共享 UI 控件回到按下时显示的1.23，因此Committed.changed为false。
    current.water_surfaces[0].height_offset = 9.75; // 同记录的后来非目标字段不能被完整before覆盖。
    let transaction = PlacementWaterNumericScrub {
        entity: Entity::PLACEHOLDER, // 纯owned-field恢复无需真实ECS根。
        kind: PlacementWaterNumericKind::FlowSpeed, // 本事务只拥有流速字段。
        target,                      // 冻结同一surface稳定身份和槽位。
        before,                      // 完整起点保留精确raw值。
        initial_value: 1.234_567,    // 业务raw起点高于控件显示精度。
        latest_value: 1.23,          // 最终显示档回到按下时的1.23。
        expected_revision: 7,        // 当前测试只验证字段所有权恢复。
        dirty_before: false,         // no-op不应产生新的dirty。
    };
    assert!(
        super::transaction::restore_interrupted_record(&mut current, &transaction)
            .expect("显示起点仍由本事务拥有时必须恢复raw尾数")
    );
    assert_eq!(
        current.water_surfaces[0].flow_speed.to_bits(),
        1.234_567_f32.to_bits(),
    ); // 位级比较证明恢复的是作者raw而不是重新格式化后的近似值。
    assert_eq!(current.water_surfaces[0].height_offset, 9.75); // 非目标后来值保持不变。
}

#[test]
fn aborted_scrub_latch_rejects_late_changes_until_matching_release() {
    let root = Entity::from_bits(41); // 模拟首个Changed已经建立事务的公共根。
    let mut state = PlacementWaterNumericInputState::default(); // 外部修订冲突前没有拒绝门。
    state.abort_scrub_until_release(root, PlacementWaterNumericKind::FlowSpeed); // 第二个Changed发现外部revision后回滚并锁存。
    assert!(state.scrub_is_aborted()); // 第三个Changed必须被系统入口直接丢弃，不能重新begin。
    assert!(!state.release_aborted_scrub(root, PlacementWaterNumericKind::HeightOffset)); // 其他字段迟到提交不能误清流速拒绝门。
    assert!(state.scrub_is_aborted()); // 错字段release后仍继续拒绝后续Changed。
    assert!(state.release_aborted_scrub(root, PlacementWaterNumericKind::FlowSpeed)); // 对应Scrub Committed只清理拒绝门。
    assert!(!state.scrub_is_aborted()); // 松键后下一次独立按下才允许重新建立事务。
}

#[test]
fn continuous_interruption_endpoint_still_finishes_as_one_history_entry() {
    let (before, target) = record_and_target(); // 模拟已发生至少一帧实时变化的活动事务。
    let mut after = before.clone(); // 最终快照继续包含完整placement状态。
    after.water_surfaces[0].height_offset = 18.25; // 异常发生前最后成功应用的世界Y偏移。
    let transaction = PlacementWaterNumericScrub {
        entity: Entity::PLACEHOLDER, // 异常恢复按冻结事务处理，不依赖根仍存活。
        kind: PlacementWaterNumericKind::HeightOffset, // 当前事务只拥有高度字段。
        target,
        before,
        initial_value: 2.5,   // 拖动起点世界Y偏移。
        latest_value: 18.25,  // 异常前最后成功应用的世界Y偏移。
        expected_revision: 9, // 当前作者修订仍由这段拖动连续推进。
        dirty_before: false,  // 真实变化结束后仍应保持dirty。
    };
    let mut session = EditorAuthoringSession {
        revision: 9, // context或选择失败没有夹入外部作者动作。
        ..Default::default()
    };
    let mut editor = SceneEditorState {
        dirty: true, // 先前实时Changed已经标记未保存。
        ..Default::default()
    };
    let revision = session.revision; // context失败边沿冻结当前连续修订。
    let changed = super::transaction::finish_numeric_scrub_record(
        &mut editor,
        &mut session.undo,
        &mut session.redo,
        revision,
        transaction,
        &after,
    )
    .expect("连续事务即使异常停止也必须安全收口");
    assert!(changed); // context异常只停止后续输入，既有连续终点仍应形成历史。
    assert_eq!(session.undo.len(), 1); // context、选择或写入错误不能丢掉已有撤销起点。
    assert!(matches!(
        session.undo.first(),
        Some(EditorCommand::PlacementChanged { .. })
    )); // 异常收口仍使用统一完整快照命令。
}

#[test]
fn release_validation_failure_can_roll_back_only_the_scrub_field() {
    let (before, target) = record_and_target(); // 完整起点只有一项合法水绑定。
    let mut invalid_after = before.clone(); // 当前草稿先模拟已经实时应用最后流速档。
    invalid_after.water_surfaces[0].flow_speed = 7.5; // 本事务最后拥有的临时字段值。
    invalid_after
        .water_surfaces
        .push(invalid_after.water_surfaces[0].clone()); // 非目标外部结构故意制造重复稳定身份。
    let transaction = PlacementWaterNumericScrub {
        entity: Entity::PLACEHOLDER, // 纯事务回归不依赖实际输入实体。
        kind: PlacementWaterNumericKind::FlowSpeed, // 本事务只能回滚流速。
        target,                      // 冻结首个水绑定的稳定身份与数组槽。
        before,                      // 唯一合法完整起点供撤销边界和标量证据使用。
        initial_value: 1.0,          // 拖动开始前流速。
        latest_value: 7.5,           // 松键时草稿中的最后实时流速。
        expected_revision: 3,        // 没有外部作者修订，失败来自最终完整校验。
        dirty_before: false,         // 回滚后仍由推进的新修订保持明确dirty。
    };
    let mut session = EditorAuthoringSession {
        revision: 3, // 没有外部作者修订，失败只来自完整记录格式。
        ..Default::default()
    };
    let mut editor = SceneEditorState {
        dirty: true, // 实时拖动已经把草稿标为未保存。
        ..Default::default()
    };
    let revision = session.revision; // 松键边沿冻结连续作者修订。
    let error = super::transaction::finish_numeric_scrub_record(
        &mut editor,
        &mut session.undo,
        &mut session.redo,
        revision,
        transaction,
        &invalid_after,
    )
    .expect_err("最终完整校验必须拒绝重复稳定身份");
    assert!(error.reason.contains("重复")); // 失败详情来自公共完整记录校验。
    assert!(session.undo.is_empty()); // 非法终点绝不能进入统一撤销栈。
    let mut current = invalid_after; // 异常恢复继续以当前外部权威完整记录为基础。
    assert!(
        super::transaction::restore_interrupted_record(&mut current, &error.transaction)
            .expect("完整校验失败后必须能定向恢复事务字段")
    );
    assert_eq!(current.water_surfaces[0].flow_speed, 1.0); // 未入栈流速已经恢复到起点。
    assert_eq!(current.water_surfaces.len(), 2); // 与本拖动无关的外部结构没有被完整before覆盖。
}

#[test]
fn revision_conflict_rolls_back_only_the_scrub_owned_field() {
    let (before, target) = record_and_target(); // 流速是本事务唯一拥有的字段。
    let mut after = before.clone(); // 保存最后一次成功实时快照。
    after.water_surfaces[0].flow_speed = 7.5; // 本事务尚未进入历史的临时流速。
    let transaction = PlacementWaterNumericScrub {
        entity: Entity::PLACEHOLDER,                // 纯记录恢复不读取ECS根。
        kind: PlacementWaterNumericKind::FlowSpeed, // 只允许恢复流速。
        target,
        before,
        initial_value: 1.0,   // 起点流速来自完整before。
        latest_value: 7.5,    // 本事务最后实时应用的临时流速。
        expected_revision: 3, // 当前真实修订已在外部继续推进。
        dirty_before: false,  // 修订冲突时不得用旧值覆盖外部dirty。
    };
    let mut current = after; // 当前记录继承了尚未入栈的实时流速。
    current.translation = [8.0, 9.0, 10.0]; // 外部作者合法移动同一摆放实例。
    current.water_surfaces[0].height_offset = 42.0; // 外部作者还修改了另一个水字段。
    assert!(
        super::transaction::restore_interrupted_record(&mut current, &transaction)
            .expect("有效外部记录应能选择性回滚")
    );
    assert_eq!(current.water_surfaces[0].flow_speed, 1.0); // 未入栈流速恢复到事务完整起点。
    assert_eq!(current.water_surfaces[0].height_offset, 42.0); // 外部高度修改必须保留。
    assert_eq!(current.translation, [8.0, 9.0, 10.0]); // 外部摆放移动同样不得被完整旧快照覆盖。
}

#[test]
fn revision_conflict_restores_complete_before_when_record_is_still_exact_after() {
    let (before, target) = record_and_target(); // 完整起点包含全部摆放与水体属性。
    let mut after = before.clone(); // 当前记录没有夹带任何外部字段变化。
    after.water_surfaces[0].height_offset = -85.5; // 模拟尚未入栈的最后实时高度。
    let transaction = PlacementWaterNumericScrub {
        entity: Entity::PLACEHOLDER, // 纯恢复不访问已经结束的输入根。
        kind: PlacementWaterNumericKind::HeightOffset, // 本事务拥有世界Y偏移。
        target,
        before: before.clone(), // 完整快照是安全回滚权威值。
        initial_value: 2.5,     // 完整起点中的世界Y偏移。
        latest_value: -85.5,    // 当前记录中的最后实时高度。
        expected_revision: 6,   // 外部修订变化将由调用方决定进入恢复路径。
        dirty_before: false,    // 冲突恢复不直接使用旧dirty状态。
    };
    let mut current = after; // 当前草稿仍完全停留在旧拖动终点。
    assert!(
        super::transaction::restore_interrupted_record(&mut current, &transaction)
            .expect("未夹带外部变化时必须能完整回滚")
    );
    assert_eq!(current.id, before.id); // 摆放稳定身份恢复到完整起点。
    assert_eq!(current.translation, before.translation); // 完整摆放变换保持起点值。
    assert_eq!(current.water_surfaces, before.water_surfaces); // 全部水绑定字段精确恢复。
}

#[test]
fn revision_conflict_never_overwrites_external_same_field_value() {
    let (before, target) = record_and_target(); // 构造本事务起点。
    let mut after = before.clone(); // 模拟最后一次实时流速。
    after.water_surfaces[0].flow_speed = 7.5; // 本事务最后拥有的临时值。
    let transaction = PlacementWaterNumericScrub {
        entity: Entity::PLACEHOLDER,                // 纯恢复不依赖输入实体。
        kind: PlacementWaterNumericKind::FlowSpeed, // 冲突发生在同一字段。
        target,
        before,
        initial_value: 1.0,   // 拖动起点流速。
        latest_value: 7.5,    // 本事务最后拥有的临时流速。
        expected_revision: 3, // 外部动作已让当前修订不再连续。
        dirty_before: false,  // 外部dirty必须保持原样。
    };
    let mut current = after; // 从拖动最后快照模拟外部继续编辑。
    current.water_surfaces[0].flow_speed = 12.0; // 外部作者明确接管并改写同一流速字段。
    assert!(
        !super::transaction::restore_interrupted_record(&mut current, &transaction)
            .expect("外部同字段值合法时只需保持")
    );
    assert_eq!(current.water_surfaces[0].flow_speed, 12.0); // 旧拖动恢复绝不能覆盖外部权威值。
}
