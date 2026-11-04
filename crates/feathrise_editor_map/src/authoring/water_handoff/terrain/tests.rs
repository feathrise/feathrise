//! 独立水画布正式身份、串行门与失败归属回归；不启动渲染或读取项目资产。
use super::*;
use std::sync::Arc;
use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce};
use crate::authoring::terrain_documents::{TerrainCanvas, TerrainDocumentIdentity};

fn target(id: &str) -> TerrainWaterSaveHandoffSnapshot {
    TerrainWaterSaveHandoffSnapshot {
        terrain_id: TerrainId::parse(id).unwrap(), water_asset_path: format!("maps/MAP-water/terrains/{id}/water.map_water.json"),
        baselines: Vec::new(), dirty_layers: vec![("lake".into(), format!("{id}/mask.r8"), format!("{id}/mask.ktx2"))],
        catalog_registration_required: false, retry_required: false, runtime_required: true,
    }
}
fn batch(targets: Vec<TerrainWaterSaveHandoffSnapshot>) -> TerrainWaterBatch {
    TerrainWaterBatch { water_revision: 7, changed: targets.iter().map(|target| target.terrain_id.clone()).collect(), targets,
        rollback_paths: None, manifest_path: "maps/MAP-water/map.map_manifest.json".into(),
        cursor: 0, armed: true, asset_cursor: 0, runtime_requested: false }
}
fn pending(map: &MapId, targets: Vec<TerrainWaterSaveHandoffSnapshot>) -> PendingWaterFileHandoff {
    let handle = Handle::default();
    PendingWaterFileHandoff {
        gate: WaterFileHandoffGate { purpose: WaterFileHandoffPurpose::SavePublish, map_id: map.clone(),
            water_revision: 7, terrain_revision: 5, baseline_generation: 0, started_at: Duration::ZERO,
            all_inputs_succeeded_frame: Some(30) },
        manifest: super::super::PendingWaterAsset { id: handle.id(), _handle: handle, succeeded: true },
        water: None, masks: Vec::new(), textures: Vec::new(), dirty_layer_ids: Vec::new(),
        catalog_registration_required: false, manifest_asset_path: "maps/MAP-water/map.map_manifest.json".into(),
        root_pending: false,
        terrains: Some(batch(targets)),
    }
}
fn status(map: &MapId, generation: u64) -> DynamicWaterStatus {
    DynamicWaterStatus { phase: DynamicWaterPhase::Ready, map_id: Some(map.clone()),
        desired_generation: generation, applied_generation: generation, ..default() }
}

#[test]
fn only_this_entitys_strict_new_formal_active_generation_can_complete() {
    let map = MapId::parse("MAP-water").unwrap();
    let mut world = World::new();
    let old = world.spawn_empty().id();
    let replacement = world.spawn_empty().id();
    let mut saved = target("a"); saved.baselines.push((old, 11));
    assert_eq!(object_decision(&saved, &map, old, &status(&map, 11), true), WaterFileHandoffDecision::Waiting);
    let mut runtime = status(&map, 12);
    runtime.input_draft_revision = Some(7);
    assert_eq!(object_decision(&saved, &map, old, &runtime, true), WaterFileHandoffDecision::Waiting);
    runtime.input_draft_revision = None;
    assert_eq!(object_decision(&saved, &map, old, &runtime, false), WaterFileHandoffDecision::Waiting,
        "隐藏候选Ready尚未原子采用，不能完成保存");
    runtime.applied_generation = 11;
    assert_eq!(object_decision(&saved, &map, old, &runtime, true), WaterFileHandoffDecision::Waiting);
    runtime.phase = DynamicWaterPhase::Failed;
    assert_eq!(object_decision(&saved, &map, old, &runtime, false), WaterFileHandoffDecision::RuntimeFailed,
        "明确Failed通常保留旧applied，仍必须恢复作者责任");
    assert_eq!(object_decision(&saved, &map, replacement, &status(&map, 1), true), WaterFileHandoffDecision::Complete,
        "新实体的代次从零开始，不能借旧实例较大的下限卡死");
    assert_eq!(object_decision(&saved, &map, old, &status(&MapId::parse("MAP-other").unwrap(), 100), true), WaterFileHandoffDecision::Waiting);
}

#[test]
fn serial_progress_retains_whole_batch_ownership_and_skips_unchanged_water() {
    let map = MapId::parse("MAP-water").unwrap();
    let mut handoff = WaterFileHandoff { pending: Some(pending(&map, vec![target("a"), target("b"), target("c")])), ..default() };
    handoff.pending.as_mut().unwrap().terrains.as_mut().unwrap().changed.remove(&TerrainId::parse("b").unwrap());
    for name in ["a", "c"] {
        assert_eq!(handoff.formal_water_revision_for_canvas(&map, Some(&TerrainId::parse(name).unwrap())), Some(7));
    }
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, Some(&TerrainId::parse("b").unwrap())), None);
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, None), None, "根容器不得冒充对象画布");
    finish_current(handoff.pending.as_mut().unwrap());
    assert_eq!(handoff.pending.as_ref().unwrap().terrains.as_ref().unwrap().cursor, 1);
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, Some(&TerrainId::parse("a").unwrap())), Some(7),
        "串行游标推进后仍持有整批责任，后续对象失败必须恢复之前对象");
    handoff.mark_unresolved(map.clone(), "模拟后续对象失败".into());
    assert!(handoff.terrain_water_retry_required_for(&map, &TerrainId::parse("a").unwrap()));
    assert_eq!(handoff.unresolved.as_ref().unwrap().terrain_recovery.as_ref().unwrap().cursor, 0);
}

#[derive(Resource)]
struct Drive { pending: PendingWaterFileHandoff, frame: u32, decision: WaterFileHandoffDecision }
fn drive(mut state: ResMut<Drive>, mut runtime: WaterHandoffRuntime, active: Res<ActiveDynamicMap>) {
    let frame = state.frame;
    let mut failure = None;
    state.decision = advance(&mut state.pending, &mut runtime, &active, frame, &mut failure);
    assert!(failure.is_none());
}
#[test]
fn disabled_or_deleted_object_waits_for_files_and_a_frame_but_enabled_missing_instance_keeps_waiting() {
    let map = MapId::parse("MAP-water").unwrap();
    let mut app = App::new(); app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    // 根状态即使Ready也不能被缺失对象借用；纯门已模拟全部文件成功，不触发任何IO。
    app.insert_resource(status(&map, 900));
    app.insert_resource(ActiveDynamicMap { map_id: Some(map.clone()), ..default() });
    app.add_message::<TerrainWaterRebuildRequest>();
    let mut saved = target("disabled"); saved.runtime_required = false;
    app.insert_resource(Drive { pending: pending(&map, vec![saved]), frame: 30, decision: WaterFileHandoffDecision::Waiting });
    app.world_mut().run_system_once(drive).unwrap();
    assert_eq!(app.world().resource::<Drive>().pending.terrains.as_ref().unwrap().cursor, 0);
    app.world_mut().resource_mut::<Drive>().pending.gate.all_inputs_succeeded_frame = None;
    app.world_mut().resource_mut::<Drive>().frame = 31;
    app.world_mut().run_system_once(drive).unwrap();
    assert_eq!(app.world().resource::<Drive>().pending.terrains.as_ref().unwrap().cursor, 0, "缺正式文件事件仍不能放行禁用对象");
    app.world_mut().resource_mut::<Drive>().pending.gate.all_inputs_succeeded_frame = Some(30);
    app.world_mut().run_system_once(drive).unwrap();
    assert_eq!(app.world().resource::<Drive>().pending.terrains.as_ref().unwrap().cursor, 1);
    assert!(app.world().resource::<Messages<TerrainWaterRebuildRequest>>().is_empty(), "禁用/删除目标不应发送无法满足的运行时请求");
    app.world_mut().run_system_once(drive).unwrap();
    assert_eq!(app.world().resource::<Drive>().decision, WaterFileHandoffDecision::Complete);
    app.world_mut().insert_resource(Drive { pending: pending(&map, vec![target("enabled")]), frame: 31,
        decision: WaterFileHandoffDecision::Waiting });
    app.world_mut().run_system_once(drive).unwrap();
    app.world_mut().run_system_once(drive).unwrap();
    assert_eq!(app.world().resource::<Drive>().decision, WaterFileHandoffDecision::Waiting);
    assert_eq!(app.world().resource::<Drive>().pending.terrains.as_ref().unwrap().cursor, 0);
    assert_eq!(app.world().resource::<Messages<TerrainWaterRebuildRequest>>().len(), 1);
}

#[test]
fn failure_restores_active_and_parked_targets_in_eight_canvas_batches_without_copying_pixels() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.map_id.clone();
    let template = draft.manifest.terrain_objects_for_edit()[0].clone();
    let pixels = Arc::new(vec![7; 16]);
    let targets: Vec<_> = (0..10).map(|index| target(&format!("target-{index:02}"))).collect();
    for (index, saved) in targets.iter().enumerate() {
        let mut object = template.clone(); object.id = saved.terrain_id.clone();
        let identity = TerrainDocumentIdentity { manifest: object.source_manifest(&draft.manifest),
            folder: object.private_folder(&map), definition: Arc::new(object) };
        draft.terrain_documents.saved.insert(saved.terrain_id.clone());
        if index == 9 {
            draft.terrain_documents.active = Some(identity);
        } else {
            draft.terrain_documents.values.insert(saved.terrain_id.clone(), Arc::new(TerrainCanvas {
                identity: Some(identity), water_masks: [("lake".into(), pixels.clone())].into(), ..default() }));
        }
    }
    let mut handoff = WaterFileHandoff { pending: Some(pending(&map, targets)), ..default() };
    handoff.mark_unresolved(map.clone(), "模拟水运行时失败".into());
    let mut session = EditorAuthoringSession { draft: Some(draft), ..default() };
    restore_failed_inputs(&mut handoff, &mut session);
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.terrain_documents.dirty_stored.len(), 8);
    assert!(!draft.terrain_documents.active_dirty);
    assert!(handoff.blocks_new_save_for(Some(&map)), "责任尚未恢复完时，下一保存不能漏掉尾部画布");
    restore_failed_inputs(&mut handoff, &mut session);
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.terrain_documents.dirty_stored.len(), 9);
    assert!(draft.terrain_documents.active_dirty);
    assert!(draft.water_dirty_layers.contains("lake"));
    assert!(draft.terrain_documents.saved.is_empty());
    for canvas in draft.terrain_documents.values.values() {
        assert!(canvas.water_dirty_layers.contains("lake"));
        assert!(Arc::ptr_eq(&canvas.water_masks["lake"], &pixels));
    }
    assert!(!handoff.blocks_new_save_for(Some(&map)), "完整恢复后同图可重试");
    assert!(handoff.pending_for(&map, 999, 999), "明确失败仍不可绕过关闭");
}

#[test]
fn combined_root_and_private_handoff_keep_separate_revisions_and_ownership() {
    let map = MapId::parse("MAP-water").unwrap();
    let id = TerrainId::parse("private").unwrap();
    let mut root = pending(&map, Vec::new());
    root.terrains = None;
    root.root_pending = true;
    let mut handoff = WaterFileHandoff { pending: Some(root), ..default() };
    let mut targets = vec![target("private")];
    let mut changed = [id.clone()].into();
    handoff.attach_terrain_batch(&map, 19, "maps/MAP-water/map.map_manifest.json", &mut targets, &mut changed, None).unwrap();
    assert!(targets.is_empty() && changed.is_empty());
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, None), Some(7));
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, Some(&id)), Some(19));
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, Some(&TerrainId::parse("other").unwrap())), None);
    handoff.pending.as_mut().unwrap().root_pending = false; // 根正式完成后仍持有整批私有责任。
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, None), None);
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, Some(&id)), Some(19));
    handoff.mark_unresolved(map.clone(), "后续私有水失败".into());
    assert!(handoff.terrain_water_retry_required_for(&map, &id));
}

/// 容器清单最后发布；故障发生在此之前时，回滚表可以只涉及B，而完整冻结表仍携带A的旧retry。
fn rollback_touching_b_with_prior_retry_a(map: &MapId) -> PendingWaterFileHandoff {
    let mut a = target("a");
    a.retry_required = true; // WaterSaveCapture在旧恢复cursor完成后，从同图未解决态冻结此责任。
    let b = target("b");
    let touched = b.water_asset_path.clone();
    let mut rollback = pending(map, vec![a, b]);
    rollback.gate.purpose = WaterFileHandoffPurpose::RollbackRestore;
    let batch = rollback.terrains.as_mut().unwrap();
    batch.changed.clear();
    batch.rollback_paths = Some([touched].into());
    rollback
}

#[test]
fn partial_rollback_failure_keeps_unaffected_prior_retry_membership() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.map_id.clone();
    let a = TerrainId::parse("a").unwrap();
    let b = TerrainId::parse("b").unwrap();
    for id in [&a, &b] {
        draft.terrain_documents.values.insert(id.clone(), Arc::new(TerrainCanvas::default()));
    }
    let mut session = EditorAuthoringSession { draft: Some(draft), ..default() };
    let mut handoff = WaterFileHandoff { pending: Some(pending(&map, vec![target("a"), target("b")])), ..default() };
    handoff.mark_unresolved(map.clone(), "旧A/B正式保存失败".into());
    restore_failed_inputs(&mut handoff, &mut session); // 真实恢复完旧游标，下一次完整保存才能捕获A/B旧retry。
    assert!(!handoff.blocks_new_save_for(Some(&map)));
    handoff.pending = Some(rollback_touching_b_with_prior_retry_a(&map));
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, Some(&a)), None,
        "A未受本次磁盘回滚影响，不得扩大正式输入冻结或运行时等待");
    assert_eq!(handoff.formal_water_revision_for_canvas(&map, Some(&b)), Some(7));
    super::super::failure::fail_map_file_handoff(&mut handoff, &mut session,
        &mut crate::resources::MapEditorState::default(), "B回滚正式输入失败");
    assert!(handoff.terrain_water_retry_required_for(&map, &a),
        "失败后接管完整冻结表仍须保留未受本次回滚影响的A旧重试责任");
    assert!(handoff.terrain_water_retry_required_for(&map, &b));
}

#[test]
fn partial_rollback_only_requests_touched_runtime_but_recovers_prior_retry_canvas() {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.map_id.clone();
    let a = TerrainId::parse("a").unwrap();
    let b = TerrainId::parse("b").unwrap();
    let pixels = Arc::new(vec![7; 16]);
    for id in [&a, &b] {
        draft.terrain_documents.saved.insert(id.clone());
        draft.terrain_documents.values.insert(id.clone(), Arc::new(TerrainCanvas {
            water_masks: [("lake".into(), pixels.clone())].into(), ..default()
        }));
    }
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(status(&map, 900))
        .insert_resource(ActiveDynamicMap { map_id: Some(map.clone()) })
        .add_message::<TerrainWaterRebuildRequest>()
        .insert_resource(Drive { pending: rollback_touching_b_with_prior_retry_a(&map), frame: 31,
            decision: WaterFileHandoffDecision::Waiting });
    app.world_mut().run_system_once(drive).unwrap();
    let requests: Vec<_> = app.world_mut().resource_mut::<Messages<TerrainWaterRebuildRequest>>().drain().collect();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].terrain_id, b, "回滚运行时门只重建B，不借恢复资格向A发送请求");
    let pending = app.world_mut().remove_resource::<Drive>().unwrap().pending;
    let mut handoff = WaterFileHandoff { pending: Some(pending), ..default() };
    let mut session = EditorAuthoringSession { draft: Some(draft), ..default() };
    super::super::failure::fail_map_file_handoff(&mut handoff, &mut session,
        &mut crate::resources::MapEditorState::default(), "B回滚运行时失败");
    restore_failed_inputs(&mut handoff, &mut session);
    let draft = session.draft.as_ref().unwrap();
    assert!(draft.terrain_documents.dirty_stored.contains(&a), "A旧retry必须恢复到自己的停放作者槽");
    assert!(draft.terrain_documents.dirty_stored.contains(&b));
    assert!(!draft.terrain_documents.saved.contains(&a));
    assert!(draft.terrain_documents.values[&a].water_dirty_layers.contains("lake"));
    assert!(Arc::ptr_eq(&draft.terrain_documents.values[&a].water_masks["lake"], &pixels));
}
