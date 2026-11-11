//! 多选的共同移动、Y轴旋转和等比缩放：常数父级实时预览，后台生成完整单次历史。

use super::{
    group_types::*,
    input::MoveInputContext,
    math::{MoveCamera, handle_point},
    resources::PlacementMoveState,
    transaction::MoveWorld,
    types::MoveHandle,
};
use crate::authoring::{
    CourtyardBatchChange, EditorAuthoringSession, PlacementTerrainSampler,
    update_follow_offset_from_world_y,
};
use crate::authoring::{placement_record_bounds,
    placement_pose::{is_automatic_volume, placement_record_frame, source_to_world, world_to_source}};
use crate::resources::{SceneEditorState, SceneEditorViewportState};
use crate::viewport::{modes::ViewportMode, multi_select::PlacementSelectionSet};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
    ui::RelativeCursorPosition,
};
use feathrise_scene::DynamicClipmapGroundSurface;

pub(super) fn handle_group(
    state: &mut PlacementMoveState,
    mut selection: Option<&mut PlacementSelectionSet>,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    mouse: &ButtonInput<MouseButton>,
    keyboard: &mut ButtonInput<KeyCode>,
    context: &MoveInputContext,
    viewport: Option<&SceneEditorViewportState>,
    node: Option<(&RelativeCursorPosition, &ComputedNode)>,
    camera: Option<MoveCamera>,
    available: bool,
    terrain_content: Option<[u64; 7]>,
    clipmap: Option<&DynamicClipmapGroundSurface>,
    world: &mut MoveWorld,
) -> bool {
    let mut ready = state.group_task.as_mut().and_then(check_ready);
    if ready.is_some() {
        state.group_task = None;
    }
    if state
        .group
        .as_ref()
        .is_none_or(|drag| drag.phase != GroupPhase::Waiting)
    {
        discard_ready(state, &mut ready);
    }
    if let Some(mut drag) = state.group.take() {
        state.owns_mouse = true;
        let escaped = keyboard.just_pressed(KeyCode::Escape);
        keyboard.clear();
        if drag.phase == GroupPhase::Finishing {
            if !world.group_cleanup(session, &mut drag) {
                state.group = Some(drag);
            }
            return true;
        }
        let current = available
            && context.mode.mode == drag.handle.mode()
            && camera == Some(drag.camera)
            && node.is_some_and(|(_, node)| node.size() == drag.ui_size)
            && session.revision == drag.author_revision
            && session.placement_structure_revision == drag.structure_revision
            && session.terrain_revision == drag.terrain_revision
            && session.placement_derived_pose_revision == drag.derived_pose_revision
            && world.group_runtime_identity_current(&drag)
            && drag
                .terrain_content
                .is_none_or(|old| terrain_content == Some(old))
            && selection.as_ref().is_some_and(|selection| {
                selection.revision == drag.selection_revision && selection.ready_for(session)
            })
            && session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.scene_id == drag.scene_id && drag.terrain_sources.iter()
                    .all(|source| world.patch_source_current(draft, source)))
            && editor.selected_scene_id.as_ref() == Some(&drag.scene_id)
            && viewport.is_some_and(|viewport| viewport.framed_scene_id.as_ref() == Some(&drag.scene_id));
        if !current
            || escaped
            || mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
            || (drag.prepared > 0 || drag.pending_preview.is_some())
                && !world.group_root_identity_current(session, &drag)
        {
            discard_ready(state, &mut ready);
            cancel_group(state, world, &mut drag);
            state.group = Some(drag);
            editor.status = "已取消整组变换，恢复当前作者姿态".into();
            return true;
        }
        if drag.terrain_content.is_none() {
            drag.terrain_content = terrain_content;
        }
        if drag.phase == GroupPhase::Preparing {
            if !mouse.pressed(MouseButton::Left) {
                cancel_group(state, world, &mut drag);
                state.group = Some(drag);
                return true;
            }
            if drag.pending_preview.take().is_some() || drag.prepared == 0 {
                drag.root_epoch = world.root_epoch();
            } // 上方已确认确切自有补生根；采用其一次成员Add后才继续本帧固定预算。
            let end = (drag.prepared + 16).min(drag.members.len());
            let mut error = None;
            while drag.prepared < end {
                let member = &drag.members[drag.prepared];
                let record = session
                    .draft
                    .as_ref()
                    .and_then(|draft| draft.placements.placements.get(member.index))
                    .filter(|record| record.id == member.id && record.editor.selectable()
                        && record.building_preset_id.is_none() && !is_automatic_volume(record))
                    .cloned();
                let Some(record) = record else {
                    error = Some("已选普通资产已变化".to_owned());
                    break;
                };
                let source = session.draft.as_ref().ok_or_else(|| "场景已离开".to_owned()).and_then(|draft| {
                    let bounds = placement_record_bounds(draft, &record)?;
                    record.validated_transform(&drag.scene_id, bounds)?;
                    Ok((bounds, placement_record_frame(draft, &record)?, world.patch_source_snapshot(draft, &record)?))
                });
                let (bounds, frame, patch_source) = match source {
                    Ok(source) => source,
                    Err(message) => { error = Some(message); break; }
                };
                match world.group_bind(session, member.index, &record, frame, &mut drag.pivots) {
                    Ok(Some(roots)) => {
                        let scale = Vec3::from_array(record.scale);
                        drag.min_factor = drag.min_factor.max(feathrise_scene_content::MIN_SCENE_PLACEMENT_SCALE / scale.min_element());
                        drag.max_factor = drag.max_factor.min(feathrise_scene_content::MAX_SCENE_PLACEMENT_SCALE / scale.max_element());
                        // 全组选一个合法模型倍率；不以旧场景矩形限制成员原点。
                        drag.roots.extend(roots);
                        drag.sources.push(GroupSource { index: member.index, before: record, frame, bounds });
                        if let Some(source) = patch_source
                            && !drag.terrain_sources.iter().any(|old| old.id == source.id) {
                            drag.terrain_sources.push(source);
                        }
                        drag.prepared += 1;
                    }
                    Ok(None) => {
                        drag.pending_preview = session.placement_previews.get(&record.id).copied();
                        if drag.pending_preview.is_none() { error = Some("作者预览未取得稳定地址".into()); }
                        break;
                    }
                    Err(message) => {
                        error = Some(message);
                        break;
                    }
                }
            }
            if let Some(error) = error {
                cancel_group(state, world, &mut drag);
                editor.status = format!("整组变换取消：{error}");
            } else if drag.prepared == drag.members.len() {
                drag.root_epoch = world.root_epoch();
                drag.phase = GroupPhase::Dragging;
                editor.status = "正在整组变换；Ctrl吸附，Esc取消".into();
            } else {
                editor.status = format!("准备整组预览 {}/{}…", drag.prepared, drag.members.len());
            }
            state.group = Some(drag);
            return true;
        }
        if drag.phase == GroupPhase::Waiting {
            if let Some(result) = ready {
                let result = match result {
                    Ok(prepared) => world
                        .group_adopt(session, editor, prepared, &drag.scene_id)
                        .map_err(|(error, prepared)| {
                            recycle_owned(state, prepared);
                            error
                        }),
                    Err(error) => Err(error),
                };
                match result {
                    Ok(Some(retired)) => {
                        drag.committed = true;
                        recycle_owned(state, retired); // 旧记录析构也不集中到采用帧，复用已空闲的同一个worker槽。
                        if let Some(selection) = selection.as_deref_mut() {
                            let mut members = selection.members.as_ref().clone();
                            for member in &mut members {
                                member.position = drag.transformed_position(member.position);
                            }
                            selection.publish(members, session);
                        }
                        drag.delta = Vec3::ZERO; // 选择缓存已原子更新，标记和手柄不再重复叠加预览位移。
                        drag.rotation_degrees = 0.0;
                        drag.factor = 1.0;
                        drag.phase = GroupPhase::Finishing;
                        editor.status = "已完成整组变换；Ctrl+Z一次撤销整组".into();
                    }
                    Ok(None) => {
                        cancel_group(state, world, &mut drag);
                        editor.status = "整组变换未变化".into();
                    }
                    Err(error) => {
                        cancel_group(state, world, &mut drag);
                        editor.status = format!("整组变换未应用：{error}");
                    }
                }
            }
            state.group = Some(drag);
            return true;
        }
        let normalized = node
            .and_then(|(cursor, _)| cursor.normalized)
            .filter(|p| p.is_finite());
        let point = normalized
            .and_then(|p| drag.camera.ray(p))
            .and_then(|ray| handle_point(ray, drag.origin, drag.handle));
        let Some((normalized, point)) = normalized.zip(point) else {
            cancel_group(state, world, &mut drag);
            state.group = Some(drag);
            return true;
        };
        let moved =
            ((normalized - drag.start_cursor) * drag.camera.size * drag.camera.inverse_scale)
                .length_squared()
                > 4.0;
        let snapping = drag.snapping.active(keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]));
        let step = if snapping {
            drag.snapping.translation_meters
        } else {
            0.0
        };
        let position = if moved && drag.handle.mode() == ViewportMode::Move {
            super::math::handle_translation_mode(
                drag.origin,
                point - drag.start_point,
                drag.handle,
                step,
                drag.snapping.mode,
                drag.bounds.min,
                drag.bounds.max,
            )
        } else {
            Some(drag.origin)
        };
        let Some(position) = position else {
            cancel_group(state, world, &mut drag);
            state.group = Some(drag);
            return true;
        };
        let mut delta = position - drag.origin;
        // 整组统一裁切位移，禁止各对象单独贴边导致相对间距改变。
        delta.x = delta.x.clamp(drag.delta_min.x, drag.delta_max.x);
        delta.z = delta.z.clamp(drag.delta_min.z, drag.delta_max.z);
        drag.delta = delta;
        let valid = match drag.handle {
            MoveHandle::RotationY if moved => super::rotation::rotation_yaw(
                0.0, drag.origin, drag.start_point, point,
                if snapping { drag.snapping.rotation_degrees } else { 0.0 },
            ).map(|angle| drag.rotation_degrees = angle).is_some(),
            MoveHandle::UniformScale if moved => super::scale::group_scale_factor(
                (normalized - drag.start_cursor) * drag.camera.size * drag.camera.inverse_scale,
                if snapping { drag.snapping.scale_factor } else { 0.0 }, drag.min_factor, drag.max_factor,
            ).map(|factor| drag.factor = factor).is_some(),
            _ => { drag.rotation_degrees = 0.0; drag.factor = 1.0; true },
        };
        if !valid {
            cancel_group(state, world, &mut drag);
            state.group = Some(drag);
            editor.status = "整组变换目标无效，已取消".into();
            return true;
        }
        world.group_preview(&drag, delta);
        if let Some(snapshot) = clipmap
            .and_then(|surface| surface.snapshot.as_ref())
            .filter(|snapshot| {
                snapshot.scene_id == drag.scene_id
                    && snapshot.authoring_height_revision() == Some(drag.terrain_revision)
            })
        {
            // 只请求最多16个目标附近的高度页，不为相距很远的选集申请完整大矩形。
            for _ in 0..16.min(drag.members.len()) {
                let position = drag.transformed_position(drag.members[drag.prefetch_cursor].position).xz();
                let _ = snapshot.request_authoritative_rect(
                    position - Vec2::splat(0.01),
                    position + Vec2::splat(0.01),
                );
                drag.prefetch_cursor = (drag.prefetch_cursor + 1) % drag.members.len();
            }
        }
        if mouse.just_released(MouseButton::Left) {
            if delta.length_squared() <= 1.0e-10 && drag.rotation_degrees.abs() <= 1.0e-5 && (drag.factor - 1.0).abs() <= 1.0e-6 {
                cancel_group(state, world, &mut drag);
                editor.status = "整组变换未变化".into();
            } else {
                let sampler = session.draft.as_ref().and_then(|draft| {
                    PlacementTerrainSampler::from_sources(draft, clipmap, session.terrain_revision)
                        .ok()
                });
                let sources = std::mem::take(&mut drag.sources);
                let map = drag.scene_id.clone();
                let origin = drag.origin;
                let angle = drag.rotation_degrees;
                let factor = drag.factor;
                state.group_task = Some(AsyncComputeTaskPool::get().spawn(async move {
                    prepare_group_transform(sources, origin, delta, angle, factor, sampler, map)
                }));
                drag.phase = GroupPhase::Waiting;
                editor.status = "正在完成整组变换校验…".into();
            }
        }
        state.group = Some(drag);
        return true;
    }
    let Some(selection) = selection.filter(|selection| selection.members.len() > 1) else {
        return false;
    };
    if state.drag.is_some() || state.pending_building.is_some() {
        return false;
    } // 旧单体手势先完成失效恢复。
    if !context.mode.mode.transforms_placement() {
        return true;
    }
    if !available || !selection.ready_for(session) || state.owns_mouse || context.pointer_over_ui()
    {
        return true;
    }
    let (Some(camera), Some((cursor, node)), Some(viewport), Some(draft)) =
        (camera, node, viewport, session.draft.as_ref())
    else {
        return true;
    };
    if !cursor.cursor_over() || viewport.framed_scene_id.as_ref() != Some(&draft.scene_id) {
        return true;
    }
    let Some(normalized) = cursor.normalized.filter(|p| p.is_finite()) else {
        return true;
    };
    let handle = match context.mode.mode {
        ViewportMode::Move => camera.pick_handle(selection.center, normalized, false),
        ViewportMode::Rotate => super::rotation::pick_rotation(camera, selection.center, normalized).then_some(MoveHandle::RotationY),
        ViewportMode::Scale => super::scale::pick_scale(camera, selection.center, normalized).then_some(MoveHandle::UniformScale),
        _ => None,
    };
    let Some(handle) = handle else {
        return true;
    };
    state.hover = Some(handle);
    if !mouse.just_pressed(MouseButton::Left)
        || mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
    {
        return true;
    }
    if state.group_task.is_some() {
        editor.status = "上一项整组后台任务仍在回收，请稍候".into();
        return true;
    }
    let (Some(point), Some(bounds)) = (
        camera
            .ray(normalized)
            .and_then(|ray| handle_point(ray, selection.center, handle)),
        draft.manifest.world_bounds(),
    ) else {
        return true;
    };
    state.group = Some(GroupDrag {
        scene_id: draft.scene_id.clone(),
        author_revision: session.revision,
        structure_revision: session.placement_structure_revision,
        terrain_revision: session.terrain_revision,
        derived_pose_revision: session.placement_derived_pose_revision,
        terrain_content,
        selection_revision: selection.revision,
        members: selection.members.clone(),
        origin: selection.center,
        start_point: point,
        start_cursor: normalized,
        camera,
        ui_size: node.size(),
        handle,
        snapping: context.preferences.as_ref().map_or_else(Default::default, |settings| settings.editor_snapping.sanitized()),
        bounds,
        delta_min: Vec3::splat(f32::NEG_INFINITY),
        delta_max: Vec3::splat(f32::INFINITY),
        delta: Vec3::ZERO,
        rotation_degrees: 0.0,
        factor: 1.0,
        min_factor: 0.0,
        max_factor: f32::INFINITY,
        pivots: Vec::new(),
        phase: GroupPhase::Preparing,
        committed: false,
        sources: Vec::new(),
        terrain_sources: Vec::new(),
        roots: Vec::new(),
        prepared: 0,
        prefetch_cursor: 0,
        cleanup_cursor: 0,
        root_epoch: world.root_epoch(),
        pending_preview: None,
        formal_runtime_identity: world.group_runtime_identity(),
    });
    state.owns_mouse = true;
    true
}

fn prepare_group_transform(
    sources: Vec<GroupSource>,
    origin: Vec3,
    delta: Vec3,
    angle: f32,
    factor: f32,
    sampler: Option<PlacementTerrainSampler>,
    map: feathrise_scene_content::SceneId,
) -> Result<PreparedGroupMove, String> {
    let mut changed = Vec::new();
    let mut apply = Vec::new();
    for GroupSource { index, before, frame, bounds } in sources {
        let mut after = before.clone();
        let rotation = Quat::from_rotation_y(angle.to_radians());
        if before.terrain_patch.is_some() {
            let world = source_to_world(feathrise_scene_content::TerrainObjectTransform {
                translation: before.translation, yaw_degrees: before.yaw_degrees, scale: before.scale, ..default()
            }, frame)?;
            let change = Mat4::from_scale_rotation_translation(Vec3::splat(factor), rotation,
                origin + delta - rotation * (origin * factor));
            let pose = world_to_source(change * world, frame)?;
            after.translation = pose.translation;
            // 纯移动/等比缩放不编辑局部朝向；精确可表达性已校验，保留原标量避免舍入变成无关编辑。
            after.yaw_degrees = if angle == 0.0 { before.yaw_degrees } else { pose.yaw_degrees };
            after.scale = if angle == 0.0 { (Vec3::from_array(before.scale) * factor).to_array() } else { pose.scale };
        } else {
            after.translation = (origin + delta + rotation * ((Vec3::from_array(before.translation) - origin) * factor)).to_array();
            after.yaw_degrees = (before.yaw_degrees + angle).rem_euclid(360.0);
            after.scale = (Vec3::from_array(before.scale) * factor).to_array();
        }
        if after.terrain_patch.is_none() && after.height_mode.follows_terrain() {
            let sampler = sampler
                .as_ref()
                .ok_or("当前作者地形高度尚未就绪，整组已取消")?;
            update_follow_offset_from_world_y(&mut after, sampler)?; // 保持整组真实世界位移，用偏移保存新高度而不逐项改变间距。
        }
        if after.terrain_patch.is_none() && let Some(sampler)=sampler.as_ref() {sampler.refresh_record_binding(&mut after)?;}
        after.validated_transform(&map, bounds)?;
        if after != before {
            apply.push((index, after.clone()));
            changed.push((index, before, after));
        }
    }
    let history = if changed.is_empty() {
        None
    } else {
        Some(CourtyardBatchChange::new(Vec::new(), changed)?)
    };
    let retired = Vec::with_capacity(apply.len());
    Ok(PreparedGroupMove {
        apply,
        history,
        retired,
    })
}

/// 取消捕获、失效结果和采用后旧记录都在原单槽回收，禁止在帧内集中析构整批水绑定字符串。
fn recycle_owned<T: Send + 'static>(state: &mut PlacementMoveState, owned: T) {
    debug_assert!(state.group_task.is_none());
    state.group_task = Some(AsyncComputeTaskPool::get().spawn(async move {
        drop(owned);
        Ok(PreparedGroupMove {
            apply: Vec::new(),
            history: None,
            retired: Vec::new(),
        })
    }));
}

fn discard_ready(
    state: &mut PlacementMoveState,
    ready: &mut Option<Result<PreparedGroupMove, String>>,
) {
    if let Some(result) = ready.take()
        && result
            .as_ref()
            .is_ok_and(|prepared| !prepared.apply.is_empty() || prepared.history.is_some())
    {
        recycle_owned(state, result);
    }
}

fn cancel_group(state: &mut PlacementMoveState, world: &mut MoveWorld, drag: &mut GroupDrag) {
    world.group_cancel(drag);
    if !drag.sources.is_empty() {
        recycle_owned(state, std::mem::take(&mut drag.sources));
    }
}
