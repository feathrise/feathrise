//! 源父仍可用但作者预览已退役时，按真实身份恢复预览，保护被其他操作接管的实体。

use super::*;
use bevy::ecs::system::RunSystemOnce;
use feathrise_map_content::{MapPlacementTerrainPatch, MapTerrainKind, TerrainId, TerrainObjectTransform};
use std::sync::Arc;

#[path = "deletion_ownership_tests.rs"]
mod deletion_ownership_tests; // 真实删除/恢复后的迟到正式根不得凭旧保存候选复活。

#[test]
fn terrain_patch_preview_recovers_retired_address_without_overwriting_another_owner() {
    for foreign_owner in [false, true] {
        let (mut app, root, _) = crate::viewport::placement_move::interaction_tests::app();
        app.add_plugins(bevy::transform::TransformPlugin)
            .init_asset::<bevy::world_serialization::WorldAsset>();
        let (map, source, candidate) = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
            source.id = TerrainId::parse("preview-recovery-source").unwrap();
            source.terrain.kind = MapTerrainKind::SparseClipmap;
            source.transform = TerrainObjectTransform { translation: [18.0, 6.0, -9.0],
                yaw_degrees: 23.0, pitch_degrees: 15.0, roll_degrees: -8.0,
                scale: [2.0, 3.0, 5.0] };
            draft.manifest.terrains = Some(Arc::from([source.clone()]));
            draft.placement_water_surface_count = 0;
            let record = &mut draft.placements.placements[0];
            record.water_surfaces.clear();
            record.terrain_patch = Some(MapPlacementTerrainPatch { terrain_id: Some(source.id.clone()), ..default() });
            let mut candidate = record.clone();
            candidate.translation[1] += 1.0;
            (draft.map_id.clone(), source, candidate)
        };
        let parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(
            app.world_mut(), &map, std::slice::from_ref(&source),
        )[0];
        app.world_mut().resource_mut::<EditorAuthoringSession>()
            .placement_previews.insert(candidate.id.clone(), root);
        let foreign = if foreign_owner {
            let mut foreign = candidate.clone();
            foreign.id = "another-preview-owner".into();
            app.world_mut().entity_mut(root).insert((MapPlacementInstance::new(&map, &foreign),
                Transform::from_xyz(9.0, 8.0, 7.0), Visibility::Hidden));
            Some(foreign)
        } else {
            app.world_mut().despawn(root);
            None
        };
        let expected = candidate.validated_transform(&map, Rect::from_corners(
            source.world_min.into(), source.world_max.into())).unwrap();
        let id = candidate.id.clone();
        assert_eq!(app.world_mut().run_system_once(move |mut commands: Commands, assets: Res<AssetServer>,
            mut session: ResMut<EditorAuthoringSession>, index: Res<PlacementInstanceIndex>,
            roots: Query<(Entity, &MapPlacementInstance)>, terrain: feathrise_scene::TerrainObjectQueries| {
            let bounds = crate::authoring::placement_record_bounds(session.draft.as_ref().unwrap(), &candidate)?;
            sync_placement_record_world_with_terrain(&mut commands, &assets, &mut session, &index,
                &roots, &candidate, bounds, true, true, &terrain)
        }).unwrap().unwrap(), 1);
        let recovered = app.world().resource::<EditorAuthoringSession>().placement_previews[&id];
        assert_ne!(recovered, root);
        assert_eq!(app.world().get::<ChildOf>(recovered).unwrap().parent(), parent);
        assert_eq!(app.world().get::<Transform>(recovered), Some(&expected));
        assert_eq!(app.world().get::<MapPlacementInstance>(recovered).unwrap().id(), id);
        assert!(app.world().get::<feathrise_scene::TerrainPatchCollisionRequest>(recovered).is_none());
        app.update();
        let actual = app.world().get::<GlobalTransform>(recovered).unwrap().to_matrix();
        let expected = source.transform.as_transform().to_matrix() * expected.to_matrix();
        for point in [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z] {
            assert!(actual.transform_point3(point).abs_diff_eq(expected.transform_point3(point), 0.001));
        }
        if let Some(foreign) = foreign {
            assert_eq!(app.world().get::<MapPlacementInstance>(root).unwrap().id(), foreign.id);
            assert_eq!(app.world().get::<Transform>(root).unwrap().translation, Vec3::new(9.0, 8.0, 7.0));
            assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
        }
    }
}

/// 删除按钮沿用真实作者选择和活动根索引；旧预览地址不应获得销毁实体的信用。
fn deletion_fixture(terrain_patch: bool) -> (App, Entity, MapPlacementRecord, feathrise_map_content::MapId) {
    let (mut app, root, _) = crate::viewport::placement_move::interaction_tests::app();
    app.add_observer(crate::ui::placement_water::register_placement_instance_root)
        .add_observer(crate::ui::placement_water::unregister_placement_instance_root);
    let (map, record, source) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let source = terrain_patch.then(|| {
            let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
            source.id = TerrainId::parse("preview-deletion-source").unwrap();
            source.terrain.kind = MapTerrainKind::SparseClipmap;
            source.transform = TerrainObjectTransform::default();
            source
        });
        if let Some(source) = &source {
            draft.manifest.terrains = Some(Arc::from([source.clone()]));
        }
        draft.placement_water_surface_count = 0;
        let record = &mut draft.placements.placements[0];
        record.water_surfaces.clear();
        record.terrain_patch = source.as_ref().map(|source| MapPlacementTerrainPatch {
            terrain_id: Some(source.id.clone()), ..default()
        });
        (draft.map_id.clone(), record.clone(), source)
    };
    app.world_mut().entity_mut(root).insert(MapPlacementInstance::new(&map, &record));
    if let Some(source) = source {
        let parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(
            app.world_mut(), &map, std::slice::from_ref(&source),
        )[0];
        app.world_mut().entity_mut(root).insert(ChildOf(parent));
    }
    (app, root, record, map)
}

fn click_delete(app: &mut App) {
    let button = app.world_mut().spawn((feathrise_editor_ui::EditorButtonClick::triggered(),
        MapEditorPlacementTransformActionButton(PlacementTransformAction::Delete))).id();
    app.world_mut().run_system_once(handle_placement_transform_actions).unwrap();
    app.world_mut().despawn(button);
}

#[test]
fn terrain_patch_delete_stale_preview_addresses_preserve_other_owners_and_hide_active_root() {
    for terrain_patch in [false, true] {
        for stale_address in ["dead", "same-map-other-id", "other-map-same-id"] {
            let (mut app, root, record, map) = deletion_fixture(terrain_patch);
            let foreign = if stale_address == "dead" {
                app.world_mut().despawn(root);
                None
            } else {
                let mut foreign = record.clone();
                let foreign_map = if stale_address == "other-map-same-id" {
                    feathrise_map_content::MapId::parse("MAP-foreign-delete-owner").unwrap()
                } else {
                    foreign.id = "another-delete-owner".into();
                    map.clone()
                };
                let transform = Transform::from_xyz(9.0, 8.0, 7.0);
                app.world_mut().entity_mut(root).insert((MapPlacementInstance::new(&foreign_map, &foreign),
                    transform, Visibility::Inherited));
                let child = app.world_mut().spawn(ChildOf(root)).id();
                Some((foreign_map, foreign.id, transform, child))
            };
            // 退役预览不能遮蔽同记录的新活动根；删除草稿时必须同时隐藏它。
            let active = app.world_mut().spawn((MapPlacementInstance::new(&map, &record),
                Transform::default(), Visibility::Inherited)).id();
            click_delete(&mut app);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert!(session.draft.as_ref().unwrap().placements.placements.is_empty(), "{stale_address}");
            assert!(!session.placement_previews.contains_key(&record.id), "{stale_address}");
            assert!(matches!(session.undo.last(), Some(EditorCommand::PlacementRemoved {
                had_author_preview: false, ..
            })), "{stale_address}");
            assert_eq!(app.world().get::<Visibility>(active), Some(&Visibility::Hidden), "{stale_address}");
            if let Some((foreign_map, foreign_id, transform, child)) = foreign {
                let instance = app.world().get::<MapPlacementInstance>(root).expect("外部拥有者的根必须保留");
                assert_eq!(instance.map_id(), &foreign_map);
                assert_eq!(instance.id(), foreign_id);
                assert_eq!(app.world().get::<Transform>(root), Some(&transform));
                assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
                assert_eq!(app.world().get::<ChildOf>(child).unwrap().parent(), root);
            }
        }
    }
}

#[test]
fn terrain_patch_delete_owned_preview_still_despawns_root_and_preserves_rebuild_history() {
    for terrain_patch in [false, true] {
        let (mut app, root, record, map) = deletion_fixture(terrain_patch);
        let child = app.world_mut().spawn(ChildOf(root)).id();
        let formal_parent = app.world_mut().spawn((Transform::from_xyz(8.0, 9.0, 10.0), Visibility::Inherited)).id();
        let formal_transform = Transform::from_xyz(25.0, 26.0, 27.0).with_scale(Vec3::new(1.5, 2.5, 3.5));
        // 正式根可能在作者预览建立后才到达，不能假定它已经被先前预览入口隐藏。
        let formal = app.world_mut().spawn((MapPlacementInstance::new(&map, &record),
            formal_transform, Visibility::Inherited, ChildOf(formal_parent))).id();
        let formal_child = app.world_mut().spawn(ChildOf(formal)).id();
        click_delete(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.draft.as_ref().unwrap().placements.placements.is_empty());
        assert!(!session.placement_previews.contains_key(&record.id));
        assert!(matches!(session.undo.last(), Some(EditorCommand::PlacementRemoved {
            had_author_preview: true, ..
        })));
        assert!(app.world().get_entity(root).is_err());
        assert!(app.world().get_entity(child).is_err());
        assert!(app.world().get_entity(formal).is_ok());
        assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<Transform>(formal), Some(&formal_transform));
        assert_eq!(app.world().get::<ChildOf>(formal).map(ChildOf::parent), Some(formal_parent));
        assert_eq!(app.world().get::<ChildOf>(formal_child).map(ChildOf::parent), Some(formal));
    }
}

#[test]
fn terrain_patch_delete_builtin_formal_root_disables_collision_without_rebuilding_visual() {
    use avian3d::prelude::{Collider, ColliderDisabled};
    use feathrise_map_content::MapPlacementAssetTarget;

    for owned_preview in [false, true] {
        let (mut app, root, mut record, map) = deletion_fixture(false);
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>();
        record.asset_path.clear();
        record.asset_target = MapPlacementAssetTarget::DEFAULT_CUBOID;
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.draft.as_mut().unwrap().placements.placements[0] = record.clone();
            if !owned_preview { session.placement_previews.clear(); }
        }
        app.world_mut().entity_mut(root).insert(MapPlacementInstance::new(&map, &record));
        let parent = app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited)).id();
        let transform = Transform::from_xyz(8.0, 9.0, 10.0);
        let formal = app.world_mut().spawn((MapPlacementInstance::new(&map, &record),
            transform, Visibility::Inherited, ChildOf(parent))).id();
        assert!(app.world_mut().run_system_once(move |mut commands: Commands| {
            feathrise_scene::spawn_builtin_placement_visual(&mut commands, formal, record.asset_target)
        }).unwrap());
        let visual = app.world().get::<Children>(formal).unwrap()[0];
        assert!(app.world().get::<Collider>(visual).is_some());
        assert!(app.world().get::<ColliderDisabled>(visual).is_none());
        let mesh = app.world().get::<Mesh3d>(visual).unwrap().0.clone();
        click_delete(&mut app);
        assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<Transform>(formal), Some(&transform));
        assert_eq!(app.world().get::<ChildOf>(formal).map(ChildOf::parent), Some(parent));
        assert_eq!(app.world().get::<Children>(formal).unwrap().iter().collect::<Vec<_>>(), vec![visual]);
        assert_eq!(app.world().get::<Mesh3d>(visual).unwrap().0, mesh);
        assert!(app.world().get::<Collider>(visual).is_some());
        assert!(app.world().get::<ColliderDisabled>(visual).is_some());
        assert_eq!(app.world().get::<MapPlacementInstance>(formal).unwrap().asset_target(),
            MapPlacementAssetTarget::DEFAULT_CUBOID);
    }
}
