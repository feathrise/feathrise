//! 捕获状态机只非阻塞轮询；来源换代、手势未收口和编码失败都不能发布半份运行场景。

use super::{
    EmbeddedSnapshot, EmbeddedSnapshotState,
    capture::{Capture, source_identity},
    encode,
};
use crate::authoring::{AuthoringRetirement, EditorAuthoringSession};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};
use std::sync::Arc;

pub(super) fn advance(world: &mut World) {
    let mut state = world.remove_resource::<EmbeddedSnapshotState>().unwrap();
    advance_inner(world, &mut state);
    world.insert_resource(state);
}

fn retire<T: Send + Sync + 'static>(world: &mut World, value: T) {
    world.resource_mut::<AuthoringRetirement>().push(value);
}

fn advance_inner(world: &mut World, state: &mut EmbeddedSnapshotState) {
    let current = state.enabled.then(|| source_identity(world)).flatten();
    if current != state.source {
        state.revision = state.revision.wrapping_add(1).max(1);
        state.source = current.clone();
        state.map_id = current.as_ref().map(|identity| identity.map.0.clone());
        state.error = None;
        if let Some(capture) = state.capture.take() {
            retire(world, capture);
        }
        // 上一完整字节可以继续作为已显示画面的来源，ready_is_current 明确拒绝用它启动新试玩。
        state.pending = current.is_some();
    }
    if let Some((source, revision, result)) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        if state.source.as_ref() == Some(&source) && state.revision == revision && state.enabled {
            match result {
                Ok(ready) => {
                    if let Some(previous) = state.ready.replace(ready) {
                        retire(world, previous);
                    }
                    state.pending = false;
                    state.error = None;
                }
                Err(error) => {
                    state.pending = false;
                    state.error = Some(error);
                }
            }
        } else if let Ok(stale) = result {
            retire(world, stale);
        }
    }
    let Some(current) = current else {
        state.pending = false;
        if let Some(previous) = state.ready.take() {
            retire(world, previous);
        }
        return;
    };
    if busy_reason(world).is_some() {
        state.pending = true;
        return;
    }
    if state.task.is_some() || state.error.is_some() {
        return;
    }
    if state
        .ready
        .as_ref()
        .is_some_and(|ready| ready.author_revision == state.revision)
    {
        state.pending = false;
        return;
    }
    if state.capture.is_none() {
        match Capture::new(world, current.clone()) {
            Ok(capture) => state.capture = Some(capture),
            Err(error) => {
                state.pending = false;
                state.error = Some(error);
                return;
            }
        }
    }
    match state.capture.as_mut().unwrap().step(world) {
        Ok(false) => {
            state.pending = true;
        }
        Err(error) => {
            if let Some(capture) = state.capture.take() {
                retire(world, capture);
            }
            state.pending = false;
            state.error = Some(error);
        }
        Ok(true) => {
            let capture = state.capture.take().unwrap();
            let revision = state.revision;
            state.pending = true;
            state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
                let source = capture.source.clone();
                let map_id = source.map.0.clone();
                let pose_targets = Arc::new(super::resources::EmbeddedPoseTargets::capture(&capture.manifest));
                let result = encode::encode(capture).map(|bytes| {
                    Arc::new(EmbeddedSnapshot {
                        map_id,
                        author_revision: revision,
                        bytes: Arc::new(bytes),
                        pose_targets,
                    })
                });
                (source, revision, result)
            }));
        }
    }
}

/// 原作者完整笔划/派生事务未结束时保持上一显示值，F12 必须等待当前原始输入形成完整版本。
pub(super) fn busy_reason(world: &World) -> Option<&'static str> {
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    // 公共鼠标门也被日志、设置、关闭确认及其静默释放帧占有，只说明输入归属，
    // 不能据此把已经完整的作者数据重新标为pending，或阻止被遮挡视口的首份快照。
    // 此处只等待真实作者事务；数值取消后的按压锁存不再代表临时值尚待提交。
    if world
        .get_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .is_some_and(|scrub| scrub.entity.is_some())
        || world
            .get_resource::<crate::ui::placement_transform::PlacementTransformInputState>()
            .is_some_and(|state| state.transaction_active())
        || world
            .get_resource::<crate::viewport::placement_move::PlacementMoveState>()
            .is_some_and(|state| state.exit_pending())
        || world
            .get_resource::<crate::viewport::sub_maps::SubMapTransformState>()
            .is_some_and(|state| state.transaction_active())
        || crate::viewport::terrain_transform_transaction_active(world)
        || crate::viewport::water_transform_transaction_active(world)
        || world
            .get_resource::<crate::authoring::WaterLevelScrubState>()
            .is_some_and(|scrub| scrub.target.is_some() && !scrub.aborted)
        || world
            .get_resource::<crate::authoring::placement_batch::PlacementBatchState>()
            .is_some_and(|state| state.busy())
        || world
            .get_resource::<crate::ui::scene_objects::SceneObjectState>()
            .is_some_and(|state| state.busy())
        || world
            .get_resource::<crate::ui::terrain_objects::TerrainObjectsEditorState>()
            .is_some_and(|state| state.preparing() || state.brushing || state.documents_loading)
        || world
            .get_resource::<crate::authoring::mesh_edit::MeshEditState>()
            .is_some_and(|state| state.moving())
        || session.saving
        || session.vegetation_cross_busy
        || draft.terrain_documents.loading
        || world
            .get_resource::<crate::resources::MapEditorState>()
            .is_some_and(|editor| editor.map_transition_in_progress || editor.save_requested)
        || world
            .get_resource::<crate::authoring::SurfaceBrushStroke>()
            .is_some_and(|stroke| stroke.active)
        || world
            .get_resource::<crate::authoring::VegetationBrushStroke>()
            .is_some_and(|stroke| stroke.active)
        || world
            .get_resource::<crate::authoring::WaterBrushStroke>()
            .is_some_and(|stroke| stroke.active)
        || world
            .get_resource::<crate::authoring::TerrainBrushStroke>()
            .is_some_and(|stroke| stroke.active)
        || world
            .get_resource::<crate::authoring::SurfaceLayerEditTask>()
            .is_some_and(|task| task.is_deleting())
        || world
            .get_resource::<crate::authoring::PuddleAuthoringState>()
            .is_some_and(|state| state.busy())
        || world
            .get_resource::<crate::authoring::sparse_terrain::SparseTerrainAuthoringState>()
            .is_some_and(|state| !state.document_can_park())
        || world
            .get_resource::<crate::authoring::volume_sculpt::VolumeAuthoringState>()
            .is_some_and(|state| state.busy())
        || world
            .get_resource::<crate::authoring::BoundaryEditState>()
            .is_some_and(|state| state.dragging())
        || world
            .get_resource::<crate::authoring::boundary::BoundaryThicknessInputState>()
            .is_some_and(|state| state.busy())
    {
        return Some("等待当前作者操作完整收口");
    }
    // 容器未选择独立地形时，根槽不属于植被作者画布，生产编译器也明确不为它启动。
    // 与encode的来源范围一致：空图不能等待一个永远不会产生的编译回执；停放画布
    // 由其自身完整快照提供运行源，活动画布仍必须等待真实编译修订追平。
    if (draft.manifest.terrains.is_none() || draft.terrain_documents.active.is_some())
        && draft
            .vegetation
            .as_ref()
            .is_some_and(|vegetation| vegetation.authoring.is_some())
        && (session.vegetation_compiled_revision != session.vegetation_revision
            || session.vegetation_compiled_structure_revision
                != session.vegetation_structure_revision)
    {
        return Some("等待植被作者层完整编译");
    }
    None
}
