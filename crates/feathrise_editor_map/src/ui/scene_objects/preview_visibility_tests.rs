//! 正式根的作者遮蔽与场景树隐藏叠加回归；验证释放、冷重载和内置子碰撞的独立责任。

use super::visibility;
use crate::{
    authoring::{EditorAuthoringSession, test_heightfield_draft,
        placement_deletion::{self, PlacementDeletionSuppression},
        placement_preview_ownership::components::PlacementPreviewSuppression},
    resources::MapEditorState,
    ui::{gltf_browser::EditorGltfBrowserState, placement_water::{
        PlacementInstanceIndex, register_placement_instance_root, unregister_placement_instance_root,
    }},
};
use avian3d::prelude::{ColliderDisabled, RigidBody, RigidBodyDisabled};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use feathrise_map_content::{MapPlacementAssetTarget, MapPlacementRecord};
use feathrise_scene::MapPlacementInstance;
use std::sync::Arc;

fn fixture(hidden: bool, builtin: bool) -> (App, Entity, Entity) {
    let mut draft = test_heightfield_draft(Arc::new(vec![2; 121]));
    let mut record: MapPlacementRecord = serde_json::from_value(serde_json::json!({
        "id":"preview-suppressed", "asset_path":"gltf/preview-suppressed.glb",
        "translation":[0,1,0], "yaw_degrees":0, "scale":[1,1,1],
    })).unwrap();
    if builtin {
        record.asset_path.clear();
        record.asset_target = MapPlacementAssetTarget::DEFAULT_CUBOID;
    }
    Arc::make_mut(&mut record.editor).hidden = hidden;
    draft.placements.placements.push(record.clone());
    let map = draft.map_id.clone();
    let mut browser = EditorGltfBrowserState::default();
    browser.select_indexed_test_placement(&record.id, 0, 0);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<PlacementInstanceIndex>()
        .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
        .insert_resource(browser)
        .insert_resource(MapEditorState { open: true, selected_map_id: Some(map.clone()), ..default() })
        .add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root)
        .add_systems(Update, visibility::sync_visibility);
    super::install_visibility_observers_for_tests(&mut app);
    let batch = app.world_mut().spawn((Transform::IDENTITY, RigidBody::Static, RigidBodyDisabled)).id();
    let root = app.world_mut().spawn((MapPlacementInstance::new(&map, &record),
        Transform::IDENTITY, Visibility::Inherited, ChildOf(batch))).id();
    if builtin {
        app.world_mut().run_system_once(move |mut commands: Commands| {
            feathrise_scene::spawn_builtin_placement_visual(&mut commands, root, MapPlacementAssetTarget::DEFAULT_CUBOID);
        }).unwrap();
    }
    (app, root, batch)
}

fn suppress(app: &mut App, root: Entity) {
    let original_visibility = super::placement_base_visibility(app.world(), root);
    let owner = app.world_mut().spawn_empty().id();
    app.world_mut().entity_mut(root).insert(PlacementPreviewSuppression { owner, original_visibility });
    *app.world_mut().get_mut::<Visibility>(root).unwrap() = Visibility::Hidden;
    // 模拟正式所有权模块的真实合同：自身遮蔽直接改值，不发布外部Insert显隐意图。
}

fn release(app: &mut App, root: Entity) {
    let original = app.world().get::<PlacementPreviewSuppression>(root).unwrap().original_visibility;
    app.world_mut().entity_mut(root).remove::<PlacementPreviewSuppression>();
    super::restore_placement_base_visibility(app.world_mut(), root, original);
}

fn set_deleted(app: &mut App, deleted: bool) {
    app.world_mut().run_system_once(move |mut commands: Commands, mut session: ResMut<EditorAuthoringSession>| {
        let (map, id) = {
            let draft = session.draft.as_ref().unwrap();
            (draft.map_id.clone(), draft.placements.placements[0].id.clone())
        };
        if deleted { placement_deletion::mark_deleted(&mut commands, &mut session, &map, &id); }
        else { placement_deletion::restore_deleted(&mut commands, &mut session, &map, &id); }
    }).unwrap(); // 通过真实文档票据取得删除mask，夹具不构造私有生命周期证据。
}

#[test]
fn visibility_intent_tracks_same_value_insert_and_reclaims_removed_root_identity() {
    let (mut app, root, _) = fixture(false, false);
    let initial = app.world().resource::<visibility::ObjectVisibilityState>().visibility_intent(root).unwrap();
    let instance = app.world().get::<MapPlacementInstance>(root).unwrap().clone();
    app.world_mut().entity_mut(root).insert(instance.clone());
    assert!(app.world().resource::<visibility::ObjectVisibilityState>().visibility_intent_matches(root, &initial),
        "正常重施完整MPI不能撤销显隐票据");
    app.world_mut().entity_mut(root).insert(Visibility::Hidden);
    let hidden = app.world().resource::<visibility::ObjectVisibilityState>().visibility_intent(root).unwrap();
    app.world_mut().entity_mut(root).insert(Visibility::Hidden);
    assert!(!app.world().resource::<visibility::ObjectVisibilityState>().visibility_intent_matches(root, &hidden),
        "相同Hidden也必须记录新的外部意图");
    app.world_mut().entity_mut(root).remove::<MapPlacementInstance>();
    assert!(app.world().resource::<visibility::ObjectVisibilityState>().visibility_intent(root).is_none());
    app.world_mut().entity_mut(root).insert(instance);
    assert!(!app.world().resource::<visibility::ObjectVisibilityState>().visibility_intent_matches(root, &initial),
        "同Entity重新登记不能复用已撤销的旧票据");
    app.world_mut().despawn(root);
    assert!(app.world().resource::<visibility::ObjectVisibilityState>().visibility_intent(root).is_none());
}

fn set_row_hidden(app: &mut App, hidden: bool) {
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    Arc::make_mut(&mut session.draft.as_mut().unwrap().placements.placements[0].editor).hidden = hidden;
    session.scene_objects_revision += 1;
}

#[test]
fn preview_suppression_keeps_original_row_hidden_base_through_release_and_close() {
    let (mut app, root, _) = fixture(true, false);
    app.update();
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    assert_eq!(super::placement_base_visibility(app.world(), root), Visibility::Inherited);
    suppress(&mut app, root);
    app.update();
    release(&mut app, root);
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden),
        "释放作者遮蔽不能绕过仍生效的场景树隐藏");
    app.update();
    app.world_mut().resource_mut::<MapEditorState>().request_close();
    app.update();
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
    assert!(app.world().resource::<visibility::ObjectVisibilityState>().restored());
}

#[test]
fn row_hidden_toggle_during_preview_suppression_does_not_capture_mask_as_original() {
    let (mut app, root, _) = fixture(false, false);
    app.update();
    suppress(&mut app, root);
    set_row_hidden(&mut app, true);
    app.update();
    assert_eq!(super::placement_base_visibility(app.world(), root), Visibility::Inherited);
    set_row_hidden(&mut app, false);
    app.update();
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    assert!(app.world().resource::<visibility::ObjectVisibilityState>().restored());
    release(&mut app, root);
    app.update();
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
}

#[test]
fn real_external_visibility_insert_stays_masked_and_becomes_the_restored_base() {
    for external in [Visibility::Hidden, Visibility::Visible] {
        let (mut app, root, _) = fixture(true, false);
        app.update();
        suppress(&mut app, root);
        app.world_mut().entity_mut(root).insert(external);
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<PlacementPreviewSuppression>(root).unwrap().original_visibility, external);
        release(&mut app, root);
        app.world_mut().resource_mut::<MapEditorState>().request_close();
        app.update();
        assert_eq!(app.world().get::<Visibility>(root), Some(&external),
            "真实外部删除或恢复的意图不能被作者遮蔽及行隐藏的旧原值覆盖");
    }
}

#[test]
fn closing_editor_releases_row_hidden_override_but_keeps_preview_suppression() {
    let (mut app, root, _) = fixture(true, false);
    app.update();
    suppress(&mut app, root);
    app.world_mut().resource_mut::<MapEditorState>().request_close();
    app.update();
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    assert!(app.world().resource::<visibility::ObjectVisibilityState>().restored());
    release(&mut app, root);
    app.update();
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
}

#[test]
fn newly_loaded_hidden_root_stays_hidden_when_preview_releases_before_visibility_sweep() {
    let (mut app, root, _) = fixture(true, false);
    // 新根尚未进入Presentation：没有EditorObjectVisibility，也不能在旧预览清理时暴露隐藏对象。
    suppress(&mut app, root);
    release(&mut app, root);
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    assert_eq!(super::placement_base_visibility(app.world(), root), Visibility::Inherited);
    app.update();
    app.world_mut().resource_mut::<MapEditorState>().request_close();
    app.update();
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
}

#[test]
fn preview_and_row_hidden_keep_builtin_collision_disabled_without_opening_shared_body() {
    let (mut app, root, batch) = fixture(false, true);
    let visual = app.world().get::<Children>(root).unwrap()[0];
    app.update();
    suppress(&mut app, root);
    set_row_hidden(&mut app, true);
    app.update();
    assert!(app.world().get::<ColliderDisabled>(visual).is_some());
    // 外部试图恢复可见时，作者遮蔽必须同次保持Hidden及非碰撞，而非等待下一轮全图刷新。
    app.world_mut().entity_mut(root).insert(Visibility::Inherited);
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
    assert!(app.world().get::<ColliderDisabled>(visual).is_some());
    release(&mut app, root);
    app.update();
    assert!(app.world().get::<ColliderDisabled>(visual).is_some());
    app.world_mut().resource_mut::<MapEditorState>().request_close();
    app.update();
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
    assert!(app.world().get::<ColliderDisabled>(visual).is_none());
    assert!(app.world().get::<RigidBodyDisabled>(batch).is_some(),
        "单根显隐不得切换正式批次共享刚体，其他对象继续由完整采用门拥有");
}

#[test]
fn deletion_preview_and_row_hidden_release_in_either_order_without_opening_old_collision() {
    for delete_first in [false, true] {
        let (mut app, root, batch) = fixture(true, true);
        let visual = app.world().get::<Children>(root).unwrap()[0];
        app.update();
        set_deleted(&mut app, true);
        suppress(&mut app, root);
        assert_eq!(super::placement_base_visibility(app.world(), root), Visibility::Inherited);
        if delete_first {
            set_deleted(&mut app, false);
            assert!(app.world().get::<PlacementPreviewSuppression>(root).is_some());
        } else {
            release(&mut app, root);
            assert!(app.world().get::<PlacementDeletionSuppression>(root).is_some());
        }
        app.update();
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
        assert!(app.world().get::<ColliderDisabled>(visual).is_some());
        if delete_first { release(&mut app, root); } else { set_deleted(&mut app, false); }
        app.update();
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden), "行隐藏仍独立拥有最终显隐");
        assert!(app.world().get::<ColliderDisabled>(visual).is_some());
        app.world_mut().resource_mut::<MapEditorState>().request_close();
        app.update();
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
        assert!(app.world().get::<ColliderDisabled>(visual).is_none());
        assert!(app.world().get::<RigidBodyDisabled>(batch).is_some(), "单根归还不得切换共享批次门");
    }
}

#[test]
fn external_visibility_insert_updates_both_masks_and_survives_their_independent_release() {
    for external in [Visibility::Hidden, Visibility::Visible] {
        let (mut app, root, _) = fixture(true, false);
        app.update();
        set_deleted(&mut app, true);
        suppress(&mut app, root);
        app.world_mut().entity_mut(root).insert(external);
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<PlacementDeletionSuppression>(root).unwrap().original_visibility, external);
        assert_eq!(app.world().get::<PlacementPreviewSuppression>(root).unwrap().original_visibility, external);
        set_deleted(&mut app, false);
        release(&mut app, root);
        app.world_mut().resource_mut::<MapEditorState>().request_close();
        app.update();
        assert_eq!(app.world().get::<Visibility>(root), Some(&external));
    }
}

#[test]
fn closing_and_withdrawing_deletion_in_one_frame_returns_the_row_base_after_both_masks_leave() {
    let (mut app, root, _) = fixture(true, false);
    app.update();
    set_deleted(&mut app, true);
    app.world_mut().run_system_once(move |mut commands: Commands, session: Res<EditorAuthoringSession>| {
        let map = &session.draft.as_ref().unwrap().map_id;
        placement_deletion::withdraw_document(&mut commands, map, &session.placement_deletions);
    }).unwrap();
    app.world_mut().resource_mut::<MapEditorState>().request_close();
    app.update();
    assert!(app.world().get::<PlacementDeletionSuppression>(root).is_none());
    assert!(app.world().resource::<visibility::ObjectVisibilityState>().restored());
    assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited),
        "行覆盖先移除再归还过期删除，否则旧行mask会把最终值永久留在Hidden");
}

#[test]
fn closed_editor_returns_expired_deletion_masks_in_the_shared_sixty_four_root_budget() {
    let (mut app, first, _) = fixture(false, false);
    let (map, record) = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        (draft.map_id.clone(), draft.placements.placements[0].clone())
    };
    // 多个正式代次只用于跨过预算边界；根登记和删除仍走生产Add及真实文档票据。
    let mut roots = vec![first];
    for _ in 0..64 {
        roots.push(app.world_mut().spawn((MapPlacementInstance::new(&map, &record), Visibility::Inherited)).id());
    }
    app.world_mut().flush();
    set_deleted(&mut app, true);
    app.world_mut().resource_mut::<MapEditorState>().request_close();
    for _ in 0..4 { app.update(); } // 先耗尽已有dirty与关闭context，之后只允许撤回边沿启动一次扫描。
    assert!(roots.iter().all(|&root| app.world().get::<PlacementDeletionSuppression>(root).is_some()));
    app.world_mut().run_system_once(move |mut commands: Commands, session: Res<EditorAuthoringSession>| {
        placement_deletion::withdraw_document(&mut commands, &map, &session.placement_deletions);
    }).unwrap();
    app.update();
    assert_eq!(roots.iter().filter(|&&root| app.world().get::<PlacementDeletionSuppression>(root).is_some()).count(), 1,
        "关闭后也须每帧最多归还64根，不能同步遍历全部删除根");
    app.update();
    assert!(roots.iter().all(|&root| app.world().get::<PlacementDeletionSuppression>(root).is_none()));
    assert!(roots.iter().all(|&root| app.world().get::<Visibility>(root) == Some(&Visibility::Inherited)));
    app.update();
    assert!(roots.iter().all(|&root| app.world().get::<Visibility>(root) == Some(&Visibility::Inherited)));
}
