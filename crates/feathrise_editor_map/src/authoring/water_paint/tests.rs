//! 河水复合笔刷入口层回归测试；不启动地图加载或渲染世界。

use super::*; // 测试子模块可直接验证父模块私有的笔划收口与冻结段判断。
use crate::authoring::resources::EditorCommand;

fn deferred_click_fixture() -> (
    EditorAuthoringSession,
    MapWaterLayer,
    WaterBrushStroke,
    PendingWaterBrushSample,
) {
    let heights = Arc::new(vec![32_768_u16; 11 * 11]); // base0/scale20下约为10米平面。
    let mut draft = crate::authoring::terrain::tests::test_heightfield_draft(heights);
    draft.manifest.terrain.base_height = 0.0;
    draft.manifest.terrain.height_scale = 20.0;
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&draft.map_id, &draft.folder);
    water.layers[0].resolution = [11, 11];
    water.layers[0].water_level = 10.0;
    let layer = water.layers[0].clone();
    draft
        .water_masks
        .insert(layer.id.clone(), Arc::new(vec![0; 11 * 11]));
    draft.water = Some(water);
    let map_id = draft.map_id.clone();
    let point = Vec3::new(5.0, 10.0, 5.0);
    let sample = PendingWaterBrushSample {
        from: point,
        to: point,
        radius: 2.0,
        erase: false,
        amount: 0.35, // 按下时完整单击强度，等待后不得退化成delta积分。
        cursor: 0,
        mask_changed: false,
        height_changed: false,
    };
    let stroke = WaterBrushStroke {
        active: true,
        map_id: Some(map_id),
        layer_id: Some(layer.id.clone()),
        water_level: Some(layer.water_level),
        last_world: Some(point),
        pending_sample: Some(sample),
        release_requested: true,
        snapshot_mask: draft.water_masks.get(&layer.id).cloned(), // 测试同样冻结落笔前完整版本。
        snapshot_terrain: draft.terrain_mesh.clone(), // 模拟生产后台写侧物化前的河床快照。
        ..Default::default()
    };
    let mask = draft.water_masks.get_mut(&layer.id).expect("测试R8存在"); // 纯内存夹具刻意同步模拟后台完成。
    *mask = Arc::new(mask.as_ref().clone()); // 小测试画布不使用异步调度。
    let mesh = draft.terrain_mesh.as_mut().expect("测试GLB存在"); // 正式入口使用single-flight复制。
    *mesh = Arc::new(mesh.as_ref().clone()); // 保持原来共享读者测试的唯一写侧前提。
    (
        EditorAuthoringSession {
            draft: Some(draft),
            ..Default::default()
        },
        layer,
        stroke,
        sample,
    )
}

fn apply_fixture_sample(
    session: &mut EditorAuthoringSession,
    layer: &MapWaterLayer,
    stroke: &mut WaterBrushStroke,
    sample: PendingWaterBrushSample,
) -> WaterSegmentOutcome {
    paint_water_segment(
        session.draft.as_mut().expect("测试草稿存在"),
        layer,
        sample.from,
        sample.to,
        sample.radius,
        stroke.water_level.expect("测试水位已冻结"),
        sample.erase,
        sample.amount,
        sample.cursor,
        stroke,
    )
    .expect("测试刷段合法")
}

#[test]
fn shared_r8_quick_click_retries_the_frozen_full_strength_sample_after_release() {
    let (mut session, layer, mut stroke, sample) = deferred_click_fixture();
    let reader = session
        .draft
        .as_ref()
        .and_then(|draft| draft.water_masks.get(&layer.id))
        .cloned()
        .expect("模拟后台R8读者");

    assert!(matches!(
        apply_fixture_sample(&mut session, &layer, &mut stroke, sample),
        WaterSegmentOutcome::Deferred
    ));
    assert!(stroke.pending_sample.is_some() && stroke.release_requested);
    drop(reader);

    assert!(matches!(
        apply_fixture_sample(&mut session, &layer, &mut stroke, sample),
        WaterSegmentOutcome::Progressed { changed, complete: true, .. } if changed.any()
    ));
    stroke.pending_sample = None;
    finish_water_stroke(&mut MapEditorState::default(), &mut session, &mut stroke);
    let max_mask = session
        .draft
        .as_ref()
        .and_then(|draft| draft.water_masks.get(&layer.id))
        .and_then(|mask| mask.iter().copied().max())
        .expect("测试R8存在");
    assert!(max_mask >= 80); // 35%按下强度约为89，不能退化为等待后的一帧delta小值。
    assert!(matches!(
        session.undo.last(),
        Some(EditorCommand::WaterStrokeSnapshot(_))
    ));
}

#[test]
fn shared_glb_quick_click_paints_without_waiting_or_changing_terrain() {
    let (mut session, layer, mut stroke, sample) = deferred_click_fixture();
    let reader = session
        .draft
        .as_ref()
        .and_then(|draft| draft.terrain_mesh.as_ref())
        .cloned()
        .expect("模拟后台GLB派生读者");

    assert!(matches!(
        apply_fixture_sample(&mut session, &layer, &mut stroke, sample),
        WaterSegmentOutcome::Progressed { changed, complete: true, .. }
            if changed.mask_changed && !changed.height_changed
    )); // 共享地形不再阻塞纯水域覆盖。
    stroke.pending_sample = None;
    finish_water_stroke(&mut MapEditorState::default(), &mut session, &mut stroke);
    assert!(
        session
            .draft
            .as_ref()
            .and_then(|draft| draft.terrain_mesh.as_ref())
            .is_some_and(|mesh| Arc::ptr_eq(mesh, &reader) && mesh.positions == reader.positions)
    ); // 不复制或修改任何地形顶点。
    assert!(!stroke.active);
}

#[test]
fn pending_sample_rejects_same_layer_id_from_another_map_or_closed_lifecycle() {
    let (mut session, _, stroke, _) = deferred_click_fixture();
    let origin = stroke.map_id.clone().expect("测试原图身份");
    let mut editor = MapEditorState {
        open: true,
        selected_map_id: Some(origin),
        ..Default::default()
    };
    assert!(pending_origin_is_current(&editor, &session, &stroke));

    editor.selected_map_id =
        Some(feathrise_map_content::MapId::parse("MAP-another").expect("另一地图ID合法"));
    assert!(!pending_origin_is_current(&editor, &session, &stroke));
    editor.selected_map_id = stroke.map_id.clone();
    session.saving = true;
    assert!(!pending_origin_is_current(&editor, &session, &stroke));
    session.saving = false;
    editor.open = false;
    assert!(!pending_origin_is_current(&editor, &session, &stroke));
}

#[test]
fn same_frame_press_and_release_still_requests_one_water_sample() {
    assert!(should_sample_water_button(false, true)); // pressed已释放，但just_pressed仍保留本帧单击。
    assert!(should_sample_water_button(true, false)); // 连续拖动继续采样。
    assert!(!should_sample_water_button(false, false)); // 稳定松开帧不产生额外dab。
}

#[test]
fn pending_drag_queues_and_applies_the_release_tail_before_finish() {
    let (mut session, layer, mut stroke, mut first) = deferred_click_fixture();
    first.from = Vec3::new(2.0, 10.0, 5.0);
    first.to = Vec3::new(5.0, 10.0, 5.0);
    first.radius = 1.25;
    first.amount = 1.0;
    first.cursor = 0;
    stroke.pending_sample = Some(first);
    stroke.last_world = Some(first.from);

    let release_world = Vec3::new(8.0, 10.0, 5.0);
    queue_water_tail(&mut stroke, first, first.to, release_world, 1.0);
    let tail = stroke.queued_sample.take().expect("等待期间应保留B到C尾段");
    assert_eq!(tail.from, first.to);
    assert_eq!(tail.to, release_world);

    assert!(matches!(
        apply_fixture_sample(&mut session, &layer, &mut stroke, first),
        WaterSegmentOutcome::Progressed { complete: true, .. }
    ));
    assert!(matches!(
        apply_fixture_sample(&mut session, &layer, &mut stroke, tail),
        WaterSegmentOutcome::Progressed { complete: true, .. }
    ));
    let mask = session
        .draft
        .as_ref()
        .and_then(|draft| draft.water_masks.get(&layer.id))
        .expect("测试R8存在");
    for x in 2..=8 {
        assert!(mask[5 * 11 + x] > 0, "A到释放点C之间不能丢尾段：x={x}");
    }
}

#[test]
fn history_checkpoint_keeps_mask_and_bed_in_one_command_without_ending_stroke() {
    let (mut session, layer, mut stroke, _) = deferred_click_fixture();
    let draft = session.draft.as_mut().expect("测试草稿存在");
    let mask = Arc::make_mut(draft.water_masks.get_mut(&layer.id).expect("测试R8存在"));
    stroke.mask_before.insert(0, mask[0]);
    mask[0] = 123;
    let mesh = Arc::make_mut(draft.terrain_mesh.as_mut().expect("测试GLB存在"));
    stroke.height_before.insert(0, mesh.positions[0][1]);
    mesh.positions[0][1] -= 7.0;

    checkpoint_water_stroke(&mut session, &mut stroke);

    assert!(stroke.active && stroke.pending_sample.is_some()); // 检查点不结束按压或丢冻结段。
    assert!(stroke.mask_before.is_empty() && stroke.height_before.is_empty());
    assert!(session.undo.is_empty()); // 内部检查点不是用户可见撤销步骤。
    assert!(stroke.snapshot_changed && stroke.snapshot_height_changed); // 两侧变化一直累计到最终松开。
    finish_water_stroke(&mut MapEditorState::default(), &mut session, &mut stroke); // 全笔仅此处压栈。
    assert!(
        matches!(session.undo.last(), Some(EditorCommand::WaterStrokeSnapshot(change)) if change.terrain.is_some())
    ); // 覆盖和河床共享唯一历史点。
}

#[test]
fn two_checkpoints_and_revisited_samples_undo_and_redo_as_one_stroke() {
    let (mut session, layer, mut stroke, _) = deferred_click_fixture(); // 使用真实笔划收口与正式历史应用函数。
    let original = Arc::new(vec![0; 70_000]); // 大于两个32768块，确保重现原拆笔条件。
    stroke.snapshot_mask = Some(original.clone()); // 按下时固定最初版本。
    session
        .draft
        .as_mut()
        .unwrap()
        .water_masks
        .insert(layer.id.clone(), Arc::new(original.as_ref().clone())); // 小型无窗口夹具模拟后台物化完成。
    let before_height = stroke.snapshot_terrain.as_ref().unwrap().positions[0][1]; // 保存首次河床值。
    for (start, end, value) in [(0, 32_768, 50), (32_768, 65_536, 100), (0, 30, 200)] {
        // 第三块重访第一块样本。
        let draft = session.draft.as_mut().unwrap(); // 每次内部块直接模拟实际局部写入。
        let mask = Arc::get_mut(draft.water_masks.get_mut(&layer.id).unwrap()).unwrap(); // 生产写侧同样保持唯一。
        for index in start..end {
            stroke.mask_before.entry(index).or_insert(mask[index]);
            mask[index] = value;
        } // 后续检查点不得覆盖整笔最初值。
        let mesh = Arc::get_mut(draft.terrain_mesh.as_mut().unwrap()).unwrap(); // 同笔河床反复触及。
        stroke.height_before.insert(0, mesh.positions[0][1]); // 当前块局部旧值。
        mesh.positions[0][1] -= 0.1; // 各块下挖累加。
        checkpoint_water_stroke(&mut session, &mut stroke); // 内部块不生成独立历史。
        assert!(session.undo.is_empty()); // 松开前不出现多个撤销点。
    }
    finish_water_stroke(&mut MapEditorState::default(), &mut session, &mut stroke); // 一次松开只压栈一次。
    assert_eq!(session.undo.len(), 1); // 原故障会出现三条历史。
    let command = session.undo.pop().unwrap(); // 模拟一次撤销按钮只取一个栈顶。
    let EditorCommand::WaterStrokeSnapshot(change) = &command else {
        panic!("应当生成整笔快照")
    }; // 测试正式命令类型。
    let final_height = change.terrain.as_ref().unwrap().1.positions[0][1]; // 全笔最终河床值。
    crate::authoring::edit::apply_water_snapshot_for_tests(
        session.draft.as_mut().unwrap(),
        change,
        true,
    )
    .unwrap(); // 使用正式原子撤销路径。
    assert_eq!(
        session.draft.as_ref().unwrap().water_masks[&layer.id].as_ref(),
        original.as_ref()
    ); // 所有块及重复触及像素一起恢复。
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .terrain_mesh
            .as_ref()
            .unwrap()
            .positions[0][1],
        before_height
    ); // 重复河床样本恢复第一次值。
    session.redo.push(command); // 整笔转入重做栈。
    let command = session.redo.pop().unwrap(); // 一次重做同样只取一条。
    let EditorCommand::WaterStrokeSnapshot(change) = &command else {
        unreachable!()
    }; // 同一完整事务。
    crate::authoring::edit::apply_water_snapshot_for_tests(
        session.draft.as_mut().unwrap(),
        change,
        false,
    )
    .unwrap(); // 正式完整重做。
    let draft = session.draft.as_ref().unwrap(); // 验证最终而非中间块版本。
    assert_eq!(draft.water_masks[&layer.id][0], 200); // 重访像素恢复最后一次值。
    assert_eq!(draft.water_masks[&layer.id][32_768], 100); // 第二块也恢复。
    assert_eq!(
        draft.terrain_mesh.as_ref().unwrap().positions[0][1],
        final_height
    ); // 河床与覆盖同次重做。
    session.undo.push(command); // 第一笔完整保留。
    stroke.active = true; // 开始独立第二笔。
    stroke.layer_id = Some(layer.id.clone()); // 同一层也不能合并两次按压。
    snapshot::capture_stroke_snapshot(&session, &mut stroke, true); // 第二笔纯擦除不持有地形。
    let mask = Arc::make_mut(
        session
            .draft
            .as_mut()
            .unwrap()
            .water_masks
            .get_mut(&layer.id)
            .unwrap(),
    ); // 测试夹具同步模拟第二次后台写侧。
    stroke.mask_before.insert(0, mask[0]); // 第二笔最初值是第一笔最终值。
    mask[0] = 0; // 单独擦除一个像素。
    finish_water_stroke(&mut MapEditorState::default(), &mut session, &mut stroke); // 第二次松手生成第二条。
    assert_eq!(session.undo.len(), 2); // 独立两笔不能合并。
    let EditorCommand::WaterStrokeSnapshot(second) = session.undo.pop().unwrap() else {
        unreachable!()
    }; // 单独撤销第二笔。
    assert!(second.terrain.is_none()); // Shift不复制或恢复地形。
    crate::authoring::edit::apply_water_snapshot_for_tests(
        session.draft.as_mut().unwrap(),
        &second,
        true,
    )
    .unwrap(); // 第二笔撤销只回到第一笔末尾。
    assert_eq!(
        session.draft.as_ref().unwrap().water_masks[&layer.id][0],
        200
    ); // 不误回到第一笔之前。
}

#[test]
fn water_stroke_advances_the_global_revision_once_for_mask_and_height() {
    let mut session = EditorAuthoringSession::default(); // 空草稿仍足够验证修订身份。
    let mut editor = MapEditorState::default(); // dirty由统一作者门负责设置。

    session.mark_water_stroke_changed(&mut editor, "river", true); // 同一dab同时改变R8与GLB顶点。

    assert_eq!(session.revision, 1); // 联合修改不能分别调用两次全局mark。
    assert_eq!(session.water_revision, 1); // R8水面候选换代一次。
    assert_eq!(session.terrain_revision, 1); // GLB地形、碰撞候选换代一次。
    assert!(editor.dirty); // 联合作者修改进入统一保存门。
}

#[test]
fn shift_only_water_stroke_does_not_advance_terrain_revision() {
    let mut session = EditorAuthoringSession::default(); // 模拟纯R8擦除笔划。
    let mut editor = MapEditorState::default(); // 记录全局dirty。

    session.mark_water_stroke_changed(&mut editor, "river", false); // 没有任何GLB顶点高度差分。

    assert_eq!(session.revision, 1); // 纯擦除仍是一次作者操作。
    assert_eq!(session.water_revision, 1); // 水域覆盖必须换代。
    assert_eq!(session.terrain_revision, 0); // Shift绝不触发地形重建或回填。
}
