//! 水域预览候选、已采用资产与被隐藏正式水面的统一清理辅助逻辑。

use bevy::prelude::*;
use feathrise_scene::{DynamicWaterMaterial, WaterGpuUploadQueue};

use super::super::resources::WaterPreviewState;

pub(super) fn remove_preview_assets(
    commands: &mut Commands,
    _meshes: &mut Assets<Mesh>,
    _materials: &mut Assets<DynamicWaterMaterial>,
    preview: &mut WaterPreviewState,
) {
    preview.replacement_source = None;
    preview.replacement_target = None;
    for spawn in preview.spawns.drain(..) {
        spawn.retire(commands);
    }
}

pub(super) fn discard_pending_preview(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<DynamicWaterMaterial>,
    uploads: &mut WaterGpuUploadQueue,
    preview: &mut WaterPreviewState,
) {
    let Some(mut pending) = preview.pending.take() else {
        return;
    };
    if let Some(ticket) = pending.ticket.take() {
        uploads.retire(ticket);
    }
    for entity in pending.warmup_entities.drain(..) {
        commands.entity(entity).try_despawn();
    }
    for staged in pending.staged.drain(..) {
        staged.discard(commands, meshes, materials);
    }
    feathrise_scene::retire_water_cpu(commands, pending);
}

pub(super) fn restore_hidden_runtime(commands: &mut Commands, preview: &mut WaterPreviewState) {
    for (entity, visibility) in preview.hidden_runtime_entities.drain(..) {
        commands.entity(entity).try_insert(visibility);
    }
}
