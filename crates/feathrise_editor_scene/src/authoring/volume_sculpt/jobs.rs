//! 完整密度物化、网格及编码都留在单worker；主帧只移动拥有输出。
use super::types::*;
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    tasks::AsyncComputeTaskPool,
};
use feathrise_scene_content::{ScenePlacementRecord, SceneTerrainCutout};
use feathrise_terrain::volume::VolumeBrush;
use feathrise_terrain::volume::sparse::*;
#[cfg(test)]
use feathrise_terrain::volume::*;
use feathrise_terrain::{derive_vertex_normals, parse_dynamic_terrain_glb};
use feathrise_scene::ClipmapGroundSnapshot;
use sha2::{Digest, Sha256};
use std::{io::Read, sync::Arc};

/// 起终点联合范围只生成作者区域元数据；长路径的基础高度由各批次局部请求。
pub(super) fn brush_region(
    ground: &ClipmapGroundSnapshot,
    brush: VolumeBrush,
    bounds: Rect,
) -> Result<(Vec3, f32, SceneTerrainCutout), String> {
    region_span(
        ground,
        Vec3::from_array(brush.from).xz(),
        Vec3::from_array(brush.to).xz(),
        brush.sample_half_extents(0.).xz().max_element(),
        bounds,
    )
}

fn region_span(
    ground: &ClipmapGroundSnapshot,
    from: Vec2,
    to: Vec2,
    radius: f32,
    bounds: Rect,
) -> Result<(Vec3, f32, SceneTerrainCutout), String> {
    let (origin, _) = ground
        .authoritative_grid()
        .ok_or("当前场景没有同代基础作者格点")?;
    aligned_region(origin, from, to, radius, bounds)
}

fn aligned_region(
    origin: Vec2,
    from: Vec2,
    to: Vec2,
    radius: f32,
    bounds: Rect,
) -> Result<(Vec3, f32, SceneTerrainCutout), String> {
    let step = SPARSE_CELL_SIZE;
    if !(0.01..=0.5).contains(&step) {
        return Err("三维雕刻格距超出原型预算".into());
    }
    let extent = SPARSE_CHUNK_CELLS as f32 * step;
    let domain = radius + (3.0_f32.sqrt() + 1.0) * step;
    let min = origin + ((from.min(to) - origin - Vec2::splat(domain)) / extent).floor() * extent;
    let max = origin
        + (((from.max(to) - origin + Vec2::splat(domain)) / extent).floor() + Vec2::ONE) * extent;
    let hole = SceneTerrainCutout {
        min: min.to_array(),
        max: max.to_array(),
    };
    if min.cmplt(bounds.min).any() || Vec2::from_array(hole.max).cmpgt(bounds.max).any() {
        return Err(format!("三维区域边长{extent:.1}米，当前位置太靠近场景边界"));
    }
    Ok((Vec3::new(origin.x, 0., origin.y), step, hole))
}

#[cfg(test)]
mod region_tests {
    use super::*;
    #[test]
    fn spatial_first_region_covers_both_endpoints_and_converts_the_whole_segment() {
        let grid = Vec2::new(-256., -128.);
        let bounds = Rect::from_corners(grid, Vec2::splat(256.));
        let from = Vec3::new(-18., -6., 10.);
        let to = Vec3::new(18., -8., 10.);
        let (origin, _, hole) = aligned_region(grid, from.xz(), to.xz(), 1., bounds).unwrap();
        assert!(hole.min[0] < from.x - 1. && hole.max[0] > to.x + 1.);
        let brush = VolumeBrush {
            from: from.to_array(),
            to: to.to_array(),
            plane_height: from.y,
            ..default()
        }
        .translated(-origin);
        assert_eq!(Vec3::from_array(brush.from) + origin, from);
        assert_eq!(Vec3::from_array(brush.to) + origin, to);
        assert_eq!(brush.plane_height + origin.y, from.y);
        assert!(aligned_region(grid, from.xz(), Vec2::splat(300.), 1., bounds).is_err());
    }
}

#[cfg(test)]
pub(super) fn edit(
    snapshot: VolumeSnapshot,
    brush: VolumeBrush,
    folder: String,
) -> Result<JobOutput, String> {
    let VolumeSource::Dense(dense) = snapshot.source.as_ref() else {
        return Err("分块三维编辑必须带当前作者高度快照".into());
    };
    let source = VolumeSource::Dense(apply_volume_brush(dense, brush)?);
    if source == *snapshot.source {
        return Ok(JobOutput::Unchanged {
            at_boundary: brush.is_clipped_by_boundary(dense),
        });
    }
    prepare(snapshot.record, Arc::new(source), &folder)
}

/// 同一层的共享源变更、覆盖范围与GLB派生使用一个候选，任一检查失败都不写作者记录。
#[cfg(test)]
pub(super) fn edit_layer(
    snapshot: VolumeSnapshot,
    brush: VolumeBrush,
    folder: String,
    local_bounds: Rect,
    budget: Arc<VolumeMemoryBudget>,
    forbidden: Vec<SceneTerrainCutout>,
    height: impl FnMut(Vec2) -> Result<f32, String>,
) -> Result<JobOutput, String> {
    let origin = Vec3::from_array(snapshot.record.translation);
    let data = match snapshot.source.as_ref() {
        VolumeSource::Dense(data) => migrate_dense_volume(data, budget)?,
        VolumeSource::Sparse(data) => data.clone(),
        VolumeSource::Paged(_) => return Err("流送作者层须使用IO/计算分阶段任务".into()),
    };
    let next = apply_sparse_brush(&data, brush, local_bounds, height)?;
    if next.footprint().iter().any(|h| {
        forbidden.iter().any(|f| {
            f.intersects(
                Vec2::from_array(h[0]) + origin.xz(),
                Vec2::from_array(h[1]) + origin.xz(),
            )
        })
    }) {
        return Err("三维扩展区域与另一份旧作者源或外部GLB重叠；未修改任何原有地形".into());
    }
    let source = VolumeSource::Sparse(next);
    if source == *snapshot.source {
        return Ok(JobOutput::Unchanged { at_boundary: false });
    }
    prepare(snapshot.record, Arc::new(source), &folder)
}
pub(super) fn restore(snapshot: VolumeSnapshot, folder: String) -> Result<JobOutput, String> {
    prepare(snapshot.record, snapshot.source, &folder) // 恢复直接共享历史源，满额度仍能撤销，不再复制旧密度数组。
}
fn prepare(
    mut record: ScenePlacementRecord,
    source: Arc<VolumeSource>,
    folder: &str,
) -> Result<JobOutput, String> {
    let input = build_author_volume_mesh(&source)?;
    let bytes = encode_author_volume_glb(&source, &input)?;
    if source.is_sparse() {
        let patch = record
            .terrain_patch
            .as_mut()
            .ok_or("分块作者层缺少地形身份")?;
        patch.cutout = None;
        patch.cutouts = source
            .footprint()
            .into_iter()
            .map(|h| SceneTerrainCutout {
                min: h[0],
                max: h[1],
            })
            .collect();
    }
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    record.asset_path = format!("{folder}/volume_sculpt/v-{hash}.glb");
    let normals = derive_vertex_normals(&input)?;
    let uv: Vec<[f32; 2]> = input.positions.iter().map(|p| [p[0], p[2]]).collect();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, input.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(input.indices));
    Ok(JobOutput::Prepared(Box::new(PreparedVolume {
        render_record: record.clone(),
        resident_window: None,
        snapshot: VolumeSnapshot { record, source },
        bytes: Arc::new(bytes),
        meshes: vec![mesh],
        fields: default(),
    })))
}
pub(super) async fn open(
    record: ScenePlacementRecord,
    budget: Arc<VolumeMemoryBudget>,
) -> Result<JobOutput, String> {
    let bytes = read_at(&super::super::files::workspace_asset_root(), &record)?;
    // IO槽只负责有界文件读取，密度解码及等值面校验交计算池；await不会阻塞IO worker。
    // 作者域仍持有同一个Work票据，计算真正完成前不能开启下一笔或归还取消槽。
    AsyncComputeTaskPool::get()
        .spawn(async move { decode(record, bytes, budget) })
        .await
}
#[cfg(test)]
pub(super) fn open_at(
    root: &std::path::Path,
    record: ScenePlacementRecord,
) -> Result<JobOutput, String> {
    let bytes = read_at(root, &record)?;
    decode(record, bytes, Arc::default())
}
fn read_at(root: &std::path::Path, record: &ScenePlacementRecord) -> Result<Vec<u8>, String> {
    let path = super::super::files::safe_asset_path(root, &record.asset_path)?;
    let file = std::fs::File::open(&path).map_err(|e| format!("读取三维雕刻源失败：{e}"))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_AUTHOR_VOLUME_GLB_BYTES as u64 {
        return Err("三维雕刻GLB超过32MiB预算".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_AUTHOR_VOLUME_GLB_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_AUTHOR_VOLUME_GLB_BYTES {
        return Err("三维雕刻GLB读取期间超过32MiB预算".into());
    }
    Ok(bytes)
}
fn decode(
    record: ScenePlacementRecord,
    bytes: Vec<u8>,
    budget: Arc<VolumeMemoryBudget>,
) -> Result<JobOutput, String> {
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if std::path::Path::new(&record.asset_path)
        .file_name()
        .and_then(|n| n.to_str())
        != Some(format!("v-{hash}.glb").as_str())
    {
        return Err("三维雕刻文件与摘要路径不一致，不能用缓存源覆盖外部改动".into());
    }
    let source = decode_author_volume_glb(&bytes, budget)?;
    // 外部改网格而未改密度不能被下一笔悄悄覆盖；重开必须证明缓存和作者源同代。
    if !matches!(source, VolumeSource::Paged(_))
        && build_author_volume_mesh(&source)? != parse_dynamic_terrain_glb(&bytes)?
    {
        return Err("三维雕刻源与GLB网格不一致；请恢复原生成文件".into());
    }
    let patch = record
        .terrain_patch
        .as_ref()
        .ok_or("三维源缺少地表覆盖声明")?;
    if patch.streamed_volume != matches!(source, VolumeSource::Paged(_)) {
        return Err("体积版本与流送声明不一致".into());
    }
    if let VolumeSource::Paged(v) = &source
        && patch.volume_cell_size != Some(v.cell_size)
    {
        return Err("流送版本格距与作者声明不一致".into());
    }
    let actual: Vec<_> = patch
        .cutout
        .iter()
        .chain(&patch.cutouts)
        .map(|h| [h.min, h.max])
        .collect();
    if actual != source.footprint() {
        return Err("三维雕刻源尺寸与自动替换区域不一致".into());
    }
    Ok(JobOutput::Opened(VolumeSnapshot {
        record,
        source: Arc::new(source),
    }))
}
