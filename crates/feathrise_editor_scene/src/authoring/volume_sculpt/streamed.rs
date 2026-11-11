//! v3笔刷分阶段读取、计算和不可变落盘；帧系统只保留固定大小版本与任务句柄。
use super::types::*;
use bevy::{
    math::{Rect, Vec2, Vec3},
    prelude::Vec3Swizzles,
    tasks::AsyncComputeTaskPool,
};
use feathrise_scene_content::{ScenePlacementRecord, SceneTerrainCutout};
use feathrise_terrain::volume::{
    VolumeBrush, VolumeBrushBatch,
    sparse::{paged::*, *},
};
use feathrise_scene::{ClipmapGroundHit, ClipmapGroundSnapshot};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};

pub(super) async fn initial(
    ground: ClipmapGroundSnapshot,
    origin: Vec3,
    step: f32,
    mut record: ScenePlacementRecord,
    brush: VolumeBrush,
    folder: String,
    bounds: Rect,
    budget: Arc<VolumeMemoryBudget>,
    physics: Option<Vec3>,
    lods: Arc<[VolumeDisplayLod]>,
) -> Result<JobOutput, String> {
    record.translation = origin.to_array();
    edit(
        VolumeSnapshot {
            record,
            source: Arc::new(VolumeSource::Paged(PagedVolume::new(step)?)),
        },
        brush,
        folder,
        ground,
        bounds,
        budget,
        Vec::new(),
        physics,
        lods,
    )
    .await
}
pub(super) async fn edit(
    snapshot: VolumeSnapshot,
    brush: VolumeBrush,
    folder: String,
    ground: ClipmapGroundSnapshot,
    bounds: Rect,
    budget: Arc<VolumeMemoryBudget>,
    forbidden: Vec<SceneTerrainCutout>,
    physics: Option<Vec3>,
    lods: Arc<[VolumeDisplayLod]>,
) -> Result<JobOutput, String> {
    let step = match snapshot.source.as_ref() {
        VolumeSource::Paged(v) => v.cell_size,
        VolumeSource::Dense(v) => v.cell_size,
        VolumeSource::Sparse(v) => v.cell_size(),
    };
    let batch = VolumeBrushBatch::single(brush, step)?;
    let Some(next) =
        stage_batch(&snapshot, batch, &folder, ground, bounds, budget, forbidden).await?
    else {
        return Ok(JobOutput::Unchanged { at_boundary: false });
    };
    finish(snapshot.record, next, &folder, physics, lods).await
}

/// 只落盘一个不可变私有根，不生成可见候选；长笔划在最后一批后才派生显示与载体。
pub(super) async fn stage_batch(
    snapshot: &VolumeSnapshot,
    batch: VolumeBrushBatch,
    folder: &str,
    ground: ClipmapGroundSnapshot,
    bounds: Rect,
    budget: Arc<VolumeMemoryBudget>,
    forbidden: Vec<SceneTerrainCutout>,
) -> Result<Option<PagedVolume>, String> {
    let root = super::super::files::workspace_asset_root();
    let objects =
        super::super::files::safe_asset_path(&root, &format!("{folder}/volume_sculpt/objects"))?;
    let previous = match snapshot.source.as_ref() {
        VolumeSource::Paged(v) => v.clone(),
        VolumeSource::Dense(v) => PagedVolume::new(v.cell_size)?,
        VolumeSource::Sparse(v) => PagedVolume::new(v.cell_size())?,
    };
    let loaded = match snapshot.source.as_ref() {
        VolumeSource::Paged(v) => {
            Some(load_edit_batch_window(&objects, v, &batch, budget.clone())?)
        }
        _ => None,
    };
    let origin = Vec3::from_array(snapshot.record.translation);
    let local_bounds = Rect::from_corners(bounds.min - origin.xz(), bounds.max - origin.xz());
    let source = snapshot.source.clone();
    let last_center = batch.last_center().to_array();
    let prepared = AsyncComputeTaskPool::get()
        .spawn(async move {
            let before = match source.as_ref() {
                VolumeSource::Paged(_) => loaded.unwrap(),
                VolumeSource::Sparse(v) => v.clone(),
                VolumeSource::Dense(v) => migrate_dense_volume(v, budget)?,
            };
            let next = apply_sparse_brush_batch(&before, batch, local_bounds, |p| {
                match ground.sample_authoritative_surface(p + origin.xz()) {
                    ClipmapGroundHit::Hit(p) => Ok(p.y - origin.y),
                    ClipmapGroundHit::WaitingForHeight => {
                        Err("正在等待体积扩展区域作者高度页".into())
                    }
                    _ => Err("体积扩展区域没有真实作者高度".into()),
                }
            })?;
            if VolumeSource::Sparse(before.clone()) == VolumeSource::Sparse(next.clone()) {
                return Ok(None);
            }
            if next.footprint().iter().any(|h| {
                forbidden.iter().any(|f| {
                    f.intersects(
                        Vec2::from_array(h[0]) + origin.xz(),
                        Vec2::from_array(h[1]) + origin.xz(),
                    )
                })
            }) {
                return Err("体积扩展邻域与其他作者源重叠，未改变原地形".into());
            }
            let delta = prepare_delta(
                matches!(source.as_ref(), VolumeSource::Paged(_)).then_some(&before),
                &next,
            )?;
            Ok::<_, String>(Some(delta))
        })
        .await?;
    let Some(delta) = prepared else {
        return Ok(None);
    };
    let mut next = write_delta(&objects, &previous, delta)?;
    next.focus = last_center;
    Ok(Some(next))
}

pub(super) async fn finish(
    record: ScenePlacementRecord,
    next: PagedVolume,
    folder: &str,
    physics: Option<Vec3>,
    lods: Arc<[VolumeDisplayLod]>,
) -> Result<JobOutput, String> {
    let root = super::super::files::workspace_asset_root();
    let objects =
        super::super::files::safe_asset_path(&root, &format!("{folder}/volume_sculpt/objects"))?;
    prepare(record, next, folder, &objects, &root, physics, lods).await
}

pub(super) async fn restore(
    snapshot: VolumeSnapshot,
    folder: String,
    physics: Option<Vec3>,
    lods: Arc<[VolumeDisplayLod]>,
) -> Result<JobOutput, String> {
    if let VolumeSource::Paged(v) = snapshot.source.as_ref() {
        let root = super::super::files::workspace_asset_root();
        let objects = super::super::files::safe_asset_path(
            &root,
            &format!("{folder}/volume_sculpt/objects"),
        )?;
        return prepare(
            snapshot.record.clone(),
            v.clone(),
            &folder,
            &objects,
            &root,
            physics,
            lods,
        )
        .await;
    }
    AsyncComputeTaskPool::get()
        .spawn(async move { super::jobs::restore(snapshot, folder) })
        .await
}

/// 三维涂绘复用密度编辑的单飞槽、不可变版本和联合采用门，独立字段根不改密度。
pub(super) async fn paint(
    snapshot: VolumeSnapshot,
    mut brush: VolumeFieldBrush,
    ray: (Vec3, Vec3),
    ground: ClipmapGroundSnapshot,
    folder: String,
    physics: Option<Vec3>,
    lods: Arc<[VolumeDisplayLod]>,
) -> Result<JobOutput, String> {
    let root = super::super::files::workspace_asset_root();
    let objects =
        super::super::files::safe_asset_path(&root, &format!("{folder}/volume_sculpt/objects"))?;
    let previous = match snapshot.source.as_ref() {
        VolumeSource::Paged(v) => v.clone(),
        _ => {
            // 旧版本直接迁移，不用额外雕刻动作改变密度或要求补做一次操作。
            let source = snapshot.source.clone();
            let (initial, delta) = AsyncComputeTaskPool::get()
                .spawn(async move {
                    let sparse = match source.as_ref() {
                        VolumeSource::Sparse(v) => v.clone(),
                        VolumeSource::Dense(v) => {
                            migrate_dense_volume(v, Arc::new(VolumeMemoryBudget::default()))?
                        }
                        VolumeSource::Paged(_) => unreachable!(),
                    };
                    Ok::<_, String>((
                        PagedVolume::new(sparse.cell_size())?,
                        prepare_delta(None, &sparse)?,
                    ))
                })
                .await?;
            write_delta(&objects, &initial, delta)?
        }
    };
    let origin = Vec3::from_array(snapshot.record.translation);
    let frozen = ground.clone();
    brush.normal = AsyncComputeTaskPool::get()
        .spawn(async move {
            frozen
                .three_dimensional_hit(ray.0, ray.1, 100000.0)
                .map(|(_, n)| n.to_array())
                .ok_or("三维涂绘射线已失去真实表面".to_owned())
        })
        .await?;
    let window = load_field_window(&objects, &previous, &brush)?;
    let compute_source = previous.clone();
    let delta = AsyncComputeTaskPool::get()
        .spawn(async move {
            prepare_field_paint(window, &compute_source, &brush, |p, n, max| {
                ground
                    .three_dimensional_hit(p + origin, n, max)
                    .map(|(p, n)| (p - origin, n))
            })
        })
        .await?;
    let next = write_field_paint(&objects, &previous, delta)?;
    if next == previous {
        return Ok(JobOutput::Unchanged { at_boundary: false });
    }
    prepare(
        snapshot.record,
        next,
        &folder,
        &objects,
        &root,
        physics,
        lods,
    )
    .await
}
async fn prepare(
    mut record: ScenePlacementRecord,
    source: PagedVolume,
    folder: &str,
    objects: &Path,
    asset_root: &Path,
    physics: Option<Vec3>,
    lods: Arc<[VolumeDisplayLod]>,
) -> Result<JobOutput, String> {
    let extent = source.extent();
    let resident_window = feathrise_scene::StreamedVolumeResident {
        center: (Vec3::from_array(source.focus) / extent)
            .floor()
            .as_ivec3()
            .to_array(),
        lod_signature: feathrise_scene::volume_lod_signature(&lods),
        physics_center: physics.map(|p| {
            ((p - Vec3::from_array(record.translation)) / extent)
                .floor()
                .as_ivec3()
                .to_array()
        }),
    };
    let mut foci = vec![source.focus];
    if let Some(p) = physics {
        let p = (p - Vec3::from_array(record.translation)).to_array();
        if p != source.focus {
            foci.push(p);
        }
    }
    let resident = feathrise_scene::build_volume_display(objects, &source, &foci, lods).await?;
    let meshes = resident.meshes;
    if meshes.is_empty() {
        return Err("当前落笔邻域没有可用体积表面".into());
    }
    let fields = resident.fields;
    let input = feathrise_terrain::TerrainMeshInput {
        positions: Vec::new(),
        indices: Vec::new(),
    };
    let source = Arc::new(VolumeSource::Paged(source));
    let bytes = encode_author_volume_glb(&source, &input)?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    record.asset_path = format!("{folder}/volume_sculpt/v-{hash}.glb");
    let patch = record
        .terrain_patch
        .as_mut()
        .ok_or("体积版本缺少作者声明")?;
    patch.streamed_volume = true;
    if let VolumeSource::Paged(v) = source.as_ref() {
        patch.volume_cell_size = Some(v.cell_size);
    }
    patch.cutout = None;
    patch.cutouts = source
        .footprint()
        .into_iter()
        .map(|h| SceneTerrainCutout {
            min: h[0],
            max: h[1],
        })
        .collect();
    let mut render_record = record.clone();
    render_record.terrain_patch.as_mut().unwrap().cutouts = resident
        .footprint
        .into_iter()
        .map(|h| SceneTerrainCutout {
            min: h[0],
            max: h[1],
        })
        .collect();
    // 先保存不可变载体供未保存草稿的窗口流送；正式placement仍只在原保存事务中发布。
    let path = super::super::files::safe_asset_path(asset_root, &record.asset_path)?;
    if path.exists() {
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() != bytes.len() as u64
            || std::fs::read(&path).map_err(|e| e.to_string())? != bytes
        {
            return Err("体积版本摘要路径已有不同内容".into());
        }
    } else {
        let temp = path.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        let result = file.write_all(&bytes).and_then(|_| file.sync_all());
        drop(file);
        if let Err(e) = result {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("体积版本落盘失败：{e}"));
        }
        let result = std::fs::hard_link(&temp, &path);
        let _ = std::fs::remove_file(temp);
        result.map_err(|e| e.to_string())?;
    }
    Ok(JobOutput::Prepared(Box::new(PreparedVolume {
        snapshot: VolumeSnapshot { record, source },
        render_record,
        resident_window: Some(resident_window),
        bytes: Arc::new(bytes),
        meshes,
        fields,
    })))
}

#[cfg(test)]
#[path = "streamed_tests.rs"]
mod tests;
