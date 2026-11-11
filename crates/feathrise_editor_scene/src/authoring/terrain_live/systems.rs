//! 未保存 GLB 地形草稿对正式动态地形显示与碰撞的联合接管。

use avian3d::prelude::Collider;
use bevy::prelude::*;
use feathrise_scene_content::ActiveDynamicScene;
use feathrise_scene::{
    DynamicSceneTerrainPrimitive, DynamicTerrainHeightPhase, DynamicTerrainHeightStatus,
};
use feathrise_scene_core::state::ScenePhase;

use super::super::resources::{EditorAuthoringSession, TerrainLiveApplyState};
use super::handoff::terrain_file_handoff_timed_out;
use super::helpers::{collider_contains_trimesh, colliders_share_shape};
use crate::components::EditorLiveTerrainSource;
use crate::resources::SceneEditorState;

type LiveTerrainSourceData = (
    Entity,
    &'static mut Mesh3d,
    Option<&'static mut Collider>,
    Option<&'static EditorLiveTerrainSource>,
);

/// 保存后的正式 GLB 派生失败时恢复草稿脏状态；上一套完整 Mesh/Trimesh 继续托底。
fn fail_terrain_file_handoff(
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    live: &mut TerrainLiveApplyState,
    reason: &str,
) {
    let Some(handoff) = live.file_handoff.take() else {
        return;
    };
    if let Some(draft) = session.draft.as_mut()
        && draft.scene_id == handoff.scene_id
        && session.terrain_revision == handoff.terrain_revision
    {
        draft.terrain_dirty = true;
    }
    editor.dirty = true;
    editor.cancel_close_request(format!(
        "场景{}地形GLB热重载失败：{reason}；本次关闭已取消，已继续保留内存地形",
        handoff.scene_id
    ));
    error!("{}", editor.status);
}

/// 把同一后台批次产生的 Mesh 与 Trimesh 安装到正式动态地形实体，并跨帧确认。
#[allow(clippy::too_many_arguments)]
pub(crate) fn maintain_terrain_live_apply(
    mut commands: Commands,
    scene: Res<State<ScenePhase>>,
    active_dynamic: Res<ActiveDynamicScene>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut live: ResMut<TerrainLiveApplyState>,
    terrain_status: Res<DynamicTerrainHeightStatus>,
    real_time: Res<Time<Real>>,
    mut sources: Query<LiveTerrainSourceData, With<DynamicSceneTerrainPrimitive>>,
) {
    if let Some(handoff) = live.file_handoff.as_ref() {
        let handoff_current = *scene.get() == ScenePhase::Active
            && active_dynamic.scene_id.as_ref() == Some(&handoff.scene_id)
            && session.draft.as_ref().is_some_and(|draft| {
                draft.scene_id == handoff.scene_id
                    && session.terrain_revision == handoff.terrain_revision
                    && !draft.terrain_dirty
            });
        if !handoff_current {
            live.file_handoff = None;
        } else if terrain_status.scene_id.as_ref() == Some(&handoff.scene_id)
            && terrain_status.desired_generation > handoff.generation_baseline
            && terrain_status.phase == DynamicTerrainHeightPhase::Failed
        {
            let reason = terrain_status.detail.clone();
            fail_terrain_file_handoff(&mut session, &mut editor, &mut live, &reason);
            return;
        } else if terrain_status.scene_id.as_ref() == Some(&handoff.scene_id)
            && terrain_status.phase == DynamicTerrainHeightPhase::Ready
            && terrain_status.applied_generation == terrain_status.desired_generation
            && terrain_status.applied_generation > handoff.generation_baseline
        {
            let completed_scene_id = handoff.scene_id.clone();
            if let Ok((entity, _, _, _)) = sources.single_mut() {
                commands.entity(entity).remove::<EditorLiveTerrainSource>();
            }
            live.file_handoff = None;
            live.discard_candidate();
            editor.status = format!(
                "场景{completed_scene_id}已保存；正式GLB网格、内存高度、GPU高度纹理和Trimesh已采用"
            );
            info!("{}", editor.status);
            return;
        } else if terrain_file_handoff_timed_out(handoff.started_at, real_time.elapsed()) {
            fail_terrain_file_handoff(
                &mut session,
                &mut editor,
                &mut live,
                "30秒内未取得保存后GLB的完整派生代次",
            );
            return;
        } else {
            return; // 文件派生尚未完成时不得让内存候选覆盖正在更新的正式实体。
        }
    }

    let (Some(scene_id), Some(expected_mesh), Some(expected_collider)) = (
        live.active_scene_id.as_ref(),
        live.active_mesh.as_ref(),
        live.active_collider.as_ref(),
    ) else {
        return;
    };
    let revision = live.active_revision;
    let current_scene = *scene.get() == ScenePhase::Active
        && active_dynamic.scene_id.as_ref() == Some(scene_id)
        && session.draft.as_ref().is_some_and(|draft| {
            &draft.scene_id == scene_id && draft.terrain_dirty && session.terrain_revision == revision
        });
    if !current_scene {
        live.discard_candidate();
        return;
    }

    let Ok((source_entity, mut source_mesh, current_collider, source_revision)) =
        sources.single_mut()
    else {
        live.ready = false;
        return;
    };
    let revision_matches = source_revision
        .is_some_and(|source| &source.scene_id == scene_id && source.revision == revision);
    let mesh_matches = source_mesh.0.id() == expected_mesh.id();
    let collider_matches = current_collider.as_deref().is_some_and(|current| {
        collider_contains_trimesh(current) && colliders_share_shape(current, expected_collider)
    });
    let mut changed = false;
    if !mesh_matches {
        *source_mesh = Mesh3d(expected_mesh.clone());
        changed = true;
    }
    if !revision_matches || !collider_matches {
        if let Some(mut current) = current_collider {
            *current = expected_collider.clone();
        } else {
            commands
                .entity(source_entity)
                .insert(expected_collider.clone());
        }
        commands
            .entity(source_entity)
            .insert(EditorLiveTerrainSource {
                scene_id: scene_id.clone(),
                revision,
            });
        changed = true;
    }
    if changed {
        live.ready = false;
        return;
    }

    // 本域只发布真实采用资格；游戏对角色的安全贴地在React阶段处理，再经过F12关闭门。
    if !live.ready {
        live.ready = true;
        editor.status =
            "地形草稿已同步到预览和碰撞".to_owned();
    }
}

/// 离开动态场景时使旧候选和文件交接身份失效。
pub(crate) fn clear_terrain_live_apply(mut live: ResMut<TerrainLiveApplyState>) {
    live.invalidate();
}
