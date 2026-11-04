//! 保存成功后的正式摆放发布计划、交接门建立与唯一动作执行。
//!
//! 【功能设计】磁盘语义未变化且没有删除确认责任时保持正式`MapPlacementsAsset`静默；真实发布、失败重试或删除确认
//! 必须先建立当前地图严格新Ready/Failed修订门，再原位替换、reload或发送非修改式重建请求。

use std::time::Duration; // 交接截止沿用F12暂停期间仍推进的真实时间。

use bevy::prelude::*; // AssetServer、Assets和消息写入。
use feathrise_map_content::{MapContentRegistry, MapId, MapPlacementsAsset}; // 唯一正式句柄与保存快照。
use feathrise_scene::MapPlacementRebuildRequest; // 原样重建入口不靠伪Asset Modified。

use super::publish::{
    SavedPlacementsPublishAction, plan_saved_placements_publish, publish_saved_placements_asset,
}; // 只读计划与真实差异原位替换保持分离。
use crate::authoring::resources::PlacementSaveHandoffSnapshot; // IO启动时冻结的严格修订基线。
use crate::authoring::water_handoff::{WaterFileHandoff, schedule_placement_preview_cleanup}; // 正式门与无门成功都共用唯一精确预览回收队列。

/// 保存完成帧冻结的摆放发布与交接决定。
pub(super) struct PlacementPublishPreparation {
    pub(super) action: SavedPlacementsPublishAction, // 之后只能执行这一个明确动作。
    pub(super) handoff_required: bool,               // 真实磁盘变化、失败重试及待确认删除必须等待正式新修订。
}

/// 只读检查磁盘变化、正式资产内容和IO期间运行时进度，不取得`AssetMut`。
pub(super) fn prepare_placement_publish(
    registry: Option<&MapContentRegistry>,
    map_id: &MapId,
    snapshot: Option<&MapPlacementsAsset>,
    placements: &Assets<MapPlacementsAsset>,
    placements_changed: bool,
    handoff_snapshot: Option<&PlacementSaveHandoffSnapshot>,
) -> PlacementPublishPreparation {
    let retry_failed_runtime =
        handoff_snapshot.is_some_and(|snapshot| snapshot.retry_failed_runtime); // 上次失败允许原样重建而不伪造Asset Modified。
    let deletion_confirmation_required = handoff_snapshot.is_some_and(|snapshot| !snapshot.deleted.is_empty());
    let rebuild_unchanged = retry_failed_runtime || deletion_confirmation_required;
    let mut action = if placements_changed || rebuild_unchanged {
        snapshot.map_or(SavedPlacementsPublishAction::Reload, |snapshot| {
            plan_saved_placements_publish(
                registry,             // 只读检查当前地图唯一正式句柄。
                map_id,               // 禁止比较到另一地图缓存。
                snapshot,             // 本次已落盘完成值。
                placements,           // `get`不触发Modified。
                rebuild_unchanged, // 新增后删除可能使文件同值；仍须Rebuild确认旧候选失效，避免删除表满后永远无法通过保存释放。
            )
        })
    } else {
        SavedPlacementsPublishAction::None // 磁盘语义未变且无失败或删除责任时整个正式摆放链路保持静默。
    };
    if placements_changed && action == SavedPlacementsPublishAction::None {
        action = SavedPlacementsPublishAction::Rebuild; // 文件监听即使在IO期间已把同值装入缓存，也要在建门后补发一次候选，旧Ready绝不能冒充本次保存成功。
    }
    PlacementPublishPreparation {
        action, // 后续执行不再重新读取可能变化的正式资产。
        handoff_required: placements_changed || rebuild_unchanged, // 删除也必须取得严格新Ready，IO成功和同值缓存都不能确认。
    }
}

/// 当前玩法地图在任何摆放发布动作之前建立正式运行时交接门。
pub(super) fn begin_placement_handoff(
    handoff: &mut WaterFileHandoff,
    preparation: &PlacementPublishPreparation,
    snapshot: Option<&mut PlacementSaveHandoffSnapshot>,
    active_map_id: Option<&MapId>,
    map_id: &MapId,
    author_revision: u64,
    current_revision: u64,
    current_attempt_revision: u64,
    current_progress_revision: u64,
    started_at: Duration,
) -> Result<bool, String> {
    if !preparation.handoff_required || active_map_id != Some(map_id) {
        return Ok(false); // 无变化或非当前玩法地图只更新缓存，不建立F12关闭门。
    }
    let snapshot = snapshot.ok_or_else(|| "缺少IO启动时的正式摆放修订基线".to_owned())?;
    if &snapshot.map_id != map_id {
        return Err("IO启动时的正式摆放修订基线属于另一张地图".to_owned());
    }
    handoff.begin_placement(
        map_id.clone(),
        author_revision, // 最终状态提示区分保存后继续编辑。
        snapshot.baseline_revision.max(current_revision), // 建门前任何旧候选完成都不能冒充本次发布后的新Ready。
        snapshot
            .baseline_attempt_revision
            .max(current_attempt_revision), // 建门前已经开始的尝试同样不是本次发布证据。
        current_progress_revision, // 建门前已经发生的真实进展不能重置发布后的停滞计时。
        &mut snapshot.editor_previews, // 只有门完整建立后才移动精确批次；错误时保留预览供修复重试，不能交给无门清理。
        &mut snapshot.deleted, // 与预览同批接管；纯删除仍持有独立的严格Ready确认责任。
        started_at,
    )?;
    Ok(true)
}

/// 非当前地图或无语义变化时，也把保存快照交给同一固定帧预算回收队列。
pub(super) fn schedule_unclaimed_placement_previews(
    handoff: &mut WaterFileHandoff,
    snapshot: Option<&mut PlacementSaveHandoffSnapshot>,
) -> Result<(), String> {
    let Some(snapshot) = snapshot else {
        return Ok(()); // 没有摆放快照时不分配空队列。
    };
    let previews = std::mem::take(&mut snapshot.editor_previews); // 只移交IO启动时冻结的精确实体。
    // 无门成功没有证明旧正式候选已退役；删除弱快照不交给清理，逻辑票据继续由文档持有。
    match schedule_placement_preview_cleanup(handoff, &snapshot.map_id, previews) {
        Ok(()) => Ok(()),
        Err(previews) => {
            snapshot.editor_previews = previews; // 前一批未清完时完整归还所有权，绝不丢失或无界合并。
            Err("上一批草稿Scene0预览仍在分帧回收".to_owned())
        }
    }
}

/// 在门已建立后执行唯一正式动作；`None`路径完全不修改或reload摆放资产。
#[allow(clippy::too_many_arguments)]
pub(super) fn execute_placement_publish(
    preparation: &PlacementPublishPreparation,
    handoff_started: bool,
    active_map_id: Option<&MapId>,
    map_id: &MapId,
    snapshot: Option<&MapPlacementsAsset>,
    asset_path: Option<&str>,
    registry: Option<&MapContentRegistry>,
    placements: &mut Assets<MapPlacementsAsset>,
    asset_server: &AssetServer,
    rebuild_requests: &mut MessageWriter<MapPlacementRebuildRequest>,
) {
    let publish_allowed =
        !preparation.handoff_required || active_map_id != Some(map_id) || handoff_started; // 当前地图必须先建门，非当前地图只更新正式缓存供下次进入。
    if !publish_allowed {
        return; // 建门失败时不主动制造一个无法证明完成的正式事件。
    }
    match preparation.action {
        SavedPlacementsPublishAction::None => {} // 内容未变或监听已开始本次尝试，禁止制造第二个Modified。
        SavedPlacementsPublishAction::Replace => {
            if let Some(snapshot) = snapshot
                && !publish_saved_placements_asset(registry, map_id, snapshot, placements)
                && let Some(path) = asset_path
            {
                asset_server.reload(path.to_owned()); // 句柄意外失效时仍沿已建立交接门兜底。
            }
        }
        SavedPlacementsPublishAction::Reload => {
            if let Some(path) = asset_path {
                asset_server.reload(path.to_owned()); // 句柄尚未就绪时显式读取刚落盘完成值。
            }
        }
        SavedPlacementsPublishAction::Rebuild => {
            if active_map_id == Some(map_id) && handoff_started {
                rebuild_requests.write(MapPlacementRebuildRequest {
                    map_id: map_id.clone(), // 带身份的请求不会让另一张活动地图换代。
                }); // 原样重试只设置运行时同一脏标记，不修改正式资产内容。
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bevy::prelude::{Handle, World};
    use feathrise_map_content::{
        MAP_SCHEMA_VERSION, MapCatalogAsset, MapContentHandles, MapManifestAsset, MapPortalsAsset,
    };

    use super::*;

    fn loaded_snapshot() -> (
        MapId,
        MapPlacementsAsset,
        Assets<MapPlacementsAsset>,
        MapContentRegistry,
    ) {
        let map_id = MapId::parse("MAP-placement-io-ready").expect("测试地图ID合法");
        let snapshot = MapPlacementsAsset {
            schema_version: MAP_SCHEMA_VERSION,
            map_id: map_id.clone(),
            placements: Vec::new(),
        };
        let mut assets = Assets::<MapPlacementsAsset>::default();
        let placements = assets.add(snapshot.clone()); // 模拟IO期间文件监听已经装入本次新快照。
        let registry = MapContentRegistry {
            catalog: Handle::<MapCatalogAsset>::default(),
            revision: 1,
            maps: BTreeMap::from([(
                map_id.clone(),
                MapContentHandles {
                    manifest: Handle::<MapManifestAsset>::default(),
                    portals: Handle::<MapPortalsAsset>::default(),
                    placements,
                    surface: None,
                    vegetation: None,
                    water: None,
                    characters_path: None, // 旧测试地图没有角色作者源。
                    boundaries: None,
                },
            )]),
            last_error: None,
        };
        (map_id, snapshot, assets, registry)
    }

    #[test]
    fn changed_disk_with_already_loaded_snapshot_still_rebuilds_after_the_gate() {
        let (map_id, snapshot, assets, registry) = loaded_snapshot();

        let changed = prepare_placement_publish(
            Some(&registry),
            &map_id,
            Some(&snapshot),
            &assets,
            true,
            None,
        );
        assert_eq!(changed.action, SavedPlacementsPublishAction::Rebuild); // 已同值只能说明监听发生过，不能证明建门后的新Ready。
        assert!(changed.handoff_required); // 保存成功仍必须等待显式重建产生严格更新修订。

        let unchanged = prepare_placement_publish(
            Some(&registry),
            &map_id,
            Some(&snapshot),
            &assets,
            false,
            None,
        );
        assert_eq!(unchanged.action, SavedPlacementsPublishAction::None); // 普通语义无变化保持正式资产完全静默。
        assert!(!unchanged.handoff_required);
    }

    #[test]
    fn unchanged_placements_with_deletions_rebuild_only_through_the_active_map_gate() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::authoring::{EditorAuthoringSession, placement_deletion::mark_deleted};

        let (map, placements, assets, registry) = loaded_snapshot();
        let mut world = World::new();
        world.init_resource::<EditorAuthoringSession>();
        let delete_map = map.clone();
        world.run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
            mark_deleted(&mut commands, &mut session, &delete_map, "added-then-deleted");
        }).unwrap();
        let saved_deletions = world.resource::<EditorAuthoringSession>().placement_deletions.snapshots();
        let mut saved = PlacementSaveHandoffSnapshot {
            map_id: map.clone(), baseline_revision: 7, baseline_attempt_revision: 11,
            retry_failed_runtime: false, editor_previews: Vec::new(), deleted: saved_deletions.clone(),
        };
        let preparation = prepare_placement_publish(Some(&registry), &map, Some(&placements), &assets, false, Some(&saved));
        assert_eq!(preparation.action, SavedPlacementsPublishAction::Rebuild, "新增后删除使磁盘同值，也要取得新Ready释放删除容量");
        assert!(preparation.handoff_required);
        let mut handoff = WaterFileHandoff::default();
        assert!(begin_placement_handoff(&mut handoff, &preparation, Some(&mut saved), Some(&map), &map,
            19, 7, 11, 0, Duration::ZERO).unwrap());
        assert!(saved.deleted.is_empty(), "只有当前图完整建门才接管删除确认快照");
        assert!(handoff.placement_handoff_in_flight_for(&map));

        let other = MapId::parse("MAP-placement-deletion-inactive").unwrap();
        let mut inactive_saved = PlacementSaveHandoffSnapshot {
            map_id: map.clone(), baseline_revision: 7, baseline_attempt_revision: 11,
            retry_failed_runtime: false, editor_previews: Vec::new(), deleted: saved_deletions,
        };
        let mut inactive_handoff = WaterFileHandoff::default();
        assert!(!begin_placement_handoff(&mut inactive_handoff, &preparation, Some(&mut inactive_saved), Some(&other), &map,
            19, 7, 11, 0, Duration::ZERO).unwrap());
        schedule_unclaimed_placement_previews(&mut inactive_handoff, Some(&mut inactive_saved)).unwrap();
        assert_eq!(inactive_saved.deleted.len(), 1, "非当前图不能借同值保存提前确认删除");
        assert_eq!(world.resource::<EditorAuthoringSession>().placement_deletions.snapshots().len(), 1);
        assert!(!inactive_handoff.placement_preview_cleanup_pending());
    }

    #[test]
    fn no_handoff_routes_previews_to_shared_queue_without_dropping_a_later_batch() {
        let map_id = MapId::parse("MAP-placement-preview-cleanup").expect("测试地图ID合法");
        let mut world = World::new();
        let first_root = world.spawn(crate::components::EditorPlacedAsset).id();
        let later_root = world.spawn(crate::components::EditorPlacedAsset).id();
        let first_preview = crate::authoring::resources::PlacementPreviewHandoffSnapshot::capture(&world, &map_id, "first", first_root).unwrap();
        let later_preview = crate::authoring::resources::PlacementPreviewHandoffSnapshot::capture(&world, &map_id, "later", later_root).unwrap();
        let mut first = PlacementSaveHandoffSnapshot {
            map_id: map_id.clone(),
            baseline_revision: 0,
            baseline_attempt_revision: 0,
            retry_failed_runtime: false,
            editor_previews: vec![first_preview],
            deleted: Vec::new(),
        };
        let mut handoff = WaterFileHandoff::default();

        schedule_unclaimed_placement_previews(&mut handoff, Some(&mut first))
            .expect("空清理槽必须接受无handoff批次");

        assert!(first.editor_previews.is_empty());
        assert!(handoff.placement_preview_cleanup_pending()); // 后续由64个世界操作/帧的系统统一消费。
        let mut later = PlacementSaveHandoffSnapshot {
            map_id,
            baseline_revision: 0,
            baseline_attempt_revision: 0,
            retry_failed_runtime: false,
            editor_previews: vec![later_preview],
            deleted: Vec::new(),
        };

        assert!(schedule_unclaimed_placement_previews(&mut handoff, Some(&mut later)).is_err());
        assert_eq!(later.editor_previews.len(), 1); // 队列背压时后续批次仍由原快照完整持有。
    }

    #[test]
    fn no_handoff_success_keeps_the_frozen_and_current_delete_unconfirmed() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::authoring::{EditorAuthoringSession, placement_deletion::mark_deleted};

        let map = MapId::parse("MAP-placement-ungated-delete").unwrap();
        let mut world = World::new();
        world.init_resource::<EditorAuthoringSession>();
        let delete_map = map.clone();
        world.run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
            mark_deleted(&mut commands, &mut session, &delete_map, "deleted-id");
        }).unwrap();
        let mut snapshot = PlacementSaveHandoffSnapshot {
            map_id: map,
            baseline_revision: 7,
            baseline_attempt_revision: 11,
            retry_failed_runtime: false,
            editor_previews: Vec::new(),
            deleted: world.resource::<EditorAuthoringSession>().placement_deletions.snapshots(),
        };
        let mut handoff = WaterFileHandoff::default();

        schedule_unclaimed_placement_previews(&mut handoff, Some(&mut snapshot)).unwrap();

        assert_eq!(snapshot.deleted.len(), 1, "无门成功不取得删除确认所有权");
        assert_eq!(world.resource::<EditorAuthoringSession>().placement_deletions.snapshots().len(), 1);
        assert!(!handoff.placement_preview_cleanup_pending(), "只有删除且没有严格Ready时不排确认任务");
    }
}
