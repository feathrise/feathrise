//! 作者撤销辅助函数的纯内存回归。

use super::*; // 直接构造复合笔划局部差分。

#[test]
fn terrain_patch_delete_undo_added_checks_preview_owner_before_reclaiming() {
    assert_history_delete_preview_ownership(0);
}

#[test]
fn terrain_patch_delete_redo_removed_checks_preview_owner_before_reclaiming() {
    assert_history_delete_preview_ownership(1);
}

#[test]
fn terrain_patch_delete_undo_building_added_checks_preview_owner_before_reclaiming() {
    assert_history_delete_preview_ownership(2);
}

/// 历史可撤销作者记录，却不能凭陈旧映射销毁已被其他地图或稳定ID接管的实体。
fn assert_history_delete_preview_ownership(operation: usize) {
    use crate::authoring::commands::BuildingTerrainChange;
    use crate::viewport::placement_move::interaction_tests::{app, batch_undo_resources, record};
    use feathrise_map_content::{MapId, MapPlacementTerrainPatch, MapTerrainKind, TerrainId};

    for owner in ["owned", "dead", "other-id", "other-map"] {
        let (mut app, _, _) = app();
        batch_undo_resources(&mut app);
        app.add_observer(crate::ui::placement_water::register_placement_instance_root)
            .add_observer(crate::ui::placement_water::unregister_placement_instance_root);
        let water = record(); // 同图无关记录及非零水绑定计数不得随目标回收而改变。
        let mut target = water.clone();
        target.id = "history-delete-preview-owner".into();
        target.water_surfaces.clear();
        let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
        if operation != 2 {
            let mut object = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap()
                .manifest.terrain_objects_for_edit()[0].clone();
            object.id = TerrainId::parse("history-delete-preview-source").unwrap();
            object.terrain.kind = MapTerrainKind::SparseClipmap;
            target.terrain_patch = Some(MapPlacementTerrainPatch {
                terrain_id: Some(object.id.clone()), ..default()
            });
            app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
                .manifest.terrains = Some(Arc::from([object.clone()]));
            feathrise_scene::runtime_test_support::install_active_terrain_instances(app.world_mut(), &map, &[object]);
        }
        let mut actual_record = target.clone();
        if owner == "other-id" { actual_record.id = "external-owner".into(); }
        let actual_map = if owner == "other-map" { MapId::parse("MAP-history-external-owner").unwrap() }
            else { map.clone() };
        let parent = app.world_mut().spawn((Transform::from_xyz(8.0, 9.0, 10.0), Visibility::Inherited)).id();
        let transform = Transform::from_xyz(2.0, 3.0, 4.0).with_scale(Vec3::new(2.0, 3.0, 4.0));
        let preview = app.world_mut().spawn((MapPlacementInstance::new(&actual_map, &actual_record),
            transform, Visibility::Hidden, ChildOf(parent))).id();
        let child = app.world_mut().spawn((Transform::IDENTITY, ChildOf(preview))).id();
        let preserved = if owner == "dead" {
            app.world_mut().despawn(preview);
            // 地址已退役后分配新代实体，旧句柄不能误伤复用同一槽的新拥有者。
            Some(app.world_mut().spawn((MapPlacementInstance::new(&actual_map, &actual_record),
                transform, Visibility::Hidden, ChildOf(parent))).id())
        } else { (owner != "owned").then_some(preview) };
        // 作者预览存在或地址退役后，仍可能迟到一个可见正式根；记录删除必须承担它的隐藏责任。
        let formal_transform = Transform::from_xyz(25.0, 26.0, 27.0).with_scale(Vec3::new(1.5, 2.5, 3.5));
        let formal = app.world_mut().spawn((MapPlacementInstance::new(&map, &target),
            formal_transform, Visibility::Inherited, ChildOf(parent))).id();
        let formal_child = app.world_mut().spawn((Transform::IDENTITY, ChildOf(formal))).id();
        let undoing = operation != 1;
        let command = match operation {
            0 => EditorCommand::PlacementAdded(target.clone()),
            1 => EditorCommand::PlacementRemoved { index: 1, record: target.clone(), had_author_preview: true },
            _ => EditorCommand::BuildingTerrainChanged(Box::new(BuildingTerrainChange {
                placement: BuildingPlacementChange::Added { record_index: 1, record: target.clone() },
                heights: Vec::new(), // 只检验建筑记录回收，不借地形差分失败门阻止实际删除。
            })),
        };
        let revisions = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.placements.placements = vec![water.clone(), target.clone()];
            draft.placement_water_surface_count = water.water_surfaces.len();
            session.placement_previews.clear();
            session.placement_previews.insert(target.id.clone(), preview);
            session.undo.clear(); session.redo.clear();
            if undoing { session.undo.push(command); } else { session.redo.push(command); }
            (session.revision, session.placement_structure_revision, session.terrain_revision)
        };
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.undo_requested = undoing; editor.redo_requested = !undoing;
        }
        crate::authoring::drive_undo_redo_to_completion(&mut app);
        let label = format!("operation={operation}, owner={owner}");
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(draft.placements.placements, vec![water.clone()], "{label}");
        assert_eq!(draft.placement_water_surface_count, water.water_surfaces.len(), "{label}");
        assert_eq!((session.revision, session.placement_structure_revision, session.terrain_revision),
            (revisions.0.wrapping_add(1), revisions.1.wrapping_add(1), revisions.2), "{label}");
        assert!(!session.placement_previews.contains_key(&target.id), "{label}");
        assert_eq!((session.undo.len(), session.redo.len()), if undoing { (0, 1) } else { (1, 0) }, "{label}");
        assert!(app.world().resource::<MapEditorState>().dirty, "{label}");
        assert!(app.world().get_entity(parent).is_ok(), "{label}");
        assert!(app.world().get_entity(formal).is_ok(), "正式根等待运行时接管，不应提前回收：{label}");
        assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden), "迟到正式根必须隐藏：{label}");
        assert_eq!(app.world().get::<Transform>(formal), Some(&formal_transform), "隐藏不能改写正式姿态：{label}");
        assert_eq!(app.world().get::<ChildOf>(formal).map(ChildOf::parent), Some(parent), "{label}");
        assert_eq!(app.world().get::<ChildOf>(formal_child).map(ChildOf::parent), Some(formal), "{label}");
        if let Some(entity) = preserved {
            assert!(app.world().get_entity(entity).is_ok(), "陈旧映射误删外部拥有者：{label}");
            assert_eq!(app.world().get::<Transform>(entity), Some(&transform), "{label}");
            assert_eq!(app.world().get::<Visibility>(entity), Some(&Visibility::Hidden), "{label}");
            assert_eq!(app.world().get::<ChildOf>(entity).map(ChildOf::parent), Some(parent), "{label}");
            let instance = app.world().get::<MapPlacementInstance>(entity).unwrap();
            assert_eq!((instance.map_id(), instance.id()), (&actual_map, actual_record.id.as_str()), "{label}");
            if owner != "dead" { assert!(app.world().get_entity(child).is_ok(), "{label}"); }
        } else {
            assert!(app.world().get_entity(preview).is_err(), "本会话根应正常回收：{label}");
            assert!(app.world().get_entity(child).is_err(), "本会话后代应正常回收：{label}");
        }
    }
}

#[test]
fn terrain_patch_preview_parent_waits_for_the_complete_author_source_frame() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::viewport::placement_move::interaction_tests::{app, record};
    use feathrise_map_content::{MapPlacementTerrainPatch, MapTerrainKind, TerrainId, TerrainObjectTransform};

    let (mut app, _, _) = app();
    let mut patch = record();
    patch.water_surfaces.clear();
    patch.terrain_patch = Some(MapPlacementTerrainPatch {
        terrain_id: Some(TerrainId::parse("parent-frame-gate").unwrap()), ..default()
    });
    let (map, object) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = patch.terrain_patch.as_ref().unwrap().terrain_id.clone().unwrap();
        object.terrain.kind = MapTerrainKind::SparseClipmap;
        object.transform = TerrainObjectTransform { translation: [18.0, 6.0, -9.0],
            yaw_degrees: 23.0, pitch_degrees: 15.0, roll_degrees: -8.0,
            scale: [2.0, 3.0, 5.0] };
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.placements.placements = vec![patch];
        (draft.map_id.clone(), object)
    };
    let parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(
        app.world_mut(), &map, &[object.clone()],
    )[0];
    let resolve = |session: Res<EditorAuthoringSession>, queries: feathrise_scene::TerrainObjectQueries| {
        let draft = session.draft.as_ref().unwrap();
        crate::authoring::patch_preview_parent(draft, &queries, &draft.placements.placements[0])
    };
    assert_eq!(app.world_mut().run_system_once(resolve).unwrap().unwrap(), Some(parent));
    // MapSourceFrame仍与作者完全一致时，旧类型或旧定义也不能凭矩阵相同取得新预览父资格。
    for stale_definition in ["kind", "transform"] {
        let mut stale = object.clone();
        if stale_definition == "kind" { stale.terrain.kind = MapTerrainKind::Heightfield; }
        else { stale.transform.pitch_degrees += 4.0; }
        app.world_mut().entity_mut(parent).insert(feathrise_scene::TerrainObjectInstance { definition: Arc::new(stale) });
        assert!(app.world_mut().run_system_once(resolve).unwrap().unwrap_err().contains("尚未采用"), "{stale_definition}");
    }
    app.world_mut().entity_mut(parent).insert(feathrise_scene::TerrainObjectInstance { definition: Arc::new(object.clone()) });
    assert_eq!(app.world_mut().run_system_once(resolve).unwrap().unwrap(), Some(parent));
    // 已编辑的作者框架不能借仍处于上一姿态的真实父级；完整矩阵含俯仰、翻滚和三轴比例。
    let expected_transform = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let objects = Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap());
        objects[0].transform.roll_degrees += 4.0;
        objects[0].transform
    };
    let mut definition = object;
    definition.transform = expected_transform; // 定义已采用仍不能掩盖实际源矩阵尚旧，必须继续验证完整框架。
    app.world_mut().entity_mut(parent).insert(feathrise_scene::TerrainObjectInstance { definition: Arc::new(definition) });
    assert!(app.world_mut().run_system_once(resolve).unwrap().unwrap_err().contains("源框架"));
    assert!(app.world().get_entity(parent).is_ok());
}

#[test]
fn terrain_patch_redo_delete_rejects_invalid_source_without_mutating_author_or_history() {
    use crate::viewport::placement_move::interaction_tests::{app, batch_undo_resources, record};

    for invalid_source in ["missing", "disabled", "not-clipmap"] {
        let (mut app, _, _) = app();
        batch_undo_resources(&mut app);
        let water = record(); // 有效普通网格水保留非零计数，失败不能污染同图其他对象。
        let mut patch = water.clone();
        patch.id = "terrain-patch-redo-delete".into();
        patch.water_surfaces.clear();
        let terrain = feathrise_map_content::TerrainId::parse("patch-redo-parent").unwrap();
        patch.terrain_patch = Some(feathrise_map_content::MapPlacementTerrainPatch {
            terrain_id: Some(terrain.clone()), ..default()
        });
        let (map, mut object) = {
            let session = app.world().resource::<EditorAuthoringSession>();
            let draft = session.draft.as_ref().unwrap();
            (draft.map_id.clone(), draft.manifest.terrain_objects_for_edit()[0].clone())
        };
        object.id = terrain;
        object.enabled = invalid_source != "disabled";
        object.terrain.kind = if invalid_source == "not-clipmap" {
            feathrise_map_content::MapTerrainKind::Heightfield
        } else {
            feathrise_map_content::MapTerrainKind::SparseClipmap
        };
        let preview = app.world_mut().spawn((
            MapPlacementInstance::new(&map, &patch), Transform::IDENTITY, Visibility::Inherited,
        )).id();
        let expected_records = vec![patch.clone(), water.clone()];
        let expected_revision;
        let expected_structure;
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.manifest.terrains = Some(if invalid_source == "missing" {
                Arc::from([])
            } else {
                Arc::from([object])
            });
            draft.placements.placements = expected_records.clone();
            draft.placement_water_surface_count = water.water_surfaces.len();
            session.placement_previews.insert(patch.id.clone(), preview);
            session.undo.clear();
            session.redo = vec![EditorCommand::PlacementRemoved {
                index: 0, record: patch.clone(), had_author_preview: false,
            }];
            expected_revision = session.revision;
            expected_structure = session.placement_structure_revision;
        }
        app.world_mut().resource_mut::<MapEditorState>().redo_requested = true;
        crate::authoring::drive_undo_redo_to_completion(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(draft.placements.placements, expected_records, "{invalid_source}");
        assert_eq!(draft.placement_water_surface_count, water.water_surfaces.len(), "{invalid_source}");
        assert_eq!((session.revision, session.placement_structure_revision),
            (expected_revision, expected_structure), "{invalid_source}");
        assert!(session.undo.is_empty(), "{invalid_source}");
        assert_eq!(session.redo.len(), 1, "{invalid_source}");
        let EditorCommand::PlacementRemoved { index, record, had_author_preview } = &session.redo[0]
            else { panic!("{invalid_source}失败后必须原样保留重做命令"); };
        assert_eq!((*index, record, *had_author_preview), (0, &patch, false));
        assert_eq!(session.placement_previews.get(&patch.id), Some(&preview));
        assert!(app.world().get_entity(preview).is_ok(), "{invalid_source}");
        assert_eq!(app.world().get::<Visibility>(preview), Some(&Visibility::Inherited));
        assert!(app.world().resource::<MapEditorState>().status.contains("重做未执行"), "{invalid_source}");
    }
}

#[test]
fn terrain_patch_legacy_bounds_validate_source_without_restricting_regular_records() {
    use crate::viewport::placement_move::interaction_tests::{app, record};
    use feathrise_map_content::{MapPlacementTerrainPatch, MapTerrainKind, TerrainId};

    let (mut app, _, _) = app();
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    draft.manifest.terrains = None;
    draft.manifest.terrain.kind = MapTerrainKind::Heightfield;
    let world_bounds = draft.manifest.world_bounds().unwrap();
    let mut patch = record();
    assert_eq!(super::super::placement_record_bounds(draft, &patch).unwrap(), world_bounds);
    patch.water_surfaces.clear();
    patch.terrain_patch = Some(MapPlacementTerrainPatch::default());
    assert!(super::super::placement_record_bounds(draft, &patch).is_err());
    draft.manifest.terrain.kind = MapTerrainKind::SparseClipmap;
    assert_eq!(super::super::placement_record_bounds(draft, &patch).unwrap(), world_bounds);
    patch.terrain_patch.as_mut().unwrap().terrain_id = Some(TerrainId::parse("missing-legacy-source").unwrap());
    assert!(super::super::placement_record_bounds(draft, &patch).is_err());
    assert!(super::super::placement_record_bounds_by_source(world_bounds, None, &patch).is_err());
}

#[test]
fn terrain_patch_legacy_history_deletions_reject_invalid_source_before_any_mutation() {
    use crate::authoring::commands::CourtyardBatchChange;
    use crate::viewport::placement_move::interaction_tests::{app, batch_undo_resources, record};
    use feathrise_map_content::{MapPlacementTerrainPatch, MapTerrainKind, TerrainId};

    for invalid_source in ["missing-id", "not-clipmap"] {
        // 同时覆盖有/无作者预览的单个删除，以及单个/整组新增的反向删除。
        for operation in 0..5 {
            let (mut app, _, _) = app();
            batch_undo_resources(&mut app);
            let water = record();
            let mut patch = water.clone();
            patch.id = "legacy-history-patch".into();
            patch.water_surfaces.clear();
            patch.terrain_patch = Some(MapPlacementTerrainPatch {
                terrain_id: Some(if invalid_source == "missing-id" {
                    TerrainId::parse("missing-legacy-source").unwrap()
                } else { TerrainId::legacy() }),
                ..default()
            });
            let expected_records = if operation == 3 {
                vec![patch.clone(), water.clone()] // 批量删除先预检普通尾项，再遇到无效补丁。
            } else { vec![water.clone(), patch.clone()] };
            let command = match operation {
                0 | 1 => EditorCommand::PlacementRemoved {
                    index: 1, record: patch.clone(), had_author_preview: operation == 1,
                },
                2 => EditorCommand::PlacementAdded(patch.clone()),
                3 => EditorCommand::CourtyardBatch(Box::new(CourtyardBatchChange::new_removed(
                    expected_records.iter().cloned().enumerate().collect(), expected_records.len(),
                ).unwrap())),
                _ => EditorCommand::CourtyardBatch(Box::new(CourtyardBatchChange::new(
                    expected_records.clone(), Vec::new(),
                ).unwrap())),
            };
            let undoing = matches!(operation, 2 | 4);
            let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
            let preview = app.world_mut().spawn((
                MapPlacementInstance::new(&map, &patch), Transform::IDENTITY, Visibility::Inherited,
            )).id();
            let expected_revisions;
            {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                let draft = session.draft.as_mut().unwrap();
                draft.manifest.terrains = None;
                draft.manifest.terrain.kind = if invalid_source == "not-clipmap" {
                    MapTerrainKind::Heightfield
                } else { MapTerrainKind::SparseClipmap };
                draft.placements.placements = expected_records.clone();
                draft.placement_water_surface_count = water.water_surfaces.len();
                session.placement_previews.insert(patch.id.clone(), preview);
                session.undo.clear();
                session.redo.clear();
                if undoing { session.undo.push(command); } else { session.redo.push(command); }
                expected_revisions = (session.revision, session.placement_structure_revision, session.terrain_revision);
            }
            {
                let mut editor = app.world_mut().resource_mut::<MapEditorState>();
                editor.undo_requested = undoing;
                editor.redo_requested = !undoing;
            }
            crate::authoring::drive_undo_redo_to_completion(&mut app);
            let session = app.world().resource::<EditorAuthoringSession>();
            let draft = session.draft.as_ref().unwrap();
            let label = format!("{invalid_source}, operation={operation}");
            assert_eq!(draft.placements.placements, expected_records, "{label}");
            assert_eq!(draft.placement_water_surface_count, water.water_surfaces.len(), "{label}");
            assert_eq!((session.revision, session.placement_structure_revision, session.terrain_revision), expected_revisions, "{label}");
            let history = if undoing { &session.undo } else { &session.redo };
            assert_eq!(history.len(), 1, "{label}");
            assert!(if undoing { session.redo.is_empty() } else { session.undo.is_empty() }, "{label}");
            match &history[0] {
                EditorCommand::PlacementAdded(actual) => assert_eq!(actual, &patch, "{label}"),
                EditorCommand::PlacementRemoved { index, record, had_author_preview } => {
                    assert_eq!((*index, record, *had_author_preview), (1, &patch, operation == 1), "{label}");
                }
                EditorCommand::CourtyardBatch(change) if operation == 3 => {
                    assert_eq!(change.removed, vec![(1, water.clone()), (0, patch.clone())], "{label}");
                    assert_eq!(change.original_len, Some(2), "{label}");
                }
                EditorCommand::CourtyardBatch(change) => assert_eq!(change.added, expected_records, "{label}"),
                _ => panic!("{label}: 失败后必须保留原历史快照"),
            }
            assert_eq!(session.placement_previews.get(&patch.id), Some(&preview), "{label}");
            assert_eq!(app.world().get::<Visibility>(preview), Some(&Visibility::Inherited), "{label}");
            let expected_status = if undoing { "撤销未执行" } else { "重做未执行" };
            assert!(app.world().resource::<MapEditorState>().status.contains(expected_status), "{label}");
        }
    }
}

#[test]
fn global_celestial_history_restores_complete_snapshots_in_both_directions() {
    let before = feathrise_map_content::CelestialLightingProfile::default();
    let mut after = before.clone();
    after.day_base_illuminance_lux = 14_000.0;
    after.night_peak_illuminance_lux = 125.0;
    after.day_base_color_srgb = [0.8, 0.7, 0.6];
    after.night_base_color_srgb = [0.1, 0.2, 0.3];
    after.azimuth_offset_degrees = 405.0;
    after.light_source_angular_diameter_degrees = 1.25;
    let command = EditorCommand::GlobalCelestialLightingChanged {
        sequence: 1,
        before: before.clone(),
        after: after.clone(),
    };
    let mut global = GlobalEnvironmentAuthoringState::default();
    global.draft.celestial_lighting = after.clone();

    apply_global_celestial_history(&command, true, &mut global).expect("全局撤销快照应可应用");
    assert_eq!(global.draft.celestial_lighting, before);

    apply_global_celestial_history(&command, false, &mut global).expect("全局重做快照应可应用");
    let mut expected = after;
    expected.azimuth_offset_degrees = 45.0; // 作者快照采用时统一保存等价方位角。
    assert_eq!(global.draft.celestial_lighting, expected);
}

#[test]
fn water_stroke_diffs_undo_and_redo_mask_and_height_together() {
    let mut mask = vec![0_u8, 200_u8]; // 模拟当前命令撤销后的R8。
    let mut positions = vec![[0.0, 100.0, 0.0], [1.0, 50.0, 0.0]]; // 模拟当前GLB顶点Y。
    let mask_changes = vec![ByteChange {
        index: 0,   // 第一像素属于本次水笔。
        before: 0,  // 笔划前为空。
        after: 180, // 笔划后出现水覆盖。
    }];
    let height_changes = vec![HeightChange {
        index: 0,      // 同一世界笔划命中第一个高度样本。
        before: 100.0, // 笔划前地面高度。
        after: 70.0,   // 笔划后只下挖河床。
    }];

    assert!(apply_water_stroke_diffs(
        &mut mask,            // 当前R8。
        Some(&mut positions), // 当前GLB顶点位置。
        &mask_changes,        // R8局部差分。
        &height_changes,      // R16局部差分。
        false,                // 先执行重做方向。
    ));
    assert_eq!(mask, vec![180, 200]); // 重做完整应用R8 after。
    assert_eq!(positions, vec![[0.0, 70.0, 0.0], [1.0, 50.0, 0.0]]); // 重做只应用世界Y。

    assert!(apply_water_stroke_diffs(
        &mut mask,            // 已重做R8。
        Some(&mut positions), // 已重做GLB位置。
        &mask_changes,        // 同一R8差分。
        &height_changes,      // 同一R16差分。
        true,                 // 再执行撤销方向。
    ));
    assert_eq!(mask, vec![0, 200]); // 撤销完整恢复R8 before。
    assert_eq!(positions, vec![[0.0, 100.0, 0.0], [1.0, 50.0, 0.0]]); // 撤销恢复Y且保持X/Z。
}

#[test]
fn invalid_height_diff_rejects_the_whole_water_command_before_mask_write() {
    let mut mask = vec![0_u8]; // R8下标本身合法。
    let mut positions = vec![[0.0, 100.0, 0.0]]; // GLB只有一个顶点。
    let mask_changes = vec![ByteChange {
        index: 0,   // 若非原子实现会先错误改写该像素。
        before: 0,  // 笔划前值。
        after: 255, // 笔划后值。
    }];
    let height_changes = vec![HeightChange {
        index: 4,      // 故意越过R16数组范围。
        before: 100.0, // 无效命令的旧值。
        after: 70.0,   // 无效命令的新值。
    }];

    assert!(!apply_water_stroke_diffs(
        &mut mask,            // 当前R8。
        Some(&mut positions), // 当前GLB位置。
        &mask_changes,        // 合法R8差分。
        &height_changes,      // 非法R16差分。
        false,                // 尝试重做。
    ));
    assert_eq!(mask, vec![0]); // R16失败前置校验使R8保持原样。
    assert_eq!(positions, vec![[0.0, 100.0, 0.0]]); // GLB同样没有部分写入。
}
