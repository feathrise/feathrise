use super::*; // 纯门测试不启动AssetServer、渲染或动态场景。
use bevy::asset::AssetPlugin; // 连续begin回归使用真实AssetServer但不启动渲染。
use feathrise_scene::{DynamicWaterPhase, ScenePlacementRuntimePhase}; // 构造水域与摆放公开运行时完成证据。

fn pending_asset<A: Asset>() -> PendingWaterAsset<A> {
    let handle = Handle::<A>::default();
    PendingWaterAsset {
        id: handle.id(),
        _handle: handle,
        succeeded: false,
    }
}

fn pending_handoff(scene_id: &SceneId) -> PendingWaterFileHandoff {
    PendingWaterFileHandoff {
        gate: gate(scene_id, None),
        manifest: pending_asset(),
        water: Some(pending_asset()),
        masks: vec![pending_asset()],
        textures: vec![pending_asset()],
        dirty_layer_ids: vec!["lake".to_owned()],
        catalog_registration_required: true,
        manifest_asset_path: "scenes/MAP-water-handoff/scene.scene_manifest.json".into(),
        root_pending: true,
        terrains: None,
    }
}

fn gate(scene_id: &SceneId, success_frame: Option<u32>) -> WaterFileHandoffGate {
    WaterFileHandoffGate {
        purpose: WaterFileHandoffPurpose::SavePublish,
        scene_id: scene_id.clone(),
        water_revision: 7,
        terrain_revision: 5,
        baseline_generation: 11,
        started_at: Duration::from_secs(20),
        all_inputs_succeeded_frame: success_frame,
    }
}

fn evidence<'a>(scene_id: &'a SceneId, frame: u32) -> WaterFileHandoffEvidence<'a> {
    WaterFileHandoffEvidence {
        active_scene_id: Some(scene_id),
        status_scene_id: Some(scene_id),
        phase: DynamicWaterPhase::Ready,
        input_draft_revision: None,
        desired_generation: 12,
        applied_generation: 12,
        now: Duration::from_secs(21),
        frame,
    }
}

#[test]
fn formal_water_revision_is_visible_only_for_the_matching_inflight_scene() {
    let scene_id = SceneId::parse("MAP-water-handoff").expect("测试场景ID合法");
    let other = SceneId::parse("MAP-water-other").expect("测试场景ID合法");
    let mut handoff = WaterFileHandoff {
        pending: Some(pending_handoff(&scene_id)),
        ..default()
    };
    assert_eq!(
        handoff.formal_water_revision_in_flight_for(&scene_id),
        Some(7)
    );
    assert_eq!(handoff.formal_water_revision_in_flight_for(&other), None);
    handoff.pending = None;
    assert_eq!(handoff.formal_water_revision_in_flight_for(&scene_id), None);
} // 实时Clipmap草稿只在正式文件交接期间让出优先级，其他场景和完成后不受影响。

#[test]
fn same_frame_last_input_success_cannot_complete() {
    let scene_id = SceneId::parse("MAP-water-handoff").expect("测试场景ID合法");

    assert_eq!(
        water_file_handoff_decision(&gate(&scene_id, Some(30)), evidence(&scene_id, 30)),
        WaterFileHandoffDecision::Waiting
    ); // 即使运行时已Ready，最终JSON/R8事件所在帧仍不能完成。
}

#[test]
fn incomplete_inputs_cannot_complete() {
    let scene_id = SceneId::parse("MAP-water-handoff").expect("测试场景ID合法");

    assert_eq!(
        water_file_handoff_decision(&gate(&scene_id, None), evidence(&scene_id, 31)),
        WaterFileHandoffDecision::Waiting
    ); // 缺最终JSON或任一脏R8时不得只凭运行时代次放行。
}

#[test]
fn author_ready_and_failed_cannot_complete_a_formal_root_handoff() {
    let map = SceneId::parse("MAP-water-handoff").unwrap();
    for phase in [DynamicWaterPhase::Ready, DynamicWaterPhase::Failed] {
        let mut author = evidence(&map, 31);
        author.phase = phase;
        author.input_draft_revision = Some(27);
        assert_eq!(water_file_handoff_decision(&gate(&map, Some(30)), author), WaterFileHandoffDecision::Waiting);
    }
}

#[test]
fn unequal_desired_and_applied_generations_cannot_complete() {
    let scene_id = SceneId::parse("MAP-water-handoff").expect("测试场景ID合法");
    let mut evidence = evidence(&scene_id, 31);
    evidence.applied_generation = 11; // 新期望已经发布但仍显示上一完整批次。

    assert_eq!(
        water_file_handoff_decision(&gate(&scene_id, Some(30)), evidence),
        WaterFileHandoffDecision::Waiting
    );
}

#[test]
fn next_frame_complete_generation_is_accepted() {
    let scene_id = SceneId::parse("MAP-water-handoff").expect("测试场景ID合法");

    assert_eq!(
        water_file_handoff_decision(&gate(&scene_id, Some(30)), evidence(&scene_id, 31)),
        WaterFileHandoffDecision::Complete
    ); // 已跨帧且新desired/applied相等时才形成正式交接证据。
}

#[test]
fn long_running_or_offscreen_completed_inputs_release_the_handoff() {
    let scene_id = SceneId::parse("MAP-water-handoff").expect("测试场景ID合法");
    let mut long_wait = evidence(&scene_id, 31);
    long_wait.now = Duration::from_secs(3600); // 长耗时不终止完整正式结果交接。
    assert_eq!(
        water_file_handoff_decision(&gate(&scene_id, Some(30)), long_wait),
        WaterFileHandoffDecision::Complete
    );

    let other_scene = SceneId::parse("MAP-water-handoff-other").expect("另一测试场景ID合法");
    let mut changed_active_scene = evidence(&scene_id, 31);
    changed_active_scene.active_scene_id = Some(&other_scene);
    assert_eq!(
        water_file_handoff_decision(&gate(&scene_id, Some(30)), changed_active_scene),
        WaterFileHandoffDecision::Complete
    ); // 已离开场景时仍先等全部输入跨帧成功，再释放正式缓存强句柄。
}

#[test]
fn consecutive_begin_cannot_replace_an_inflight_recovery_transaction() {
    let original_scene = SceneId::parse("MAP-water-handoff").expect("测试场景ID合法");
    let next_scene = SceneId::parse("MAP-water-handoff-next").expect("测试场景ID合法");
    let mut handoff = WaterFileHandoff {
        pending: Some(pending_handoff(&original_scene)),
        placement: None,
        unresolved: None,
        preview_cleanup: None,
    };
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    let asset_server = app.world().resource::<AssetServer>();

    assert!(
        handoff
            .begin(
                asset_server,
                next_scene,
                8,
                6,
                12,
                "scenes/MAP-water-handoff-next/scene.scene_manifest.json",
                "scenes/MAP-water-handoff-next/water.scene_water.json",
                &[],
                &[],
                &[],
                None,
                false,
                Duration::from_secs(22),
            )
            .is_err()
    );
    assert_eq!(
        handoff.pending.as_ref().map(|pending| &pending.gate.scene_id),
        Some(&original_scene)
    ); // 新保存不能丢掉旧R8/目录失败恢复责任。
}

#[test]
fn failed_handoff_restores_masks_and_catalog_registration_together() {
    let mut draft =
        crate::authoring::terrain::tests::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    let scene_id = draft.scene_id.clone();
    draft.water_catalog_registration_required = false;
    let mut session = EditorAuthoringSession {
        draft: Some(draft),
        ..Default::default()
    };
    let pending = pending_handoff(&scene_id);

    restore_pending_authoring_inputs(&pending, &mut session);

    let restored = session.draft.as_ref().expect("测试草稿仍存在");
    assert!(restored.water_dirty_layers.contains("lake"));
    assert!(!restored.terrain_dirty); // GLB交接由独立地形代次门负责，不再混入水域R16恢复。
    assert!(restored.water_catalog_registration_required);
}

#[test]
fn json_only_root_failure_restores_structural_save_responsibility() {
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(std::sync::Arc::from([]));
    let map = draft.scene_id.clone();
    let mut session = EditorAuthoringSession { draft: Some(draft), ..default() };
    let mut pending = pending_handoff(&map);
    pending.dirty_layer_ids.clear();
    pending.catalog_registration_required = false;
    let mut handoff = WaterFileHandoff { pending: Some(pending), ..default() };
    let mut editor = SceneEditorState::default();
    fail_scene_file_handoff(&mut handoff, &mut session, &mut editor, "模拟水位JSON正式构建失败");
    assert!(editor.dirty);
    assert!(session.draft.as_ref().unwrap().terrain_documents.active_dirty,
        "没有R8修改的根水位/删除末层也必须保留可重试写入责任");
}

#[test]
fn rollback_gate_rejects_old_runtime_generation() {
    let scene_id = SceneId::parse("MAP-water-rollback").expect("测试场景ID合法");
    let mut rollback = gate(&scene_id, Some(30));
    rollback.purpose = WaterFileHandoffPurpose::RollbackRestore;
    let mut runtime = evidence(&scene_id, 31);
    runtime.desired_generation = rollback.baseline_generation;
    runtime.applied_generation = rollback.baseline_generation;

    assert_eq!(
        water_file_handoff_decision(&rollback, runtime),
        WaterFileHandoffDecision::Waiting
    ); // 旧Ready不能冒充回滚完成。

    runtime.desired_generation += 1;
    runtime.applied_generation += 1;
    assert_eq!(
        water_file_handoff_decision(&rollback, runtime),
        WaterFileHandoffDecision::Complete
    );
}

#[test]
fn rollback_gate_can_finish_offscreen_after_inputs_cross_a_frame() {
    let scene_id = SceneId::parse("MAP-water-rollback").expect("测试场景ID合法");
    let other_scene = SceneId::parse("MAP-water-rollback-other").expect("另一测试场景ID合法");
    let mut rollback = gate(&scene_id, Some(30));
    rollback.purpose = WaterFileHandoffPurpose::RollbackRestore;
    let mut offscreen = evidence(&scene_id, 30);
    offscreen.active_scene_id = Some(&other_scene);
    offscreen.status_scene_id = Some(&other_scene);

    assert_eq!(
        water_file_handoff_decision(&rollback, offscreen),
        WaterFileHandoffDecision::Waiting
    );
    offscreen.frame = 31;
    assert_eq!(
        water_file_handoff_decision(&rollback, offscreen),
        WaterFileHandoffDecision::Complete
    ); // 不再可见的场景只等待强句柄完成，下一次进入会从已恢复缓存重建。
}

#[test]
fn unresolved_state_blocks_close_but_allows_same_scene_retry_save() {
    let scene_id = SceneId::parse("MAP-water-unresolved").expect("测试场景ID合法");
    let other_scene = SceneId::parse("MAP-water-unresolved-other").expect("另一测试场景ID合法");
    let mut handoff = WaterFileHandoff::default();
    handoff.mark_unresolved(scene_id.clone(), "测试恢复失败".to_owned());

    assert!(handoff.pending_for(&scene_id, 999, 888));
    assert!(!handoff.blocks_new_save_for(Some(&scene_id)));
    assert!(handoff.blocks_new_save_for(Some(&other_scene)));
    assert_eq!(handoff.unresolved_reason_for(&scene_id), Some("测试恢复失败"));
}

#[test]
fn save_publish_pending_blocks_close_after_later_author_revisions() {
    let scene_id = SceneId::parse("MAP-water-save-pending").expect("测试场景ID合法");
    let handoff = WaterFileHandoff {
        pending: Some(pending_handoff(&scene_id)),
        placement: None,
        unresolved: None,
        preview_cleanup: None,
    };

    assert!(handoff.pending_for(&scene_id, 700, 500)); // 正式文件责任不再被后续作者修订绕过。
    assert!(handoff.blocks_new_save_for(Some(&scene_id))); // 下一保存排队，不能覆盖仍在途的强句柄门。
}

fn placement_gate(scene_id: &SceneId) -> PlacementFileHandoffGate {
    PlacementFileHandoffGate {
        scene_id: scene_id.clone(),
        author_revision: 19,
        baseline_revision: 7,
        baseline_attempt_revision: 11,
        last_progress_revision: 13,
        last_progress_at: Duration::from_secs(20),
    }
}

#[test]
fn placement_edit_freeze_only_tracks_the_matching_inflight_candidate() {
    let scene_id = SceneId::parse("MAP-placement-freeze").expect("测试场景ID合法");
    let other = SceneId::parse("MAP-placement-freeze-other").expect("另一场景ID合法");
    let mut handoff = WaterFileHandoff {
        pending: None,
        placement: Some(PendingPlacementFileHandoff {
            gate: placement_gate(&scene_id),
            editor_previews: Vec::new(),
            deleted: Vec::new(),
        }),
        unresolved: None,
        preview_cleanup: None,
    };

    assert!(handoff.placement_handoff_in_flight_for(&scene_id)); // 保存候选未采用时禁止后续网格水写入。
    assert!(!handoff.placement_handoff_in_flight_for(&other)); // 另一张草稿不被错误冻结。

    handoff.mark_placement_unresolved(scene_id.clone(), "模拟候选失败".to_owned());
    assert!(!handoff.placement_handoff_in_flight_for(&scene_id)); // 失败后释放编辑门，保留可修复但不可关闭语义。
}

fn placement_evidence<'a>(
    scene_id: &'a SceneId,
    phase: ScenePlacementRuntimePhase,
    revision: u64,
    attempt_revision: u64,
) -> PlacementFileHandoffEvidence<'a> {
    PlacementFileHandoffEvidence {
        active_scene_id: Some(scene_id),
        status_scene_id: Some(scene_id),
        phase,
        revision,
        attempt_revision,
        progress_revision: 13,
        now: Duration::from_secs(21),
    }
}

#[test]
fn placement_handoff_requires_strictly_new_ready_or_failed_attempt() {
    let scene_id = SceneId::parse("MAP-placement-handoff").expect("测试场景ID合法");
    let mut gate = placement_gate(&scene_id);

    assert_eq!(
        placement_file_handoff_decision(
            &mut gate,
            placement_evidence(&scene_id, ScenePlacementRuntimePhase::Ready, 7, 11),
        ),
        PlacementFileHandoffDecision::Waiting,
    ); // 保存前旧Ready不能放行。
    assert_eq!(
        placement_file_handoff_decision(
            &mut gate,
            placement_evidence(&scene_id, ScenePlacementRuntimePhase::Failed, 7, 11),
        ),
        PlacementFileHandoffDecision::Waiting,
    ); // 保存前旧Failed不能误报本次失败。
    assert_eq!(
        placement_file_handoff_decision(
            &mut gate,
            placement_evidence(&scene_id, ScenePlacementRuntimePhase::Ready, 8, 11),
        ),
        PlacementFileHandoffDecision::Waiting,
    ); // 建门前已开始的旧候选即使随后Ready，也不能冒充本次发布后的新尝试。
    assert_eq!(
        placement_file_handoff_decision(
            &mut gate,
            placement_evidence(&scene_id, ScenePlacementRuntimePhase::Failed, 7, 12),
        ),
        PlacementFileHandoffDecision::RuntimeFailed,
    ); // 只有严格新尝试失败才恢复dirty。
    assert_eq!(
        placement_file_handoff_decision(
            &mut gate,
            placement_evidence(&scene_id, ScenePlacementRuntimePhase::Ready, 8, 12),
        ),
        PlacementFileHandoffDecision::Complete,
    ); // 严格新Ready证明隐藏候选已经原子替换。
}

#[test]
fn placement_handoff_can_run_over_thirty_seconds_while_real_progress_continues() {
    let scene_id = SceneId::parse("MAP-placement-timeout").expect("测试场景ID合法");
    let mut gate = placement_gate(&scene_id); // 从真实时间二十秒开始等待。
    let mut evidence = placement_evidence(&scene_id, ScenePlacementRuntimePhase::Building, 7, 12);
    evidence.progress_revision = 14;
    evidence.now = Duration::from_secs(49); // 二十九秒时真实游标推进，刷新停滞起点。
    assert_eq!(
        placement_file_handoff_decision(&mut gate, evidence),
        PlacementFileHandoffDecision::Waiting,
    );
    evidence.progress_revision = 15;
    evidence.now = Duration::from_secs(78); // 总耗时已五十八秒，但距上次推进仍不足三十秒。
    assert_eq!(
        placement_file_handoff_decision(&mut gate, evidence),
        PlacementFileHandoffDecision::Waiting,
    );
    evidence.now = Duration::from_secs(100); // 总耗时八十秒，最新推进后仍只有二十二秒。
    assert_eq!(
        placement_file_handoff_decision(&mut gate, evidence),
        PlacementFileHandoffDecision::Waiting,
    );
}

#[test]
fn placement_handoff_waits_without_a_duration_limit() {
    let scene_id = SceneId::parse("MAP-placement-stalled").expect("测试场景ID合法");
    let mut gate = placement_gate(&scene_id);
    let mut evidence = placement_evidence(&scene_id, ScenePlacementRuntimePhase::Building, 7, 12);
    evidence.now = Duration::from_secs(49);
    assert_eq!(
        placement_file_handoff_decision(&mut gate, evidence),
        PlacementFileHandoffDecision::Waiting,
    );
    evidence.now = Duration::from_secs(50);
    assert_eq!(
        placement_file_handoff_decision(&mut gate, evidence),
        PlacementFileHandoffDecision::Waiting,
    );
}

#[test]
fn old_or_other_scene_progress_cannot_complete_the_current_handoff() {
    let scene_id = SceneId::parse("MAP-placement-progress-owner").expect("测试场景ID合法");
    let other = SceneId::parse("MAP-placement-progress-other").expect("测试场景ID合法");
    let mut gate = placement_gate(&scene_id);
    let mut evidence = placement_evidence(&scene_id, ScenePlacementRuntimePhase::Building, 7, 12);
    evidence.now = Duration::from_secs(49);
    evidence.progress_revision = 13; // 相同场景的旧修订不是新推进。
    assert_eq!(
        placement_file_handoff_decision(&mut gate, evidence),
        PlacementFileHandoffDecision::Waiting,
    );
    evidence.status_scene_id = Some(&other);
    evidence.progress_revision = 99; // 另一场景即使拥有更大公开修订，也不能给本门续命。
    evidence.now = Duration::from_secs(50);
    assert_eq!(
        placement_file_handoff_decision(&mut gate, evidence),
        PlacementFileHandoffDecision::Waiting,
    );
}

#[test]
fn placement_failure_marks_same_scene_for_non_mutating_retry() {
    let scene_id = SceneId::parse("MAP-placement-retry").expect("测试场景ID合法");
    let mut handoff = WaterFileHandoff::default();

    handoff.mark_placement_unresolved(scene_id.clone(), "测试摆放失败".to_owned());

    assert!(handoff.pending_for(&scene_id, 0, 0)); // 未解决正式缓存继续阻止F12关闭。
    assert!(handoff.placement_retry_required_for(&scene_id)); // 下一保存改发显式重建而不写回相同资产。
    assert!(!handoff.blocks_new_save_for(Some(&scene_id))); // 同图完整保存仍可接管修复。
}

mod placement_preview_lifecycle;
mod placement_deletion_lifecycle; // 纯删除、失败迟到与同ID新删除沿真实正式门精确确认。
mod root_lifecycle;
