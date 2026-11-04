//! 普通GLB/高度场吸附的有界后台补查；原主光标不变，只有点放和重定位消费结果。
use super::{resources::MeshSnapCursor, surface, types::{MeshSnapKey, ObjectSnapIdentity}};
use crate::{
    authoring::{EditorAuthoringSession, PlacementTerrainSampler},
    components::MapEditorViewport,
    resources::{MapEditorState, MapEditorViewportState},
    types::EditorTool,
    ui::{asset_preview::AssetPreviewState, placement_transform::PlacementTransformInputState},
    viewport::vegetation_ground::GroundCursorTarget,
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
    ui::RelativeCursorPosition,
};
use feathrise_scene::ClipmapGroundHit;
use feathrise_editor_core::EditorSettings;
use std::sync::Arc;

/// 旧图保留作者网格路线；独立对象冻结同一份实际采用的查询，不借用当前画布网格。
enum SnapSurface {
    Legacy {
        mesh: Arc<feathrise_map_terrain::TerrainMeshInput>,
        index: Arc<feathrise_map_terrain::TerrainEditingIndex>,
        sampler: Option<PlacementTerrainSampler>,
    },
    Object(feathrise_scene::TerrainObjectRaySnapshot),
}

pub(crate) fn update_mesh_cursor(
    mut state: ResMut<MeshSnapCursor>,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    settings: Res<EditorSettings>,
    viewport: Option<Res<MapEditorViewportState>>,
    preview: Res<AssetPreviewState>,
    transform: Res<PlacementTransformInputState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    nodes: Query<&RelativeCursorPosition, With<MapEditorViewport>>,
    mode: Option<Res<crate::viewport::modes::ViewportModeState>>,
    ui: (
        Option<Res<crate::resources::MapEditorSettingsState>>,
        Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
    ),
    ownership: (
        Res<crate::resources::MapEditorNumericScrubMouseGuard>,
        Res<crate::resources::MapBrowserState>,
        Res<crate::resources::MapBrowserDragState>,
        Res<crate::authoring::TerrainExternalChangeState>,
        Option<Res<crate::viewport::placement_move::PlacementMoveState>>,
        Option<Res<crate::workbench::WorkbenchHostPageState>>,
    ),
    terrain_cursor: Option<Res<crate::authoring::terrain_ground::TerrainGroundCursor>>,
    lod: crate::authoring::mesh_lod::MeshLodInputGate,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    // 无地图、关闭或取消时也轮询，旧任务结束后才释放唯一槽。
    let completion = state.task.as_mut().and_then(check_ready);
    if completion.is_some() {
        state.task = None;
    }
    // 【输入所有权】与点放消费者使用同一公共门；禁用帧必须丢弃冻结点击，不能留待解锁后补执行。
    // 轮询放在门外，取消仍保留未完成句柄，确保模态/和弦期间不会泄漏或扩增CPU槽。
    if !lod.ready() || !crate::ui::numeric_scrub_mouse_guard::map_editor_numeric_scrub_mouse_available(
        Res::clone(&ownership.0),
        ownership.5.as_ref().map(Res::clone),
    ) || !crate::map_browser::map_browser_general_input_available(
        Res::clone(&editor),
        Res::clone(&ownership.1),
        Res::clone(&ownership.2),
        Res::clone(&ownership.3),
        ownership.4.as_ref().map(Res::clone),
    ) {
        state.cancel();
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        state.cancel(); // 新单击取代旧点击权限，但旧任务仍占用唯一槽直到完成。
    }
    let key_and_source = (|| {
        if !editor.open
            || editor.close_requested
            || editor.save_requested
            || editor.new_map_dialog_open
            || editor.suppress_world_pick_this_frame
            || editor.map_transition_in_progress
            || session.saving
            || keyboard.just_pressed(KeyCode::Escape)
            || mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
            || mode.as_ref().is_some_and(|mode| mode.pointer_over_controls)
            || ui
                .0
                .as_ref()
                .is_some_and(|state| state.open || state.owns_mouse_this_frame)
            || ui.1.as_ref().is_some_and(|state| {
                state.active_tab != crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Map
            })
            || windows.single().is_ok_and(|window| !window.focused)
        {
            return None;
        }
        let draft = session.draft.as_ref()?;
        if editor.selected_map_id.as_ref() != Some(&draft.map_id) {
            return None;
        }
        let viewport = viewport.as_deref()?;
        let (source, object, mesh_identity, index_identity, bounds) =
            if draft.manifest.terrains.is_some() {
                let cursor = terrain_cursor.as_ref()?;
                let ground = draft.terrain_ground.as_ref()
                    .filter(|ground| ground.map == draft.map_id && ground.revision == session.terrain_revision)?;
                // 只有当前完整快照仍承认的点击才能冻结承托对象；换代后回到当前hover，不能被旧ID卡住查询。
                let frozen = state.pending.as_ref().and_then(|key| key.object.as_ref())
                    .or_else(|| state.completed.as_ref().and_then(|(key, _)| key.object.as_ref()))
                    .filter(|object| object.snapshot == Arc::as_ptr(ground) as usize
                        && object.generation == ground.generation
                        && object.visibility == Arc::as_ptr(&editor.terrain_hidden) as usize
                        && !editor.terrain_hidden.contains(&object.id)
                        && ground.bindings.pose(&object.id).is_some_and(|pose|
                            object.pose == [pose.translation[0], pose.translation[1], pose.translation[2],
                                pose.yaw_degrees, pose.scale[0], pose.scale[1], pose.scale[2]].map(f32::to_bits)));
                // 同一有效快照内，移向空隙或另一对象不能改写尚在补查的真实点击落点。
                let id = if editor.ground_click_world.is_some() {
                    cursor.click_terrain.as_ref()
                } else {
                    frozen.map(|object| &object.id).or(cursor.terrain.as_ref())
                }?;
                if editor.terrain_hidden.contains(id) {
                    return None;
                }
                let pose = ground.bindings.pose(id)?;
                let query = ground.object_query(id)?;
                let identity = ObjectSnapIdentity {
                    id: id.clone(),
                    pose: [pose.translation[0], pose.translation[1], pose.translation[2],
                        pose.yaw_degrees, pose.scale[0], pose.scale[1], pose.scale[2]].map(f32::to_bits),
                    generation: ground.generation,
                    snapshot: Arc::as_ptr(ground) as usize,
                    visibility: Arc::as_ptr(&editor.terrain_hidden) as usize,
                };
                // 独立对象由查询中的真实边界裁剪，不能读取空容器的0×0源范围。
                (SnapSurface::Object(query), Some(identity), 0, 0, [0.0; 4])
            } else {
                if draft.uses_legacy_clipmap_cursor() {
                    return None;
                }
                let mesh = draft.terrain_mesh.clone()?;
                let index = draft.terrain_editing_index.clone()?;
                let source = draft.terrain_manifest();
                let bounds = [source.world_min[0], source.world_min[1], source.world_max[0], source.world_max[1]];
                let mesh_identity = Arc::as_ptr(&mesh) as usize;
                let index_identity = Arc::as_ptr(&index) as usize;
                (SnapSurface::Legacy { mesh, index, sampler: PlacementTerrainSampler::from_draft(draft).ok() },
                    None, mesh_identity, index_identity, bounds)
            };
        let (target, origin) = if let Some((map, record_index, id, structure, author)) =
            transform.relocation_ground_identity()
        {
            if map != &draft.map_id
                || structure != session.placement_structure_revision
                || author != session.revision
            {
                return None;
            }
            let record = draft
                .placements
                .placements
                .get(record_index)
                .filter(|record| record.id == id)?;
            if record.building_preset_id.is_some() {
                return None;
            }
            (
                GroundCursorTarget::RelocatePlacement(id.to_owned()),
                record.translation,
            )
        } else if editor.tool == EditorTool::PlaceAsset {
            let path = editor.selected_asset.as_ref()?;
            (
                GroundCursorTarget::PlaceAsset {
                    asset_path: path.clone(),
                    asset_target: preview.placement_target_for(path).ok()?,
                },
                [0.0; 3],
            )
        } else {
            return None;
        };
        let snap = settings.editor_snapping.translation_snap(
            keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
            origin,
        );
        let key = MeshSnapKey {
            map: draft.map_id.clone(),
            target,
            author: session.revision,
            terrain: session.terrain_revision,
            mesh: mesh_identity,
            index: index_identity,
            object,
            bounds: bounds.map(f32::to_bits),
            camera: viewport.camera,
            view: editor.view,
            projection: editor.orthographic,
            camera_pose: [
                viewport.center.x,
                viewport.center.y,
                viewport.center.z,
                viewport.extent,
                viewport.yaw,
                viewport.pitch,
                viewport.scene_center.y,
            ]
            .map(f32::to_bits),
            render_size: viewport.render_size,
            point: editor
                .ground_click_world
                .or(editor.cursor_world)
                .unwrap_or(Vec3::ZERO)
                .to_array()
                .map(f32::to_bits),
            snap,
        };
        Some((key, source))
    })();
    let Some((mut current, source)) = key_and_source else {
        state.cancel();
        return;
    };
    // 原始Collider尚无hover也能捕获单击；补查完成后使用按下时Ctrl，而不是松键后的当前语义。
    let raw_object_snap = (current.object.is_some() && editor.ground_click_world.is_some())
        .then(|| terrain_cursor.as_ref().and_then(|cursor| cursor.click_snap)).flatten();
    if let Some(snap) = raw_object_snap {
        current.snap = snap;
        if !snap.active() {
            state.cancel(); // 新点击临时关闭吸附时，旧active pending不能把它挡在补查等待中。
        }
    }
    // 独立对象原始射线也在后台；先保留按下时Ctrl，再等待真实点击点，不能提交旧悬停点。
    let frozen_object_snap = state.pending.as_ref().filter(|key| key.map == current.map
        && key.target == current.target && key.author == current.author && key.terrain == current.terrain
        && key.camera == current.camera && key.camera_pose == current.camera_pose
        && key.view == current.view && key.projection == current.projection
        && key.render_size == current.render_size && key.snap.same_preferences(current.snap))
        .map(|key| key.snap);
    if state
        .pending
        .as_ref()
        .is_some_and(|pending| !pending.same_scope(&current))
        || state
            .completed
            .as_ref()
            .is_some_and(|(key, _)| !key.same_scope(&current))
    {
        state.cancel();
    }
    state.requires_query = current.snap.active();
    let hovered =
        nodes.single().is_ok_and(|cursor| cursor.cursor_over()) && editor.cursor_world.is_some();
    if mouse.just_pressed(MouseButton::Left) && hovered && current.snap.active() {
        state.pending = Some(current.clone());
        state.awaiting_object_hit = current.object.is_some();
        state.completed = None;
        state.last_started = None;
    }
    if current.object.is_some() && editor.ground_click_world.is_some()
        && raw_object_snap.or(frozen_object_snap).unwrap_or(current.snap).active()
    {
        let mut confirmed = current.clone();
        confirmed.snap = raw_object_snap.or(frozen_object_snap).unwrap_or(current.snap);
        state.pending = Some(confirmed);
        state.awaiting_object_hit = false;
        state.accepted = None;
        state.completed = None;
        state.last_started = None;
    }
    if let Some((key, hit)) = completion.filter(|(key, _)| key.same_scope(&current)) {
        state.accepted = Some((key, hit));
    }
    if !state.awaiting_object_hit
        && let Some(pending) = &state.pending
        && let Some((key, hit)) = &state.accepted
        && key == pending
    {
        state.completed = Some((key.clone(), *hit));
        state.pending = None;
    }
    let desired = state
        .pending
        .clone()
        .or_else(|| (hovered && current.snap.active()).then_some(current.clone()));
    if let Some(key) = desired
        && state.task.is_none()
        && state.last_started.as_ref() != Some(&key)
    {
        state.last_started = Some(key.clone());
        state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            let point = Vec3::from_array(key.point.map(f32::from_bits));
            let bounds = key.bounds.map(f32::from_bits);
            let inside = key.snap.horizontal(point.to_array()).is_some_and(|point| {
                point[0] >= bounds[0]
                    && point[2] >= bounds[1]
                    && point[0] <= bounds[2]
                    && point[2] <= bounds[3]
            });
            let hit = match source {
                SnapSurface::Object(query) => surface::object_hit(&query, point, key.snap),
                SnapSurface::Legacy { mesh, index, sampler } if inside =>
                    surface::mesh_hit(&mesh, &index, point, key.snap, sampler.as_ref()),
                SnapSurface::Legacy { .. } => ClipmapGroundHit::Miss,
            };
            (key, hit)
        })); // 只移动Arc，完整BVH补查不在主帧执行。
    }
    let visible = state
        .pending
        .as_ref()
        .and_then(|pending| state.accepted.as_ref().filter(|(key, _)| key == pending))
        .or_else(|| {
            state.accepted.as_ref().filter(|(key, _)| {
                hovered
                    && current.snap.active()
                    && key.same_scope(&current)
                    && key.snap == current.snap
            })
        });
    state.preview = visible.and_then(|(_, hit)| match hit {
        ClipmapGroundHit::Hit(point) => Some(*point),
        _ => None,
    });
}
