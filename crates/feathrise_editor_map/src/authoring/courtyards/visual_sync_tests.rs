//! 整组撤销新增与重做删除先采用作者记录，再由真实视觉队列回收自有预览、隐藏正式根。

use crate::authoring::{EditorAuthoringSession, EditorCommand};
use crate::authoring::commands::CourtyardBatchChange;
use crate::authoring::courtyards::{CourtyardAuthoringState, sync_visuals};
use crate::resources::MapEditorState;
use crate::viewport::placement_move::interaction_tests::{app, batch_undo_resources, record};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use feathrise_map_content::{MapId, MapPlacementTerrainPatch, MapTerrainKind, TerrainId};
use feathrise_scene::MapPlacementInstance;
use std::sync::Arc;

#[test]
fn terrain_patch_courtyard_delete_undo_added_queue_preserves_preview_ownership() {
    assert_queued_delete(0, false);
}

#[test]
fn terrain_patch_courtyard_delete_redo_removed_queue_preserves_preview_ownership() {
    assert_queued_delete(1, false);
}

#[test]
fn terrain_patch_courtyard_delete_undo_added_hides_formal_without_rewriting_pose_or_hierarchy() {
    assert_queued_delete(0, true);
}

#[test]
fn terrain_patch_courtyard_delete_redo_removed_hides_formal_without_rewriting_pose_or_hierarchy() {
    assert_queued_delete(1, true);
}

#[test]
fn terrain_patch_courtyard_delete_undo_added_cleans_queue_after_source_is_disabled_or_removed() {
    for source_change in ["disabled", "removed"] {
        assert_queued_delete_after_source_change(0, true, Some(source_change));
    }
}

#[test]
fn terrain_patch_courtyard_delete_redo_removed_cleans_queue_after_source_is_disabled_or_removed() {
    for source_change in ["disabled", "removed"] {
        assert_queued_delete_after_source_change(1, true, Some(source_change));
    }
}

/// 真实历史入口建立队列，独立消费系统验证作者删除不授予旧实体地址外部回收权。
fn assert_queued_delete(operation: usize, with_formal: bool) {
    assert_queued_delete_after_source_change(operation, with_formal, None);
}

fn assert_queued_delete_after_source_change(operation: usize, with_formal: bool, source_change: Option<&str>) {
    for owner in ["dead", "other-id", "other-map", "owned"] {
        let (mut app, _, _) = app();
        batch_undo_resources(&mut app);
        app.add_observer(crate::ui::placement_water::register_placement_instance_root)
            .add_observer(crate::ui::placement_water::unregister_placement_instance_root);
        let water = record();
        let mut target = water.clone();
        target.id = "courtyard-queued-delete-target".into();
        target.water_surfaces.clear();
        let (map, source) = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
            source.id = TerrainId::parse("courtyard-delete-source").unwrap();
            source.terrain.kind = MapTerrainKind::SparseClipmap;
            target.terrain_patch = Some(MapPlacementTerrainPatch {
                terrain_id: Some(source.id.clone()), ..default()
            });
            draft.manifest.terrains = Some(Arc::from([source.clone()]));
            (draft.map_id.clone(), source)
        };
        let parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(
            app.world_mut(), &map, &[source],
        )[0];
        let mut actual = target.clone();
        if owner == "other-id" { actual.id = "foreign-courtyard-owner".into(); }
        let actual_map = if owner == "other-map" {
            MapId::parse("MAP-foreign-courtyard-owner").unwrap()
        } else { map.clone() };
        let external_transform = Transform::from_xyz(21.0, 22.0, 23.0).with_scale(Vec3::new(2.0, 3.0, 4.0));
        let preview = app.world_mut().spawn((MapPlacementInstance::new(&actual_map, &actual),
            external_transform, Visibility::Inherited, ChildOf(parent))).id();
        let child = app.world_mut().spawn((Transform::IDENTITY, ChildOf(preview))).id();
        if owner == "dead" { app.world_mut().despawn(preview); }
        let formal_transform = Transform::from_xyz(8.0, 9.0, 10.0).with_rotation(Quat::from_rotation_z(0.4));
        let undoing = operation == 0;
        let command = if undoing {
            CourtyardBatchChange::new(vec![target.clone()], Vec::new()).unwrap()
        } else {
            CourtyardBatchChange::new_removed(vec![(1, target.clone())], 2).unwrap()
        };
        let revisions = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.placements.placements = vec![water.clone(), target.clone()];
            draft.placement_water_surface_count = water.water_surfaces.len();
            session.placement_previews.clear();
            session.placement_previews.insert(target.id.clone(), preview);
            session.undo.clear(); session.redo.clear();
            if undoing { session.undo.push(EditorCommand::CourtyardBatch(Box::new(command))); }
            else { session.redo.push(EditorCommand::CourtyardBatch(Box::new(command))); }
            (session.revision, session.placement_structure_revision)
        };
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.undo_requested = undoing; editor.redo_requested = !undoing;
        }
        let label = format!("operation={operation}, owner={owner}, formal={with_formal}, source={source_change:?}");
        crate::authoring::drive_undo_redo_to_completion(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, vec![water.clone()], "{label}");
        assert_eq!(session.placement_previews.get(&target.id), Some(&preview), "视觉回收只能由队列消费执行：{label}");
        assert_eq!((session.revision, session.placement_structure_revision),
            (revisions.0.wrapping_add(1), revisions.1.wrapping_add(1)), "{label}");
        assert_eq!((session.undo.len(), session.redo.len()), if undoing { (0, 1) } else { (1, 0) }, "{label}");
        assert!(app.world().resource::<CourtyardAuthoringState>().visual_sync_pending(), "{label}");
        if let Some(source_change) = source_change {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            match source_change {
                "disabled" => Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0].enabled = false,
                "removed" => draft.manifest.terrains = Some(Arc::from([])),
                _ => panic!("未知来源变更"),
            }
            session.terrain_revision = session.terrain_revision.wrapping_add(1);
        } // 历史已经成功采用，失效来源不能撤销隐藏及回收责任，更不能据此丢弃整条视觉队列。
        let terrain_revision = app.world().resource::<EditorAuthoringSession>().terrain_revision;
        // 正式根在历史入队、来源变更后才到达，消费时仍须hide-only，不能依赖旧同步已隐藏它。
        let formal = with_formal.then(|| app.world_mut().spawn((MapPlacementInstance::new(&map, &target),
            formal_transform, Visibility::Inherited, ChildOf(parent))).id());
        let formal_child = formal.map(|formal| app.world_mut().spawn((Transform::IDENTITY, ChildOf(formal))).id());
        app.world_mut().run_system_once(sync_visuals).unwrap();
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, vec![water.clone()], "{label}");
        assert_eq!(session.draft.as_ref().unwrap().placement_water_surface_count, water.water_surfaces.len(), "{label}");
        assert_eq!((session.revision, session.placement_structure_revision),
            (revisions.0.wrapping_add(1), revisions.1.wrapping_add(1)), "视觉消费不得再采用一次作者历史：{label}");
        assert_eq!(session.terrain_revision, terrain_revision, "隐藏消费不能改写来源修订：{label}");
        assert!(!session.placement_previews.contains_key(&target.id), "{label}");
        assert!(!app.world().resource::<CourtyardAuthoringState>().visual_sync_pending(), "{label}");
        if owner == "other-id" || owner == "other-map" {
            assert!(app.world().get_entity(preview).is_ok(), "旧映射不能回收外部拥有者：{label}");
            assert_eq!(app.world().get::<Transform>(preview), Some(&external_transform), "{label}");
            assert_eq!(app.world().get::<Visibility>(preview), Some(&Visibility::Inherited), "{label}");
            assert_eq!(app.world().get::<ChildOf>(preview).map(ChildOf::parent), Some(parent), "{label}");
            let instance = app.world().get::<MapPlacementInstance>(preview).unwrap();
            assert_eq!((instance.map_id(), instance.id()), (&actual_map, actual.id.as_str()), "{label}");
            assert_eq!(app.world().get::<ChildOf>(child).map(ChildOf::parent), Some(preview), "{label}");
        } else {
            assert!(app.world().get_entity(preview).is_err(), "自有预览正常回收或忽略退役地址：{label}");
            assert!(app.world().get_entity(child).is_err(), "{label}");
        }
        if let Some(formal) = formal {
            assert!(app.world().get_entity(formal).is_ok(), "正式根不提前回收：{label}");
            assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden), "{label}");
            assert_eq!(app.world().get::<Transform>(formal), Some(&formal_transform), "隐藏不能把作者值覆写正式姿态：{label}");
            assert_eq!(app.world().get::<ChildOf>(formal).map(ChildOf::parent), Some(parent), "{label}");
            assert_eq!(app.world().get::<ChildOf>(formal_child.unwrap()).map(ChildOf::parent), Some(formal), "{label}");
            let instance = app.world().get::<MapPlacementInstance>(formal).unwrap();
            assert_eq!((instance.map_id(), instance.id()), (&map, target.id.as_str()), "{label}");
        }
    }
}
