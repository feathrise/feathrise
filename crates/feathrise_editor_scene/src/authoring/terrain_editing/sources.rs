//! 高度地形只编辑Clipmap分页源；体积地形使用自己的密度作者源。
use bevy::{prelude::*, tasks::{AsyncComputeTaskPool, IoTaskPool}};
use feathrise_scene_content::*;
use std::path::Path;
use super::ObjectBrushInput;
use crate::types::EditorTool;

pub(crate) async fn edit(root: std::path::PathBuf, owner: SceneId, mut object: SceneTerrainObject,
    input: ObjectBrushInput, version: String) -> Result<SceneTerrainObject, String> {
    if !input.center.is_finite() || !input.radius.is_finite() || input.radius <= 0.0
        || !input.strength.is_finite() || !(0.01..=1.0).contains(&input.strength)
        || !input.speed.is_finite() || input.speed < 0.0 || !input.delta_seconds.is_finite()
        || !input.plane_point.is_finite() { return Err("地形笔刷参数无效".into()); }
    let matrix = object.transform.as_transform().to_matrix(); let inverse = matrix.inverse();
    let center = inverse.transform_point3(input.center);
    match object.terrain.kind {
        SceneTerrainKind::SparseClipmap => {
            let kind = match input.tool { EditorTool::Raise => SparseTerrainBrushKind::Raise,
                EditorTool::Lower => SparseTerrainBrushKind::Lower, EditorTool::Smooth => SparseTerrainBrushKind::Smooth,
                EditorTool::Flatten => SparseTerrainBrushKind::Flatten, _ => return Err("Clipmap高度工具使用抬升、降低、平滑和整平；洞穴继续使用体积区域工具".into()) };
            let path = object.terrain.height_path.clone().ok_or("地形缺少高度源")?;
            let namespace = sparse_height_manifest_terrain_id(&owner, &path)?;
            let folder = if namespace.is_none() { format!("scenes/{owner}/terrain") } else { object.private_folder(&owner) };
            let target = format!("{folder}/versions/{version}.scene_height_manifest.json");
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

        SceneTerrainKind::Voxel => object = super::voxel::edit(root, owner, object, input, version).await?,
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
