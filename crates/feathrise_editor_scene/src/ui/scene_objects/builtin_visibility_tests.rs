//! 立方体显隐必须同步真实盒碰撞，同时保留完整批次的独立物理门。

use super::visibility;
use crate::{
    authoring::{EditorAuthoringSession, test_heightfield_draft},
    resources::SceneEditorState,
    ui::gltf_browser::EditorGltfBrowserState,
};
use avian3d::prelude::{Collider, ColliderDisabled, RigidBody, RigidBodyDisabled};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use feathrise_scene_content::{ScenePlacementAssetTarget, ScenePlacementRecord};
use feathrise_scene::{ScenePlacementInstance, spawn_builtin_placement_visual};
use std::sync::Arc;

fn fixture() -> (App, Entity, [Entity; 2], [Entity; 2]) {
    let mut draft = test_heightfield_draft(Arc::new(vec![2; 121]));
    draft.placements.placements = (0..2).map(|index| {
        let mut record: ScenePlacementRecord = serde_json::from_value(serde_json::json!({
            "id": format!("cube-{index}"), "asset_path":"", "translation":[0,1,0],
            "yaw_degrees":0, "scale":[1,1,1], "asset_target":ScenePlacementAssetTarget::DEFAULT_CUBOID,
        })).unwrap();
        Arc::make_mut(&mut record.editor).hidden = true;
        record
    }).collect();
    let map = draft.scene_id.clone();
    let records = draft.placements.placements.clone();
    let mut browser = EditorGltfBrowserState::default();
    for (index, record) in records.iter().enumerate() {
        browser.select_indexed_test_placement(&record.id, index, 0);
    }
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<visibility::ObjectVisibilityState>()
        .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
        .insert_resource(browser)
        .insert_resource(SceneEditorState { open: true, selected_scene_id: Some(map.clone()), ..default() })
        .add_observer(visibility::register_root)
        .add_observer(visibility::remove_root)
        .add_observer(visibility::remember_external_visibility)
        .add_systems(Update, visibility::sync_visibility);
    // 正式隐藏候选和作者粘贴组都可能持有此门；单根恢复不能打开整组。
    let batch = app.world_mut().spawn((Transform::IDENTITY, RigidBody::Static, RigidBodyDisabled)).id();
    let roots = records.iter().map(|record| app.world_mut().spawn((
        ScenePlacementInstance::new(&map, record), Transform::IDENTITY,
        Visibility::Inherited, ChildOf(batch),
    )).id()).collect::<Vec<_>>();
    for &root in &roots {
        app.world_mut().run_system_once(move |mut commands: Commands| {
            spawn_builtin_placement_visual(&mut commands, root, ScenePlacementAssetTarget::DEFAULT_CUBOID);
        }).unwrap();
    }
    let visuals = roots.iter().map(|root| app.world().get::<Children>(*root).unwrap()[0]).collect::<Vec<_>>();
    (app, batch, roots.try_into().unwrap(), visuals.try_into().unwrap())
}

#[test]
fn builtin_hidden_roots_restore_boxes_on_close_without_opening_the_shared_batch_body() {
    let (mut app, batch, roots, visuals) = fixture();
    app.update();
    for (root, visual) in roots.into_iter().zip(visuals) {
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
        assert!(app.world().get::<Collider>(visual).is_some());
        assert!(app.world().get::<ColliderDisabled>(visual).is_some());
    }
    assert!(app.world().get::<RigidBodyDisabled>(batch).is_some());
    app.world_mut().resource_mut::<SceneEditorState>().request_close();
    app.update();
    for (root, visual) in roots.into_iter().zip(visuals) {
        assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Inherited));
        assert!(app.world().get::<ColliderDisabled>(visual).is_none());
    }
    assert!(app.world().get::<RigidBodyDisabled>(batch).is_some());
    assert!(app.world().resource::<visibility::ObjectVisibilityState>().restored());
}

#[test]
fn builtin_external_deletion_stays_non_colliding_when_editor_visibility_is_restored() {
    let (mut app, batch, roots, visuals) = fixture();
    app.update();
    app.world_mut().entity_mut(roots[0]).insert(Visibility::Hidden); // 删除/退役拥有最终隐藏意图。
    app.world_mut().resource_mut::<SceneEditorState>().request_close();
    app.update();
    assert_eq!(app.world().get::<Visibility>(roots[0]), Some(&Visibility::Hidden));
    assert!(app.world().get::<ColliderDisabled>(visuals[0]).is_some());
    assert_eq!(app.world().get::<Visibility>(roots[1]), Some(&Visibility::Inherited));
    assert!(app.world().get::<ColliderDisabled>(visuals[1]).is_none());
    assert!(app.world().get::<RigidBodyDisabled>(batch).is_some());
    assert!(app.world().resource::<visibility::ObjectVisibilityState>().restored());
}
