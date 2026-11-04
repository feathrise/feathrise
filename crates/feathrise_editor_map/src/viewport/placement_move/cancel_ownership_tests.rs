//! 来源失效后的取消只回滚尚存自有预览；外部接管实体不能被旧映射覆盖或随临时父级删除。

use super::*;
use feathrise_map_content::{MapPlacementTerrainPatch, MapTerrainKind, TerrainId, TerrainObjectTransform};
use feathrise_scene::MapPlacementInstance;

fn patch_app() -> (App, Entity, Entity, Entity) {
    let (mut app, root, node) = app();
    app.add_plugins(bevy::transform::TransformPlugin)
        .init_asset::<bevy::world_serialization::WorldAsset>();
    let (map, source, record) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
        source.id = TerrainId::parse("cancel-owned-source").unwrap();
        source.terrain.kind = MapTerrainKind::SparseClipmap;
        source.transform = TerrainObjectTransform { translation: [10.0, 3.0, 7.0],
            yaw_degrees: 90.0, scale: [2.0, 3.0, 4.0], ..default() };
        draft.manifest.terrains = Some(Arc::from([source.clone()]));
        draft.placement_water_surface_count = 0;
        let record = &mut draft.placements.placements[0];
        record.water_surfaces.clear();
        record.terrain_patch = Some(MapPlacementTerrainPatch { terrain_id: Some(source.id.clone()), ..default() });
        (draft.map_id.clone(), source, record.clone())
    };
    let parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(
        app.world_mut(), &map, std::slice::from_ref(&source),
    )[0];
    let transform = record.validated_transform(&map,
        Rect::from_corners(source.world_min.into(), source.world_max.into())).unwrap();
    app.world_mut().entity_mut(root).insert((transform, ChildOf(parent), MapPlacementInstance::new(&map, &record)));
    app.world_mut().resource_mut::<MapEditorViewportState>().center = source.transform.as_transform()
        .transform_point(record.translation.into());
    app.update();
    (app, root, node, parent)
}

fn drag_patch(app: &mut App, node: Entity) -> Vec2 {
    let origin = app.world().resource::<MapEditorViewportState>().center;
    let camera = camera(app);
    let point = |delta| camera.project(origin + Vec3::X * (camera.handle_length(origin).unwrap() * 0.65 + delta)).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(app, node, point(0.0));
    assert!(app.world().resource::<PlacementMoveState>().drag.is_some());
    let cursor = point(1.0);
    step(app, node, cursor);
    assert!(app.world().resource::<PlacementMoveState>().drag.as_ref().unwrap().preview_applied);
    cursor
}

fn invalidate_source(app: &mut App) {
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
        .manifest.terrains = Some(Arc::from([]));
}

#[test]
fn patch_cancel_hides_and_detaches_existing_owned_preview_when_source_is_invalid() {
    let (mut app, root, node, _) = patch_app();
    let before = draft_record(&app).clone();
    let original = *app.world().get::<Transform>(root).unwrap();
    let cursor = drag_patch(&mut app, node);
    invalidate_source(&mut app);
    step(&mut app, node, cursor);
    assert!(app.world().get_entity(root).is_ok());
    assert_eq!(app.world().get::<Transform>(root), Some(&original));
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    assert!(app.world().get::<ChildOf>(root).is_none());
    assert_eq!(app.world().get::<MapPlacementInstance>(root).unwrap().id(), before.id);
    assert_eq!(draft_record(&app), &before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<MapEditorState>().dirty);
    assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
}

#[test]
fn patch_cancel_does_not_modify_foreign_owner_through_stale_preview_mapping() {
    let (mut app, root, node, _) = patch_app();
    let before = draft_record(&app).clone();
    let cursor = drag_patch(&mut app, node);
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    let mut foreign = before.clone();
    foreign.id = "foreign-cancel-owner".into();
    let external = Transform::from_xyz(91.0, -8.0, 53.0).with_scale(Vec3::new(3.0, 2.0, 1.0));
    let parent = app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited)).id();
    app.world_mut().entity_mut(root).insert((MapPlacementInstance::new(&map, &foreign), external,
        Visibility::Hidden, ChildOf(parent)));
    invalidate_source(&mut app);
    step(&mut app, node, cursor);
    assert!(app.world().get_entity(root).is_ok());
    assert_eq!(app.world().get::<Transform>(root), Some(&external));
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
    assert_eq!(app.world().get::<MapPlacementInstance>(root).unwrap().id(), foreign.id);
    assert_eq!(draft_record(&app), &before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<MapEditorState>().dirty);
}

#[test]
fn patch_cancel_group_detaches_foreign_owner_without_overwriting_or_cascade_delete() {
    let (mut app, roots, node, original_parent) = multi_select::group_app();
    let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
    multi_select::group_begin(&mut app, node, MoveAxis::X);
    let pivot = app.world().get::<ChildOf>(roots[1]).unwrap().parent();
    assert_ne!(pivot, original_parent);
    let cursor = multi_select::group_point(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    let mut foreign = before[1].clone();
    foreign.id = "foreign-group-owner".into();
    let external = Transform::from_xyz(19.0, 28.0, 37.0).with_rotation(Quat::from_rotation_z(0.3));
    // 保留旧会话地址和临时父级，仅替换真实组件身份；旧映射不能把外部接管重新解释成自有根。
    app.world_mut().entity_mut(roots[1]).insert((MapPlacementInstance::new(&map, &foreign), external, Visibility::Hidden));
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    step(&mut app, node, cursor);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    multi_select::finish(&mut app, node, cursor);
    assert!(app.world().get_entity(roots[1]).is_ok());
    assert!(app.world().get::<ChildOf>(roots[1]).is_none());
    assert_eq!(app.world().get::<Transform>(roots[1]), Some(&external));
    assert_eq!(app.world().get::<Visibility>(roots[1]), Some(&Visibility::Hidden));
    assert_eq!(app.world().get::<MapPlacementInstance>(roots[1]).unwrap().id(), foreign.id);
    assert!(app.world().get_entity(pivot).is_err());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements, before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<MapEditorState>().dirty);
}

#[test]
fn patch_group_recovers_stale_preview_mapping() {
    use crate::viewport::placement_move::group_types::GroupPhase;
    for foreign_owner in [false, true] {
        for has_formal_root in [false, true] {
            for cancel in [false, true] {
                let (mut app, roots, node, ordinary_parent) = multi_select::group_app();
                app.init_asset::<bevy::world_serialization::WorldAsset>();
                let stale_index = 1; // 前一成员已挂临时父后再恢复缺根，必须覆盖作者根Add引起的索引换代。
                let kept_index = 0;
                let (map, source, before) = {
                    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                    let draft = session.draft.as_mut().unwrap();
                    let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
                    source.id = TerrainId::parse("group-recovery-source").unwrap();
                    source.terrain.kind = MapTerrainKind::SparseClipmap;
                    source.transform = TerrainObjectTransform::default();
                    draft.manifest.terrains = Some(Arc::from([source.clone()]));
                    let patch = &mut draft.placements.placements[stale_index];
                    patch.terrain_patch = Some(MapPlacementTerrainPatch { terrain_id: Some(source.id.clone()), ..default() });
                    patch.water_surfaces.clear();
                    draft.placement_water_surface_count = 1;
                    (draft.map_id.clone(), source, draft.placements.placements.clone())
                };
                let source_parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(
                    app.world_mut(), &map, std::slice::from_ref(&source),
                )[0];
                let original = before[stale_index].validated_transform(&map,
                    Rect::from_corners(source.world_min.into(), source.world_max.into())).unwrap();
                let external = Transform::from_xyz(91.0, -8.0, 53.0).with_scale(Vec3::new(3.0, 2.0, 1.0));
                let foreign = if foreign_owner {
                    let mut foreign = before[stale_index].clone();
                    foreign.id = "foreign-group-recovery-owner".into();
                    app.world_mut().entity_mut(roots[stale_index]).insert((MapPlacementInstance::new(&map, &foreign), external,
                        Visibility::Hidden, ChildOf(ordinary_parent)));
                    Some(foreign)
                } else {
                    app.world_mut().despawn(roots[stale_index]);
                    None
                }; // 保留原会话地址，模拟源退役删除或外部接管后尚未更新的作者映射。
                app.add_observer(crate::ui::placement_water::register_placement_instance_root)
                    .add_observer(crate::ui::placement_water::unregister_placement_instance_root)
                    .add_observer(crate::ui::placement_water::observe_placement_water_status_inserted);
                let formal = has_formal_root.then(|| app.world_mut().spawn((original, Visibility::Inherited,
                    ChildOf(source_parent), MapPlacementInstance::new(&map, &before[stale_index]))).id());
                let start = multi_select::group_point(&app, MoveAxis::X, 0.0);
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
                // 正式根归可退役批次；沿原准备预算补独立作者预览，允许其延迟命令下一帧进入查询。
                let mut status_notified = false;
                for _ in 0..8 {
                    step(&mut app, node, start);
                    if !has_formal_root && !foreign_owner && !cancel && !status_notified
                        && let Some(pending) = app.world().resource::<PlacementMoveState>().group.as_ref()
                            .and_then(|drag| drag.pending_preview) {
                        let membership = app.world().resource::<PlacementInstanceIndex>().membership_revision();
                        let status = app.world().resource::<PlacementInstanceIndex>().status_revision(&map, &before[stale_index].id);
                        app.world_mut().entity_mut(pending).insert(feathrise_scene::DynamicWaterMeshOverrideStatus::Ready);
                        assert_eq!(app.world().resource::<PlacementInstanceIndex>().membership_revision(), membership);
                        assert!(app.world().resource::<PlacementInstanceIndex>().status_revision(&map, &before[stale_index].id) > status);
                        status_notified = true; // 单纯就绪通知不代表根集合换代，不能取消本次自补预览。
                    }
                    if app.world().resource::<PlacementMoveState>().group.as_ref()
                        .is_some_and(|drag| drag.phase == GroupPhase::Dragging) { break; }
                }
                let drag = app.world().resource::<PlacementMoveState>().group.as_ref()
                    .filter(|drag| drag.phase == GroupPhase::Dragging)
                    .unwrap_or_else(|| panic!("旧地址必须恢复整组准备：foreign={foreign_owner}, formal={has_formal_root}, {}",
                        app.world().resource::<MapEditorState>().status));
                if !has_formal_root && !foreign_owner && !cancel { assert!(status_notified); }
                let recovered = drag.roots.iter().find(|root| root.id == before[stale_index].id).unwrap().entity;
                let helpers = drag.pivots.iter().map(|pivot| pivot.cleanup).collect::<Vec<_>>();
                assert_ne!(recovered, roots[stale_index]);
                assert_eq!(app.world().resource::<EditorAuthoringSession>().placement_previews[&before[stale_index].id], recovered);
                if let Some(formal) = formal {
                    assert_ne!(recovered, formal); // 恢复陈旧映射也不能把正式根接到临时作者父级。
                    assert_eq!(app.world().get::<Transform>(formal), Some(&original));
                    assert_eq!(app.world().get::<ChildOf>(formal).map(ChildOf::parent), Some(source_parent));
                    assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden));
                }
                let mut query = app.world_mut().query::<(Entity, &MapPlacementInstance)>();
                let matching = query.iter(app.world()).filter_map(|(entity, instance)|
                    (instance.map_id() == &map && instance.id() == before[stale_index].id).then_some(entity)).collect::<Vec<_>>();
                assert_eq!(matching.len(), 1 + usize::from(formal.is_some()));
                assert!(matching.contains(&recovered));
                if let Some(formal) = formal { assert!(matching.contains(&formal)); }
                let cursor = multi_select::group_point(&app, MoveAxis::X, 1.0);
                step(&mut app, node, cursor);
                assert!((app.world().get::<GlobalTransform>(recovered).unwrap().translation().x
                    - before[stale_index].translation[0] - 1.0).abs() < 0.002);
                if cancel {
                    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
                    step(&mut app, node, cursor);
                }
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
                multi_select::finish(&mut app, node, cursor);
                let session = app.world().resource::<EditorAuthoringSession>();
                assert_eq!(session.undo.len(), usize::from(!cancel));
                let after = &session.draft.as_ref().unwrap().placements.placements;
                if cancel {
                    assert_eq!(after, &before);
                    assert!(!app.world().resource::<MapEditorState>().dirty);
                } else {
                    for (after, before) in after.iter().zip(&before) {
                        assert!((after.translation[0] - before.translation[0] - 1.0).abs() < 0.002);
                    }
                }
                assert_eq!(app.world().get::<ChildOf>(recovered).unwrap().parent(), source_parent);
                if let Some(formal) = formal {
                    assert_eq!(app.world().get::<Transform>(formal), Some(&original));
                    assert_eq!(app.world().get::<ChildOf>(formal).map(ChildOf::parent), Some(source_parent));
                    assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden));
                    assert!(app.world().get::<MapPlacementInstance>(formal).unwrap()
                        .matches_record(&map, &before[stale_index], &before[stale_index].water_surfaces));
                }
                assert_eq!(app.world().get::<ChildOf>(roots[kept_index]).unwrap().parent(), ordinary_parent);
                for helper in helpers { assert!(app.world().get_entity(helper).is_err()); }
                if let Some(foreign) = foreign {
                    assert_eq!(app.world().get::<Transform>(roots[stale_index]), Some(&external));
                    assert_eq!(app.world().get::<Visibility>(roots[stale_index]), Some(&Visibility::Hidden));
                    assert_eq!(app.world().get::<ChildOf>(roots[stale_index]).unwrap().parent(), ordinary_parent);
                    assert_eq!(app.world().get::<MapPlacementInstance>(roots[stale_index]).unwrap().id(), foreign.id);
                } else {
                    assert!(app.world().get_entity(roots[stale_index]).is_err());
                }
            }
        }
    }
}

#[test]
fn patch_group_preview_recovery_rejects_unrelated_root_membership_changes() {
    use crate::viewport::placement_move::group_types::GroupPhase;
    for remove in [false, true] {
        let (mut app, roots, node, ordinary_parent) = multi_select::group_app();
        app.init_asset::<bevy::world_serialization::WorldAsset>()
            .add_observer(crate::ui::placement_water::register_placement_instance_root)
            .add_observer(crate::ui::placement_water::unregister_placement_instance_root);
        let (map, source, before) = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
            source.id = TerrainId::parse("group-interrupted-source").unwrap();
            source.terrain.kind = MapTerrainKind::SparseClipmap;
            source.transform = TerrainObjectTransform::default();
            draft.manifest.terrains = Some(Arc::from([source.clone()]));
            let patch = &mut draft.placements.placements[1];
            patch.terrain_patch = Some(MapPlacementTerrainPatch { terrain_id: Some(source.id.clone()), ..default() });
            patch.water_surfaces.clear();
            draft.placement_water_surface_count = 1;
            (draft.map_id.clone(), source, draft.placements.placements.clone())
        };
        let source_parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(
            app.world_mut(), &map, std::slice::from_ref(&source),
        )[0];
        app.world_mut().despawn(roots[1]);
        let external = Transform::from_xyz(91.0, -8.0, 53.0).with_scale(Vec3::new(3.0, 2.0, 1.0));
        let mut foreign = before[1].clone();
        foreign.id = "unrelated-group-runtime-root".into();
        let foreign_root = remove.then(|| app.world_mut().spawn((external, Visibility::Hidden,
            ChildOf(ordinary_parent), MapPlacementInstance::new(&map, &foreign))).id());
        let start = multi_select::group_point(&app, MoveAxis::X, 0.0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, start);
        step(&mut app, node, start);
        let drag = app.world().resource::<PlacementMoveState>().group.as_ref().unwrap();
        assert!(drag.phase == GroupPhase::Preparing);
        assert_eq!(drag.prepared, 1);
        let preview = drag.pending_preview.expect("第二成员应等待本次自补的确切预览地址");
        let root_epoch = drag.root_epoch;
        assert_eq!(app.world().resource::<PlacementInstanceIndex>().membership_revision(), root_epoch.wrapping_add(1));
        let foreign_root = if let Some(root) = foreign_root {
            app.world_mut().entity_mut(root).remove::<MapPlacementInstance>();
            root
        } else {
            app.world_mut().spawn((external, Visibility::Hidden, ChildOf(ordinary_parent),
                MapPlacementInstance::new(&map, &foreign))).id()
        };
        assert_eq!(app.world().resource::<PlacementInstanceIndex>().membership_revision(), root_epoch.wrapping_add(2));
        step(&mut app, node, start);
        assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
            .is_some_and(|drag| drag.phase == GroupPhase::Finishing));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        multi_select::finish(&mut app, node, start);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, before);
        assert!(session.undo.is_empty());
        assert!(!app.world().resource::<MapEditorState>().dirty);
        assert_eq!(app.world().get::<ChildOf>(roots[0]).unwrap().parent(), ordinary_parent);
        assert_eq!(app.world().get::<ChildOf>(preview).unwrap().parent(), source_parent);
        assert_eq!(app.world().get::<Transform>(foreign_root), Some(&external));
        assert_eq!(app.world().get::<Visibility>(foreign_root), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<ChildOf>(foreign_root).unwrap().parent(), ordinary_parent);
        if remove {
            assert!(app.world().get::<MapPlacementInstance>(foreign_root).is_none());
        } else {
            assert_eq!(app.world().get::<MapPlacementInstance>(foreign_root).unwrap().id(), foreign.id);
        }
    }
}

#[test]
fn patch_group_runtime_adoption_identity_cancels_without_root_membership_changes() {
    use crate::viewport::placement_move::group_types::GroupPhase;
    use feathrise_scene::{MapPlacementRuntimePhase, MapPlacementRuntimeStatus};
    // 三个可编辑阶段都读取公开采用身份；候选进度与失败仍保留旧活动批次，不应误取消手势。
    for stage in 0..3 {
        for change in 0..7 {
            let (mut app, roots, node, parent) = multi_select::group_app();
            let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
            let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
            app.insert_resource(MapPlacementRuntimeStatus {
                phase: MapPlacementRuntimePhase::Building, map_id: Some(map), revision: 17,
                placement_count: 2, ..default()
            });
            let start = multi_select::group_point(&app, MoveAxis::X, 0.0);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut app, node, start);
            assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
                .is_some_and(|drag| drag.phase == GroupPhase::Preparing));
            let cursor = multi_select::group_point(&app, MoveAxis::X, 1.0);
            if stage > 0 {
                step(&mut app, node, start);
                assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
                    .is_some_and(|drag| drag.phase == GroupPhase::Dragging));
                step(&mut app, node, cursor);
            }
            if stage == 2 {
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
                step(&mut app, node, cursor);
                assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
                    .is_some_and(|drag| drag.phase == GroupPhase::Waiting));
            }
            let membership = app.world().resource::<PlacementInstanceIndex>().membership_revision();
            if change == 6 {
                app.world_mut().remove_resource::<MapPlacementRuntimeStatus>();
            } else {
                let mut status = app.world_mut().resource_mut::<MapPlacementRuntimeStatus>();
                match change {
                    0 => status.revision += 1, // 原子采用会换正式共享门，但不会触发组件Add/Remove。
                    1 => status.map_id = None, // 离开场景只退役，不一定产生下一次完整采用。
                    2 => status.placement_count = 0,
                    3 => status.progress_revision += 1,
                    4 => status.phase = MapPlacementRuntimePhase::Ready,
                    5 => { status.attempt_revision += 1; status.phase = MapPlacementRuntimePhase::Failed; }
                    _ => unreachable!(),
                }
            }
            assert_eq!(app.world().resource::<PlacementInstanceIndex>().membership_revision(), membership);
            step(&mut app, node, if stage == 0 { start } else { cursor });
            let cancelled = change < 3 || change == 6;
            if cancelled {
                assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
                    .is_some_and(|drag| drag.phase == GroupPhase::Finishing),
                    "正式采用身份变化必须取消：stage={stage}, change={change}");
            } else if stage < 2 {
                assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
                    .is_some_and(|drag| drag.phase == GroupPhase::Dragging),
                    "仅候选状态推进不应取消：stage={stage}, change={change}");
                step(&mut app, node, cursor);
            }
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            multi_select::finish(&mut app, node, cursor);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.undo.len(), usize::from(!cancelled), "stage={stage}, change={change}");
            if cancelled {
                assert_eq!(session.draft.as_ref().unwrap().placements.placements, before);
                assert!(!app.world().resource::<MapEditorState>().dirty);
            } else {
                for (after, before) in session.draft.as_ref().unwrap().placements.placements.iter().zip(&before) {
                    assert!((after.translation[0] - before.translation[0] - 1.0).abs() < 0.002);
                }
            }
            for root in roots { assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent); }
        }
    }
}
