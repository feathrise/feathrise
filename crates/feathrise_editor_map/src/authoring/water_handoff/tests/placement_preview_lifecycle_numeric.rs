//! 保存交接与真实数值事务的所有权回归；仅验证CPU实体生命周期，不替代文件保存或GPU加载验收。

use super::*;
use bevy::ecs::system::RunSystemOnce;
use feathrise_editor_ui::{
    ActiveNumericInput, NumericInputChanged, NumericInputCommitOutcome, NumericInputCommitSource,
    NumericInputCommitted, NumericInputScrubSensitivityChanged,
    NumericInputScrubSensitivityCommitted, NumericInputSetValue, NumericValue,
};

use crate::authoring::EditorCommand;
use crate::ui::gltf_browser::EditorGltfBrowserState;
use crate::ui::placement_transform::{
    PlacementTransformField, PlacementTransformInputState, handle_placement_transform_numeric_input,
};
use crate::ui::placement_water::{
    PlacementInstanceIndex, register_placement_instance_root, unregister_placement_instance_root,
};

pub(super) fn fixture(phase: MapPlacementRuntimePhase) -> (App, Entity, Entity, Entity, MapPlacementRecord) {
    let map = MapId::parse("MAP-placement-numeric-owner").unwrap();
    let record = MapPlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: "numeric-preview".into(),
        asset_path: "gltf/numeric-preview.glb".into(),
        asset_target: Default::default(),
        building_preset_id: None,
        courtyard: None,
        translation: [0.0; 3],
        height_mode: feathrise_map_content::MapPlacementHeightMode::FixedHeight,
        terrain_offset: 0.0,
        yaw_degrees: 0.0,
        scale: [1.0; 3],
        water_surfaces: vec![MapPlacementWaterSurface {
            mesh_index: 0,
            node_path: vec!["河面".into()],
            mesh_name: "河面".into(),
            kind: MapPlacementWaterKind::Sloped,
            flow_speed: 2.0,
            height_offset: 0.0,
        }],
    };
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.map_id = map.clone();
    draft.placements.map_id = map.clone();
    draft.placement_water_surface_count = 1;
    draft.placements.placements.push(record.clone());
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .add_message::<TerrainWaterRebuildRequest>()
        .add_message::<AssetEvent<MapManifestAsset>>()
        .add_message::<AssetLoadFailedEvent<MapManifestAsset>>()
        .add_message::<AssetEvent<MapWaterAsset>>()
        .add_message::<AssetLoadFailedEvent<MapWaterAsset>>()
        .add_message::<AssetEvent<MapWaterMaskAsset>>()
        .add_message::<AssetLoadFailedEvent<MapWaterMaskAsset>>()
        .add_message::<NumericInputChanged>()
        .add_message::<NumericInputCommitted>()
        .add_message::<NumericInputScrubSensitivityChanged>()
        .add_message::<NumericInputScrubSensitivityCommitted>()
        .add_message::<NumericInputSetValue>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<PlacementTransformInputState>()
        .init_resource::<PlacementInstanceIndex>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<DynamicWaterStatus>()
        .init_resource::<bevy::diagnostic::FrameCount>()
        .init_resource::<Time<Real>>()
        .add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root)
        .insert_resource(MapEditorState {
            open: true,
            selected_map_id: Some(map.clone()),
            placement_transform_enabled: true,
            ..default()
        })
        .insert_resource(ActiveDynamicMap { map_id: Some(map.clone()) });
    let root = app.world_mut().spawn((
        EditorPlacedAsset,
        MapPlacementInstance::new(&map, &record),
        Transform::IDENTITY,
        Visibility::Inherited,
        WorldAssetRoot(Handle::default()),
        DynamicWaterMeshOverrides::new(record.water_surfaces.clone()),
        DynamicWaterMeshOverrideStatus::Ready,
    )).id(); // 不运行World资产加载器，默认句柄只证明旧清理不能注销仍属于新版本的热更入口。
    let child = app.world_mut().spawn((ChildOf(root), Visibility::Inherited)).id();
    let input = app.world_mut().spawn(PlacementTransformField::TranslationY).id();
    let mut session = EditorAuthoringSession { draft: Some(draft), revision: 19, ..default() };
    session.placement_previews.insert(record.id.clone(), root);
    app.insert_resource(session);
    app.world_mut().resource_mut::<EditorGltfBrowserState>()
        .select_indexed_test_placement(&record.id, 0, 0);
    // 与保存启动时相同，在真实输入之前冻结本批实体；随后不能从当前索引重新解释旧批责任。
    let saved_previews = app.world().resource::<EditorAuthoringSession>().placement_previews
        .iter().map(|(id, entity)| PlacementPreviewHandoffSnapshot::capture(app.world(), &map, id, *entity)
            .expect("保存启动时只冻结真实存在的作者预览")).collect();
    app.insert_resource(WaterFileHandoff {
        placement: Some(PendingPlacementFileHandoff {
            gate: placement_gate(&map),
            editor_previews: saved_previews,
            deleted: Vec::new(),
        }),
        ..default()
    })
    .insert_resource(MapPlacementRuntimeStatus {
        phase,
        map_id: Some(map),
        revision: 7,
        attempt_revision: 12,
        progress_revision: 20,
        placement_count: 0,
        detail: "保存候选测试阶段".into(),
    })
    .add_systems(Update, (poll_water_file_handoff, cleanup_placement_handoff_previews).chain());
    (app, input, root, child, record)
}

fn commit_new_version(app: &mut App, input: Entity, root: Entity, before: &MapPlacementRecord)
    -> MapPlacementRecord {
    app.world_mut().write_message(NumericInputCommitted {
        entity: input,
        value: NumericValue::F32(6.0),
        outcome: NumericInputCommitOutcome::Accepted,
        source: NumericInputCommitSource::Text,
        changed: true,
        edited: true,
    });
    app.world_mut().run_system_once(handle_placement_transform_numeric_input).unwrap();
    let mut after = before.clone();
    after.translation[1] = 6.0;
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements[0], after);
    assert_eq!(session.revision, 20);
    assert_eq!(session.undo.len(), 1);
    assert!(matches!(session.undo.last(), Some(EditorCommand::PlacementChanged {
        record_index: 0, before: saved_before, after: saved_after,
    }) if saved_before == before && saved_after == &after));
    assert_eq!(session.placement_previews.get(&before.id), Some(&root),
        "真实数值同步必须原地更新同一预览，不能靠更换Entity绕开旧批所有权");
    let map = &session.draft.as_ref().unwrap().map_id;
    let instance = app.world().get::<MapPlacementInstance>(root).unwrap();
    let water = app.world().get::<DynamicWaterMeshOverrides>(root).unwrap().surfaces();
    assert!(instance.matches_record(map, &after, water));
    assert!(!instance.matches_record(map, before, water)); // 实例PartialEq只比较ID，不能证明新旧完整作者快照。
    assert_eq!(app.world().get::<Transform>(root), Some(&Transform::from_xyz(0.0, 6.0, 0.0)));
    assert!(app.world().resource::<MapEditorState>().dirty);
    after
}

fn adopt_old_saved_version(app: &mut App) {
    let mut status = app.world_mut().resource_mut::<MapPlacementRuntimeStatus>();
    status.phase = MapPlacementRuntimePhase::Ready;
    status.revision = 8;
    status.progress_revision = 21;
    drop(status);
    app.update(); // 保留attempt=12，严格越过旧门的r7/a11，实际消费生产poll与cleanup。
}

fn assert_new_version_survives(app: &App, root: Entity, child: Entity, after: &MapPlacementRecord) {
    assert!(app.world().get_entity(root).is_ok(), "旧保存Ready不能销毁已原地编辑的新作者版本");
    assert!(app.world().get::<WorldAssetRoot>(root).is_some());
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
    assert_eq!(app.world().get::<ChildOf>(child).map(ChildOf::parent), Some(root));
    assert_eq!(app.world().get::<Visibility>(child), Some(&Visibility::Inherited));
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.placement_previews.get(&after.id), Some(&root));
    assert_eq!(session.draft.as_ref().unwrap().placements.placements[0], *after);
    assert_eq!(session.revision, 20);
    assert_eq!(session.undo.len(), 1);
    assert!(app.world().resource::<WaterFileHandoff>().preview_cleanup.is_none(),
        "失去版本所有权的旧批应结束，不能永久占用清理槽");
}

pub(super) fn clean_current_saved_version(app: &mut App, root: Entity, child: Entity) {
    let session = app.world().resource::<EditorAuthoringSession>();
    let map = session.draft.as_ref().unwrap().map_id.clone();
    let saved_previews = session.placement_previews.iter()
        .map(|(id, entity)| PlacementPreviewHandoffSnapshot::capture(app.world(), &map, id, *entity)
            .expect("新保存必须从当前世界重新冻结版本，不能手工补上旧批缺失的票据")).collect();
    super::super::super::schedule_placement_preview_cleanup(
        &mut app.world_mut().resource_mut::<WaterFileHandoff>(), &map, saved_previews,
    ).expect("旧批已结束，新保存可以取得同一实体当前版本的清理责任");
    app.update();
    assert!(app.world().get_entity(root).is_err(), "新保存仍应正常回收自己冻结的未改预览");
    assert!(app.world().get_entity(child).is_err());
    assert!(app.world().resource::<EditorAuthoringSession>().placement_previews.is_empty());
    assert!(app.world().resource::<WaterFileHandoff>().preview_cleanup.is_none());
}

#[test]
fn failed_save_late_ready_keeps_same_entity_after_real_numeric_commit_and_new_save_can_clean_it() {
    let (mut app, input, root, child, before) = fixture(MapPlacementRuntimePhase::Failed);
    app.update();
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    assert!(app.world().resource::<WaterFileHandoff>().placement_retry_required_for(&map));
    assert!(!app.world().resource::<WaterFileHandoff>().placement_handoff_in_flight_for(&map));
    let after = commit_new_version(&mut app, input, root, &before);
    adopt_old_saved_version(&mut app);
    assert_new_version_survives(&app, root, child, &after);
    assert!(app.world().resource::<WaterFileHandoff>().unresolved.as_ref().unwrap()
        .late_placement.is_none());
    clean_current_saved_version(&mut app, root, child);
}

#[test]
fn pending_save_ready_keeps_same_entity_after_real_numeric_commit_and_new_save_can_clean_it() {
    let (mut app, input, root, child, before) = fixture(MapPlacementRuntimePhase::Building);
    app.update();
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    assert!(app.world().resource::<WaterFileHandoff>().placement_handoff_in_flight_for(&map));
    // 普通对象变换未被网格水交接门冻结；候选仍Building时也必须接受并保留新的数值事务。
    let after = commit_new_version(&mut app, input, root, &before);
    adopt_old_saved_version(&mut app);
    assert_new_version_survives(&app, root, child, &after);
    assert!(app.world().resource::<WaterFileHandoff>().placement.is_none());
    clean_current_saved_version(&mut app, root, child);
}

/// 保存候选建立前已经加载的对象只有正式根；去掉作者资格时必须保留真实生命周期索引。
fn keep_only_formal_root(app: &mut App, root: Entity) {
    let membership = app.world().resource::<PlacementInstanceIndex>().membership_revision();
    app.world_mut().entity_mut(root).remove::<EditorPlacedAsset>();
    app.world_mut().resource_mut::<EditorAuthoringSession>().placement_previews.clear();
    app.world_mut().resource_mut::<WaterFileHandoff>().placement.as_mut().unwrap().editor_previews.clear();
    assert!(app.world().get::<MapPlacementInstance>(root).unwrap().is_active());
    assert_eq!(app.world().resource::<PlacementInstanceIndex>().membership_revision(), membership,
        "撤掉作者标记不能顺带移除正式根索引，否则只会测到普通缺根补生路径");
}

fn commit_formal_numeric(app: &mut App, input: Entity, value: f32) {
    app.world_mut().write_message(NumericInputCommitted {
        entity: input,
        value: NumericValue::F32(value),
        outcome: NumericInputCommitOutcome::Accepted,
        source: NumericInputCommitSource::Text,
        changed: true,
        edited: true,
    });
    app.world_mut().run_system_once(handle_placement_transform_numeric_input).unwrap();
}

/// 草稿与历史先证明真实输入已被接受，再校验正式根未被当作可退役的作者预览使用。
fn assert_formal_numeric_created_preview(app: &App, formal: Entity, before: &MapPlacementRecord,
    after: &MapPlacementRecord) -> Entity {
    let session = app.world().resource::<EditorAuthoringSession>();
    let map = &session.draft.as_ref().unwrap().map_id;
    assert_eq!(session.draft.as_ref().unwrap().placements.placements[0], *after);
    assert_eq!(session.revision, 20);
    assert_eq!(session.undo.len(), 1);
    assert!(matches!(session.undo.last(), Some(EditorCommand::PlacementChanged {
        record_index: 0, before: old, after: new,
    }) if old == before && new == after));
    assert!(app.world().resource::<MapEditorState>().dirty);
    assert_eq!(app.world().get::<Transform>(formal), Some(&Transform::IDENTITY),
        "旧保存候选仍拥有正式批次；后续数值不能原地改写稍后会被退役的正式根");
    assert!(app.world().get::<MapPlacementInstance>(formal).unwrap()
        .matches_record(map, before, &before.water_surfaces),
        "正式根的作者记录证据必须保留旧版本，内置参数也不能原地覆写");
    let preview = *session.placement_previews.get(&before.id)
        .expect("真实数值输入须取得独立作者预览，旧保存采用后才能继续显示新版本");
    assert_ne!(preview, formal);
    assert!(app.world().get::<EditorPlacedAsset>(preview).is_some());
    assert!(app.world().get::<MapPlacementInstance>(preview).unwrap()
        .matches_record(map, after, &after.water_surfaces));
    assert_eq!(app.world().get::<Transform>(preview), Some(&Transform::from_translation(Vec3::from_array(after.translation))
        .with_rotation(Quat::from_rotation_y(after.yaw_degrees.to_radians()))
        .with_scale(Vec3::from_array(after.scale))));
    assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden));
    assert_eq!(app.world().get::<Visibility>(preview), Some(&Visibility::Inherited));
    preview
}

#[test]
fn formal_glb_numeric_commit_creates_independent_preview_without_mutating_formal_transform() {
    let (mut app, input, formal, child, before) = fixture(MapPlacementRuntimePhase::Building);
    keep_only_formal_root(&mut app, formal);
    commit_formal_numeric(&mut app, input, 6.0);
    let mut after = before.clone();
    after.translation[1] = 6.0;
    let preview = assert_formal_numeric_created_preview(&app, formal, &before, &after);
    assert!(app.world().get::<WorldAssetRoot>(formal).is_some(),
        "旧正式批次的热更和子树仍由场景退役流程拥有");
    assert!(app.world().get::<WorldAssetRoot>(preview).is_some());
    assert_eq!(app.world().get::<ChildOf>(child).map(ChildOf::parent), Some(formal));
    // 此处只驱动作者入口；真实候选采用及旧批退役由场景层回归另行证明，不能手改Ready冒充采用。
}

#[test]
fn formal_builtin_size_numeric_commit_creates_independent_preview_without_mutating_formal_visual() {
    let (mut app, input, formal, _, mut before) = fixture(MapPlacementRuntimePhase::Building);
    app.init_asset::<Mesh>().init_asset::<StandardMaterial>();
    before.asset_path.clear();
    before.asset_target = feathrise_map_content::MapPlacementAssetTarget::DEFAULT_CUBOID;
    before.water_surfaces.clear();
    let map = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.placement_water_surface_count = 0;
        draft.placements.placements[0] = before.clone();
        draft.map_id.clone()
    };
    app.world_mut().entity_mut(formal)
        .remove::<WorldAssetRoot>()
        .insert((MapPlacementInstance::new(&map, &before), DynamicWaterMeshOverrides::new(Vec::new())));
    let target = before.asset_target;
    app.world_mut().run_system_once(move |mut commands: Commands| {
        assert!(feathrise_scene::spawn_builtin_placement_visual(&mut commands, formal, target));
    }).unwrap();
    let formal_visual = app.world().get::<Children>(formal).unwrap().iter()
        .find(|child| app.world().get::<Mesh3d>(*child).is_some()).unwrap();
    let formal_mesh = app.world().get::<Mesh3d>(formal_visual).unwrap().0.clone();
    keep_only_formal_root(&mut app, formal);
    app.world_mut().entity_mut(input).insert(PlacementTransformField::CuboidSizeY);
    commit_formal_numeric(&mut app, input, 3.125);
    let mut after = before.clone();
    if let feathrise_map_content::MapPlacementAssetTarget::Cuboid { size_mm, .. } = &mut after.asset_target {
        size_mm[1] = 3_125;
    }
    let preview = assert_formal_numeric_created_preview(&app, formal, &before, &after);
    assert_eq!(app.world().get::<Mesh3d>(formal_visual).map(|mesh| &mesh.0), Some(&formal_mesh),
        "尺寸编辑不能替换正式批次的旧可视子节点；旧候选采用后仍须保留独立作者几何");
    let preview_visual = app.world().get::<Children>(preview).unwrap().iter()
        .find(|child| app.world().get::<Mesh3d>(*child).is_some()).unwrap();
    assert_ne!(app.world().get::<Mesh3d>(preview_visual).unwrap().0, formal_mesh);
}
