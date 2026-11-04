//! 空间雕刻按局部窗口推进私有版本；最后一批完成才交给地形/碰撞联合采用门。
use super::{input, jobs, lifecycle::map_bounds, streamed, types::*};
use crate::{
    authoring::EditorAuthoringSession, resources::MapEditorState,
    viewport::spatial_brush::SpatialBrushRequest,
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool},
};
use feathrise_map_content::MapPlacementRecord;
use feathrise_map_terrain::volume::{
    VolumeBrushBatches,
    sparse::{VolumeSource, paged::PagedVolume},
};
use feathrise_scene::ClipmapGroundSnapshot;
use std::sync::Arc;

/// 取消后仍保留正在运行的任务信用，但迟到的私有版本和最终候选均不能复活本笔。
pub(super) fn request_active(
    state: &VolumeAuthoringState,
    request: &SpatialBrushRequest,
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    mode: Option<&crate::viewport::modes::ViewportModeState>,
    spatial: Option<&crate::viewport::spatial_brush::SpatialBrushState>,
) -> bool {
    request.matches(editor, session)
        && !session.saving
        && mode.is_none_or(|m| m.mode == crate::viewport::modes::ViewportMode::Edit)
        && spatial.is_none_or(|s| s.enabled())
        && state
            .stroke
            .as_ref()
            .is_some_and(|s| s.revision == session.revision && s.spatial.is_some())
}

pub(super) fn start_or_advance(
    state: &mut VolumeAuthoringState,
    session: &mut EditorAuthoringSession,
    ground: &ClipmapGroundSnapshot,
    physics: Option<Vec3>,
    lods: Arc<[feathrise_map_terrain::volume::sparse::paged::VolumeDisplayLod]>,
) -> Result<(), String> {
    if state.spatial_transaction.is_none() && !begin(state, session, ground)? {
        return Ok(()); // 首次重开作者源也占用同一个单飞槽，原始起终点不变。
    }
    let bounds = map_bounds(session).ok_or("空间雕刻地图草稿已失效")?;
    let transaction = state.spatial_transaction.as_mut().unwrap();
    if ground.content_revision_words() != transaction.ground {
        return Err("空间雕刻基础地形已换代，整条路径未采用".into());
    }
    if transaction.batch.is_none() {
        if transaction.batches.is_path() {
            // 路径姿态和批次包围留在同一个后台槽，不在主帧扫描局部刷印。
            let mut transaction = state.spatial_transaction.take().unwrap();
            let request = transaction.request.clone();
            let words = transaction.ground;
            state.work = Some(Work {
                task: AsyncComputeTaskPool::get().spawn(async move {
                    transaction.batch = transaction.batches.next_batch()?;
                    Ok(JobOutput::Staged(Box::new(transaction)))
                }),
                map: session.draft.as_ref().unwrap().map_id.clone(),
                revision: session.revision,
                source: state.current.as_ref().map(|s| s.source.clone()),
                index: state.stroke.as_ref().unwrap().index,
                history: None,
                ground: Some(words),
                opening: false,
                spatial: Some(request),
            });
            return Ok(());
        }
        transaction.batch = transaction.batches.next_batch()?;
    }
    let batch = transaction.batch.clone().ok_or("空间雕刻缺少待处理批次")?;
    let origin = Vec3::from_array(transaction.snapshot.record.translation);
    let step = cell_size(&transaction.snapshot);
    let extent = 32. * step;
    let (low, high) = batch.bounds();
    let min = ((low.xz() - Vec2::splat(step)) / extent).floor() * extent + origin.xz();
    let max =
        (((high.xz() + Vec2::splat(step)) / extent).floor() + Vec2::ONE) * extent + origin.xz();
    if min.cmplt(bounds.min).any() || max.cmpgt(bounds.max).any() {
        return Err("空间雕刻局部作者块越出地图边界，整条路径未采用".into());
    }
    // 【需求设计】高度缓存只请求当前批次。快照不可变，缺页后下一帧使用同内容身份的新快照。
    // 不能预取整条长路径包围盒，也不能让后台等待旧快照里永远不会补齐的高度页。
    if !ground.request_authoritative_rect(min, max)? {
        return Ok(());
    }
    let mut forbidden = Vec::new();
    for (_, other, _) in &state.index.records {
        if other.id != transaction.snapshot.record.id {
            forbidden.extend(other.terrain_cutouts(bounds)?);
        }
    }
    let draft = session.draft.as_ref().unwrap();
    let map = draft.map_id.clone();
    let folder = draft.terrain_folder().to_owned();
    let index = state.stroke.as_ref().unwrap().index;
    let source = state.current.as_ref().map(|s| s.source.clone());
    let mut transaction = state.spatial_transaction.take().unwrap();
    transaction.batch = None;
    let request = transaction.request.clone();
    let words = transaction.ground;
    let ground = ground.clone();
    let budget = state.budget.clone();
    state.work = Some(Work {
        task: IoTaskPool::get().spawn(async move {
            let last_center = batch.last_center().to_array();
            if let Some(next) = streamed::stage_batch(
                &transaction.snapshot,
                batch,
                &folder,
                ground,
                bounds,
                budget,
                forbidden,
            )
            .await?
            {
                transaction.snapshot.source = Arc::new(VolumeSource::Paged(next));
                transaction.changed = true;
            }
            if !transaction.batches.finished() {
                if transaction.batches.is_path() {
                    transaction = AsyncComputeTaskPool::get()
                        .spawn(async move {
                            transaction.batch = transaction.batches.next_batch()?;
                            Ok::<_, String>(transaction)
                        })
                        .await?;
                }
                return Ok(JobOutput::Staged(Box::new(transaction)));
            }
            if !transaction.changed {
                return Ok(JobOutput::Unchanged { at_boundary: false });
            }
            let VolumeSource::Paged(next) = transaction.snapshot.source.as_ref() else {
                return Err("空间雕刻未生成完整流送版本".into());
            };
            let mut next = next.clone();
            next.focus = last_center; // 最后一批无改动时，显示焦点仍落在整条路径的终点。
            streamed::finish(transaction.snapshot.record, next, &folder, physics, lods).await
        }),
        map,
        revision: session.revision,
        source,
        index,
        history: None,
        ground: Some(words),
        opening: false,
        spatial: Some(request),
    });
    Ok(())
}

/// 返回false表示正在后台重开已有源；初始化仅建立少量元数据，完全不改作者记录。
fn begin(
    state: &mut VolumeAuthoringState,
    session: &mut EditorAuthoringSession,
    ground: &ClipmapGroundSnapshot,
) -> Result<bool, String> {
    let request = state
        .stroke
        .as_ref()
        .and_then(|s| s.spatial.clone())
        .ok_or("空间雕刻请求已取消")?;
    let brush = request.brush.in_source_frame(session.draft.as_ref()
        .and_then(|draft| draft.terrain_documents.active.as_ref())
        .map_or(Mat4::IDENTITY, |active| active.definition.transform.as_transform().to_matrix()));
    let bounds = map_bounds(session).ok_or("地图草稿尚未加载")?;
    let draft = session.draft.as_ref().unwrap();
    let folder = draft.terrain_folder().to_owned();
    let terrain_id = draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone());
    let found = input::choose_layer(&state.index, Vec3::from_array(brush.to).xz(), bounds).cloned();
    let snapshot = if let Some((index, record, _)) = found {
        if !record
            .terrain_patch
            .as_ref()
            .is_some_and(|p| p.sculpted_volume)
        {
            return Err("该区域为外部GLB地形；空间雕刻不能覆盖其作者源".into());
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
                return Err("三维雕刻作者源不属于当前地图私有volume_sculpt目录".into());
            }
            state.reserve_source()?;
            let budget = state.budget.clone();
            state.work = Some(Work {
                task: IoTaskPool::get().spawn(jobs::open(record, budget)),
                map: draft.map_id.clone(),
                revision: session.revision,
                source: None,
                index,
                history: None,
                ground: Some(ground.content_revision_words()),
                opening: true,
                spatial: Some(request),
            });
            return Ok(false);
        }
        state.reserve_source()?;
        let snapshot = state.current.as_ref().unwrap().clone();
        let stroke = state.stroke.as_mut().unwrap();
        stroke.before = Some(snapshot.clone());
        stroke.index = index;
        snapshot
    } else {
        if state.unsaved_region.is_some() {
            return Err("请先保存当前三维区域，再创建新区域".into());
        }
        if state.index.records.len() >= feathrise_map_content::MAX_TERRAIN_CUTOUTS {
            return Err("三维地形已达到32个区域预算".into());
        }
        let index = draft.placements.placements.len();
        if index >= feathrise_map_content::MAX_MAP_PLACEMENTS_PER_MAP {
            return Err("地图摆放记录已经达到预算".into());
        }
        let (origin, step, hole) = jobs::brush_region(ground, brush, bounds)?;
        let id = super::super::next_author_id(session, "volume");
        let local_min = Vec2::from_array(hole.min) - origin.xz();
        let local_max = Vec2::from_array(hole.max) - origin.xz();
        let record: MapPlacementRecord = serde_json::from_value(serde_json::json!({
            "id": id, "asset_path": format!("{folder}/volume_sculpt/pending.glb"),
            "translation": origin.to_array(), "yaw_degrees": 0, "scale": [1,1,1],
            "terrain_patch": {"terrain_id":terrain_id, "sculpted_volume": true, "cutouts": [{"min": local_min.to_array(), "max": local_max.to_array()}]}
        })).map_err(|e| e.to_string())?;
        let stroke = state.stroke.as_mut().unwrap();
        stroke.index = index;
        stroke.new_region = true;
        VolumeSnapshot {
            record,
            source: Arc::new(VolumeSource::Paged(PagedVolume::new(step)?)),
        }
    };
    let origin = Vec3::from_array(snapshot.record.translation);
    let batches = if let Some(path) = &request.path {
        SpatialBatches::Path(path.batches(
            brush.kind,
            brush.strength,
            cell_size(&snapshot),
            -origin,
        )?)
    } else {
        VolumeBrushBatches::new(brush.translated(-origin), cell_size(&snapshot))?.into()
    };
    state.spatial_transaction = Some(SpatialSculptTransaction {
        request,
        snapshot,
        batches,
        batch: None,
        ground: ground.content_revision_words(),
        changed: false,
    });
    let stroke = state.stroke.as_mut().unwrap();
    stroke.tail = None;
    stroke.seed = None;
    stroke.last = Some(Vec3::from_array(brush.to));
    Ok(true)
}

fn cell_size(snapshot: &VolumeSnapshot) -> f32 {
    match snapshot.source.as_ref() {
        VolumeSource::Dense(v) => v.cell_size,
        VolumeSource::Sparse(v) => v.cell_size(),
        VolumeSource::Paged(v) => v.cell_size,
    }
}
