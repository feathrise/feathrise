//! F12局部体积雕刻：后台计算→真实碰撞与高度面联合采用→单笔历史→原子保存。
mod adoption;
mod input;
mod native_fields;
mod jobs;
mod lifecycle;
mod spatial; // 确认式长笔划的私有分批版本与完整采用，复用单个IO/计算槽。
mod spatial_batches;
mod streamed;
mod types;
use crate::SceneEditorSystemSet;
use bevy::prelude::*;
pub(crate) use types::{VolumeAuthoringState, VolumeSaveFile, VolumeSculptChange};

pub(crate) fn install(app: &mut App) {
    app.init_resource::<VolumeAuthoringState>()
        .init_resource::<native_fields::NativeFieldGesture>()
        .init_resource::<feathrise_scene::TerrainVolumePreviewRequest>()
        .init_resource::<feathrise_scene::TerrainVolumeStreamControl>()
        .add_systems(
            Update,
            sync_stream_control
                .after(SceneEditorSystemSet::Tasks)
                .before(feathrise_scene::TerrainVolumeStreamSystems::Drive),
        )
        .add_systems(
            Update,
            lifecycle::poll
                .in_set(SceneEditorSystemSet::Tasks)
                .after(super::poll_draft_load)
                .before(super::start_draft_save),
        )
        .add_systems(
            Update,
            adoption::validate
                .after(SceneEditorSystemSet::Tasks)
                .before(feathrise_scene::TerrainTopologySystems::Collect)
                .before(SceneEditorSystemSet::Cursor),
        )
        .add_systems(
            Update,
            adoption::acknowledge
                .after(adoption::validate)
                .after(feathrise_scene::TerrainTopologySystems::Publish)
                .before(SceneEditorSystemSet::Cursor),
        )
        .add_systems(
            Update,
            lifecycle::history
                .in_set(SceneEditorSystemSet::Input)
                .before(super::apply_undo_redo),
        )
        .add_systems(
            Update,
            input::brush
                .in_set(SceneEditorSystemSet::Presentation)
                .after(crate::viewport::resolve_vegetation_ground_cursor),
        )
        .add_systems(Update, native_fields::input.in_set(SceneEditorSystemSet::Cursor)
            .after(crate::viewport::source_fence::update).before(crate::viewport::select_editor_viewport_content)
            .run_if(crate::scene_residency::document_active));
}

fn sync_stream_control(
    state: Res<VolumeAuthoringState>,
    mut control: ResMut<feathrise_scene::TerrainVolumeStreamControl>,
) {
    control.editing = state.busy()
        || (state.unsaved_region.is_some()
            && state.current.as_ref().is_some_and(|s| {
                !matches!(
                    s.source.as_ref(),
                    feathrise_terrain::volume::sparse::VolumeSource::Paged(_)
                )
            }));
    control.excluded = if state.current.is_none() {
        state.unsaved_region.clone()
    } else {
        None
    };
}
