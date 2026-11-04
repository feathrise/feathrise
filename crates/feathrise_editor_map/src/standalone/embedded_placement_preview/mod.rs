//! 原对象手势的实时进程投影：可靠起止边沿、合并中间矩阵、单槽后台编码；不拥有作者历史。

mod source;
mod systems;
#[cfg(test)]
mod tests;

use bevy::{prelude::*, tasks::Task};
use feathrise_map_content::embedded_placement_preview::PlacementPreviewCommand;
use std::{collections::VecDeque, sync::Arc};

pub const PLACEMENT_PREVIEW_FORMAT: &str = "feathrise-placement-preview-v1";
const MAX_QUEUED_COMMANDS: usize = 32;
const MEMBERS_PER_FRAME: usize = 64;

pub struct EncodedPlacementPreview {
    pub sequence: u64,
    pub session: u64,
    pub generation: u64,
    pub map_id: String,
    pub bytes: Arc<Vec<u8>>,
}

#[derive(Resource, Default)]
pub struct EmbeddedPlacementPreview {
    /// 只有宿主当前选中常驻模式才推进；重连或切图显式改变generation使旧队列失效。
    pub enabled: bool,
    pub session: u64,
    pub generation: u64,
    pub pending: bool,
    pub error: Option<String>,
    observed_generation: u64,
    observed_session: u64,
    observed_enabled: bool,
    epoch: u64,
    next_gesture: u64,
    next_sequence: u64,
    active: Option<Active>,
    finishing: Option<Finishing>,
    queue: VecDeque<Queued>,
    task: Option<Task<(u64, u64, u64, Result<Arc<EncodedPlacementPreview>, String>)>>,
    ready: Option<Arc<EncodedPlacementPreview>>,
}

struct Active {
    source: source::Source,
    gesture: u64,
    sequence: u64,
    ids: Vec<String>,
    begin_queued: bool,
    last_transform: Option<[f32; 16]>,
}

struct Queued {
    map_id: String,
    command: PlacementPreviewCommand,
}

struct Finishing {
    active: Active,
    revision: u64,
    transform: Option<[f32; 16]>,
    cancelled: bool,
    members:
        Vec<feathrise_map_content::embedded_placement_preview::PlacementPreviewFinalMember>,
}

pub fn install(app: &mut App) {
    app.init_resource::<EmbeddedPlacementPreview>().add_systems(
        Update,
        systems::advance
            .after(crate::MapEditorSystemSet::Presentation)
            .after(crate::viewport::placement_move::handle_placement_move)
            .after(crate::ui::placement_transform::handle_placement_transform_numeric_input),
    );
}

/// peek不会取走Begin/End；宿主必须在真实PreviewApplied之后确认同sequence再推进。
pub fn peek(world: &World) -> Option<Arc<EncodedPlacementPreview>> {
    world
        .get_resource::<EmbeddedPlacementPreview>()?
        .ready
        .as_ref()
        .map(Arc::clone)
}

pub fn acknowledge(world: &mut World, sequence: u64) -> bool {
    let Some(mut state) = world.get_resource_mut::<EmbeddedPlacementPreview>() else {
        return false;
    };
    if state.ready.as_ref().is_some_and(|ready| {
        ready.sequence == sequence
            && ready.session == state.session
            && ready.generation == state.generation
    }) {
        state.ready = None;
        return true;
    }
    false
}
