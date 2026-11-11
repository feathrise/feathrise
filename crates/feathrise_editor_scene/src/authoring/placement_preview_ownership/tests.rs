//! 独立作者预览定向遮蔽、精确拥有权交接与碰撞归还顺序的CPU生命周期回归。

use super::*;
use avian3d::prelude::{Collider, ColliderDisabled, RigidBody, RigidBodyDisabled};
use bevy::asset::AssetPlugin;
use bevy::ecs::system::RunSystemOnce;
use feathrise_scene_content::{ScenePlacementAssetTarget, ScenePlacementRecord, ScenePlacementTerrainPatch};

use crate::authoring::resources::PlacementPreviewHandoffSnapshot;
use crate::authoring::water_handoff::schedule_placement_preview_cleanup;
use crate::authoring::{WaterFileHandoff, cleanup_placement_handoff_previews, spawn_placement_preview};
use crate::ui::placement_water::{register_placement_instance_root, unregister_placement_instance_root};

fn record() -> ScenePlacementRecord {
    ScenePlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None,
        id: "owned-cuboid".into(),
        asset_path: String::new(),
        asset_target: ScenePlacementAssetTarget::DEFAULT_CUBOID,
        building_preset_id: None,
        courtyard: None,
        translation: [0.0; 3],
        height_mode: feathrise_scene_content::ScenePlacementHeightMode::FixedHeight,
        terrain_offset: 0.0,
        yaw_degrees: 0.0,
        scale: [1.0; 3],
        water_surfaces: Vec::new(),
    }
}

fn fixture(record: &ScenePlacementRecord) -> (App, SceneId) {
    let map = SceneId::parse("MAP-independent-preview-owner").unwrap();
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.scene_id = map.clone();
    draft.manifest.id = map.clone();
    draft.placements.scene_id = map.clone();
    draft.placements.placements = vec![record.clone()];
    draft.placement_water_surface_count = record.water_surfaces.len();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_resource::<PlacementInstanceIndex>()
        .init_resource::<WaterFileHandoff>()
        .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
        .add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root);
    (app, map)
}

fn author_preview(app: &mut App, map: &SceneId, record: &ScenePlacementRecord) -> Entity {
    let (map, record) = (map.clone(), record.clone());
    app.world_mut().run_system_once(move |mut commands: Commands, assets: Res<AssetServer>,
        mut session: ResMut<EditorAuthoringSession>| {
        let owner = spawn_placement_preview(&mut commands, &assets, &map, &record);
        session.placement_previews.insert(record.id.clone(), owner);
        claim_preview(&mut commands, &map, &record.id, owner);
        owner
    }).unwrap()
}

/// 正式内置根共用父级刚体；子碰撞经生产构造入口安装，不能用根级禁用冒充逐对象隔离。
fn formal_builtin(app: &mut App, map: &SceneId, record: &ScenePlacementRecord, parent: Entity,
    explicit_visibility: bool) -> (Entity, Entity) {
    let root = app.world_mut().spawn((Transform::IDENTITY, ChildOf(parent))).id();
    if explicit_visibility { app.world_mut().entity_mut(root).insert(Visibility::Inherited); }
    app.world_mut().entity_mut(root).insert(ScenePlacementInstance::new(map, record));
    let target = record.asset_target;
    app.world_mut().run_system_once(move |mut commands: Commands| {
        assert!(feathrise_scene::spawn_builtin_placement_visual(&mut commands, root, target));
    }).unwrap();
    app.world_mut().flush();
    let visual = app.world().get::<Children>(root).unwrap()[0];
    assert!(app.world().get::<Collider>(visual).is_some());
    (root, visual)
}

fn shared_body(app: &mut App) -> Entity {
    app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited, RigidBody::Static)).id()
}

fn assert_suppressed(app: &App, formal: Entity, visual: Entity, owner: Entity) {
    assert_eq!(app.world().get::<PlacementPreviewSuppression>(formal).map(|mask| mask.owner), Some(owner));
    assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden));
    assert!(app.world().get::<ColliderDisabled>(visual).is_some());
}

#[test]
fn ownership_late_formal_add_is_masked_without_touching_other_ids_or_shared_body() {
    let record = record();
    let (mut app, map) = fixture(&record);
    let body = shared_body(&mut app);
    let (existing, existing_visual) = formal_builtin(&mut app, &map, &record, body, true);
    let owner = author_preview(&mut app, &map, &record);
    assert_suppressed(&app, existing, existing_visual, owner);

    // 迟到根尚无Visibility时仍须补默认值并遮蔽；它的可视/碰撞安装也不能绕开隐藏。
    let (late, late_visual) = formal_builtin(&mut app, &map, &record, body, false);
    assert_suppressed(&app, late, late_visual, owner);
    let mut neighbor = record.clone();
    neighbor.id = "neighbor".into();
    let (other, other_visual) = formal_builtin(&mut app, &map, &neighbor, body, true);
    let foreign_scene = SceneId::parse("MAP-independent-preview-foreign").unwrap();
    let (foreign, foreign_visual) = formal_builtin(&mut app, &foreign_scene, &record, body, true);
    for (root, visual) in [(other, other_visual), (foreign, foreign_visual)] {
        assert!(app.world().get::<PlacementPreviewSuppression>(root).is_none());
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
        assert!(app.world().get::<ColliderDisabled>(visual).is_none());
    }
    assert!(app.world().get::<RigidBodyDisabled>(body).is_none());
    assert!(app.world().get::<RigidBodyDisabled>(owner).is_none());
}

#[test]
fn ownership_replacement_rejects_old_release_and_old_queued_claim() {
    let record = record();
    let (mut app, map) = fixture(&record);
    let body = shared_body(&mut app);
    let (formal, visual) = formal_builtin(&mut app, &map, &record, body, true);
    let first = author_preview(&mut app, &map, &record);
    let second = author_preview(&mut app, &map, &record);
    assert_ne!(first, second);
    assert_suppressed(&app, formal, visual, second);
    let (claim_scene, id) = (map.clone(), record.id.clone());
    app.world_mut().run_system_once(move |mut commands: Commands| {
        claim_preview(&mut commands, &claim_scene, &id, first);
    }).unwrap();
    release_preview(app.world_mut(), &map, &record.id, first);
    app.world_mut().flush();
    assert_suppressed(&app, formal, visual, second);
    assert_eq!(app.world().get::<PlacementPreviewSuppression>(formal).unwrap().original_visibility,
        Visibility::Inherited, "替换owner须保留原正式显隐，不能把上一覆盖层的Hidden当原值");
    app.world_mut().despawn(first);
    app.world_mut().flush();
    assert_suppressed(&app, formal, visual, second);
}

#[derive(Resource)]
struct ReleaseObservation { formal: Entity, owner: Entity, owner_visual: Entity, observed: bool }

fn observe_formal_release(event: On<Remove<PlacementPreviewSuppression>>,
    disabled_authors: Query<&RigidBodyDisabled>,
    disabled_colliders: Query<&ColliderDisabled>,
    mut observation: ResMut<ReleaseObservation>) {
    if event.entity != observation.formal { return; }
    assert!(disabled_authors.get(observation.owner).is_ok(),
        "正式遮蔽归还之前必须先停作者独立刚体，不能等跨帧DFS最后销毁；角色承托由场景开关回归覆盖");
    assert!(disabled_colliders.get(observation.owner_visual).is_ok(),
        "没有自身刚体的组预览也必须先停唯一子碰撞，再归还正式碰撞");
    observation.observed = true;
}

fn exact_cleanup_quiesces_author_before_restoring_formal_collision(shared_author_body: bool) {
    let record = record();
    let (mut app, map) = fixture(&record);
    let body = shared_body(&mut app);
    let (formal, visual) = formal_builtin(&mut app, &map, &record, body, true);
    let owner = author_preview(&mut app, &map, &record);
    let owner_visual = app.world().get::<Children>(owner).unwrap()[0];
    let author_parent = shared_author_body.then(|| shared_body(&mut app));
    if let Some(parent) = author_parent {
        app.world_mut().entity_mut(owner).remove::<RigidBody>().insert(ChildOf(parent));
    } // 真实整组预览可共享父刚体；根级禁用不能替代逐对象停用直接子碰撞。
    assert!(app.world().get::<Collider>(owner_visual).is_some());
    for _ in 0..80 {
        app.world_mut().spawn((Visibility::Inherited, ChildOf(owner)));
    }
    let snapshot = PlacementPreviewHandoffSnapshot::capture(app.world(), &map, &record.id, owner).unwrap();
    schedule_placement_preview_cleanup(&mut app.world_mut().resource_mut::<WaterFileHandoff>(),
        &map, vec![snapshot]).expect("独立保存清理槽应可取得当前作者版本");
    app.insert_resource(ReleaseObservation { formal, owner, owner_visual, observed: false })
        .add_observer(observe_formal_release)
        .add_systems(Update, cleanup_placement_handoff_previews);
    app.update();
    app.world_mut().flush();

    assert!(app.world().resource::<ReleaseObservation>().observed);
    assert!(app.world().get_entity(owner).is_ok(), "宽树尚未回收完，不能靠整树销毁掩盖归还顺序");
    assert!(app.world().get::<RigidBodyDisabled>(owner).is_some());
    assert!(app.world().get::<ScenePlacementInstance>(owner).is_none());
    assert_eq!(app.world().get::<Visibility>(owner), Some(&Visibility::Hidden));
    assert!(app.world().get::<Collider>(owner_visual).is_some());
    assert!(app.world().get::<PlacementPreviewSuppression>(formal).is_none());
    assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Inherited));
    assert!(app.world().get::<ColliderDisabled>(visual).is_none());
    assert!(app.world().get::<RigidBodyDisabled>(body).is_none());
    if let Some(parent) = author_parent {
        assert!(app.world().get::<RigidBodyDisabled>(parent).is_none());
        assert!(app.world().get::<RigidBody>(owner).is_none());
    }
    assert!(!app.world().resource::<EditorAuthoringSession>().placement_previews.contains_key(&record.id));
    assert!(app.world().resource::<WaterFileHandoff>().placement_preview_cleanup_pending());
    for _ in 0..8 {
        app.update();
        if !app.world().resource::<WaterFileHandoff>().placement_preview_cleanup_pending() { break; }
    }
    assert!(app.world().get_entity(owner).is_err());
    assert!(app.world().get_entity(owner_visual).is_err());
    assert!(app.world().get_entity(formal).is_ok());
    assert!(app.world().get::<ColliderDisabled>(visual).is_none());
}

#[test]
fn ownership_exact_cleanup_quiesces_author_body_before_restoring_formal_collision() {
    exact_cleanup_quiesces_author_before_restoring_formal_collision(false);
}

#[test]
fn ownership_exact_cleanup_quiesces_shared_body_preview_child_before_restoring_formal_collision() {
    exact_cleanup_quiesces_author_before_restoring_formal_collision(true);
}

#[test]
fn ownership_cold_withdraw_explicitly_returns_formal_even_after_session_index_is_cleared() {
    let record = record();
    let (mut app, map) = fixture(&record);
    let body = shared_body(&mut app);
    let (formal, visual) = formal_builtin(&mut app, &map, &record, body, true);
    let owner = author_preview(&mut app, &map, &record);
    assert_suppressed(&app, formal, visual, owner);
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        session.placement_previews.clear();
        withdraw_cold_preview(&mut commands, owner);
        commands.entity(owner).try_despawn();
    }).unwrap();
    app.world_mut().flush();
    assert!(app.world().get_entity(owner).is_err());
    assert!(app.world().get::<PlacementPreviewSuppression>(formal).is_none());
    assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Inherited));
    assert!(app.world().get::<ColliderDisabled>(visual).is_none());
    assert!(app.world().get::<RigidBodyDisabled>(body).is_none());
}

#[test]
fn ownership_deleting_author_preview_does_not_implicitly_revive_deleted_formal() {
    let record = record();
    let (mut app, map) = fixture(&record);
    let body = shared_body(&mut app);
    let (formal, visual) = formal_builtin(&mut app, &map, &record, body, true);
    let owner = author_preview(&mut app, &map, &record);
    let (delete_scene, id) = (map.clone(), record.id.clone());
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        crate::authoring::placement_deletion::mark_deleted(&mut commands, &mut session, &delete_scene, &id);
        session.placement_previews.remove(&id);
        session.draft.as_mut().unwrap().placements.placements.clear();
        commands.entity(owner).try_despawn();
    }).unwrap();
    app.world_mut().flush();
    assert!(app.world().get_entity(owner).is_err());
    assert!(crate::authoring::placement_deletion::suppression_is_current(app.world(), formal));
    assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden));
    assert!(app.world().get::<ColliderDisabled>(visual).is_some());
    assert!(app.world().get::<RigidBodyDisabled>(body).is_none());
}

#[test]
fn ownership_terrain_formal_mask_keeps_existing_physics_under_terrain_protocol() {
    let mut record = record();
    record.asset_path = "gltf/terrain-preview.glb".into();
    record.asset_target = ScenePlacementAssetTarget::Scene0;
    record.terrain_patch = Some(ScenePlacementTerrainPatch::default());
    let (mut app, map) = fixture(&record);
    let formal_transform = Transform::from_xyz(3.0, 2.0, 1.0);
    let formal = app.world_mut().spawn((ScenePlacementInstance::new(&map, &record),
        formal_transform, Visibility::Inherited, RigidBody::Static)).id();
    // 只提供已经安装的CPU碰撞证据，验证遮蔽入口不改物理；不声称驱动真实GLB烘焙/联合采用。
    let collision = app.world_mut().spawn((Collider::cuboid(2.0, 1.0, 2.0), ChildOf(formal))).id();
    let owner = author_preview(&mut app, &map, &record);
    assert_eq!(app.world().get::<PlacementPreviewSuppression>(formal).map(|mask| mask.owner), Some(owner));
    assert_eq!(app.world().get::<Visibility>(formal), Some(&Visibility::Hidden));
    assert_eq!(app.world().get::<Transform>(formal), Some(&formal_transform));
    assert!(app.world().get::<RigidBodyDisabled>(formal).is_none());
    assert!(app.world().get::<Collider>(collision).is_some());
    assert!(app.world().get::<ColliderDisabled>(collision).is_none());
    assert_eq!(app.world().get::<ChildOf>(collision).map(ChildOf::parent), Some(formal));
    assert!(app.world().get::<RigidBody>(owner).is_none());
    assert!(app.world().get::<feathrise_scene::TerrainPatchCollisionRequest>(owner).is_none());
}
