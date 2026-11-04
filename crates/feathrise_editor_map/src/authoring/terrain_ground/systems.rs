//! 索引按固定16上下文预算捕获，后台核对完整对象集合；迟到结果不获得当前作者修订信用。
use bevy::{prelude::*, tasks::{AsyncComputeTaskPool, futures::check_ready}};
use feathrise_scene::TerrainObjectQueries;
use std::sync::Arc;
use super::{index::TerrainGroundSnapshot, resources::*};
use crate::{authoring::EditorAuthoringSession, resources::MapEditorState};

pub(crate) fn capture(editor: Res<MapEditorState>, mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<TerrainGroundCapture>, queries: TerrainObjectQueries,
    transform: Res<crate::ui::terrain_objects::TerrainObjectsEditorState>) {
    if let Some(result) = state.work.as_mut().and_then(|work| check_ready(&mut work.task)) {
        let work = state.work.take().unwrap();
        if editor.open && session.terrain_revision == work.revision && !transform.preparing() && !transform.transforming
            && let Some(draft) = session.draft.as_mut().filter(|draft| draft.map_id == work.map
                && draft.manifest.terrains.as_ref().is_some_and(|objects| Arc::ptr_eq(objects, &work.objects)))
            && let Ok(snapshot) = result { draft.terrain_ground = Some(snapshot); }
    }
    let Some(draft) = session.draft.as_ref().filter(|_| editor.open && !transform.preparing() && !transform.transforming) else { state.capture = None; return; };
    let Some(objects) = &draft.manifest.terrains else { state.capture = None; return; };
    if state.work.is_some() { return; }
    let valid = state.capture.as_ref().is_some_and(|c| c.map == draft.map_id && c.revision == session.terrain_revision && Arc::ptr_eq(&c.objects, objects));
    if !valid { state.capture = Some(Capture { map: draft.map_id.clone(), revision: session.terrain_revision,
        objects: objects.clone(), cursor: None, inputs: Vec::new() }); }
    for _ in 0..16 {
        let capture = state.capture.as_mut().unwrap();
        if let Some(entity) = queries.next_instance(capture.cursor) {
            capture.cursor = Some(entity);
            if let Some(input) = queries.snapshot_versioned(entity, &capture.map) { capture.inputs.push(input); }
        } else {
            let capture = state.capture.take().unwrap();
            let current = draft.terrain_ground.clone();
            let map = capture.map.clone(); let revision = capture.revision; let objects = capture.objects.clone();
            state.work = Some(Work { map, revision, objects, task: AsyncComputeTaskPool::get().spawn(async move {
                let generation = current.as_ref().map_or(1, |current| {
                    let same_content = current.versions.len() == capture.inputs.len() && current.versions.iter()
                        .zip(&capture.inputs).all(|(a,(_,_,b))| a.same_content(b));
                    if same_content { current.generation } else { current.generation.wrapping_add(1) }
                });
                if let Some(current) = current && current.revision == capture.revision {
                    let same = current.versions.len() == capture.inputs.len()
                        && current.versions.iter().zip(&capture.inputs).all(|(version, (_, _, v))| v == version);
                    if same { return Ok(current); }
                }
                let mut result = TerrainGroundSnapshot::build(capture.map, capture.revision, capture.objects, capture.inputs)?;
                Arc::get_mut(&mut result).unwrap().generation = generation;
                Ok(result)
            }) });
            break;
        }
    }
}
pub(super) fn cursor(mut editor: ResMut<MapEditorState>, session: Res<EditorAuthoringSession>, mut cursor: ResMut<TerrainGroundCursor>,
    ownership: (
        Option<Res<crate::resources::MapEditorNumericScrubMouseGuard>>,
        Option<Res<crate::resources::MapBrowserState>>,
        Option<Res<crate::resources::MapBrowserDragState>>,
        Option<Res<crate::authoring::TerrainExternalChangeState>>,
        Option<Res<crate::viewport::placement_move::PlacementMoveState>>,
        Option<Res<crate::resources::MapEditorSettingsState>>,
        Option<Res<crate::viewport::modes::ViewportModeState>>,
        Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
    ),
    keys: Option<Res<ButtonInput<KeyCode>>>, mouse: Option<Res<ButtonInput<MouseButton>>>,
    lod: crate::authoring::mesh_lod::MeshLodInputGate,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>) {
    let current = session.draft.as_ref().and_then(|draft| draft.terrain_ground.clone()
        .filter(|ground| ground.map == draft.map_id && ground.revision == session.terrain_revision));
    let owns_cursor = session.draft.as_ref().is_some_and(|draft| draft.manifest.terrains.is_some());
    let usable = owns_cursor && editor.open && lod.ready() && !editor.close_requested && !editor.save_requested
        && session.draft.as_ref().is_some_and(|draft|
            editor.selected_map_id.as_ref() == Some(&draft.map_id));
    let input_available = usable && !session.saving && !editor.new_map_dialog_open
        && !editor.suppress_world_pick_this_frame && editor.map_switch_confirmation.is_none()
        && editor.pending_map_transition.is_none() && !editor.map_transition_in_progress
        && !ownership.0.as_ref().is_some_and(|guard| guard.blocked())
        && !ownership.1.as_ref().is_some_and(|browser| browser.dialog.is_some())
        && !ownership.2.as_ref().is_some_and(|drag| drag.blocks_other_mouse())
        && !ownership.3.as_ref().is_some_and(|conflict| conflict.modal_open())
        && !ownership.4.as_ref().is_some_and(|movement| movement.owns_mouse())
        && !ownership.5.as_ref().is_some_and(|settings| settings.open || settings.owns_mouse_this_frame)
        && !ownership.6.as_ref().is_some_and(|mode| mode.pointer_over_controls)
        && !ownership.7.as_ref().is_some_and(|workspace| workspace.active_tab != crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Map)
        && !keys.as_ref().is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
        && !mouse.as_ref().is_some_and(|mouse| mouse.any_pressed([MouseButton::Right, MouseButton::Middle]))
        && windows.iter().all(|window| window.focused);
    cursor.input_available = input_available;
    cursor.paint_hit = None;
    cursor.paint_pending = false;
    let paint_available = input_available
        && mouse.as_ref().is_some_and(|mouse| mouse.pressed(MouseButton::Left))
        && !keys.as_ref().is_some_and(|keys| keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]))
        && matches!(editor.tool, crate::types::EditorTool::Surface | crate::types::EditorTool::Vegetation
            | crate::types::EditorTool::Raise | crate::types::EditorTool::Lower
            | crate::types::EditorTool::Smooth | crate::types::EditorTool::Flatten | crate::types::EditorTool::Water)
        && cursor.ray.is_some() && cursor.identity.is_some();
    if !paint_available { cursor.paint_anchor = None; }
    let mut paint_completed = false;
    // hover仍占槽时pending尚无CursorWork快照；必须直接核对按下时冻结的地形身份，换代后永久废票。
    let pending_changed = cursor.pending_click.as_ref().is_some_and(|(identity, _)| cursor.identity.as_ref() != Some(identity)
        || identity.revision != session.revision || !identity.matches_ground(session.terrain_revision, current.as_ref()));
    let running_changed = cursor.work.as_ref().is_some_and(|work| work.click && work.click_ticket == cursor.click_ticket
        && (cursor.identity.as_ref() != Some(&work.identity) || work.identity.revision != session.revision
            || !work.identity.matches_ground(session.terrain_revision, current.as_ref()) || !Arc::ptr_eq(&editor.terrain_hidden, &work.hidden)
            || current.as_ref().is_none_or(|ground| !Arc::ptr_eq(ground, &work.snapshot))));
    if !input_available || pending_changed || running_changed {
        cursor.cancel_click(); // 阻塞帧立即换票；解锁、相机或偏好恢复后，旧原始射线也不能重新获得点击权限。
        editor.ground_click_world = None;
        editor.ground_click_is_lower_layer = false;
    }
    if !input_available || cursor.display.as_ref().is_some_and(|sample|
        sample.authority != cursor.click_ticket
            || cursor.ray.is_none_or(|ray| ray.epoch != sample.ray.epoch)
            || cursor.identity.as_ref().is_none_or(|identity| !super::paint_cursor::same_display_context(
                sample, identity, session.draft.as_ref()))) {
        cursor.display = None; // 只复用同源真点；退出视口、换图、相机/按压换代立即交回普通鼠标。
    }
    if let Some(result) = cursor.work.as_mut().and_then(|work| check_ready(&mut work.task)) {
        let work = cursor.work.take().unwrap();
        let strict_context = cursor.identity.as_ref() == Some(&work.identity);
        // 本笔密度/地表采用会推进通用修订，但真实几何没有变；连续消费者各自核对预期作者修订。
        // 严格点击和悬停仍走完整相等，不能借这条门把旧射线变成新的对象选择。
        let continuous_context = cursor.identity.as_ref().is_some_and(|identity|
            super::paint_cursor::same_context(&work.identity, identity));
        if usable && (strict_context || continuous_context)
            && Arc::ptr_eq(&editor.terrain_hidden, &work.hidden)
            && current.as_ref().is_some_and(|ground| Arc::ptr_eq(ground, &work.snapshot)) {
            if input_available && work.click_ticket == cursor.click_ticket
                && cursor.ray.is_some_and(|ray| ray.epoch == work.ray.epoch) {
                cursor.display = result.as_ref().map(|hit| CursorDisplaySample {
                    identity: work.identity.clone(), ray: work.ray, authority: cursor.click_ticket,
                    hit: CursorDisplayHit { terrain: hit.terrain.clone(), point: hit.point, volume_surface: hit.volume_surface },
                    definition: work.snapshot.object_definition(&hit.terrain).cloned(),
                }); // 显示新完成真点，不把它冒充当前严格射线命中或单击。
            }
            if paint_available && work.click_ticket == cursor.click_ticket
                && cursor.ray.is_some_and(|ray| ray.epoch == work.ray.epoch) {
                paint_completed = true;
                cursor.paint_hit = result.as_ref().and_then(|hit| hit.paint_point.map(|point|
                    CursorPaintHit { terrain: hit.terrain.clone(), point, from: hit.paint_from }));
                cursor.paint_anchor = cursor.paint_hit.clone().map(|hit| CursorPaintAnchor {
                    identity: work.identity.clone(), ray: work.ray, authority: cursor.click_ticket, hit,
                });
                // 连续鼠标移动只改变射线位置，不能把真实后台完成点当成悬停过期而永久饿死。
            }
            if strict_context && work.click && input_available && work.click_ticket == cursor.click_ticket {
                if let Some(hit) = &result { editor.ground_click_world = Some(hit.point); editor.ground_click_is_lower_layer=hit.lower_layer; cursor.click_terrain = Some(hit.terrain.clone()); cursor.click_snap = work.click_snap; }
                else { editor.status = "本次单击没有命中真实地形表面".into(); }
            }
            if strict_context && cursor.ray == Some(work.ray) { cursor.accepted = Some((work.snapshot, work.ray, work.hidden, result)); }
        }
    }
    if !input_available {
        cursor.accepted = None; cursor.terrain = None; cursor.volume_ray = None;
        editor.brush_terrain_id = None;
        // 旧单地形由相机系统发布真实GLB命中；多对象查询退出时只回收自己的槽，不能抹掉另一条拾取路径。
        if owns_cursor {
            editor.cursor_world = None; editor.brush_cursor_world = None;
            editor.cursor_triangle = None; editor.brush_cursor_triangle = None;
        }
        return; // 阻塞只轮询既有任务；解锁后可恢复悬停，但必须有新按下才重新获得点击权限。
    }
    let hit = cursor.accepted.as_ref().filter(|(snapshot, ray, hidden, _)| cursor.ray == Some(*ray)
        && Arc::ptr_eq(hidden, &editor.terrain_hidden)
        && current.as_ref().is_some_and(|ground| Arc::ptr_eq(ground, snapshot))).and_then(|(_, _, _, hit)| hit.as_ref());
    let terrain = hit.map(|hit|hit.terrain.clone()); let point=hit.map(|hit|hit.point);
    let volume_ray=hit.filter(|hit|hit.volume_surface).and(cursor.ray).map(|ray|(ray.origin,ray.direction));
    let stationary_paint = hit.and_then(|hit| hit.paint_point.map(|point|
        CursorPaintHit { terrain: hit.terrain.clone(), point, from: Some(point) }));
    if paint_available && cursor.paint_hit.is_none() && !paint_completed {
        cursor.paint_hit = stationary_paint;
        if let (Some(hit), Some(identity), Some(ray)) = (cursor.paint_hit.clone(), cursor.identity.clone(), cursor.ray) {
            cursor.paint_anchor = Some(CursorPaintAnchor { identity, ray, authority: cursor.click_ticket, hit });
        }
    }
    cursor.terrain = terrain;
    cursor.volume_ray=volume_ray;
    editor.brush_terrain_id = cursor.terrain.clone();
    editor.cursor_world = point; editor.brush_cursor_world = editor.cursor_world;
    editor.cursor_triangle = None; editor.brush_cursor_triangle = None;
    if cursor.work.is_some() {
        cursor.paint_pending = paint_available && !paint_completed && cursor.paint_hit.is_none()
            && cursor.work.as_ref().is_some_and(|work| cursor.identity.as_ref().is_some_and(|identity|
                super::paint_cursor::same_context(&work.identity, identity))
                && work.click_ticket == cursor.click_ticket && cursor.ray.is_some_and(|ray| ray.epoch == work.ray.epoch));
        return;
    }
    let Some(snapshot) = current else { return; };
    let request = if let Some((identity, ray)) = cursor.pending_click.take() { Some((identity, ray, true)) }
        else { cursor.identity.clone().zip(cursor.ray).filter(|(_, ray)| cursor.accepted.as_ref()
            .is_none_or(|(accepted, old, hidden, _)| !Arc::ptr_eq(accepted, &snapshot) || *old != *ray
                || !Arc::ptr_eq(hidden, &editor.terrain_hidden))).map(|(identity, ray)| (identity, ray, false)) };
    if let Some((identity, ray, click)) = request {
        let frozen = snapshot.clone();
        let hidden = editor.terrain_hidden.clone(); let frozen_hidden = hidden.clone();
        let click_ticket = cursor.click_ticket;
        let click_snap = if click { cursor.pending_click_snap.take() } else { None };
        let paint_anchor = cursor.paint_anchor.clone().filter(|anchor| paint_available
            && anchor.authority == click_ticket && anchor.ray.epoch == ray.epoch
            && super::paint_cursor::same_context(&anchor.identity, &identity));
        let paint_identity = identity.clone();
        cursor.work = Some(CursorWork { snapshot, ray, identity, click, click_ticket, click_snap, hidden, task: AsyncComputeTaskPool::get().spawn(async move {
            frozen.ray_visible(ray.origin, ray.direction, &frozen_hidden).map(|(id,point)| {
                let lower=frozen.support(point.x,point.z,None).is_none_or(|top|(top-point.y).abs()>0.01);
                let volume_surface=frozen.object_query(&id).and_then(|query|query.ray_surface(ray.origin,ray.direction,1_000_000.))
                    .is_some_and(|(surface,_)|surface.distance(point)<0.05);
                let paint_point = paint_available.then(|| super::paint_cursor::author_point(&frozen, &id, point)).flatten();
                let paint_from = paint_point.and_then(|point| super::paint_cursor::connected_from(
                    &frozen, &frozen_hidden, paint_anchor.as_ref(), &id, point, &paint_identity));
                CursorHit {terrain:id,point,lower_layer:lower,volume_surface,paint_point,paint_from}
            })
        }) });
        cursor.paint_pending = paint_available && !paint_completed && cursor.paint_hit.is_none();
    }
}
