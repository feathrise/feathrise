//! 悬停失败时只读固定输入槽、相机和已完成查询；不造命中、不扫描网格或发起额外射线。
use crate::{authoring::EditorAuthoringSession, components::SceneEditorViewport,
    resources::{SceneEditorState, SceneEditorViewportState}, ui::sidebars::SceneEditorSidebarState};
use bevy::{ecs::system::RunSystemOnce, prelude::*, ui::RelativeCursorPosition};
use serde_json::json;

pub(super) fn hover(world: &mut World) -> serde_json::Value {
    let editor = world.resource::<SceneEditorState>();
    let session = world.resource::<EditorAuthoringSession>();
    let viewport = world.resource::<SceneEditorViewportState>();
    let mode = world.get_resource::<crate::viewport::modes::ViewportModeState>();
    let input = json!({"tool":format!("{:?}",editor.tool),"view":format!("{:?}",editor.view),
        "orthographic":editor.orthographic,"brush_terrain":editor.brush_terrain_id.as_ref().map(|id|id.as_str()),
        "brush_point":editor.brush_cursor_world.map(|point|point.to_array()),
        "selected_scene":editor.selected_scene_id.as_ref().map(|id|id.as_str()),
        "mode":mode.map(|mode|format!("{:?}",mode.mode)),"pointer_over_controls":mode.map(|mode|mode.pointer_over_controls),
        "sidebar":world.get_resource::<SceneEditorSidebarState>().map(|sidebar|format!("{:?}",sidebar.left)),
        "saving":session.saving,"save_requested":editor.save_requested,"close_requested":editor.close_requested,
        "new_scene_dialog":editor.new_scene_dialog_open,"suppress_world_pick":editor.suppress_world_pick_this_frame,
        "scene_switch_confirmation":editor.scene_switch_confirmation.is_some(),
        "pending_transition":editor.pending_scene_transition.is_some(),"transition_in_progress":editor.scene_transition_in_progress,
        "root_kind":session.draft.as_ref().map(|draft|format!("{:?}",draft.manifest.terrain.kind)),
        "terrain_hidden":editor.terrain_hidden.iter().take(16).map(|id|id.as_str()).collect::<Vec<_>>(),
        "legacy_cursor":session.draft.as_ref().is_some_and(|draft|draft.uses_legacy_clipmap_cursor())});
    let camera = json!({"center":viewport.center.to_array(),"extent":viewport.extent,
        "yaw":viewport.yaw,"pitch":viewport.pitch,"render_size":viewport.render_size.to_array(),
        "transform":world.get::<Transform>(viewport.camera).map(|pose|pose.to_matrix().to_cols_array()),
        "projection":world.get::<Projection>(viewport.camera).map(|projection|format!("{projection:?}")),
        "computed_projection":world.get::<Camera>(viewport.camera).map(|camera|camera.clip_from_view().to_cols_array())});
    let cursor = crate::authoring::terrain_ground::cursor_validation_snapshot(world);
    let pointers: Vec<_> = world.query_filtered::<&RelativeCursorPosition, With<SceneEditorViewport>>().iter(world)
        .take(4).map(|pointer|json!({"over":pointer.cursor_over(),"normalized":pointer.normalized.map(|point|point.to_array())})).collect();
    let windows: Vec<_> = world.query::<&Window>().iter(world).take(4)
        .map(|window|json!({"focused":window.focused,"visible":window.visible})).collect();
    let current_height = world.run_system_once(|session: Res<EditorAuthoringSession>,
        sparse: Res<crate::authoring::sparse_terrain::SparseTerrainAuthoringState>,
        live: Res<feathrise_scene_content::SparseHeightLiveOverride>, ground: feathrise_scene::TerrainObjectClipmapGround| {
            sparse.current_height_snapshot(&session, &live, &ground).is_some()
        }).unwrap_or(false);
    json!({"input":input,"camera":camera,"cursor":cursor,"pointers":pointers,
        "windows":windows,"current_height_snapshot":current_height})
}
