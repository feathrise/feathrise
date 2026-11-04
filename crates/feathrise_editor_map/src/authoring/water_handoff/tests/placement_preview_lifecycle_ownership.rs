//! 精确保存版本的独立所有权边界，以及跨帧回收后普通数值输入的真实根索引资格。

use super::*;
use super::numeric_ownership::{clean_current_saved_version, fixture};
use bevy::ecs::system::RunSystemOnce;
use feathrise_editor_ui::{NumericInputCommitOutcome, NumericInputCommitSource, NumericInputCommitted, NumericValue};

use crate::ui::placement_transform::handle_placement_transform_numeric_input;
use crate::ui::placement_water::PlacementInstanceIndex;

fn take_saved_batch(app: &mut App) -> (MapId, Vec<PlacementPreviewHandoffSnapshot>) {
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    let previews = app.world_mut().resource_mut::<WaterFileHandoff>().placement.take().unwrap()
        .editor_previews;
    (map, previews)
}

fn cleanup_saved_batch(app: &mut App, map: &MapId, previews: Vec<PlacementPreviewHandoffSnapshot>) {
    super::super::super::schedule_placement_preview_cleanup(
        &mut app.world_mut().resource_mut::<WaterFileHandoff>(), map, previews,
    ).expect("边界测试只移交一个真实捕获的批次");
    app.update();
}

fn assert_retained(app: &App, root: Entity, child: Entity, id: &str) {
    assert!(app.world().get::<WorldAssetRoot>(root).is_some(), "旧批不能注销新所有权的热更入口");
    assert!(app.world().get::<MapPlacementInstance>(root).is_some());
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
    assert_eq!(app.world().get::<ChildOf>(child).map(ChildOf::parent), Some(root));
    assert_eq!(app.world().get::<Visibility>(child), Some(&Visibility::Inherited));
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_previews.get(id), Some(&root));
    assert!(app.world().resource::<WaterFileHandoff>().preview_cleanup.is_none());
}

fn active_index_roots(app: &mut App, map: &MapId, id: &str) -> Vec<Entity> {
    let map = map.clone();
    let id = id.to_owned();
    app.world_mut().run_system_once(move |index: Res<PlacementInstanceIndex>,
        roots: Query<(Entity, &MapPlacementInstance)>| {
        let mut found = Vec::new();
        index.for_each_active_root(&map, &id, &roots, |entity| found.push(entity));
        found
    }).unwrap() // 通过普通作者写入共用的索引接口观察资格，不能只检查私有清理游标。
}

#[test]
fn transform_only_preview_change_revokes_old_cleanup_and_new_capture_can_reclaim_it() {
    let (mut app, _, root, child, record) = fixture(MapPlacementRuntimePhase::Building);
    let (map, saved) = take_saved_batch(&mut app);
    let version = app.world().get::<MapPlacementInstance>(root).unwrap().authoring_version();
    app.world_mut().get_mut::<Transform>(root).unwrap().translation.y = 6.0;
    cleanup_saved_batch(&mut app, &map, saved);
    assert_retained(&app, root, child, &record.id);
    assert_eq!(app.world().get::<Transform>(root).unwrap().translation.y, 6.0);
    assert_eq!(app.world().get::<MapPlacementInstance>(root).unwrap().authoring_version(), version,
        "只改Transform也应丢弃旧批，不能把实例票据变更当成唯一保护");
    clean_current_saved_version(&mut app, root, child);
}

#[test]
fn parent_only_preview_change_revokes_old_cleanup_and_preserves_new_parent_tree() {
    let (mut app, _, root, child, record) = fixture(MapPlacementRuntimePhase::Building);
    let (map, saved) = take_saved_batch(&mut app);
    let parent = app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited)).id();
    app.world_mut().entity_mut(root).insert(ChildOf(parent));
    cleanup_saved_batch(&mut app, &map, saved);
    assert_retained(&app, root, child, &record.id);
    assert_eq!(app.world().get::<ChildOf>(root).map(ChildOf::parent), Some(parent));
    clean_current_saved_version(&mut app, root, child);
    assert!(app.world().get_entity(parent).is_ok(), "新保存只能回收预览子树，不能吞掉当前所属父");
}

#[test]
fn water_numeric_component_version_alone_revokes_old_preview_cleanup() {
    let (mut app, _, root, child, record) = fixture(MapPlacementRuntimePhase::Building);
    let (map, saved) = take_saved_batch(&mut app);
    let instance_version = app.world().get::<MapPlacementInstance>(root).unwrap().authoring_version();
    let old_water_version = app.world().get::<DynamicWaterMeshOverrides>(root).unwrap().authoring_version();
    let mut water = record.water_surfaces.clone();
    water[0].flow_speed = 7.0;
    app.world_mut().entity_mut(root).insert(DynamicWaterMeshOverrides::new(water.clone()));
    assert_ne!(app.world().get::<DynamicWaterMeshOverrides>(root).unwrap().authoring_version(), old_water_version);
    cleanup_saved_batch(&mut app, &map, saved);
    assert_retained(&app, root, child, &record.id);
    assert_eq!(app.world().get::<DynamicWaterMeshOverrides>(root).unwrap().surfaces(), water);
    assert_eq!(app.world().get::<MapPlacementInstance>(root).unwrap().authoring_version(), instance_version,
        "整组件水数值替换不改实例身份，水自己的所有权票据仍应阻止旧批回收");
    clean_current_saved_version(&mut app, root, child);
}

#[test]
fn bounded_cleanup_revokes_root_index_before_next_frame_numeric_input_creates_a_new_preview() {
    let (mut app, input, root, child, record) = fixture(MapPlacementRuntimePhase::Building);
    let (map, mut saved) = take_saved_batch(&mut app);
    assert_eq!(active_index_roots(&mut app, &map, &record.id), vec![root]);
    for index in 0..64 {
        let mut extra = record.clone();
        extra.id = format!("bounded-preview-{index}");
        let entity = app.world_mut().spawn((EditorPlacedAsset, Visibility::Inherited,
            MapPlacementInstance::new(&map, &extra))).id();
        app.world_mut().resource_mut::<EditorAuthoringSession>().placement_previews
            .insert(extra.id.clone(), entity);
        saved.push(PlacementPreviewHandoffSnapshot::capture(app.world(), &map, &extra.id, entity).unwrap());
    }
    let index_revision = app.world().resource::<PlacementInstanceIndex>().membership_revision();
    cleanup_saved_batch(&mut app, &map, saved);
    assert!(app.world().get_entity(root).is_ok(), "65根使首帧64次预算全部用于静默，旧根仍实际存在");
    assert!(app.world().get_entity(child).is_ok());
    assert!(app.world().get::<MapPlacementInstance>(root).is_none());
    assert!(active_index_roots(&mut app, &map, &record.id).is_empty());
    assert!(app.world().resource::<PlacementInstanceIndex>().membership_revision() > index_revision,
        "真实Remove observer必须撤销原索引登记，不能只靠查询过滤隐藏根");
    assert!(!app.world().resource::<EditorAuthoringSession>().placement_previews.contains_key(&record.id));

    app.add_systems(Update, handle_placement_transform_numeric_input.before(poll_water_file_handoff));
    app.world_mut().write_message(NumericInputCommitted { entity: input, value: NumericValue::F32(6.0),
        outcome: NumericInputCommitOutcome::Accepted, source: NumericInputCommitSource::Text,
        changed: true, edited: true });
    app.update(); // 下一真实调度帧先消费普通输入，再继续旧批清理；旧根不能恢复作者资格。
    let replacement = app.world().resource::<EditorAuthoringSession>().placement_previews[&record.id];
    assert_ne!(replacement, root);
    assert_eq!(app.world().get::<Transform>(replacement).unwrap().translation.y, 6.0);
    assert_eq!(active_index_roots(&mut app, &map, &record.id), vec![replacement]);
    assert_eq!(app.world().get::<Visibility>(replacement), Some(&Visibility::Inherited));
    for _ in 0..3 { app.update(); }
    assert!(app.world().get_entity(root).is_err());
    assert!(app.world().get_entity(child).is_err());
    assert!(app.world().get_entity(replacement).is_ok());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_previews.get(&record.id), Some(&replacement));
    assert!(app.world().resource::<WaterFileHandoff>().preview_cleanup.is_none());
}

#[test]
fn mismatched_snapshots_consume_each_frame_budget_and_do_not_starve_a_later_matching_root() {
    let (mut app, _, root, child, record) = fixture(MapPlacementRuntimePhase::Building);
    let (map, mut saved) = take_saved_batch(&mut app);
    let mut retained = vec![(record.id.clone(), root)];
    for index in 0..64 {
        let mut extra = record.clone();
        extra.id = format!("mixed-preview-{index}");
        let entity = app.world_mut().spawn((EditorPlacedAsset, Transform::IDENTITY,
            Visibility::Inherited, MapPlacementInstance::new(&map, &extra))).id();
        app.world_mut().resource_mut::<EditorAuthoringSession>().placement_previews
            .insert(extra.id.clone(), entity);
        saved.push(PlacementPreviewHandoffSnapshot::capture(app.world(), &map, &extra.id, entity).unwrap());
        if index < 63 { retained.push((extra.id, entity)); }
    }
    let matching_root = saved.last().unwrap().root;
    let matching_id = saved.last().unwrap().id.clone();
    // 前64条均为真实捕获后只改变Transform，最后一条保留原版本；不能免费跳过整段失配项。
    for (_, entity) in &retained {
        app.world_mut().get_mut::<Transform>(*entity).unwrap().translation.y = 6.0;
    }
    cleanup_saved_batch(&mut app, &map, saved);
    assert_eq!(app.world().get::<Visibility>(matching_root), Some(&Visibility::Inherited),
        "首帧64次核验用尽，后续有效根必须尚未被静默");
    assert!(app.world().get::<MapPlacementInstance>(matching_root).is_some());
    assert!(app.world().resource::<EditorAuthoringSession>().placement_previews.contains_key(&matching_id));
    assert_retained_without_finished_batch(&app, root, child, &record.id);

    app.update();
    assert!(app.world().get_entity(matching_root).is_ok(),
        "第二帧一个静默加63个失配DFS检查已满，不能免费越过失配项提前销毁有效根");
    assert_eq!(app.world().get::<Visibility>(matching_root), Some(&Visibility::Hidden));
    assert!(app.world().get::<MapPlacementInstance>(matching_root).is_none());
    app.update();
    assert!(app.world().get_entity(matching_root).is_err(), "第三帧应跨过剩余失配项并回收有效根");
    assert!(app.world().resource::<WaterFileHandoff>().preview_cleanup.is_none());
    for (id, entity) in retained {
        assert!(app.world().get_entity(entity).is_ok());
        assert_eq!(app.world().get::<Visibility>(entity), Some(&Visibility::Inherited));
        assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_previews.get(&id), Some(&entity));
    }
    assert_retained(&app, root, child, &record.id);
}

fn assert_retained_without_finished_batch(app: &App, root: Entity, child: Entity, id: &str) {
    assert!(app.world().get::<WorldAssetRoot>(root).is_some());
    assert!(app.world().get::<MapPlacementInstance>(root).is_some());
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
    assert_eq!(app.world().get::<ChildOf>(child).map(ChildOf::parent), Some(root));
    assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_previews.get(id), Some(&root));
}

#[test]
fn resident_preview_handoff_preserves_instance_identity_and_active_index_membership() {
    let (mut app, _, root, child, record) = fixture(MapPlacementRuntimePhase::Building);
    app.world_mut().entity_mut(root).insert(feathrise_scene::StreamedVolumeResident::default());
    let (map, saved) = take_saved_batch(&mut app);
    let version = app.world().get::<MapPlacementInstance>(root).unwrap().authoring_version();
    let index_revision = app.world().resource::<PlacementInstanceIndex>().membership_revision();
    cleanup_saved_batch(&mut app, &map, saved);
    assert!(app.world().get_entity(child).is_ok());
    assert!(app.world().get::<WorldAssetRoot>(root).is_some());
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
    assert_eq!(app.world().get::<MapPlacementInstance>(root).unwrap().authoring_version(), version);
    assert_eq!(active_index_roots(&mut app, &map, &record.id), vec![root]);
    assert_eq!(app.world().resource::<PlacementInstanceIndex>().membership_revision(), index_revision,
        "驻留模型由拓扑域继续拥有，保存只移交作者索引，不触发实例Remove撤权");
    assert!(!app.world().resource::<EditorAuthoringSession>().placement_previews.contains_key(&record.id));
    assert!(app.world().resource::<WaterFileHandoff>().preview_cleanup.is_none());
}
