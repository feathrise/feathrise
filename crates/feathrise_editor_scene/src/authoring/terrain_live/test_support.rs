//! 仅测试特性下开放真实Mesh/Trimesh接管夹具；不伪造ready或公开生产状态写权限。

use super::super::*;
use crate::resources::{SceneCreationTask, SceneDeletionTask, SceneEditorState};
use avian3d::prelude::Collider;
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use feathrise_scene_content::ActiveDynamicScene;
use feathrise_scene::{DynamicSceneTerrainPrimitive, DynamicTerrainHeightStatus};
use feathrise_scene_core::state::ScenePhase;

/// 安装实际联合采用与F12关闭系统，第一帧只有组件写入、第二帧才发布采用回执。
pub fn install(app: &mut App, draft: EditorSceneDraft, revision: u64) -> Entity {
    app.insert_resource(ActiveDynamicScene {
        scene_id: Some(draft.scene_id.clone()),
    })
    .insert_resource(State::new(ScenePhase::Active))
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .insert_resource(SceneEditorState {
        open: true,
        ..default()
    })
    .init_resource::<TerrainLiveApplyState>()
    .init_resource::<DynamicTerrainHeightStatus>()
    .init_resource::<Time<Real>>()
    .init_resource::<Assets<Mesh>>()
    .init_resource::<TerrainBrushStroke>()
    .init_resource::<WaterBrushStroke>()
    .init_resource::<WaterPreviewState>()
    .init_resource::<WaterFileHandoff>()
    .init_resource::<BoundaryEditState>()
    .init_resource::<WaterNetworkEditState>()
    .init_resource::<SceneCreationTask>()
    .init_resource::<SceneDeletionTask>()
    .configure_sets(
        Update,
        (super::TerrainLiveSet::Adopt, super::TerrainLiveSet::React).chain(),
    )
    .add_systems(
        Update,
        (
            super::maintain_terrain_live_apply.in_set(super::TerrainLiveSet::Adopt),
            crate::ui::close::finish_scene_editor_close.after(super::TerrainLiveSet::React),
        ),
    );
    rebuild_candidate(app.world_mut(), revision);
    app.world_mut()
        .spawn((
            DynamicSceneTerrainPrimitive,
            Mesh3d::default(),
            Collider::cuboid(1.0, 1.0, 1.0),
        ))
        .id()
}

/// 相当于后台完成值交给联合接管之前的阶段；仍须经过正式实体安装及跨帧确认。
pub fn rebuild_candidate(world: &mut World, revision: u64) {
    let (map, positions, indices) = {
        let mut session = world.resource_mut::<EditorAuthoringSession>();
        session.terrain_revision = revision;
        let draft = session.draft.as_mut().unwrap();
        draft.terrain_dirty = true;
        let mesh = draft.terrain_mesh.as_ref().unwrap();
        (
            draft.scene_id.clone(),
            mesh.positions.clone(),
            mesh.indices.clone(),
        )
    };
    let collider = Collider::trimesh(
        positions.iter().copied().map(Vec3::from_array).collect(),
        indices
            .chunks_exact(3)
            .map(|triangle| [triangle[0], triangle[1], triangle[2]])
            .collect(),
    );
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_indices(Indices::U32(indices));
    let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
    let mut live = world.resource_mut::<TerrainLiveApplyState>();
    live.discard_candidate();
    live.active_scene_id = Some(map.clone());
    live.active_revision = revision;
    live.desired_scene_id = Some(map);
    live.desired_revision = revision;
    live.active_mesh = Some(mesh);
    live.active_collider = Some(collider);
}
