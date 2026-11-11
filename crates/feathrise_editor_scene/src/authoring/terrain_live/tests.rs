//! 普通场景只验证实际采用、完整身份及文件交接，不依赖游戏玩家组件。

use super::super::{EditorAuthoringSession, TerrainLiveApplyState, test_heightfield_draft};
use super::*;
use crate::resources::SceneEditorState;
use avian3d::prelude::Collider;
use bevy::prelude::*;
use feathrise_scene_content::{ActiveDynamicScene, SceneId};
use feathrise_scene::{
    DynamicSceneTerrainPrimitive, DynamicTerrainHeightPhase, DynamicTerrainHeightStatus,
};
use std::{sync::Arc, time::Duration};

#[derive(Resource, Default)]
struct Observed(Option<(SceneId, u64, u64, f32)>);

fn observe(adoption: TerrainLiveAdoption, mut observed: ResMut<Observed>) {
    observed.0 = adoption.current().map(|receipt| {
        (
            receipt.scene_id.clone(),
            receipt.revision,
            receipt.candidate_epoch,
            receipt.sample_ground_y([5.0, 5.0]).unwrap(),
        )
    });
}

fn fixture() -> (App, Entity) {
    let mut app = App::new();
    let source = test_support::install(&mut app, test_heightfield_draft(Arc::new(vec![4; 121])), 7);
    app.init_resource::<Observed>()
        .add_systems(Update, observe.in_set(TerrainLiveSet::React));
    (app, source)
}

#[test]
fn terrain_live_receipt_waits_for_cross_frame_mesh_and_exact_trimesh() {
    let (mut app, source) = fixture();
    app.update();
    assert!(app.world().resource::<Observed>().0.is_none());
    app.update();
    assert_eq!(
        app.world().resource::<Observed>().0.as_ref().unwrap().3,
        4.0
    );
    app.world_mut()
        .entity_mut(source)
        .insert(Collider::cuboid(2.0, 2.0, 2.0));
    app.update();
    assert!(app.world().resource::<Observed>().0.is_none()); // 错误碰撞重新安装后仍要跨帧。
    app.update();
    assert!(app.world().resource::<Observed>().0.is_some());
}

#[test]
fn terrain_live_receipt_rejects_non_unique_sources_and_stale_revisions() {
    let (mut app, _) = fixture();
    app.update();
    app.update();
    let duplicate = app
        .world_mut()
        .spawn((DynamicSceneTerrainPrimitive, Mesh3d::default()))
        .id();
    app.update();
    assert!(app.world().resource::<Observed>().0.is_none());
    app.world_mut().despawn(duplicate);
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .terrain_revision += 1;
    app.update();
    assert!(app.world().resource::<Observed>().0.is_none());
    assert!(
        app.world()
            .resource::<TerrainLiveApplyState>()
            .active_scene_id
            .is_none()
    );
}

#[test]
fn terrain_live_receipt_rejects_incomplete_source_added_after_adoption() {
    fn add_source_after_confirmation(
        mut commands: Commands,
        live: Res<TerrainLiveApplyState>,
        mut inserted: Local<bool>,
    ) {
        if live.ready && !*inserted {
            // 模拟正式源替换刚生成Mesh、碰撞和作者标记尚未到达的同帧中间态。
            commands.spawn((DynamicSceneTerrainPrimitive, Mesh3d::default()));
            *inserted = true;
        }
    }

    let (mut app, _) = fixture();
    app.world_mut().resource_mut::<SceneEditorState>().close_requested = true;
    app.add_systems(
        Update,
        add_source_after_confirmation
            .after(TerrainLiveSet::Adopt)
            .before(TerrainLiveSet::React),
    );
    app.update();
    assert!(app.world().resource::<Observed>().0.is_none());
    app.update();
    assert!(app.world().resource::<TerrainLiveApplyState>().ready); // Adopt确已确认原源，不能靠下一帧维护拒绝掩盖读取缺口。
    let source_count = app
        .world_mut()
        .query_filtered::<Entity, (With<DynamicSceneTerrainPrimitive>, With<Mesh3d>)>()
        .iter(app.world())
        .count();
    assert_eq!(source_count, 2); // Commands已经在React前应用，第二源仍缺Collider和作者标记。
    assert!(app.world().resource::<Observed>().0.is_none());
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open && editor.close_requested); // 关闭门也必须拒绝陈旧ready，不能在游戏响应跳过贴地后恢复玩法。

    let mut incomplete = app.world_mut().query_filtered::<
        Entity,
        (With<DynamicSceneTerrainPrimitive>, Without<crate::components::EditorLiveTerrainSource>),
    >();
    let duplicate = incomplete.single(app.world()).unwrap();
    app.world_mut().despawn(duplicate);
    app.update();
    assert!(app.world().resource::<Observed>().0.is_some());
    assert!(!app.world().resource::<SceneEditorState>().open); // 完整来源恢复后沿用原关闭意图，不永久锁住F12。
}

#[test]
fn terrain_live_same_revision_rebuild_changes_candidate_lifetime() {
    let (mut app, _) = fixture();
    app.update();
    app.update();
    let old = app.world().resource::<Observed>().0.clone().unwrap();
    test_support::rebuild_candidate(app.world_mut(), 7);
    app.update();
    assert!(app.world().resource::<Observed>().0.is_none());
    app.update();
    let new = app.world().resource::<Observed>().0.clone().unwrap();
    assert_eq!((&old.0, old.1), (&new.0, new.1));
    assert_ne!(old.2, new.2);
}

#[test]
fn terrain_live_scene_switch_revokes_old_receipt() {
    let (mut app, _) = fixture();
    app.update();
    app.update();
    app.world_mut().resource_mut::<ActiveDynamicScene>().scene_id =
        Some(SceneId::parse("OTHER-MAP").unwrap());
    app.update();
    assert!(app.world().resource::<Observed>().0.is_none());
    assert!(
        app.world()
            .resource::<TerrainLiveApplyState>()
            .active_scene_id
            .is_none()
    );
}

fn begin_handoff(app: &mut App) -> SceneId {
    app.update();
    app.update();
    let map = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .scene_id
        .clone();
    app.world_mut()
        .resource_mut::<TerrainLiveApplyState>()
        .begin_file_handoff(map.clone(), 7, 3, Duration::ZERO)
        .unwrap();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .terrain_dirty = false;
    map
}

#[test]
fn terrain_live_file_handoff_requires_new_complete_generation() {
    let (mut app, source) = fixture();
    let map = begin_handoff(&mut app);
    *app.world_mut().resource_mut::<DynamicTerrainHeightStatus>() = DynamicTerrainHeightStatus {
        scene_id: Some(map),
        desired_generation: 3,
        applied_generation: 3,
        phase: DynamicTerrainHeightPhase::Ready,
        ..default()
    };
    app.update();
    assert!(app.world().resource::<Observed>().0.is_none());
    assert!(
        app.world()
            .resource::<TerrainLiveApplyState>()
            .file_handoff
            .is_some()
    );
    {
        let mut status = app.world_mut().resource_mut::<DynamicTerrainHeightStatus>();
        status.desired_generation = 4;
        status.applied_generation = 4;
    }
    app.update();
    assert!(
        app.world()
            .resource::<TerrainLiveApplyState>()
            .active_scene_id
            .is_none()
    );
    assert!(
        app.world()
            .get::<crate::components::EditorLiveTerrainSource>(source)
            .is_none()
    );
    assert!(app.world().resource::<Observed>().0.is_none());
}

#[test]
fn terrain_live_file_failure_keeps_candidate_and_restores_dirty_retry() {
    let (mut app, _) = fixture();
    let map = begin_handoff(&mut app);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .close_requested = true;
    *app.world_mut().resource_mut::<DynamicTerrainHeightStatus>() = DynamicTerrainHeightStatus {
        scene_id: Some(map),
        desired_generation: 4,
        phase: DynamicTerrainHeightPhase::Failed,
        detail: "派生失败".into(),
        ..default()
    };
    app.update();
    assert!(
        app.world()
            .resource::<TerrainLiveApplyState>()
            .active_mesh
            .is_some()
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .terrain_dirty
    );
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.open && editor.dirty && !editor.close_requested);
    app.update();
    assert!(app.world().resource::<Observed>().0.is_some());
}
