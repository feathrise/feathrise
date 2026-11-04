//! 迟到视觉队列逐成员恢复：失效来源只淘汰对应项，真实父等待继续保留完整待同步集合。

use super::*;
use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce, world_serialization::WorldAsset};
use feathrise_map_content::{MapPlacementTerrainPatch, MapTerrainKind, MapTerrainObject, TerrainId, TerrainObjectTransform};
use std::sync::Arc;

fn fixture() -> (App, Entity, Entity, MapPlacementRecord, MapPlacementRecord, MapTerrainObject) {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
    source.id = TerrainId::parse("queue-patch-source").unwrap();
    source.terrain.kind = MapTerrainKind::SparseClipmap;
    source.transform = TerrainObjectTransform::default();
    draft.manifest.terrains = Some(Arc::from([source.clone()]));
    let mut plain = crate::viewport::placement_move::interaction_tests::record();
    plain.id = "queue-ordinary-member".into();
    plain.translation = [6.0, 2.0, 5.0];
    plain.water_surfaces.clear();
    let mut patch = plain.clone();
    patch.id = "queue-patch-member".into();
    patch.translation = [4.0, 3.0, 5.0];
    patch.terrain_patch = Some(MapPlacementTerrainPatch {
        terrain_id: Some(source.id.clone()), ..default()
    });
    draft.placements.placements = vec![patch.clone(), plain.clone()];
    draft.placement_water_surface_count = 0;
    let map = draft.map_id.clone();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<WorldAsset>()
        .init_resource::<PlacementInstanceIndex>()
        .init_resource::<CourtyardAuthoringState>()
        .add_observer(crate::ui::placement_water::register_placement_instance_root)
        .add_observer(crate::ui::placement_water::unregister_placement_instance_root)
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft), revision: 7, placement_structure_revision: 3, ..default()
        });
    let patch_root = app.world_mut().spawn((MapPlacementInstance::new(&map, &patch),
        Transform::from_xyz(1.0, 1.0, 1.0), Visibility::Inherited)).id();
    let plain_root = app.world_mut().spawn((MapPlacementInstance::new(&map, &plain),
        Transform::from_xyz(2.0, 2.0, 2.0), Visibility::Inherited)).id();
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    session.placement_previews.insert(patch.id.clone(), patch_root);
    session.placement_previews.insert(plain.id.clone(), plain_root);
    drop(session);
    app.world_mut().resource_scope(|world, mut state: Mut<CourtyardAuthoringState>| {
        queue_visual_sync(&mut state, world.resource::<EditorAuthoringSession>(),
            vec![(0, patch.clone(), true), (1, plain.clone(), true)]);
    });
    (app, patch_root, plain_root, patch, plain, source)
}

fn assert_plain_synced(app: &App, root: Entity, plain: &MapPlacementRecord) {
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let bounds = crate::authoring::placement_record_bounds(draft, plain).unwrap();
    assert_eq!(app.world().get::<Transform>(root), Some(&plain.validated_transform(&draft.map_id, bounds).unwrap()),
        "单个失效补丁不能丢失后续普通成员的已采用视觉状态");
}

#[test]
fn terrain_patch_visual_queue_invalid_source_preserves_later_ordinary_member() {
    for remove_source in [false, true] {
        let (mut app, patch_root, plain_root, patch, plain, source) = fixture();
        let patch_before = *app.world().get::<Transform>(patch_root).unwrap();
        let mut disabled = source;
        disabled.enabled = false;
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
            .manifest.terrains = Some(if remove_source { Arc::from([]) } else { Arc::from([disabled]) });
        app.world_mut().run_system_once(sync_visuals).unwrap();
        assert_plain_synced(&app, plain_root, &plain);
        assert_eq!(app.world().get::<Transform>(patch_root), Some(&patch_before));
        assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_previews.get(&patch.id), Some(&patch_root));
        assert!(!app.world().resource::<CourtyardAuthoringState>().visual_sync_pending());
    }
}

#[test]
fn terrain_patch_visual_queue_stale_item_with_missing_source_preserves_later_ordinary_member() {
    let (mut app, patch_root, plain_root, _, plain, _) = fixture();
    let patch_before = *app.world().get::<Transform>(patch_root).unwrap();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.placements.placements[0].translation[1] += 2.0; // 后来的单成员编辑使旧完整记录失效。
        draft.manifest.terrains = Some(Arc::from([]));
        session.revision += 1;
    }
    app.world_mut().run_system_once(sync_visuals).unwrap();
    assert_plain_synced(&app, plain_root, &plain);
    assert_eq!(app.world().get::<Transform>(patch_root), Some(&patch_before));
    assert!(!app.world().resource::<CourtyardAuthoringState>().visual_sync_pending());
}

#[test]
fn terrain_patch_visual_queue_waits_for_real_parent_without_losing_pending_members() {
    let (mut app, patch_root, plain_root, patch, plain, source) = fixture();
    let patch_before = *app.world().get::<Transform>(patch_root).unwrap();
    let plain_before = *app.world().get::<Transform>(plain_root).unwrap();
    for _ in 0..2 {
        app.world_mut().run_system_once(sync_visuals).unwrap();
        let state = app.world().resource::<CourtyardAuthoringState>();
        assert!(state.visual_sync_pending());
        assert_eq!(state.visual_sync.as_ref().unwrap().items.len(), 2);
        assert_eq!(app.world().get::<Transform>(patch_root), Some(&patch_before));
        assert_eq!(app.world().get::<Transform>(plain_root), Some(&plain_before));
        assert!(app.world().get::<ChildOf>(patch_root).is_none());
    }
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    let parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(
        app.world_mut(), &map, std::slice::from_ref(&source),
    )[0];
    app.world_mut().run_system_once(sync_visuals).unwrap();
    assert_plain_synced(&app, plain_root, &plain);
    let bounds = Rect::from_corners(source.world_min.into(), source.world_max.into());
    assert_eq!(app.world().get::<Transform>(patch_root), Some(&patch.validated_transform(&map, bounds).unwrap()));
    assert_eq!(app.world().get::<ChildOf>(patch_root).map(ChildOf::parent), Some(parent));
    assert!(!app.world().resource::<CourtyardAuthoringState>().visual_sync_pending());
}

/// 普通成员共用真实根注册及视觉入口，使预算、索引换代和切图测试都能检查实际ECS写入。
fn ordinary_queue_fixture(count: usize) -> (App, Vec<Entity>, Vec<MapPlacementRecord>) {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let records: Vec<_> = (0..count).map(|index| {
        let mut record = crate::viewport::placement_move::interaction_tests::record();
        record.id = format!("queue-ordinary-{index}");
        record.translation = [1.0 + index as f32 * 0.1, 2.0, 5.0];
        record.water_surfaces.clear();
        record
    }).collect();
    draft.placements.placements = records.clone();
    draft.placement_water_surface_count = 0;
    let map = draft.map_id.clone();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<WorldAsset>()
        .init_resource::<PlacementInstanceIndex>()
        .init_resource::<CourtyardAuthoringState>()
        .add_observer(crate::ui::placement_water::register_placement_instance_root)
        .add_observer(crate::ui::placement_water::unregister_placement_instance_root)
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft), revision: 7, placement_structure_revision: 3, ..default()
        });
    let roots: Vec<_> = records.iter().map(|record| {
        app.world_mut().spawn((MapPlacementInstance::new(&map, record),
            Transform::from_xyz(8.0, 9.0, 8.0), Visibility::Inherited)).id()
    }).collect();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        for (record, root) in records.iter().zip(&roots) {
            session.placement_previews.insert(record.id.clone(), *root);
        }
    }
    app.world_mut().resource_scope(|world, mut state: Mut<CourtyardAuthoringState>| {
        queue_visual_sync(&mut state, world.resource::<EditorAuthoringSession>(),
            records.iter().cloned().enumerate().map(|(index, record)| (index, record, true)).collect());
    });
    (app, roots, records)
}

#[test]
fn terrain_patch_visual_queue_limits_each_frame_to_sixteen_members() {
    let (mut app, roots, records) = ordinary_queue_fixture(17);
    let last_before = *app.world().get::<Transform>(roots[16]).unwrap();
    app.world_mut().run_system_once(sync_visuals).unwrap();
    for (root, record) in roots.iter().zip(&records).take(16) {
        assert_plain_synced(&app, *root, record);
    }
    assert_eq!(app.world().get::<Transform>(roots[16]), Some(&last_before),
        "首帧不得越过16个根的固定同步预算");
    assert_eq!(app.world().resource::<CourtyardAuthoringState>()
        .visual_sync.as_ref().unwrap().items.len(), 1);
    app.world_mut().run_system_once(sync_visuals).unwrap();
    assert_plain_synced(&app, roots[16], &records[16]);
    assert!(!app.world().resource::<CourtyardAuthoringState>().visual_sync_pending());
}

#[test]
fn terrain_patch_visual_queue_waits_for_rebuilt_index_and_relocates_stable_ids() {
    let (mut app, roots, records) = ordinary_queue_fixture(2);
    app.world_mut().run_system_once(advance_selection_index).unwrap();
    {
        let state = app.world().resource::<CourtyardAuthoringState>();
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(state.index.ready_for(session));
        assert_eq!(state.index.by_placement_id.get(&records[0].id), Some(&0));
    }
    let before: Vec<_> = roots.iter().map(|root| *app.world().get::<Transform>(*root).unwrap()).collect();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().placements.placements.swap(0, 1);
        session.revision += 1;
        session.placement_structure_revision += 1;
    }
    // 原索引仍就绪但属于旧结构：真实同步入口必须等待新索引，不能拿旧槽位淘汰待办。
    app.world_mut().run_system_once(sync_visuals).unwrap();
    for (root, transform) in roots.iter().zip(&before) {
        assert_eq!(app.world().get::<Transform>(*root), Some(transform));
    }
    assert_eq!(app.world().resource::<CourtyardAuthoringState>()
        .visual_sync.as_ref().unwrap().items.len(), 2);
    app.world_mut().run_system_once(advance_selection_index).unwrap();
    {
        let state = app.world().resource::<CourtyardAuthoringState>();
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(state.index.ready_for(session));
        assert_eq!(state.index.by_placement_id.get(&records[0].id), Some(&1));
        assert_eq!(state.index.by_placement_id.get(&records[1].id), Some(&0));
    }
    app.world_mut().run_system_once(sync_visuals).unwrap();
    for (root, record) in roots.iter().zip(&records) {
        assert_plain_synced(&app, *root, record);
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap()
        .placements.placements, vec![records[1].clone(), records[0].clone()]);
    assert!(!app.world().resource::<CourtyardAuthoringState>().visual_sync_pending());
}

#[test]
fn terrain_patch_visual_queue_discards_previous_map_without_writing_roots() {
    let (mut app, roots, records) = ordinary_queue_fixture(2);
    let old_map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    let before: Vec<_> = roots.iter().map(|root| *app.world().get::<Transform>(*root).unwrap()).collect();
    let mut next_draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    next_draft.map_id = MapId::parse("MAP-next-queue-map").unwrap();
    next_draft.manifest.id = next_draft.map_id.clone();
    next_draft.placements.map_id = next_draft.map_id.clone();
    // 使用同名成员覆盖新图，确保单靠稳定ID不能让旧队列跨地图写入。
    next_draft.placements.placements = records.clone();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft = Some(next_draft);
        session.revision += 1;
        session.placement_structure_revision += 1;
    }
    app.world_mut().run_system_once(sync_visuals).unwrap();
    assert!(app.world().resource::<CourtyardAuthoringState>().visual_sync.is_none());
    for ((root, transform), record) in roots.iter().zip(&before).zip(&records) {
        assert_eq!(app.world().get::<Transform>(*root), Some(transform));
        assert_eq!(app.world().get::<Visibility>(*root), Some(&Visibility::Inherited));
        let instance = app.world().get::<MapPlacementInstance>(*root).unwrap();
        assert!(instance.matches_record(&old_map, record, &record.water_surfaces));
        assert!(instance.is_active());
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap()
        .placements.placements, records);
}
