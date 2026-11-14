//! 只读桥接原地表辅助图；一帧最多一个16KiB行块，编码单飞，真实ACK后才确认该块版本。
use crate::{
    authoring::{AuthoringRetirement, EditorAuthoringSession},
    viewport::{SurfaceStrokeTrail, SurfaceTrailSource},
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use feathrise_scene_content::embedded_surface_trail::*;
use std::sync::Arc;

pub struct EncodedSurfaceTrail {
    pub session: u64,
    pub generation: u64,
    pub sequence: u64,
    pub scene_id: String,
    pub bytes: Arc<Vec<u8>>,
    epoch: u64,
    block: usize,
    version: u64,
    visible: bool,
}
#[derive(Clone, PartialEq)]
struct Source {
    session: u64,
    generation: u64,
    canvas: SurfaceTrailSource,
}
#[derive(Resource, Default)]
pub struct EmbeddedSurfaceTrail {
    pub enabled: bool,
    pub session: u64,
    pub generation: u64,
    pub acknowledged_sequence: u64,
    pub ready: Option<Arc<EncodedSurfaceTrail>>,
    pub pending: bool,
    pub error: Option<String>,
    source: Option<Source>,
    last: Option<Source>,
    epoch: u64,
    next_sequence: u64,
    clear_needed: bool,
    reset_needed: bool,
    acknowledged: [u64; SURFACE_TRAIL_BLOCKS],
    cursor: usize,
    task: Option<Task<(u64, Result<Arc<EncodedSurfaceTrail>, String>)>>,
}
pub type EmbeddedSurfaceTrailState = EmbeddedSurfaceTrail;
pub struct SurfaceTrailPreviewPlugin;
impl Plugin for SurfaceTrailPreviewPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }
}
pub fn install(app: &mut App) {
    app.init_resource::<EmbeddedSurfaceTrail>()
        .init_resource::<AuthoringRetirement>()
        .add_systems(
            Update,
            advance.after(crate::viewport::draw_surface_stroke_trail),
        );
}
pub fn invalidate(world: &mut World) {
    let Some(mut state) = world.get_resource_mut::<EmbeddedSurfaceTrail>() else {
        return;
    };
    state.epoch = state.epoch.wrapping_add(1).max(1);
    // 客体完整快照本身撤回旧辅助图；不能留下旧场景的clear等待，阻塞新空场景的F12。
    state.source = None;
    state.last = None;
    state.clear_needed = false;
    state.error = None;
    state.reset_needed = true;
    state.acknowledged = [0; SURFACE_TRAIL_BLOCKS];
    let old = state.ready.take();
    drop(state);
    if let Some(old) = old {
        world.resource_mut::<AuthoringRetirement>().push(old);
    }
}
fn advance(world: &mut World) {
    let mut state = world.remove_resource::<EmbeddedSurfaceTrail>().unwrap();
    let current = (state.enabled && state.session != 0 && state.generation != 0)
        .then(|| {
            world
                .get_resource::<SurfaceStrokeTrail>()
                .and_then(|trail| trail.projection_source())
                .cloned()
        })
        .flatten()
        .map(|canvas| Source {
            session: state.session,
            generation: state.generation,
            canvas,
        });
    if current != state.source {
        state.epoch = state.epoch.wrapping_add(1).max(1);
        state.acknowledged = [0; SURFACE_TRAIL_BLOCKS];
        state.cursor = 0;
        state.reset_needed = true;
        state.clear_needed = current.is_none() && state.last.is_some();
        if let Some(source) = &current {
            state.last = Some(source.clone());
        }
        state.source = current;
        state.error = None;
        if let Some(old) = state.ready.take() {
            world.resource_mut::<AuthoringRetirement>().push(old);
        }
    }
    if let Some((epoch, result)) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        if epoch == state.epoch {
            match result {
                Ok(ready) => state.ready = Some(ready),
                Err(error) => state.error = Some(error),
            }
        } else if let Ok(old) = result {
            world.resource_mut::<AuthoringRetirement>().push(old);
        }
    }
    if state
        .ready
        .as_ref()
        .is_some_and(|ready| ready.sequence == state.acknowledged_sequence)
    {
        let ready = state.ready.take().unwrap();
        if ready.epoch == state.epoch {
            if ready.visible {
                state.acknowledged[ready.block] = ready.version;
                state.reset_needed = false;
            } else {
                state.clear_needed = false;
                state.last = None;
            }
        }
        world.resource_mut::<AuthoringRetirement>().push(ready);
    }
    if state.task.is_none() && state.ready.is_none() && state.error.is_none() && state.enabled {
        let work = if let Some(source) = &state.source {
            let trail = world.resource::<SurfaceStrokeTrail>();
            let block = (0..SURFACE_TRAIL_BLOCKS)
                .map(|offset| (state.cursor + offset) % SURFACE_TRAIL_BLOCKS)
                .find(|block| trail.projection_versions()[*block] != state.acknowledged[*block]);
            block.and_then(|block| {
                trail.projection_block(block).map(|rgba| {
                    (
                        source.clone(),
                        block,
                        trail.projection_versions()[block],
                        true,
                        rgba,
                    )
                })
            })
        } else if state.clear_needed {
            state
                .last
                .as_ref()
                .map(|source| (source.clone(), 0, 0, false, Vec::new()))
        } else {
            None
        };
        if let Some((mut source, block, version, visible, rgba)) = work {
            source.session = state.session;
            source.generation = state.generation;
            state.cursor = (block + 1) % SURFACE_TRAIL_BLOCKS;
            state.next_sequence = state.next_sequence.wrapping_add(1).max(1);
            let sequence = state.next_sequence;
            let epoch = state.epoch;
            let session = world.resource::<EditorAuthoringSession>();
            let patch = SurfaceTrailPatch {
                scene_id: source.canvas.scene_id.clone(),
                terrain_id: source.canvas.terrain,
                terrain_revision: source.canvas.terrain_revision,
                author_revision: session.revision,
                surface_revision: session.surface_revision,
                source_epoch: epoch,
                source_to_world: source.canvas.transform,
                terrain_kind: source.canvas.kind,
                visible,
                reset: state.reset_needed || !visible,
                block: block as u8,
                block_revision: version,
                rgba,
            };
            state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
                let result = encode_surface_trail(&patch).map(|bytes| {
                    Arc::new(EncodedSurfaceTrail {
                        session: source.session,
                        generation: source.generation,
                        sequence,
                        scene_id: source.canvas.scene_id.as_str().into(),
                        bytes: Arc::new(bytes),
                        epoch,
                        block,
                        version,
                        visible,
                    })
                });
                (epoch, result)
            }));
        }
    }
    state.pending = state.task.is_some()
        || state.ready.is_some()
        || state.clear_needed
        || state.source.as_ref().is_some_and(|_| {
            world.resource::<SurfaceStrokeTrail>().projection_versions() != &state.acknowledged
        });
    world.insert_resource(state);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_epochs_separate_reconnect_and_terrain_pose() {
        let canvas = SurfaceTrailSource {
            scene_id: feathrise_scene_content::SceneId::parse("M1-TEST").unwrap(),
            terrain: None,
            terrain_revision: 1,
            transform: Mat4::IDENTITY.to_cols_array(),
            kind: feathrise_scene_content::SceneTerrainKind::Gltf,
        };
        let source = Source {
            session: 1,
            generation: 2,
            canvas,
        };
        let mut next = source.clone();
        next.session = 2;
        assert!(source != next);
        next = source.clone();
        next.canvas.transform[12] = 1.;
        assert!(source != next);
    }
}
