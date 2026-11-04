use bevy::asset::{AssetEvent, AssetLoadFailedEvent, AssetPlugin};
use bevy::prelude::*;
use feathrise_map_content::{
    ActiveDynamicMap, MapId, MapManifestAsset, MapPlacementRecord, MapPlacementWaterKind,
    MapPlacementWaterSurface, MapWaterAsset, MapWaterMaskAsset,
};
use feathrise_scene::{
    DynamicWaterMeshOverrideStatus, DynamicWaterMeshOverrides, DynamicWaterStatus,
    MapPlacementInstance, MapPlacementRuntimePhase, MapPlacementRuntimeStatus,
    TerrainWaterRebuildRequest,
};

use super::super::{
    PendingPlacementFileHandoff, WaterFileHandoff, cleanup_placement_handoff_previews,
    poll_water_file_handoff,
};
use super::placement_gate;
use crate::authoring::resources::{EditorAuthoringSession, PlacementPreviewHandoffSnapshot};
use crate::components::EditorPlacedAsset;
use crate::resources::MapEditorState;

#[path = "placement_preview_lifecycle_numeric.rs"]
mod numeric_ownership; // 真实数值事务原地更新预览后，旧保存批次不能再回收新作者版本。

#[path = "placement_preview_lifecycle_ownership.rs"]
mod ownership_boundaries; // 分别验证姿态、父归属、水版本及跨帧清理的真实作者写入资格。

#[test]
fn failed_candidate_keeps_retry_preview_and_late_ready_only_cleans_saved_entity() {
    let map_id = MapId::parse("MAP-placement-system-failure").expect("测试地图ID合法");
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default())); // 统一水交接参数需要真实AssetServer，但本摆放回归不加载文件或渲染。
    app.add_message::<TerrainWaterRebuildRequest>(); // 即使本批仅有摆放责任，系统初始化仍须注册定点水重建消息。
    app.add_message::<AssetEvent<MapManifestAsset>>()
        .add_message::<AssetLoadFailedEvent<MapManifestAsset>>()
        .add_message::<AssetEvent<MapWaterAsset>>()
        .add_message::<AssetLoadFailedEvent<MapWaterAsset>>()
        .add_message::<AssetEvent<MapWaterMaskAsset>>()
        .add_message::<AssetLoadFailedEvent<MapWaterMaskAsset>>();
    let surface = MapPlacementWaterSurface {
        mesh_index: 0,
        node_path: vec!["河面对象".to_owned()],
        mesh_name: "河面网格".to_owned(),
        kind: MapPlacementWaterKind::Sloped,
        flow_speed: 2.0,
        height_offset: 0.0,
    };
    let record = MapPlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: "river-mesh".to_owned(),
        asset_path: "gltf/river.glb".to_owned(),
        asset_target: Default::default(), // 整网格水预览明确使用完整Scene0。
        building_preset_id: None,         // 水交接夹具保持普通显式摆放。
        courtyard: None,                  // 独立摆放记录不属于任何院落副本。
        translation: [0.0; 3],
        height_mode: feathrise_map_content::MapPlacementHeightMode::FixedHeight, // 预览交接夹具保持旧世界Y语义。
        terrain_offset: 0.0, // 固定高度不携带离地偏移。
        yaw_degrees: 0.0,
        scale: [1.0; 3],
        water_surfaces: vec![surface.clone()],
    };
    let mut draft =
        crate::authoring::terrain::tests::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.map_id = map_id.clone();
    draft.placements.map_id = map_id.clone();
    draft.placements.placements.push(record.clone()); // 预览根与草稿记录共享同一完整作者快照。
    let preview = app
        .world_mut()
        .spawn((
            EditorPlacedAsset,
            MapPlacementInstance::new(&map_id, &record),
            DynamicWaterMeshOverrides::new(vec![surface]),
            DynamicWaterMeshOverrideStatus::Ready,
        ))
        .id(); // 保存前已通过真实Scene0验证的预览必须跨失败继续存在。
    let mut session = EditorAuthoringSession {
        draft: Some(draft),
        revision: 19,
        ..Default::default()
    };
    session
        .placement_previews
        .insert("river-mesh".to_owned(), preview);
    app.insert_resource(session);
    let saved_preview = PlacementPreviewHandoffSnapshot::capture(app.world(), &map_id, "river-mesh", preview)
        .expect("保存前从实际预览冻结完整版本所有权");
    app.insert_resource(WaterFileHandoff {
        pending: None,
        placement: Some(PendingPlacementFileHandoff {
            gate: placement_gate(&map_id),
            editor_previews: vec![saved_preview],
            deleted: Vec::new(),
        }),
        unresolved: None,
        preview_cleanup: None,
    });
    app.insert_resource(MapEditorState {
        open: true,
        close_requested: true,
        dirty: false,
        ..Default::default()
    });
    app.insert_resource(ActiveDynamicMap {
        map_id: Some(map_id.clone()),
    });
    app.init_resource::<DynamicWaterStatus>();
    app.insert_resource(MapPlacementRuntimeStatus {
        phase: MapPlacementRuntimePhase::Failed,
        map_id: Some(map_id.clone()),
        revision: 7,
        attempt_revision: 12,
        progress_revision: 20,
        placement_count: 0,
        detail: "目标网格缺少UV0".to_owned(),
    });
    app.init_resource::<bevy::diagnostic::FrameCount>();
    app.init_resource::<Time<Real>>();
    app.add_systems(
        Update,
        (poll_water_file_handoff, cleanup_placement_handoff_previews).chain(),
    );

    app.update();

    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.dirty);
    assert!(!editor.close_requested);
    assert!(editor.status.contains("UV0"));
    let handoff = app.world().resource::<WaterFileHandoff>();
    assert!(handoff.pending_for(&map_id, 0, 0));
    assert!(handoff.placement_retry_required_for(&map_id));
    assert!(app.world().get_entity(preview).is_ok()); // Failed不销毁也不重建原预览。
    let document = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .expect("草稿仍存在")
        .placements
        .clone();
    let instance = app
        .world()
        .get::<MapPlacementInstance>(preview)
        .expect("保留根携带稳定ID")
        .clone();
    let overrides = app
        .world()
        .get::<DynamicWaterMeshOverrides>(preview)
        .expect("保留根携带当前水覆盖")
        .clone();
    let ready = DynamicWaterMeshOverrideStatus::Ready;
    assert_eq!(
        crate::authoring::save::placement_water_save_readiness(
            &document.map_id,
            &document.placements,
            std::iter::once((&instance, &overrides, Some(&ready), false)),
        ),
        crate::authoring::save::PlacementWaterSaveReadiness::Ready,
    ); // 原Ready预览天然恢复重试预检，不再落入“没有可验证Scene0根”的死路。

    let later_preview = app.world_mut().spawn(EditorPlacedAsset).id();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .placement_previews
        .insert("river-mesh".to_owned(), later_preview);
    *app.world_mut().resource_mut::<MapPlacementRuntimeStatus>() = MapPlacementRuntimeStatus {
        phase: MapPlacementRuntimePhase::Ready,
        map_id: Some(map_id),
        revision: 8,
        attempt_revision: 12,
        progress_revision: 21,
        placement_count: 1,
        detail: "迟到候选已采用".to_owned(),
    };
    app.update();

    assert!(app.world().get_entity(preview).is_err());
    assert!(app.world().get_entity(later_preview).is_ok());
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .placement_previews
            .get("river-mesh"),
        Some(&later_preview),
    ); // 严格新Ready只清本批实体，同ID后来预览和索引保持不变。
}
