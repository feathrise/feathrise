//! GLB/Clipmap源编辑保持原文件不动；不可变版本可随地图清单撤销、重做和原子保存。
use bevy::{prelude::*, tasks::{AsyncComputeTaskPool, IoTaskPool}};
use feathrise_map_content::*;
use std::path::Path;
use super::ObjectBrushInput;
use crate::types::EditorTool;

pub(crate) async fn edit(root: std::path::PathBuf, owner: MapId, mut object: MapTerrainObject,
    input: ObjectBrushInput, version: String) -> Result<MapTerrainObject, String> {
    if !input.center.is_finite() || !input.radius.is_finite() || input.radius <= 0.0
        || !input.strength.is_finite() || !(0.01..=1.0).contains(&input.strength)
        || !input.speed.is_finite() || input.speed < 0.0 || !input.delta_seconds.is_finite()
        || !input.plane_point.is_finite() { return Err("地形笔刷参数无效".into()); }
    let matrix = object.transform.as_transform().to_matrix(); let inverse = matrix.inverse();
    let center = inverse.transform_point3(input.center);
    match object.terrain.kind {
        MapTerrainKind::SparseClipmap => {
            let kind = match input.tool { EditorTool::Raise => SparseTerrainBrushKind::Raise,
                EditorTool::Lower => SparseTerrainBrushKind::Lower, EditorTool::Smooth => SparseTerrainBrushKind::Smooth,
                EditorTool::Flatten => SparseTerrainBrushKind::Flatten, _ => return Err("Clipmap高度工具使用抬升、降低、平滑和整平；洞穴继续使用体积区域工具".into()) };
            let path = object.terrain.height_path.clone().ok_or("地形缺少高度源")?;
            let namespace = sparse_height_manifest_terrain_id(&owner, &path)?;
            let folder = if namespace.is_none() { format!("maps/{owner}/terrain") } else { object.private_folder(&owner) };
            let target = format!("{folder}/versions/{version}.map_height_manifest.json");
            let brush = SparseTerrainBrushInput { kind, center_xz: [center.x as f64, center.z as f64], radius_meters: input.radius,
                strength: input.strength, speed_meters_per_second: input.speed / object.transform.scale[1], delta_seconds: input.delta_seconds,
                flatten_target_meters: Some(inverse.transform_point3(input.plane_point).y) };
            let footprint = [object.transform.scale[0], object.transform.scale[2]];
            let asset_root = root.clone(); let map = owner.clone();
            let mut session = IoTaskPool::get().spawn(async move {
                let mut session = SparseTerrainAuthorSession::load_source(&asset_root, &map, namespace.as_ref(), &path)?;
                session.prefetch_scaled_brush_tiles(brush, footprint)?; Ok::<_, String>(session)
            }).await?;
            let (session, changed) = AsyncComputeTaskPool::get().spawn(async move {
                let result = session.apply_scaled_brush_loaded(brush, footprint)?;
                Ok::<_, String>((session, !result.changes.is_empty()))
            }).await?;
            if changed {
                let next = target.clone();
                IoTaskPool::get().spawn(async move { let mut session = session; session.fork_to(&next)?; session.save_dirty().map(|_| ()) }).await?;
                object.terrain.height_path = Some(target);
            }
        }
        MapTerrainKind::Heightfield => {
            if input.tool.is_volume_sculpt() { return Err("GLB高度网格请使用高度雕刻；三维挖掘请选择体素对象".into()); }
            let source = object.terrain.glb_path.clone(); let asset_root = root.clone();
            let (bytes, digest) = IoTaskPool::get().spawn(async move { read_source(&asset_root, &source, 128 * 1024 * 1024) }).await?;
            let definition = object.clone();
            let encoded = AsyncComputeTaskPool::get().spawn(async move {
                let mut mesh = feathrise_map_terrain::parse_dynamic_terrain_glb(&bytes)?;
                // 在旋转前的米制网格平面雕刻：圆半径和速度仍是世界米，位移沿网格本地Y。
                // 若先改世界Y再逆变换只取Y，倾斜时会丢失位移并把压平面错误地转成世界水平面。
                let scale = Vec3::from_array(definition.transform.scale);
                let original = mesh.positions.clone();
                for point in &mut mesh.positions { *point = (Vec3::from_array(*point) * scale).to_array(); }
                let index = feathrise_map_terrain::build_terrain_editing_index(&mesh)?;
                let kind = match input.tool { EditorTool::Smooth => feathrise_map_terrain::TerrainHeightBrushKind::Smooth,
                    EditorTool::Flatten => feathrise_map_terrain::TerrainHeightBrushKind::Flatten, _ => feathrise_map_terrain::TerrainHeightBrushKind::RaiseLower };
                let sign = if input.tool == EditorTool::Lower { -1.0 } else { 1.0 };
                let min = definition.terrain.base_height * scale.y;
                let changes = feathrise_map_terrain::sculpt_height_vertices(&mut mesh, &index, feathrise_map_terrain::TerrainHeightBrush {
                    kind, center: center * scale, radius: input.radius, strength: input.strength, delta_seconds: input.delta_seconds,
                    delta_height: sign * input.speed * input.strength * input.delta_seconds.clamp(0.0, 0.05),
                    flatten_target: Some(inverse.transform_point3(input.plane_point).y * scale.y),
                    minimum: min, maximum: min + definition.terrain.height_scale * scale.y, hit_triangle: None })?;
                if changes.is_empty() { return Ok::<_, String>(None); }
                // 未命中的作者点逐位保留，不能让一次缩放往返给刷圈外顶点制造ULP差异。
                mesh.positions = original;
                for change in changes { mesh.positions[change.index][1] = change.after / scale.y; }
                feathrise_map_terrain::build_white_terrain_glb(&mesh).map(Some)
            }).await?;
            if let Some(bytes) = encoded {
                let target = format!("{}/versions/{version}.glb", object.private_folder(&owner));
                let asset_root = root; let path = target.clone(); let original = object.terrain.glb_path.clone();
                IoTaskPool::get().spawn(async move { write_version(&asset_root, &original, &digest, &path, bytes) }).await?;
                object.terrain.glb_path = target;
            }
        }
        MapTerrainKind::Voxel => object = super::voxel::edit(root, owner, object, input, version).await?,
        _ => return Err("原始GLB保留完整Scene0和材质；几何修改请编辑原资产或创建可雕刻网格".into()),
    }
    Ok(object)
}
pub(super) fn read_source(root: &Path, relative: &str, limit: usize) -> Result<(Vec<u8>, String), String> {
    use std::io::Read;
    let path = crate::authoring::safe_asset_path(root, relative)?;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new(); file.take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > limit { return Err("作者源超过本次后台读取预算".into()); }
    let digest = authoring_io::bytes_hash(&bytes); Ok((bytes, digest))
}
pub(super) fn write_version(root: &Path, original: &str, digest: &str, relative: &str, bytes: Vec<u8>) -> Result<(), String> {
    let source = crate::authoring::safe_asset_path(root, original)?;
    let target = crate::authoring::safe_asset_path(root, relative)?;
    if authoring_io::file_hash(&target)? != "missing" { return Err("新的作者版本路径已被占用".into()); }
    let expected = digest.to_owned(); let guard = target.clone();
    authoring_transaction::commit_prepared_writes_checked(vec![authoring_transaction::PreparedWrite::new(target, bytes)], move |_, _| {
        authoring_io::compare_content_hash(&expected, &authoring_io::file_hash(&source)?)?;
        authoring_io::compare_content_hash("missing", &authoring_io::file_hash(&guard)?)
    }).map_err(|e| e.message().to_owned())
}
