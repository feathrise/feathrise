//! 视口只冻结一个段和一个最新尾段；缺页、模式与来源身份均为写入门。
use super::{jobs, types::*};
use crate::{
    authoring::{EditorAuthoringSession, WaterFileHandoff},
    components::SceneEditorViewport,
    resources::SceneEditorState,
    types::EditorTool,
    viewport::{
        VegetationGroundCursor,
        modes::{ViewportMode, ViewportModeState},
        vegetation_ground::GroundCursorTarget,
    },
};
use bevy::{prelude::*, tasks::IoTaskPool, ui::RelativeCursorPosition};
use feathrise_scene_content::{ScenePlacementRecord, SceneTerrainKind};
use feathrise_terrain::volume::{
    VolumeBrush, VolumeBrushKind,
    sparse::{SPARSE_CHUNK_CELLS, VolumeSource},
};
use feathrise_scene::{ClipmapGroundHit, ClipmapGroundSnapshot};

pub(super) fn kind(tool: EditorTool) -> Option<VolumeBrushKind> {
    match tool {
        EditorTool::VolumeDig => Some(VolumeBrushKind::Dig),
        EditorTool::VolumeFill => Some(VolumeBrushKind::Fill),
        EditorTool::VolumeSmooth => Some(VolumeBrushKind::Smooth),
        EditorTool::VolumeFlatten => Some(VolumeBrushKind::Flatten),
        _ => None,
    }
}

/// 首次真实命中后锁定整平面；后台忙、缺页重试和鼠标高度变化都不能改变本笔基准。
fn lock_flatten_height(stroke: &mut Stroke, point: &mut Vec3) {
    if stroke.tool == EditorTool::VolumeFlatten
        || stroke.tool.is_volume_sculpt() && stroke.lock_height
    {
        point.y = *stroke.flatten_height.get_or_insert(point.y);
    }
}

pub(super) fn brush(
    time: Res<Time<Real>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    viewport: Query<&RelativeCursorPosition, With<SceneEditorViewport>>,
    mode: Res<ViewportModeState>,
    mut state: ResMut<VolumeAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut ground: ResMut<VegetationGroundCursor>,
    object_cursor: Option<Res<super::super::terrain_ground::TerrainGroundCursor>>,
    surface: Option<feathrise_scene::TerrainObjectClipmapGround>,
    handoff: Res<WaterFileHandoff>,
    stream: Res<feathrise_scene::TerrainVolumeStreamControl>,
    mut spatial: Option<ResMut<crate::viewport::spatial_brush::SpatialBrushState>>,
    paint_scope: Option<Res<crate::authoring::vegetation_paint_scope::VegetationPaintSettings>>,
) {
    let sparse = session
        .draft
        .as_ref()
        .is_some_and(|d| d.terrain_manifest().terrain.kind == SceneTerrainKind::SparseClipmap);
    let field_selection = session.draft.as_ref().and_then(|d| match editor.tool {
        EditorTool::Puddles => Some((
            feathrise_terrain::volume::sparse::paged::VolumeFieldChannel::Puddles,
            "puddles".to_owned(),
            GroundCursorTarget::Puddles,
        )),
        EditorTool::Surface => d
            .surface
            .as_ref()?
            .layers
            .get(editor.selected_surface_layer)
            .and_then(|layer| {
                Some((
                    feathrise_terrain::volume::sparse::paged::VolumeFieldChannel::Surface,
                    editor.surface_selected_preset_id.clone()?,
                    GroundCursorTarget::Surface {
                        layer_id: layer.id.clone(),
                        preset_id: editor.surface_selected_preset_id.clone(),
                    },
                ))
            }),
        EditorTool::Vegetation => d
            .vegetation
            .as_ref()?
            .authoring
            .as_ref()?
            .layers
            .get(editor.selected_vegetation_layer)
            .filter(|l| l.enabled && !l.locked)
            .map(|layer| {
                (
                    feathrise_terrain::volume::sparse::paged::VolumeFieldChannel::Vegetation,
                    layer.output_layer_id.clone(),
                    GroundCursorTarget::Vegetation(layer.id.clone()),
                )
            }),
        _ => None,
    });
    let field_selection=field_selection.filter(|_|editor.tool!=EditorTool::Vegetation
        || paint_scope.as_ref().is_none_or(|s|s.scope==crate::authoring::vegetation_paint_scope::PaintScope::Current)); // 跨网格首版只写室外本地R8，不能同时落到当前网格的三维作者层。
    let field_ray = if let Some(draft) = session.draft.as_ref().filter(|draft|draft.manifest.terrains.is_some()) {
        object_cursor.as_ref().and_then(|cursor|cursor.volume_ray).map(|(origin,direction)| {
            let inverse=draft.terrain_documents.active.as_ref().map_or(Mat4::IDENTITY,|active|
                active.definition.transform.as_transform().to_matrix().inverse());
            (inverse.transform_point3(origin),inverse.transform_vector3(direction).normalize())
        })
    } else { ground.volume_paint_ray() };
    if field_ray.is_some() && field_selection.is_some() {
        editor.brush_radius = editor.brush_radius.clamp(0.1, 8.0);
    } // 光标与实际三维笔触共用半径。
    let field_allowed = field_selection.is_some()
        && (field_ray.is_some() || state.stroke.as_ref().is_some_and(|s| s.field.is_some()));
    let base_allowed = editor.open
        && !editor.close_requested
        && !session.saving
        && !stream.retirement_busy // 退役字段达到信用上限时不接收新源任务，保留当前笔触收口意图。
        && sparse
        && super::super::terrain_documents::canvas_writable(&session, &editor)
        && mode.mode == ViewportMode::Edit
        && (kind(editor.tool).is_some() || field_allowed);
    let allowed = base_allowed
        && !mode.pointer_over_controls
        && !(editor.tool.is_volume_sculpt() && spatial.as_ref().is_some_and(|s| s.enabled()))
        && !keyboard.pressed(KeyCode::ControlLeft)
        && !keyboard.pressed(KeyCode::ControlRight);
    // 【需求设计】执行按钮只交一个冻结段；摆放/拖动不建立Stroke，也不消耗撤销历史。
    let spatial_available = spatial.as_ref().is_some_and(|s| s.authoring_available());
    if let Some(spatial) = spatial.as_mut() {
        match spatial.take_request(&editor, &session) {
            Ok(Some(request)) if base_allowed && spatial_available && !state.busy() => {
                state.stroke = Some(spatial_stroke(
                    request,
                    editor.tool,
                    session.revision,
                    time.elapsed_secs_f64(),
                ));
            }
            Ok(Some(_)) => editor.status = "空间雕刻暂不能执行，请待当前任务结束后重新确认".into(),
            Err(error) => editor.status = error,
            Ok(None) => {}
        }
    }
    let mut cancel_spatial = false;
    if let Some(stroke) = state.stroke.as_mut() {
        let selection_changed = stroke.field.as_ref().is_some_and(|(old, _, _)| {
            field_selection
                .as_ref()
                .is_none_or(|(channel, id, _)| old.channel != *channel || old.id != *id)
        });
        stroke.released |= !mouse.pressed(MouseButton::Left)
            || !allowed
            || stroke.tool != editor.tool
            || selection_changed;
        if selection_changed
            || stroke.tool != editor.tool
            || !editor.open
            || editor.close_requested
            || stroke.spatial.as_ref().is_some_and(|r| {
                !base_allowed || !spatial_available || !r.matches(&editor, &session)
            })
        {
            stroke.first_click_done = true;
            stroke.tail = None; // 工具取消只保留已受理后台段，尚未受理意图不迟到落笔。
            stroke.seed = None;
            cancel_spatial = stroke.spatial.take().is_some();
        }
    }
    if cancel_spatial {
        state.spatial_transaction = None; // 尚未正式采用的批次一起取消，已运行任务继续占槽直至结束。
    }
    if allowed
        && (mouse.just_pressed(MouseButton::Left)
            || field_allowed && mouse.pressed(MouseButton::Left))
        && !state.busy()
    {
        state.stroke = Some(Stroke {
            spatial: None,
            shape: editor.volume_brush_shape,
            half_height: editor.volume_brush_half_height,
            lock_height: editor.volume_brush_lock_height,
            flatten_height: None,
            field: None,
            tool: editor.tool,
            revision: session.revision,
            before: None,
            index: 0,
            released: false,
            last: None,
            tail: None,
            first_click_done: field_allowed,
            seed: None,
            new_region: false,
            history_slot: None,
            started: time.elapsed_secs_f64(),
        });
    }
    let Some((tool, revision)) = state.stroke.as_ref().map(|s| (s.tool, s.revision)) else {
        return;
    };
    if revision != session.revision {
        return;
    } // 外部作者写入会由poll失败关闭本笔。
    let Some(draft) = session.draft.as_ref() else {
        return;
    };
    if state.work.is_none()
        && state.adoption.is_none()
        && time.elapsed_secs_f64() - state.stroke.as_ref().unwrap().started > 30.0
    {
        let stroke = state.stroke.as_mut().unwrap();
        stroke.tail = None;
        stroke.seed = None;
        stroke.released = true;
        stroke.first_click_done = true;
        let spatial = stroke.spatial.take().is_some();
        state.spatial_transaction = None;
        editor.cancel_close_request(if spatial {
            "空间雕刻等待基础页超时；整条路径未采用，可重新执行"
        } else {
            "三维笔刷等待完整基础页超时；已采用部分保留，可重试落笔"
        });
        editor.save_requested = false;
        return;
    }
    if state
        .stroke
        .as_ref()
        .is_some_and(|s| s.released && !s.first_click_done)
        && !ground.volume_click_pending(&draft.scene_id, tool)
    {
        state.stroke.as_mut().unwrap().first_click_done = true; // 视口外或模式控件吞掉点击时也能正常收口。
    }
    if handoff.placement_handoff_in_flight_for(&draft.scene_id) {
        return;
    }
    let Some(snapshot) = surface
        .as_ref()
        .and_then(|s| s.snapshot.as_ref())
        .filter(|s| {
            s.scene_id == draft.scene_id
                && s.ready
                && (draft.manifest.terrains.is_some() || s.authoring_height_revision() == Some(session.terrain_revision))
        })
    else {
        return;
    };
    let frozen_click = (!state.stroke.as_ref().unwrap().first_click_done)
        .then(|| ground.take_volume_click(snapshot, &draft.scene_id, tool, session.revision))
        .flatten();
    if frozen_click.is_some() {
        state.stroke.as_mut().unwrap().first_click_done = true;
    }
    let point = if draft.manifest.terrains.is_some() && allowed && !state.stroke.as_ref().unwrap().released {
        super::super::terrain_documents::paint_point(&session, &editor, editor.brush_cursor_world)
    } else if let Some(ClipmapGroundHit::Hit(p)) = frozen_click {
        Some(p)
    } else if allowed
        && !state.stroke.as_ref().unwrap().released
        && viewport
            .single()
            .is_ok_and(RelativeCursorPosition::cursor_over)
    {
        ground
            .accepted_paint_hit(
                snapshot,
                &draft.scene_id,
                &field_selection
                    .as_ref()
                    .map(|s| s.2.clone())
                    .unwrap_or(GroundCursorTarget::VolumeSculpt(tool)),
                session.terrain_revision,
            )
            .filter(|p| editor.brush_cursor_world == Some(*p))
    } else {
        None
    };
    if let Some(mut point) = point {
        let stroke = state.stroke.as_mut().unwrap();
        lock_flatten_height(stroke, &mut point);
        let from = stroke.last.unwrap_or(point);
        if let Some((channel, id, _)) = field_selection
            && let Some((ray_origin, ray_direction)) = field_ray
        {
            let erase =
                keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
            let (strength, flow_exposure) = if channel
                == feathrise_terrain::volume::sparse::paged::VolumeFieldChannel::Vegetation
            {
                let (strength, flow) = super::super::paint::vegetation_paint_exposure(
                    editor.brush_strength,
                    time.delta_secs(),
                    stroke.last.is_none() && stroke.tail.is_none(),
                );
                // 单飞任务忙时仍累计未受理曝光；只保留最新落点，但不能丢掉等待期间的绘制强度。
                let pending = stroke
                    .field
                    .as_ref()
                    .filter(|(field, _, _)| {
                        stroke.tail.is_some()
                            && field.channel == channel
                            && field.id == id
                            && field.erase == erase
                    })
                    .map(|(field, _, _)| (field.strength, field.flow_exposure));
                pending.map_or((strength, flow), |(stamp, old_flow)| {
                    (stamp, old_flow + flow)
                })
            } else {
                (
                    (1.0 - (-editor.brush_strength * time.delta_secs().min(0.1) * 12.0).exp())
                        .clamp(0.0, 1.0),
                    0.0,
                )
            };
            stroke.field = Some((
                feathrise_terrain::volume::sparse::paged::VolumeFieldBrush {
                    channel,
                    id,
                    from: from.to_array(),
                    to: point.to_array(),
                    normal: Vec3::Y.to_array(),
                    radius: editor.brush_radius.clamp(0.1, 8.0),
                    strength,
                    flow_exposure,
                    erase,
                    source_to_world: draft.terrain_documents.active.as_ref().map_or(Mat3::IDENTITY, |active|
                        Mat3::from_mat4(active.definition.transform.as_transform().to_matrix())),
                },
                ray_origin,
                ray_direction,
            ));
        }
        stroke.tail = Some((
            point,
            VolumeBrush {
                kind: kind(tool).unwrap_or(VolumeBrushKind::Dig),
                from: from.to_array(),
                to: point.to_array(),
                radius: editor.brush_radius,
                strength: editor.brush_strength,
                shape: stroke.shape,
                half_height: stroke.half_height,
                plane_height: stroke.flatten_height.unwrap_or(point.y),
                source_to_world: draft.terrain_documents.active.as_ref().map_or(Mat3::IDENTITY, |active|
                    Mat3::from_mat4(active.definition.transform.as_transform().to_matrix())),
                frame: None,
            },
        ));
        stroke.first_click_done = true; // 持续命中已经受理时，不再重复消费同一按下点击。
    }
    if state.work.is_some() || state.adoption.is_some() || state.release.is_some() {
        return;
    }
    if stream.native_busy {
        return;
    } // 共用IO/计算槽：运行时任务真正结束后才开始作者段。
    if !state.index.ready {
        editor.status = "正在按固定预算建立三维地形区域索引…".into();
        return;
    }
    if let Some(error) = state.index.error.as_ref() {
        editor.status = error.clone();
        state.spatial_transaction = None;
        let stroke = state.stroke.as_mut().unwrap();
        stroke.tail = None;
        stroke.seed = None;
        stroke.first_click_done = true;
        return;
    }
    if let Err(error) = start_segment(
        &mut state,
        &mut session,
        snapshot,
        stream.physics_focus,
        stream.display_lods.clone(),
    ) {
        editor.status = error;
        state.spatial_transaction = None;
        if let Some(stroke) = state.stroke.as_mut() {
            stroke.tail = None;
            stroke.seed = None;
            stroke.last = None;
        }
    }
}

fn spatial_stroke(
    request: crate::viewport::spatial_brush::SpatialBrushRequest,
    tool: EditorTool,
    revision: u64,
    started: f64,
) -> Stroke {
    let brush = request.brush;
    Stroke {
        spatial: Some(request),
        shape: brush.shape,
        half_height: brush.half_height,
        lock_height: false,
        flatten_height: (tool == EditorTool::VolumeFlatten).then_some(brush.plane_height),
        field: None,
        tool,
        revision,
        before: None,
        index: 0,
        released: true,
        last: None,
        tail: Some((Vec3::from_array(brush.to), brush)),
        first_click_done: true,
        seed: None,
        new_region: false,
        history_slot: None,
        started,
    }
}

fn start_segment(
    state: &mut VolumeAuthoringState,
    session: &mut EditorAuthoringSession,
    ground: &ClipmapGroundSnapshot,
    physics: Option<Vec3>,
    lods: std::sync::Arc<[feathrise_terrain::volume::sparse::paged::VolumeDisplayLod]>,
) -> Result<(), String> {
    if state.stroke.as_ref().is_some_and(|s| s.spatial.is_some()) {
        return super::spatial::start_or_advance(state, session, ground, physics, lods);
    }
    let Some(stroke) = state.stroke.as_ref() else {
        return Ok(());
    };
    let draft = session.draft.as_ref().ok_or("场景草稿尚未加载")?;
    let map = draft.scene_id.clone();
    let terrain_id = draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone());
    let folder = draft.terrain_folder().to_owned();
    let bounds = Rect::from_corners(
        Vec2::from_array(draft.terrain_manifest().world_min),
        Vec2::from_array(draft.terrain_manifest().world_max),
    );
    // 缺页重试保留首次区域和笔刷，不因鼠标移动悄悄更换基础高度源。
    if let Some((record, step, brush)) = stroke.seed.clone() {
        let hole = record.terrain_cutout(bounds)?.ok_or("缺少自动替换区域")?;
        if !ground
            .request_authoritative_rect(Vec2::from_array(hole.min), Vec2::from_array(hole.max))?
        {
            return Ok(());
        }
        let index = stroke.index;
        state.reserve_source()?;
        let snapshot = ground.clone();
        let budget = state.budget.clone();
        let origin = Vec3::from_array(record.translation);
        state.work = Some(Work {
            task: IoTaskPool::get().spawn(async move {
                super::streamed::initial(
                    snapshot, origin, step, record, brush, folder, bounds, budget, physics, lods,
                )
                .await
            }),
            scene_id: map,
            revision: session.revision,
            source: None,
            index,
            history: None,
            ground: Some(ground.content_revision_words()),
            opening: false,
            spatial: None,
        });
        return Ok(());
    }
    let Some((point, mut brush)) = stroke.tail else {
        return Ok(());
    };
    let found = choose_layer(&state.index, point.xz(), bounds).cloned();
    if let Some((index, record, _)) = found {
        if !record
            .terrain_patch
            .as_ref()
            .is_some_and(|p| p.sculpted_volume)
        {
            return Err("该区域为外部GLB地形；三维雕刻原型不能覆盖其作者源".into());
        }
        if state
            .unsaved_region
            .as_ref()
            .is_some_and(|id| *id != record.id)
        {
            return Err("请先保存当前三维区域，再切换另一块区域".into());
        }
        if state.current.as_ref().is_none_or(|s| s.record != record) {
            if !record
                .asset_path
                .starts_with(&format!("{folder}/volume_sculpt/"))
            {
                return Err("三维雕刻作者源不属于当前场景私有volume_sculpt目录".into());
            }
            state.reserve_source()?;
            state.current_index = index;
            let opening_record = record.clone();
            let budget = state.budget.clone();
            state.work = Some(Work {
                task: IoTaskPool::get().spawn(jobs::open(opening_record, budget)),
                scene_id: map,
                revision: session.revision,
                source: None,
                index,
                history: None,
                ground: None,
                opening: true,
                spatial: None,
            });
            return Ok(());
        }
        let current = state.current.as_ref().unwrap().clone();
        state.reserve_source()?;
        let origin = Vec3::from_array(record.translation);
        brush = brush.translated(-origin);
        if let Some((mut field, ray_origin, ray_direction)) =
            state.stroke.as_ref().and_then(|s| s.field.clone())
        {
            field.from = (Vec3::from_array(field.from) - origin).to_array();
            field.to = (point - origin).to_array();
            let stroke = state.stroke.as_mut().unwrap();
            if stroke.before.is_none() {
                stroke.before = Some(current.clone());
                stroke.index = index;
            }
            stroke.tail = None;
            stroke.last = Some(point);
            let snapshot = ground.clone();
            let source = Some(current.source.clone());
            state.work = Some(Work {
                task: IoTaskPool::get().spawn(super::streamed::paint(
                    current,
                    field,
                    (ray_origin, ray_direction),
                    snapshot,
                    folder,
                    physics,
                    lods,
                )),
                scene_id: map,
                revision: session.revision,
                source,
                index,
                history: None,
                ground: Some(ground.content_revision_words()),
                opening: false,
                spatial: None,
            });
            return Ok(());
        }
        let step = match current.source.as_ref() {
            VolumeSource::Dense(data) => data.cell_size,
            VolumeSource::Sparse(data) => data.cell_size(),
            VolumeSource::Paged(data) => data.cell_size,
        };
        // 新列必须先有完整作者高度页；等待期间不消费尾段，也不改变本笔起点。
        let extent = SPARSE_CHUNK_CELLS as f32 * step;
        let halo = (brush.sample_half_extents(step) + Vec3::splat(step)).xz();
        let from = Vec3::from_array(brush.from).xz();
        let to = Vec3::from_array(brush.to).xz();
        let min = ((from.min(to) - halo) / extent).floor() * extent + origin.xz();
        let max = (((from.max(to) + halo) / extent).floor() + Vec2::ONE) * extent + origin.xz();
        if min.cmplt(bounds.min).any() || max.cmpgt(bounds.max).any() {
            return Err("三维笔刷已经到达场景边界，请向场景内移动落点".into());
        }
        if !ground.request_authoritative_rect(min, max)? {
            return Ok(());
        }
        let mut forbidden = Vec::new();
        for (_, other, _) in &state.index.records {
            if other.id != record.id {
                forbidden.extend(other.terrain_cutouts(bounds)?);
            }
        }
        let snapshot = ground.clone();
        let budget = state.budget.clone();
        let stroke = state.stroke.as_mut().unwrap();
        if stroke.before.is_none() && !stroke.new_region {
            stroke.before = Some(current.clone());
            stroke.index = index;
        }
        stroke.tail = None;
        stroke.last = Some(point);
        let source = Some(current.source.clone());
        state.work = Some(Work {
            task: IoTaskPool::get().spawn(async move {
                super::streamed::edit(
                    current, brush, folder, snapshot, bounds, budget, forbidden, physics, lods,
                )
                .await
            }),
            scene_id: map,
            revision: session.revision,
            source,
            index,
            history: None,
            ground: Some(ground.content_revision_words()),
            opening: false,
            spatial: None,
        });
        return Ok(());
    }
    if state.unsaved_region.is_some() {
        return Err("请先保存当前三维区域；这次落笔已经越出它的范围".into());
    }
    if state.stroke.as_ref().is_some_and(|s| s.field.is_some()) {
        return Err("请在已经雕刻的三维表面涂绘".into());
    }
    if state.index.records.len() >= feathrise_scene_content::MAX_TERRAIN_CUTOUTS {
        return Err("三维地形已达到32个区域预算".into());
    }
    let (origin, step, hole) = jobs::brush_region(ground, brush, bounds)?;
    if state.index.records.iter().any(|(_, _, other)| {
        other.is_some_and(|h| h.intersects(Vec2::from_array(hole.min), Vec2::from_array(hole.max)))
    }) {
        return Err("新的三维区域会与既有地形重叠；请在已有三维表面继续雕刻".into());
    }
    let index = draft.placements.placements.len();
    if index >= feathrise_scene_content::MAX_SCENE_PLACEMENTS_PER_SCENE {
        return Err("场景摆放记录已经达到预算".into());
    }
    let id = super::super::next_author_id(session, "volume");
    let local_min = Vec2::from_array(hole.min) - origin.xz();
    let local_max = Vec2::from_array(hole.max) - origin.xz();
    let record: ScenePlacementRecord = serde_json::from_value(serde_json::json!({
        "id": id, "asset_path": format!("{folder}/volume_sculpt/pending.glb"), "translation": origin.to_array(), "yaw_degrees":0, "scale":[1,1,1],
        "terrain_patch":{"terrain_id":terrain_id,"sculpted_volume":true,"cutouts":[{"min":local_min.to_array(),"max":local_max.to_array()}]}
    })).map_err(|e| e.to_string())?;
    brush.from = point.to_array();
    brush.to = brush.from;
    brush = brush.translated(-origin);
    let stroke = state.stroke.as_mut().unwrap();
    stroke.index = index;
    stroke.new_region = true;
    stroke.seed = Some((record, step, brush));
    stroke.last = Some(point);
    stroke.tail = None;
    // 首次区域需求先提交；下一帧仍由同一个种子决定能否启动后台。
    ground.request_authoritative_rect(Vec2::from_array(hole.min), Vec2::from_array(hole.max))?;
    Ok(())
}

/// 首先命中真实覆盖矩形；未覆盖位置沿用同一体积层，断开的列之间仍保留Clipmap地表。
pub(super) fn choose_layer(
    index: &RecordIndex,
    point: Vec2,
    bounds: Rect,
) -> Option<&(
    usize,
    ScenePlacementRecord,
    Option<feathrise_scene_content::SceneTerrainCutout>,
)> {
    index
        .records
        .iter()
        .find(|(_, record, _)| {
            record
                .terrain_cutouts(bounds)
                .is_ok_and(|holes| holes.iter().any(|h| h.contains(point)))
        })
        .or_else(|| {
            index.records.iter().find(|(_, record, _)| {
                record
                    .terrain_patch
                    .as_ref()
                    .is_some_and(|p| p.sculpted_volume && !p.cutouts.is_empty())
            })
        })
        .or_else(|| {
            index.records.iter().find(|(_, record, _)| {
                record
                    .terrain_patch
                    .as_ref()
                    .is_some_and(|p| p.sculpted_volume)
            })
        })
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
