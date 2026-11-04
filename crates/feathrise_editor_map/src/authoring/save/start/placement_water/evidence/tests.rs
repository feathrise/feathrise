//! 保存证据缓存的事件唤醒、固定预算与生命周期回归。

use std::collections::BTreeMap;

use bevy::asset::{AssetApp, AssetEvent, AssetId, AssetPlugin, uuid::Uuid};
use bevy::ecs::message::MessageCursor; // 测试使用与正式保存相同的可检测丢失显式游标。
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot};
use feathrise_map_content::{
    MAP_SCHEMA_VERSION, MapId, MapPlacementRecord, MapPlacementWaterKind, MapPlacementWaterSurface,
    MapPlacementsAsset,
};
use feathrise_scene::{
    DynamicWaterMeshOverrideStatus, DynamicWaterMeshOverrides, MapPlacementInstance,
    MapPlacementRuntimePhase, MapPlacementRuntimeStatus, MapPlacementWaterEvidence,
};

use super::{
    PlacementWaterEvidenceAdvance, advance_placement_water_evidence_cache,
    placement_water_cached_readiness,
};
use crate::authoring::resources::{
    PlacementWaterEvidenceRegistry, PlacementWaterSavePreflightCache,
};
use crate::authoring::save::start::PlacementWaterSaveReadiness;
use crate::components::EditorPlacedAsset;

#[derive(Resource)]
struct TestCache(PlacementWaterSavePreflightCache);

#[derive(Resource)]
struct TestInput {
    map_id: MapId,
    session_previews: BTreeMap<String, Entity>,
}

#[derive(Resource, Default)]
struct TestProbe {
    advances: Vec<PlacementWaterEvidenceAdvance>,
}

#[derive(Resource, Default)]
struct TestAssetEventBurst(Vec<AssetEvent<WorldAsset>>); // PreUpdate写入模拟本帧资产系统产生的大批消息。

fn emit_test_asset_event_burst(
    mut burst: ResMut<TestAssetEventBurst>, // 单次夹具写完后自动为空，下一帧不会重复Modified。
    mut events: ResMut<Messages<AssetEvent<WorldAsset>>>, // 与AssetServer共用正式类型化消息集合。
) {
    for event in std::mem::take(&mut burst.0) {
        events.write(event); // 消息在First维护之后写入，保存Update本帧和下一帧各有一次读取机会。
    }
}

fn water_surface() -> MapPlacementWaterSurface {
    MapPlacementWaterSurface {
        mesh_index: 0,
        node_path: vec!["河面对象".to_owned()],
        mesh_name: "河面网格".to_owned(),
        kind: MapPlacementWaterKind::Sloped,
        flow_speed: 2.0,
        height_offset: 0.0,
    }
}

fn placement_record(id: &str, with_water: bool) -> MapPlacementRecord {
    MapPlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: id.to_owned(),
        asset_path: "gltf/river.glb".to_owned(),
        asset_target: Default::default(), // 证据根必须对应完整Scene0。
        building_preset_id: None,         // 水证据夹具保持普通摆放身份。
        courtyard: None,                  // 独立摆放记录不属于任何院落副本。
        translation: [0.0, 0.0, 0.0],
        height_mode: feathrise_map_content::MapPlacementHeightMode::FixedHeight, // 证据聚合测试不依赖R16。
        terrain_offset: 0.0, // 固定高度不携带离地偏移。
        yaw_degrees: 0.0,
        scale: [1.0, 1.0, 1.0],
        water_surfaces: with_water.then(|| water_surface()).into_iter().collect(),
    }
}

#[allow(clippy::type_complexity)]
fn drive_evidence_cache(
    mut cache: ResMut<TestCache>,
    registry: Res<PlacementWaterEvidenceRegistry>,
    runtime: Res<MapPlacementRuntimeStatus>,
    input: Res<TestInput>,
    asset_server: Res<AssetServer>,
    scene_events: Res<Messages<AssetEvent<WorldAsset>>>, // 测试读取正式双缓冲消息集合。
    mut scene_event_cursor: Local<MessageCursor<AssetEvent<WorldAsset>>>, // 显式游标验证积压代与丢失检测。
    roots: Query<
        (
            &MapPlacementInstance,
            &DynamicWaterMeshOverrides,
            Option<&DynamicWaterMeshOverrideStatus>,
            &WorldAssetRoot,
            Option<&EditorPlacedAsset>,
        ),
        With<MapPlacementWaterEvidence>,
    >,
    mut probe: ResMut<TestProbe>,
) {
    let _ = super::super::registry::consume_relevant_world_asset_events(
        &scene_events,
        &mut scene_event_cursor,
        &mut cache.0,
        registry.status_revision,
        true,
    )
    .expect("普通回归不应淘汰显式游标消息"); // 测试始终模拟仍保持的保存请求，相关事件必须立即锁存失效。
    let advance = advance_placement_water_evidence_cache(
        &mut cache.0,
        &registry,
        &runtime,
        &input.map_id,
        &roots,
        &asset_server,
        &input.session_previews,
    );
    probe.advances.push(advance);
}

fn build_app(status: DynamicWaterMeshOverrideStatus) -> (App, Entity) {
    let map_id = MapId::parse("MAP-evidence-cache").expect("测试地图ID合法");
    let record = placement_record("river-mesh", true);
    let document = MapPlacementsAsset {
        schema_version: MAP_SCHEMA_VERSION,
        map_id: map_id.clone(),
        placements: vec![record.clone()],
    };
    let mut cache = PlacementWaterSavePreflightCache::default();
    let scan = super::super::advance_placement_water_record_cache(&mut cache, 11, 7, &document)
        .expect("单条合法记录缓存成功");
    assert!(scan.complete);

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    app.init_asset::<WorldAsset>(); // 正式AssetEvent消息队列由类型化资产注册创建。
    app.init_resource::<PlacementWaterEvidenceRegistry>();
    app.insert_resource(MapPlacementRuntimeStatus {
        phase: MapPlacementRuntimePhase::Ready,
        map_id: Some(map_id.clone()),
        revision: 3,
        attempt_revision: 3,
        ..default()
    });
    app.insert_resource(TestCache(cache));
    app.insert_resource(TestInput {
        map_id: map_id.clone(),
        session_previews: BTreeMap::new(),
    });
    app.init_resource::<TestProbe>();
    app.add_observer(super::super::registry::register_placement_water_evidence_root);
    app.add_observer(super::super::registry::unregister_placement_water_evidence_root);
    app.add_observer(super::super::registry::observe_added_placement_water_status);
    app.add_observer(super::super::registry::observe_replaced_placement_water_status);
    app.add_observer(super::super::registry::observe_removed_placement_water_status);
    app.add_systems(Update, drive_evidence_cache);
    let root = app
        .world_mut()
        .spawn((
            MapPlacementInstance::new(&map_id, &record),
            DynamicWaterMeshOverrides::new(vec![water_surface()]),
            status,
            WorldAssetRoot(Handle::<WorldAsset>::default()),
            MapPlacementWaterEvidence,
        ))
        .id();
    (app, root)
}

fn readiness(app: &App) -> PlacementWaterSaveReadiness {
    placement_water_cached_readiness(&app.world().resource::<TestCache>().0)
}

#[test]
fn stable_pending_only_polls_the_single_unresolved_entity() {
    let (mut app, _) = build_app(DynamicWaterMeshOverrideStatus::Pending);
    app.update();
    assert!(matches!(
        readiness(&app),
        PlacementWaterSaveReadiness::Waiting(_)
    ));
    for _ in 0..300 {
        app.update();
    }
    let advances = &app.world().resource::<TestProbe>().advances;
    assert_eq!(advances[0].roots_processed, 1);
    assert!(
        advances[1..]
            .iter()
            .all(|advance| advance.roots_processed == 0)
    );
    assert!(
        advances[1..]
            .iter()
            .all(|advance| advance.candidates_polled == 1)
    );
    assert!(matches!(
        readiness(&app),
        PlacementWaterSaveReadiness::Waiting(_)
    ));
}

#[test]
fn ordinary_roots_do_not_enter_the_water_evidence_registry() {
    let (mut app, _) = build_app(DynamicWaterMeshOverrideStatus::Pending);
    let map_id = app.world().resource::<TestInput>().map_id.clone();
    let plain_record = placement_record("plain", false);
    for _ in 0..8_192 {
        app.world_mut().spawn((
            MapPlacementInstance::new(&map_id, &plain_record),
            DynamicWaterMeshOverrides::new(Vec::new()),
            WorldAssetRoot(Handle::<WorldAsset>::default()),
        ));
    }
    assert_eq!(
        app.world()
            .resource::<PlacementWaterEvidenceRegistry>()
            .total_count,
        1,
    );
    app.update();
    let first = app.world().resource::<TestProbe>().advances[0];
    assert_eq!(first.roots_processed, 1);
    assert_eq!(first.roots_total, 1);
}

#[test]
fn status_replacement_wakes_ready_and_failed_without_global_rescan() {
    let (mut app, root) = build_app(DynamicWaterMeshOverrideStatus::Pending);
    app.update();
    app.world_mut()
        .entity_mut(root)
        .insert(DynamicWaterMeshOverrideStatus::Ready);
    app.update();
    assert_eq!(readiness(&app), PlacementWaterSaveReadiness::Ready);
    let ready_advance = *app.world().resource::<TestProbe>().advances.last().unwrap();
    assert_eq!(ready_advance.roots_processed, 0);
    assert_eq!(ready_advance.candidates_polled, 1);

    app.world_mut()
        .entity_mut(root)
        .insert(DynamicWaterMeshOverrideStatus::Failed("UV0缺失".to_owned()));
    app.update();
    let PlacementWaterSaveReadiness::Failed(detail) = readiness(&app) else {
        panic!("Changed状态必须定向唤醒并发布失败");
    };
    assert!(detail.contains("UV0缺失"));
    let failed_advance = *app.world().resource::<TestProbe>().advances.last().unwrap();
    assert_eq!(failed_advance.roots_processed, 0);
    assert_eq!(failed_advance.candidates_polled, 1);
}

#[test]
fn modified_asset_revokes_old_ready_until_a_new_status_lifecycle_arrives() {
    let (mut app, root) = build_app(DynamicWaterMeshOverrideStatus::Ready); // 初始根已经完整通过上一轮运行时验证。
    app.update(); // 首帧完成证据聚合并建立Scene0引用计数。
    assert_eq!(readiness(&app), PlacementWaterSaveReadiness::Ready); // 证明测试确实从可启动保存状态开始。
    let asset_id = app
        .world()
        .entity(root)
        .get::<WorldAssetRoot>()
        .expect("测试根必须持有Scene0强句柄")
        .0
        .id(); // Modified必须命中候选真实引用的同一类型化资产ID。
    app.world_mut()
        .resource_mut::<Messages<AssetEvent<WorldAsset>>>()
        .write(AssetEvent::Modified { id: asset_id }); // 模拟Blender保存后同InstanceId的普通GLB热重载。

    app.update(); // 保存预检在SpawnScene重建前先消费资产事件。
    assert!(matches!(
        readiness(&app),
        PlacementWaterSaveReadiness::Waiting(_)
    )); // 重新读取到旧Status::Ready也必须被资产代际门降为Pending。
    for _ in 0..4 {
        app.update(); // 强制候选周期完成后继续等待若干稳定帧。
    }
    assert!(matches!(
        readiness(&app),
        PlacementWaterSaveReadiness::Waiting(_)
    )); // 没有新状态事件时旧Ready永远不能重新启动IO。

    app.world_mut()
        .entity_mut(root)
        .insert(DynamicWaterMeshOverrideStatus::Pending); // scene侧Modified前置失效提供事件后的新Pending。
    app.update(); // 候选消费晚于资产下限的自身状态序号。
    assert!(matches!(
        readiness(&app),
        PlacementWaterSaveReadiness::Waiting(_)
    )); // 新生命周期已经授信，但真实水体仍未完成。
    app.world_mut()
        .entity_mut(root)
        .insert(DynamicWaterMeshOverrideStatus::Ready); // 新WorldInstanceReady后的完整水体预检发布新Ready。
    app.update(); // 定向状态事件只复验这一根。
    assert_eq!(readiness(&app), PlacementWaterSaveReadiness::Ready); // 新生命周期收敛后保存资格恢复。
}

#[test]
fn delayed_modified_uses_the_backlog_start_floor_instead_of_the_late_ready_tail() {
    let (mut app, root) = build_app(DynamicWaterMeshOverrideStatus::Ready); // 初始证据来自上一版完整Scene0。
    app.update(); // 建立候选资产引用计数与初始状态实体序号。
    let target_asset_id = app
        .world()
        .entity(root)
        .get::<WorldAssetRoot>()
        .expect("测试根必须持有Scene0强句柄")
        .0
        .id(); // 第65条消息才放入真正相关的Modified。
    let mut burst = Vec::with_capacity(65); // 一次写入超过保存读者单帧64条预算。
    for index in 0..64_u128 {
        burst.push(AssetEvent::Modified {
            id: AssetId::Uuid {
                uuid: Uuid::from_u128(index + 1), // 前64条均为与当前水根无关的互异Scene0。
            },
        });
    }
    burst.push(AssetEvent::Modified {
        id: target_asset_id, // 目标事件必须留到下一Update才能被保存读者看到。
    });
    app.insert_resource(TestAssetEventBurst(burst)); // PreUpdate生产时序与真实资产系统一致。
    app.add_systems(PreUpdate, emit_test_asset_event_burst); // Update保存读者随后按64条预算消费。

    app.update(); // 首批只消费64条，并冻结本段积压开始时的状态尾部。
    assert!(
        app.world()
            .resource::<TestCache>()
            .0
            .scene_asset_event_backlog_status_floor
            .is_some()
    ); // 未追平前积压代下限必须跨帧保留。
    app.world_mut()
        .entity_mut(root)
        .insert(DynamicWaterMeshOverrideStatus::Pending); // Bevy spawner已在保存读者追到后段前开始新生命周期。
    app.world_mut()
        .entity_mut(root)
        .insert(DynamicWaterMeshOverrideStatus::Ready); // 新WorldReady又先于第65条消息读取消费完成。

    app.update(); // 最终批现在才读到目标Modified，但必须沿用首批状态下限。

    assert_eq!(readiness(&app), PlacementWaterSaveReadiness::Ready); // 已发生的新Ready严格越过旧下限，不会永久Pending。
    assert!(
        app.world()
            .resource::<TestCache>()
            .0
            .scene_asset_event_backlog_status_floor
            .is_none()
    ); // 最终批追平后才清除连续积压身份。
}

#[test]
fn dropped_asset_event_backlog_is_reported_instead_of_appearing_caught_up() {
    let mut events = Messages::<AssetEvent<WorldAsset>>::default(); // 独立构造Bevy正式双缓冲消息集合。
    let mut cursor = MessageCursor::<AssetEvent<WorldAsset>>::default(); // 游标故意停在消息零号之前。
    for index in 0..129_u128 {
        events.write(AssetEvent::Modified {
            id: AssetId::Uuid {
                uuid: Uuid::from_u128(index + 1), // 超过两帧持续积压的互异事件不能被静默忽略。
            },
        });
    }
    events.update(); // 第一轮交换后消息仍保留在旧缓冲。
    events.update(); // 第二轮交换淘汰全部未读消息，模拟保存读者长期落后。
    let mut cache = PlacementWaterSavePreflightCache::default(); // 即使没有候选索引，消息身份丢失也必须先报告。

    let result = super::super::registry::consume_relevant_world_asset_events(
        &events,
        &mut cursor,
        &mut cache,
        0,
        true,
    ); // 正式入口必须在read自动校正游标前读取missed_messages。

    assert_eq!(result, Err(129)); // 不能把空的现存缓冲误判为“已经安全追平”。
    assert!(cursor.is_empty(&events)); // 当前失败结束后游标O(1)追尾，下一次人工保存从新事件开始。
    assert!(cache.scene_asset_invalidation_pending); // 同作者修订重试也必须先完整复验候选，不能直接复用旧Ready。
}

#[test]
fn marker_removal_and_despawn_immediately_revoke_ready_credit() {
    for despawn in [false, true] {
        let (mut app, root) = build_app(DynamicWaterMeshOverrideStatus::Ready);
        app.update();
        assert_eq!(readiness(&app), PlacementWaterSaveReadiness::Ready);
        if despawn {
            app.world_mut().despawn(root);
        } else {
            app.world_mut()
                .entity_mut(root)
                .remove::<MapPlacementWaterEvidence>();
        }
        app.update();
        assert_ne!(readiness(&app), PlacementWaterSaveReadiness::Ready);
        for _ in 0..8 {
            if !app
                .world()
                .resource::<TestProbe>()
                .advances
                .last()
                .unwrap()
                .collecting
            {
                break;
            }
            app.update();
        }
        assert!(matches!(
            readiness(&app),
            PlacementWaterSaveReadiness::Failed(_)
        ));
    }
}
