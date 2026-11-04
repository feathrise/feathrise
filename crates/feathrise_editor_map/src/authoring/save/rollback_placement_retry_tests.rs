//! 真实保存失败与水文件回滚跨越后，旧摆放重试及严格迟到批次的所有权回归。
//!
//! 【验证边界】在隔离目录执行真实部分发布与完整回滚；IO结果由生产保存poll采用，
//! 正式资产消息和公开运行时代次由夹具提供，不读取项目地图或声称覆盖GPU加载。

use super::*;
use bevy::{
    asset::{AssetLoadError, AssetLoadFailedEvent},
    diagnostic::FrameCount,
    ecs::system::RunSystemOnce,
};
use feathrise_map_content::{MapWaterMaskAsset, TerrainId};
use feathrise_scene::{DynamicWaterPhase, MapPlacementInstance, MapPlacementRuntimePhase};

use crate::authoring::placement_deletion::{mark_deleted, restore_deleted};
use crate::authoring::resources::{
    PlacementPreviewHandoffSnapshot, PlacementSaveHandoffSnapshot, WaterSaveHandoffSnapshot,
};
use crate::authoring::terrain_documents::TerrainCanvas;
use crate::authoring::{cleanup_placement_handoff_previews, poll_water_file_handoff};
use crate::components::EditorPlacedAsset;

const ROLLBACK_DELETED_ID: &str = "rollback-deleted-id";

#[derive(Clone, Copy, Debug)]
enum WaterScope {
    Root,
    Private,
}

struct RetryFixture {
    app: App,
    map: MapId,
    root: Entity,
    child: Entity,
    edited: Entity,
    edited_child: Entity,
    manifest_path: String,
    water_path: String,
    scope: WaterScope,
}

impl RetryFixture {
    fn new(scope: WaterScope) -> Self {
        Self::new_with_deletion(scope, false)
    }

    fn new_with_deletion(scope: WaterScope, with_deleted_ticket: bool) -> Self {
        let (mut app, map, root, child) = completed_placement_save_app(false, false, true);
        app.init_asset::<MapWaterMaskAsset>()
            .add_message::<feathrise_scene::TerrainWaterRebuildRequest>()
            .add_message::<AssetLoadFailedEvent<MapManifestAsset>>()
            .add_message::<AssetLoadFailedEvent<MapWaterAsset>>()
            .add_message::<AssetLoadFailedEvent<MapWaterMaskAsset>>()
            .init_resource::<FrameCount>();
        // 只复用完整App资源，撤销原夹具尚未采用的成功任务，随后全部使用本回归的真实事务结果。
        *app.world_mut().resource_mut::<DraftSaveTask>() = default();
        app.world_mut().resource_mut::<EditorAuthoringSession>().saving = false;
        app.world_mut().entity_mut(root).insert(Transform::IDENTITY);
        let mut edited_record = app.world().resource::<EditorAuthoringSession>()
            .draft.as_ref().unwrap().placements.placements[0].clone();
        edited_record.id = "rollback-later-transform".into();
        let edited = app.world_mut().spawn((
            EditorPlacedAsset,
            MapPlacementInstance::new(&map, &edited_record),
            Transform::IDENTITY,
            Visibility::Inherited,
            WorldAssetRoot(Handle::default()),
        )).id();
        let edited_child = app.world_mut().spawn((ChildOf(edited), Visibility::Inherited)).id();
        let (manifest_path, water_path, water) = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.placement_previews.insert(edited_record.id.clone(), edited);
            let draft = session.draft.as_mut().unwrap();
            draft.placements.placements.push(edited_record);
            let manifest = format!("{}/{}", draft.folder, MAP_MANIFEST_FILE_NAME);
            let water_path = match scope {
                WaterScope::Root => draft.water_asset_path.clone(),
                WaterScope::Private => format!("{}/terrains/private/water.map_water.json", draft.folder),
            };
            let water = crate::authoring::water_asset_with_layer_for_tests(&map, &draft.folder);
            match scope {
                WaterScope::Root => draft.water = Some(water.clone()),
                WaterScope::Private => {
                    draft.terrain_documents.values.insert(TerrainId::parse("private").unwrap(), Arc::new(TerrainCanvas {
                        water: Some(water.clone()),
                        water_asset_path: water_path.clone(),
                        ..default()
                    }));
                }
            }
            (manifest, water_path, water)
        };
        let water_handle = app.world_mut().resource_mut::<Assets<MapWaterAsset>>().add(water);
        if matches!(scope, WaterScope::Root) {
            app.world_mut().resource_mut::<MapContentRegistry>().maps.get_mut(&map).unwrap().water = Some(water_handle);
        }
        let placements_handle = app.world().resource::<MapContentRegistry>().maps[&map].placements.clone();
        let placements = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.clone();
        *app.world_mut().resource_mut::<Assets<MapPlacementsAsset>>().get_mut(&placements_handle).unwrap() = placements;
        if with_deleted_ticket {
            // 新增后再删除可使正式文件同值；确认责任只属于冻结该票的摆放门，不依赖预览Entity。
            app.world_mut().run_system_once(|mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
                let map = session.draft.as_ref().unwrap().map_id.clone();
                mark_deleted(&mut commands, &mut session, &map, ROLLBACK_DELETED_ID);
            }).unwrap();
        }
        let mut saved = app.world().resource::<EditorAuthoringSession>().placement_previews.iter()
            .map(|(id, root)| PlacementPreviewHandoffSnapshot::capture(app.world(), &map, id, *root).unwrap())
            .collect();
        let mut deleted = app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots();
        app.world_mut().resource_mut::<WaterFileHandoff>().begin_placement(
            map.clone(), 7, 7, 11, 20, &mut saved, &mut deleted, std::time::Duration::ZERO,
        ).unwrap();
        *app.world_mut().resource_mut::<MapPlacementRuntimeStatus>() = MapPlacementRuntimeStatus {
            map_id: Some(map.clone()),
            phase: MapPlacementRuntimePhase::Failed,
            revision: 7,
            attempt_revision: 12,
            progress_revision: 20,
            detail: "旧保存的正式摆放候选失败".into(),
            ..default()
        };
        let mut fixture = Self { app, map, root, child, edited, edited_child, manifest_path, water_path, scope };
        fixture.poll_water(); // 真实Failed采用生成unresolved及原late_placement，夹具不手造私有故障状态。
        assert!(fixture.app.world().resource::<WaterFileHandoff>().placement_retry_required_for(&fixture.map));
        fixture
    }

    fn assert_deletion_pending(&self) {
        let deleted = self.app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots();
        assert_eq!(deleted.len(), 1, "{:?}：严格摆放Ready前必须保留删除票", self.scope);
        assert_eq!(deleted[0].id, ROLLBACK_DELETED_ID);
    }

    fn snapshot(&self) -> WaterSaveHandoffSnapshot {
        WaterSaveHandoffSnapshot {
            map_id: self.map.clone(),
            water_revision: 0,
            root_water_revision: 0,
            terrain_revision: 0,
            baseline_generation: 0,
            manifest_asset_path: self.manifest_path.clone(),
            water_asset_path: if matches!(self.scope, WaterScope::Root) { self.water_path.clone() } else { String::new() },
            dirty_layers: Vec::new(),
            dirty_height_path: None,
            catalog_registration_required: false,
            terrains: if matches!(self.scope, WaterScope::Private) {
                vec![crate::authoring::resources::TerrainWaterSaveHandoffSnapshot {
                    terrain_id: TerrainId::parse("private").unwrap(),
                    water_asset_path: self.water_path.clone(),
                    baselines: Vec::new(),
                    dirty_layers: Vec::new(),
                    catalog_registration_required: false,
                    retry_required: self.app.world().resource::<WaterFileHandoff>()
                        .terrain_water_retry_required_for(&self.map, &TerrainId::parse("private").unwrap()),
                    runtime_required: false, // 合法禁用画布仍须证明正式文件完整回滚，不借根水代次直接放行。
                }]
            } else { Vec::new() },
        }
    }

    fn poll_water(&mut self) {
        self.app.world_mut().resource_mut::<FrameCount>().0 += 1;
        self.app.world_mut().run_system_once(poll_water_file_handoff).unwrap();
    }

    fn water_in_flight(&self) -> bool {
        let private = TerrainId::parse("private").unwrap();
        self.app.world().resource::<WaterFileHandoff>()
            .formal_water_revision_for_canvas(&self.map, if matches!(self.scope, WaterScope::Private) {
                Some(&private)
            } else { None }).is_some()
    }

    fn begin_water_save(&mut self) {
        let server = self.app.world().resource::<AssetServer>().clone();
        let mut snapshot = self.snapshot();
        let mut handoff = self.app.world_mut().resource_mut::<WaterFileHandoff>();
        match self.scope {
            WaterScope::Root => handoff.begin(&server, self.map.clone(), 0, 0, 0,
                &self.manifest_path, &self.water_path, &[], &[], &[], None, false,
                std::time::Duration::ZERO).unwrap(),
            WaterScope::Private => handoff.begin_terrains(&server, self.map.clone(), 0, 0,
                &self.manifest_path, &mut snapshot.terrains,
                &mut BTreeSet::from([TerrainId::parse("private").unwrap()]), None,
                std::time::Duration::ZERO).unwrap(),
        }
    }

    fn install_io_result(&mut self, result: Result<DraftSaveSuccess, DraftSaveError>) {
        let session = self.app.world().resource::<EditorAuthoringSession>();
        let revision = session.revision;
        let previews = session.placement_previews.iter()
            .map(|(id, root)| PlacementPreviewHandoffSnapshot::capture(self.app.world(), &self.map, id, *root).unwrap())
            .collect();
        let status = self.app.world().resource::<MapPlacementRuntimeStatus>();
        let placement_snapshot = PlacementSaveHandoffSnapshot {
            map_id: self.map.clone(),
            baseline_revision: status.revision,
            baseline_attempt_revision: status.attempt_revision,
            retry_failed_runtime: self.app.world().resource::<WaterFileHandoff>().placement_retry_required_for(&self.map),
            editor_previews: previews,
            deleted: session.placement_deletions.snapshots(),
        };
        let water_snapshot = self.snapshot();
        *self.app.world_mut().resource_mut::<DraftSaveTask>() = DraftSaveTask {
            ticket: 1,
            task: Some(bevy::tasks::IoTaskPool::get().spawn(async move { (1, revision, result) })),
            water_handoff_snapshot: Some(water_snapshot),
            placement_handoff_snapshot: Some(placement_snapshot),
            ..default()
        };
        self.app.world_mut().resource_mut::<EditorAuthoringSession>().saving = true;
        self.app.world_mut().resource_mut::<MapEditorState>().close_requested = true;
        consume_completed_placement_save(&mut self.app);
    }

    fn fail_retry_io_after_real_rollback(&mut self) {
        let directory = SaveTransactionTestDirectory::new();
        let manifest = directory.path.join(&self.manifest_path);
        let water = directory.path.join(&self.water_path);
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::create_dir_all(water.parent().unwrap()).unwrap();
        let old_manifest = b"old-manifest";
        let old_water = b"old-water";
        std::fs::write(&manifest, old_manifest).unwrap();
        std::fs::write(&water, old_water).unwrap();
        let failure = commit_with_replace_hook(vec![
            PreparedWrite::new(manifest.clone(), b"published-manifest".to_vec()),
            PreparedWrite::new(water.clone(), b"unpublished-water".to_vec()),
        ], |index, _| if index == 2 { Err("测试注入重试事务第二项发布失败".into()) } else { Ok(()) })
            .expect_err("首目标真实发布后第二项失败必须回滚");
        assert!(failure.rollback_complete());
        assert_eq!(failure.rollback_targets(), [manifest.clone()]);
        assert_eq!(std::fs::read(&manifest).unwrap(), old_manifest);
        assert_eq!(std::fs::read(&water).unwrap(), old_water);
        self.install_io_result(Err(DraftSaveError::from_transaction(&directory.path, failure)));
        assert!(self.water_in_flight(), "真实保存poll必须建立对应根/私有RollbackRestore门");
        assert!(!self.app.world().resource::<EditorAuthoringSession>().saving);
        assert_eq!(self.app.world().resource::<EditorAuthoringSession>().successful_save_sequence, 0);
    }

    fn finish_water_rollback(&mut self, fail: bool) {
        self.poll_water(); // 私有画布先按生产游标武装自己的文件；根门在此帧仍无完成证据。
        let server = self.app.world().resource::<AssetServer>();
        let manifest = server.get_handle::<MapManifestAsset>(self.manifest_path.clone()).unwrap();
        let water = server.get_handle::<MapWaterAsset>(self.water_path.clone()).unwrap();
        if fail && matches!(self.scope, WaterScope::Private) {
            self.app.world_mut().write_message(AssetLoadFailedEvent::<MapWaterAsset> {
                id: water.id(), path: self.water_path.clone().into(), error: AssetLoadError::AssetMetaReadError,
            }); // 私有文件加载失败必须经过同一个原子fail_map_file_handoff恢复路径。
        } else {
            self.app.world_mut().write_message(AssetEvent::<MapManifestAsset>::LoadedWithDependencies { id: manifest.id() });
            self.app.world_mut().write_message(AssetEvent::<MapWaterAsset>::LoadedWithDependencies { id: water.id() });
            *self.app.world_mut().resource_mut::<DynamicWaterStatus>() = DynamicWaterStatus {
                map_id: Some(self.map.clone()),
                phase: if fail { DynamicWaterPhase::Failed } else { DynamicWaterPhase::Ready },
                desired_generation: 1,
                applied_generation: if fail { 0 } else { 1 },
                input_draft_revision: None,
                detail: "回滚版本正式水域结果".into(),
                ..default()
            };
        }
        for _ in 0..5 {
            self.poll_water();
            if !self.water_in_flight() { break; }
        }
        assert!(!self.water_in_flight(), "回滚门应由正式输入和跨帧Ready/Failed收口");
        assert!(self.app.world().resource::<MapEditorState>().dirty);
        assert!(!self.app.world().resource::<MapEditorState>().close_requested);
        assert_eq!(self.app.world().resource::<EditorAuthoringSession>().successful_save_sequence, 0);
    }

    fn unchanged_save_success(&self) -> DraftSaveSuccess {
        DraftSaveSuccess {
            terrain_manifest: None, terrain_saved: Vec::new(), terrain_reload_paths: Vec::new(),
            terrain_digests: Vec::new(), terrain_water_changed: BTreeSet::new(),
            root_water_changed: false, domain_hash: None, map_id: self.map.clone(),
            placements_changed: false, surface_definition_changed: false, surface_weights_changed: false,
            active_terrain_surface_changes: None, surface_source_identity: None,
            terrain_sha256: None, terrain_blend_generated: false, global_environment_saved: false,
        }
    }

    fn set_placement_ready(&mut self, map: MapId, revision: u64, attempt: u64) {
        *self.app.world_mut().resource_mut::<MapPlacementRuntimeStatus>() = MapPlacementRuntimeStatus {
            map_id: Some(map), phase: MapPlacementRuntimePhase::Ready, revision,
            attempt_revision: attempt, progress_revision: 21, ..default()
        };
        self.poll_water();
        self.app.world_mut().run_system_once(cleanup_placement_handoff_previews).unwrap();
    }
}

fn keep_placement_retry_until_unchanged_save_rebuilds(scope: WaterScope, fail: bool) {
    let mut fixture = RetryFixture::new(scope);
    fixture.fail_retry_io_after_real_rollback();
    fixture.finish_water_rollback(fail);
    // 回滚只恢复本次失败IO的水文件；旧摆放从未Ready，下一次同值保存仍必须显式重建。
    let success = fixture.unchanged_save_success();
    fixture.install_io_result(Ok(success));
    assert!(!fixture.app.world().resource::<WaterFileHandoff>().placement_preview_cleanup_pending(),
        "{scope:?}/rollback_failed={fail}：旧正式摆放未恢复前，原样重试不能误走无门清理");
    assert!(fixture.app.world().resource::<Messages<MapPlacementRebuildRequest>>().len() > 0,
        "{scope:?}/rollback_failed={fail}：JSON同值也必须重发正式摆放候选");
    assert!(fixture.app.world().resource::<WaterFileHandoff>().placement_handoff_in_flight_for(&fixture.map));
    assert_eq!(fixture.app.world().resource::<EditorAuthoringSession>().successful_save_sequence, 1);
    fixture.app.world_mut().run_system_once(cleanup_placement_handoff_previews).unwrap();
    assert!(fixture.app.world().get_entity(fixture.root).is_ok());
    assert_eq!(fixture.app.world().get::<ChildOf>(fixture.child).map(ChildOf::parent), Some(fixture.root));
}

fn preserve_original_late_gate_batch_and_later_transform(scope: WaterScope, fail: bool) {
    let mut fixture = RetryFixture::new(scope);
    // 在旧门失败后模拟后来预览接管：旧批仍拥有原根，当前地址表不得被拿来重建旧late批次。
    let record = fixture.app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements[0].clone();
    let later = fixture.app.world_mut().spawn((EditorPlacedAsset, Transform::IDENTITY,
        Visibility::Inherited, MapPlacementInstance::new(&fixture.map, &record))).id();
    fixture.app.world_mut().resource_mut::<EditorAuthoringSession>().placement_previews.insert(record.id.clone(), later);
    fixture.app.world_mut().get_mut::<Transform>(fixture.edited).unwrap().translation.y = 6.0;
    fixture.fail_retry_io_after_real_rollback();
    fixture.finish_water_rollback(fail);

    let foreign = MapId::parse("MAP-foreign-water-rollback").unwrap();
    let server = fixture.app.world().resource::<AssetServer>().clone();
    let reject = fixture.app.world_mut().resource_mut::<WaterFileHandoff>().begin_rollback(
        &server, foreign.clone(), 0, 0, 0, &fixture.manifest_path, &fixture.water_path,
        &[], &[], None, std::time::Duration::ZERO,
    );
    assert!(reject.is_err(), "另一地图不能借水回滚丢弃本图的旧摆放恢复责任");
    fixture.set_placement_ready(foreign, 100, 100);
    assert!(fixture.app.world().get_entity(fixture.root).is_ok());
    fixture.set_placement_ready(fixture.map.clone(), 7, 12);
    assert!(fixture.app.world().get_entity(fixture.root).is_ok(), "旧Ready必须仍受原严格revision门阻止");
    fixture.set_placement_ready(fixture.map.clone(), 8, 11);
    assert!(fixture.app.world().get_entity(fixture.root).is_ok(), "旧attempt不能冒充原门后开始的新候选");
    fixture.set_placement_ready(fixture.map.clone(), 8, 12);
    assert!(fixture.app.world().get_entity(fixture.root).is_err(),
        "{scope:?}/rollback_failed={fail}：严格迟到Ready仍须精确回收原批旧根，不能丢掉late批次");
    assert!(fixture.app.world().get_entity(fixture.child).is_err());
    assert!(fixture.app.world().get_entity(later).is_ok());
    assert_eq!(fixture.app.world().resource::<EditorAuthoringSession>().placement_previews.get(&record.id), Some(&later));
    assert_eq!(fixture.app.world().get::<Transform>(fixture.edited).unwrap().translation.y, 6.0);
    assert_eq!(fixture.app.world().get::<Visibility>(fixture.edited), Some(&Visibility::Inherited));
    assert_eq!(fixture.app.world().get::<ChildOf>(fixture.edited_child).map(ChildOf::parent), Some(fixture.edited));
    assert!(fixture.app.world().get::<WorldAssetRoot>(fixture.edited).is_some());
    assert!(fixture.app.world().resource::<WaterFileHandoff>().placement_retry_required_for(&fixture.map));
    assert!(fixture.app.world().resource::<WaterFileHandoff>().pending_for(&fixture.map, 0, 0));
    assert!(!fixture.app.world().resource::<WaterFileHandoff>().blocks_new_save_for(Some(&fixture.map)),
        "严格迟到Ready后的未解决态仍允许同图完整保存接管");
}

fn preserve_deleted_ticket_across_water_rollback_until_late_ready(scope: WaterScope) {
    let mut fixture = RetryFixture::new_with_deletion(scope, true);
    fixture.assert_deletion_pending();
    fixture.fail_retry_io_after_real_rollback();
    fixture.assert_deletion_pending();
    fixture.finish_water_rollback(false);
    // RollbackRestoreReady只证明旧水文件恢复，不拥有摆放删除票的确认权。
    fixture.assert_deletion_pending();

    fixture.set_placement_ready(MapId::parse("MAP-foreign-deletion-rollback").unwrap(), 100, 100);
    fixture.assert_deletion_pending();
    fixture.set_placement_ready(fixture.map.clone(), 7, 12);
    fixture.assert_deletion_pending();
    fixture.set_placement_ready(fixture.map.clone(), 8, 11);
    fixture.assert_deletion_pending();
    fixture.set_placement_ready(fixture.map.clone(), 8, 12);
    assert!(fixture.app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots().is_empty(),
        "{scope:?}：只有原失败门的严格迟到Ready经生产清理槽确认删除");
    assert!(fixture.app.world().resource::<WaterFileHandoff>().placement_retry_required_for(&fixture.map));
    assert_eq!(fixture.app.world().resource::<EditorAuthoringSession>().successful_save_sequence, 0);
}

#[test]
fn rollback_root_ready_preserves_delete_until_original_late_placement_ready() {
    preserve_deleted_ticket_across_water_rollback_until_late_ready(WaterScope::Root);
}

#[test]
fn rollback_private_ready_preserves_delete_until_original_late_placement_ready() {
    preserve_deleted_ticket_across_water_rollback_until_late_ready(WaterScope::Private);
}

#[test]
fn rollback_late_placement_ready_cannot_confirm_a_later_delete_of_the_same_id() {
    let mut fixture = RetryFixture::new_with_deletion(WaterScope::Root, true);
    fixture.fail_retry_io_after_real_rollback();
    fixture.finish_water_rollback(false);
    fixture.assert_deletion_pending();
    fixture.app.world_mut().run_system_once(|mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        let map = session.draft.as_ref().unwrap().map_id.clone();
        restore_deleted(&mut commands, &mut session, &map, ROLLBACK_DELETED_ID);
        mark_deleted(&mut commands, &mut session, &map, ROLLBACK_DELETED_ID);
    }).unwrap();

    // 同ID恢复后再删除产生S2；S1经历IO回滚仍可迟到，但不得借旧门确认S2。
    fixture.set_placement_ready(fixture.map.clone(), 8, 12);
    fixture.assert_deletion_pending();
    let success = fixture.unchanged_save_success();
    fixture.install_io_result(Ok(success));
    assert!(fixture.app.world().resource::<Messages<MapPlacementRebuildRequest>>().len() > 0,
        "S2的同值保存必须由生产保存poll重发正式摆放候选");
    assert!(fixture.app.world().resource::<WaterFileHandoff>().placement_handoff_in_flight_for(&fixture.map));
    fixture.assert_deletion_pending();
    fixture.set_placement_ready(fixture.map.clone(), 9, 13);
    assert!(fixture.app.world().resource::<EditorAuthoringSession>().placement_deletions.snapshots().is_empty(),
        "S2只能由其自身保存门的严格新Ready经生产清理槽确认");
}

// 四种回滚结局分别报告，任一路径的首个红灯都不会掩盖另一条建门/失败恢复路径。
#[test]
fn rollback_outcomes_root_ready_keeps_placement_retry() {
    keep_placement_retry_until_unchanged_save_rebuilds(WaterScope::Root, false);
}

#[test]
fn rollback_outcomes_root_failed_keeps_placement_retry() {
    keep_placement_retry_until_unchanged_save_rebuilds(WaterScope::Root, true);
}

#[test]
fn rollback_outcomes_private_ready_keeps_placement_retry() {
    keep_placement_retry_until_unchanged_save_rebuilds(WaterScope::Private, false);
}

#[test]
fn rollback_outcomes_private_failed_keeps_placement_retry() {
    keep_placement_retry_until_unchanged_save_rebuilds(WaterScope::Private, true);
}

#[test]
fn rollback_outcomes_root_ready_preserves_late_gate_and_transform() {
    preserve_original_late_gate_batch_and_later_transform(WaterScope::Root, false);
}

#[test]
fn rollback_outcomes_root_failed_preserves_late_gate_and_transform() {
    preserve_original_late_gate_batch_and_later_transform(WaterScope::Root, true);
}

#[test]
fn rollback_outcomes_private_ready_preserves_late_gate_and_transform() {
    preserve_original_late_gate_batch_and_later_transform(WaterScope::Private, false);
}

#[test]
fn rollback_outcomes_private_failed_preserves_late_gate_and_transform() {
    preserve_original_late_gate_batch_and_later_transform(WaterScope::Private, true);
}

fn water_publish_failure_keeps_prior_late_gate_and_retry(scope: WaterScope) {
    let mut fixture = RetryFixture::new(scope);
    fixture.app.world_mut().resource_mut::<WaterFileHandoff>().mark_unresolved(
        fixture.map.clone(), "后续事务回滚不完整，仍须保留旧摆放".into(),
    );
    // 另一地图的诊断不能抢走单槽拥有权，也不能把本图严格门/预览批次混入另一图。
    let foreign = MapId::parse("MAP-foreign-mark-unresolved").unwrap();
    fixture.app.world_mut().resource_mut::<WaterFileHandoff>().mark_unresolved(
        foreign.clone(), "不属于当前恢复事务的另一地图".into(),
    );
    assert!(!fixture.app.world().resource::<WaterFileHandoff>().placement_retry_required_for(&foreign));
    fixture.begin_water_save();
    fixture.finish_water_rollback(true); // 共享正式Ready/Failed输入夹具；本次门的purpose实际是SavePublish。
    fixture.set_placement_ready(fixture.map.clone(), 7, 12);
    assert!(fixture.app.world().get_entity(fixture.root).is_ok());
    fixture.set_placement_ready(fixture.map.clone(), 8, 12);
    assert!(fixture.app.world().get_entity(fixture.root).is_err(),
        "{scope:?}：mark_unresolved与仅水保存失败不能丢失旧严格门拥有的精确批次");
    assert!(fixture.app.world().get_entity(fixture.child).is_err());
    let success = fixture.unchanged_save_success();
    fixture.install_io_result(Ok(success));
    assert!(fixture.app.world().resource::<Messages<MapPlacementRebuildRequest>>().len() > 0,
        "{scope:?}：迟到Ready只清旧预览，后续原样保存仍应履行显式重试责任");
    assert!(fixture.app.world().resource::<WaterFileHandoff>().placement_handoff_in_flight_for(&fixture.map));
}

fn placement_gate_after_water_publish_takes_over_prior_batch(scope: WaterScope) {
    let mut fixture = RetryFixture::new(scope);
    fixture.begin_water_save();
    let mut previews = fixture.app.world().resource::<EditorAuthoringSession>().placement_previews.iter()
        .map(|(id, root)| PlacementPreviewHandoffSnapshot::capture(fixture.app.world(), &fixture.map, id, *root).unwrap())
        .collect();
    fixture.app.world_mut().resource_mut::<WaterFileHandoff>().begin_placement(
        fixture.map.clone(), 7, 100, 100, 20, &mut previews, &mut Vec::new(), std::time::Duration::ZERO,
    ).unwrap();
    // 新摆放门真正建立后，旧门可接受的Ready不再拥有清理权，必须越过新完整基线。
    fixture.set_placement_ready(fixture.map.clone(), 8, 12);
    assert!(fixture.app.world().get_entity(fixture.root).is_ok());
    assert_eq!(fixture.app.world().get::<Visibility>(fixture.root), Some(&Visibility::Inherited));
    fixture.set_placement_ready(fixture.map.clone(), 101, 101);
    assert!(fixture.app.world().get_entity(fixture.root).is_err());
    assert!(fixture.app.world().get_entity(fixture.child).is_err());
    assert!(!fixture.app.world().resource::<WaterFileHandoff>().placement_retry_required_for(&fixture.map));
}

#[test]
fn rollback_outcomes_root_save_publish_failure_keeps_prior_late_gate_and_retry() {
    water_publish_failure_keeps_prior_late_gate_and_retry(WaterScope::Root);
}

#[test]
fn rollback_outcomes_private_save_publish_failure_keeps_prior_late_gate_and_retry() {
    water_publish_failure_keeps_prior_late_gate_and_retry(WaterScope::Private);
}

#[test]
fn rollback_outcomes_root_new_placement_gate_takes_over_prior_batch() {
    placement_gate_after_water_publish_takes_over_prior_batch(WaterScope::Root);
}

#[test]
fn rollback_outcomes_private_new_placement_gate_takes_over_prior_batch() {
    placement_gate_after_water_publish_takes_over_prior_batch(WaterScope::Private);
}
