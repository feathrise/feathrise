//! 保存启动前整网格水身份、失败聚合与分帧记录索引回归。

use std::collections::BTreeMap; // 精确session预览所有权测试复用正式确定性索引类型。

use bevy::prelude::*; // App与Entity用于构造不启动渲染的根所有权夹具。
use feathrise_map_content::{
    MAP_SCHEMA_VERSION, MapId, MapPlacementRecord, MapPlacementWaterKind, MapPlacementWaterSurface,
    MapPlacementsAsset,
}; // 构造最小整网格水作者文档，不启动AssetServer或渲染。

use super::*; // 直接验证父模块保存门与私有分帧辅助。
use crate::authoring::resources::PlacementWaterSavePreflightCache; // 独立缓存夹具证明等待帧不重扫普通记录。

fn water_surface() -> MapPlacementWaterSurface {
    MapPlacementWaterSurface {
        mesh_index: 0,                          // 诊断索引不参与最终对象身份。
        node_path: vec!["河面对象".to_owned()], // 显式Blender节点路径满足公共校验。
        mesh_name: "河面网格".to_owned(),       // 稳定网格名称与节点路径组成最终匹配键。
        kind: MapPlacementWaterKind::Sloped,    // 测试斜河整网格样式。
        flow_speed: 2.0,                        // 合法米每秒流速。
        height_offset: 0.0,                     // 不额外改变测试水面高度。
    }
}

fn placement_record(id: impl Into<String>) -> MapPlacementRecord {
    MapPlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: id.into(),                           // 调用方指定稳定placement身份。
        asset_path: "gltf/river.glb".to_owned(), // 当前草稿精确Scene0来源。
        asset_target: Default::default(),        // 网格水证据只验证完整Scene0。
        building_preset_id: None,                // 保存水证据不依赖建筑目录。
        courtyard: None,                         // 独立摆放记录不属于任何院落副本。
        translation: [0.0, 0.0, 0.0],            // 原始作者平移快照。
        height_mode: feathrise_map_content::MapPlacementHeightMode::FixedHeight, // 保存水证据测试不等待R16跟随。
        terrain_offset: 0.0,                   // 固定高度不携带离地偏移。
        yaw_degrees: 0.0,                      // 原始作者Y轴旋转快照。
        scale: [1.0, 1.0, 1.0],                // 原始作者三轴缩放快照。
        water_surfaces: vec![water_surface()], // 当前记录包含一项非空整网格水绑定。
    }
}

fn placement_document() -> MapPlacementsAsset {
    let map_id = MapId::parse("MAP-placement-preflight").expect("测试地图ID合法");
    MapPlacementsAsset {
        schema_version: MAP_SCHEMA_VERSION, // 使用当前公共格式版本。
        map_id,                             // 文档与根证明必须严格绑定同一地图。
        placements: vec![placement_record("river-mesh")], // 单条记录足以覆盖状态聚合分支。
    }
}

fn readiness_for<'a>(
    document: &MapPlacementsAsset,
    roots: impl Iterator<
        Item = (
            &'a MapPlacementInstance,
            &'a DynamicWaterMeshOverrides,
            Option<&'a DynamicWaterMeshOverrideStatus>,
            bool,
        ),
    >,
) -> PlacementWaterSaveReadiness {
    placement_water_save_readiness(&document.map_id, &document.placements, roots) // 测试文档仅含非空记录，可直接作为缓存完成值。
}

#[test]
fn placement_water_save_preflight_requires_matching_ready_runtime_root() {
    let document = placement_document();
    let instance = MapPlacementInstance::new(&document.map_id, &document.placements[0]);
    let overrides = DynamicWaterMeshOverrides::new(vec![water_surface()]);
    let pending = DynamicWaterMeshOverrideStatus::Pending;
    let ready = DynamicWaterMeshOverrideStatus::Ready;

    assert!(matches!(
        readiness_for(&document, std::iter::empty()),
        PlacementWaterSaveReadiness::Failed(_)
    )); // 没有当前/候选/精确预览World根时不能先写入可能无效的节点引用。
    assert!(matches!(
        readiness_for(
            &document,
            std::iter::once((&instance, &overrides, Some(&pending), false)),
        ),
        PlacementWaterSaveReadiness::Waiting(_)
    )); // World或水材质验证尚未完成时保留保存请求等待。
    assert_eq!(
        readiness_for(
            &document,
            std::iter::once((&instance, &overrides, Some(&ready), false)),
        ),
        PlacementWaterSaveReadiness::Ready,
    ); // 完整记录快照Ready后才允许启动IO。
}

#[test]
fn placement_water_save_preflight_rejects_cross_map_and_different_source_snapshots() {
    let document = placement_document(); // 草稿属于MAP-placement-preflight和river.glb。
    let other_map = MapId::parse("MAP-other-preflight").expect("另一测试地图ID合法");
    let cross_map = MapPlacementInstance::new(&other_map, &document.placements[0]);
    let mut other_asset_record = document.placements[0].clone(); // 保持同ID、同水数组和同变换。
    other_asset_record.asset_path = "gltf/other-river.glb".to_owned(); // 只改变Scene0来源证明。
    let other_asset = MapPlacementInstance::new(&document.map_id, &other_asset_record);
    let mut other_transform_record = document.placements[0].clone(); // 保持同地图、ID、资产和水数组。
    other_transform_record.translation = [8.0, 0.0, 3.0]; // 只改变作者平移证明。
    let other_transform = MapPlacementInstance::new(&document.map_id, &other_transform_record);
    let overrides = DynamicWaterMeshOverrides::new(vec![water_surface()]);
    let ready = DynamicWaterMeshOverrideStatus::Ready;

    for rejected in [&cross_map, &other_asset, &other_transform] {
        assert!(matches!(
            readiness_for(
                &document,
                std::iter::once((rejected, &overrides, Some(&ready), false)),
            ),
            PlacementWaterSaveReadiness::Failed(_)
        )); // A/B同ID同water也不能跨MapId、不同GLB或不同变换借用Ready。
    }
}

#[test]
fn placement_water_root_scope_requires_active_formal_or_exact_session_preview() {
    let document = placement_document();
    let instance = MapPlacementInstance::new(&document.map_id, &document.placements[0]);
    let mut app = App::new(); // 只生成两个稳定Entity身份，不安装任何插件。
    let exact = app.world_mut().spawn_empty().id(); // 当前session明确拥有的预览实体。
    let stray = app.world_mut().spawn_empty().id(); // 同名但不在映射中的残留预览实体。
    let previews = BTreeMap::from([(instance.id().to_owned(), exact)]); // ID和Entity必须同时匹配。

    assert_eq!(
        placement_water_root_scope(exact, &instance, true, &previews),
        Some(PlacementWaterRootScope::SessionPreview),
    ); // 当前session精确预览可以提供未保存Scene0证明。
    assert_eq!(
        placement_water_root_scope(stray, &instance, true, &previews),
        None,
    ); // 同ID残留预览不能冒充当前作者会话。
    assert_eq!(
        placement_water_root_scope(stray, &instance, false, &previews),
        Some(PlacementWaterRootScope::ActiveFormal),
    ); // 没有EditorPlacedAsset标记的活动根属于当前正式批次。
}

#[test]
fn placement_water_save_preflight_surfaces_runtime_failure() {
    let document = placement_document();
    let instance = MapPlacementInstance::new(&document.map_id, &document.placements[0]);
    let overrides = DynamicWaterMeshOverrides::new(vec![water_surface()]);
    let failed = DynamicWaterMeshOverrideStatus::Failed("UV0缺失".to_owned());

    let PlacementWaterSaveReadiness::Failed(detail) = readiness_for(
        &document,
        std::iter::once((&instance, &overrides, Some(&failed), false)),
    ) else {
        panic!("明确运行时失败必须拒绝保存");
    };
    assert!(detail.contains("UV0缺失")); // 作者底栏直接取得GLB运行时的具体失败原因。
}

#[test]
fn placement_water_save_preflight_waits_when_failed_and_pending_roots_coexist() {
    let document = placement_document();
    let failed_instance = MapPlacementInstance::new(&document.map_id, &document.placements[0]);
    let pending_instance = MapPlacementInstance::new(&document.map_id, &document.placements[0]);
    let failed_overrides = DynamicWaterMeshOverrides::new(vec![water_surface()]);
    let pending_overrides = DynamicWaterMeshOverrides::new(vec![water_surface()]);
    let failed = DynamicWaterMeshOverrideStatus::Failed("旧预览缺少UV0".to_owned());
    let pending = DynamicWaterMeshOverrideStatus::Pending;

    assert!(matches!(
        readiness_for(
            &document,
            [
                (&failed_instance, &failed_overrides, Some(&failed), false),
                (&pending_instance, &pending_overrides, Some(&pending), false,),
            ]
            .into_iter(),
        ),
        PlacementWaterSaveReadiness::Waiting(_)
    )); // 同完整快照的新根仍可能成功，旧根失败不能提前终止自动保存等待。
}

#[test]
fn placement_water_save_preflight_stops_waiting_for_failed_scene_without_status() {
    let document = placement_document();
    let instance = MapPlacementInstance::new(&document.map_id, &document.placements[0]);
    let overrides = DynamicWaterMeshOverrides::new(vec![water_surface()]);

    let PlacementWaterSaveReadiness::Failed(detail) = readiness_for(
        &document,
        std::iter::once((&instance, &overrides, None, true)),
    ) else {
        panic!("Scene0明确失败后不得因缺少水状态永久等待");
    };
    assert!(detail.contains("Scene0或其依赖加载失败")); // WorldInstanceReady不会到达时也必须给作者可见的确定失败。
    assert!(detail.contains("gltf/river.glb")); // 诊断携带草稿路径，可直接定位损坏或缺失资产。
}

#[test]
fn placement_water_save_preflight_prefers_matching_ready_over_scene_failure() {
    let document = placement_document();
    let failed_instance = MapPlacementInstance::new(&document.map_id, &document.placements[0]);
    let ready_instance = MapPlacementInstance::new(&document.map_id, &document.placements[0]);
    let failed_overrides = DynamicWaterMeshOverrides::new(vec![water_surface()]);
    let ready_overrides = DynamicWaterMeshOverrides::new(vec![water_surface()]);
    let ready = DynamicWaterMeshOverrideStatus::Ready;

    assert_eq!(
        readiness_for(
            &document,
            [
                (&failed_instance, &failed_overrides, None, true),
                (&ready_instance, &ready_overrides, Some(&ready), false),
            ]
            .into_iter(),
        ),
        PlacementWaterSaveReadiness::Ready,
    ); // 同完整快照的另一真实Ready根已证明作者值可用，旧预览失败不得误拒保存。
}

#[test]
fn placement_water_record_cache_scans_large_plain_map_once_with_fixed_budget() {
    let map_id = MapId::parse("MAP-large-plain-preflight").expect("测试地图ID合法");
    let mut records = (0..8_191)
        .map(|index| {
            let mut record = placement_record(format!("plain-{index}")); // 复用合法结构后清空水绑定。
            record.water_surfaces.clear(); // 8191项普通placement不得进入根证明索引。
            record
        })
        .collect::<Vec<_>>();
    records.push(placement_record("only-water")); // 最后一项确保首帧不能靠提前命中结束扫描。
    let document = MapPlacementsAsset {
        schema_version: MAP_SCHEMA_VERSION, // 使用当前公共格式版本。
        map_id,                             // 缓存身份绑定此稳定地图。
        placements: records,                // 合法最大8192项作者记录。
    };
    let mut cache = PlacementWaterSavePreflightCache::default(); // 初始没有任何扫描信用。

    let first = advance_placement_water_record_cache(&mut cache, 41, 7, &document)
        .expect("合法文档首帧扫描成功");
    assert_eq!(
        first.processed,
        PLACEMENT_WATER_RECORD_SCAN_BUDGET_PER_FRAME,
    ); // 首帧只读取固定256项而不是同步扫描8192项。
    assert!(!first.complete); // 水记录位于末项，首帧不能发布不完整索引。
    assert!(cache.water_records.is_empty()); // 已检查前缀全部是普通placement。

    while !cache.complete {
        let progress = advance_placement_water_record_cache(&mut cache, 41, 7, &document)
            .expect("后续固定切片扫描成功");
        assert!(progress.processed <= PLACEMENT_WATER_RECORD_SCAN_BUDGET_PER_FRAME); // 每个等待帧都服从相同固定记录预算。
    }
    assert_eq!(cache.water_records.len(), 1); // 完成值只保留唯一非空水记录。
    assert_eq!(cache.water_records[0].id, "only-water"); // 后段记录没有遗漏。

    let reused = advance_placement_water_record_cache(&mut cache, 41, 7, &document)
        .expect("同作者修订复用完成缓存");
    assert_eq!(reused.processed, 0); // 水根Pending导致的后续等待帧不再重扫8192条普通记录。
    assert!(reused.complete); // 完成缓存可以直接进入最多256项根状态证明。
}

#[test]
fn multi_terrain_content_failure_cancels_save_and_close_with_error_receipt() {
    use crate::authoring::content_bindings::ContentTerrainFollowState;
    use crate::types::{EditorStatusKind, EditorStatusOperation};
    use feathrise_map_content::{MapTerrainDescriptor, TerrainId};

    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let map_id = draft.map_id.clone();
    let mut objects = draft.manifest.terrain_objects_for_edit().to_vec();
    let mut second = objects[0].clone();
    second.id = TerrainId::parse("second-terrain").unwrap();
    objects.push(second);
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    draft.manifest.terrains = Some(objects.into()); // 使用真实多地形容器，不借用旧单地形保存路径。

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<WorldAsset>()
        .insert_resource(MapEditorState {
            open: true,
            selected_map_id: Some(map_id.clone()),
            save_requested: true,
            close_requested: true,
            dirty: true,
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            ..default()
        })
        .init_resource::<DraftSaveTask>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<GlobalEnvironmentSaveTask>()
        .init_resource::<VegetationCompileState>()
        .init_resource::<MapDeletionTask>()
        .init_resource::<ActiveDynamicMap>()
        .init_resource::<DynamicWaterStatus>()
        .init_resource::<MapPlacementRuntimeStatus>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<TerrainPreviewState>()
        .init_resource::<TerrainLiveApplyState>()
        .init_resource::<PlacementTerrainFollowState>()
        .init_resource::<WaterFileHandoff>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<PlacementWaterEvidenceRegistry>()
        .init_resource::<ContentTerrainFollowState>()
        .add_systems(Update, start_draft_save);
    app.world_mut()
        .resource_mut::<ContentTerrainFollowState>()
        .failure = Some("关联地形second-terrain不存在".into());

    app.update(); // 运行正式保存入口；失败必须在冻结快照或启动磁盘任务之前收口。
    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.open && editor.dirty);
    assert!(!editor.save_requested && !editor.close_requested);
    let receipt = editor.status_results[EditorStatusOperation::MapSave.slot()]
        .as_ref()
        .expect("关联失败必须形成统一底栏可保留的保存回执");
    assert_eq!(receipt.kind, EditorStatusKind::Error);
    assert_eq!(receipt.map_id.as_ref(), Some(&map_id));
    assert!(receipt.detail.contains("second-terrain"));
    let sequence = receipt.sequence;
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(!session.saving);
    assert_eq!(session.revision, 7);
    assert_eq!(session.successful_save_sequence, 0);
    assert_eq!(session.draft.as_ref().unwrap().manifest.terrain_count(), 2);
    let save = app.world().resource::<DraftSaveTask>();
    assert!(save.task.is_none());
    assert_eq!(save.ticket, 0); // 未启动任何IO事务，也未推进保存票据。

    app.update();
    let editor = app.world().resource::<MapEditorState>();
    assert_eq!(
        editor.status_results[EditorStatusOperation::MapSave.slot()]
            .as_ref()
            .unwrap()
            .sequence,
        sequence,
    ); // 请求已停止，稳定失败帧不能重复发布错误或重新启动保存。
    assert!(app.world().resource::<DraftSaveTask>().task.is_none());
}
