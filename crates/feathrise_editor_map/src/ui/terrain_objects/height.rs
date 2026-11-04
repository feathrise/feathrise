//! 源高度域与体素实高的独立语义；范围校验在后台读取冻结网格，绝不拉伸高度网格顶点。
use feathrise_map_content::{MapTerrainKind, MapTerrainObject};

pub(super) fn voxel_height(object: &MapTerrainObject) -> Option<f32> {
    (object.terrain.kind == MapTerrainKind::Voxel).then_some(())?;
    let source = object.voxel.as_ref()?;
    Some((source.max[1] - source.min[1]) * object.transform.scale[1])
}

/// 总量程围绕原作者域中心伸缩；已有雕刻必须完整落在新域内，不能钳制或移动任何顶点。
pub(super) fn set_domain(
    mut object: MapTerrainObject,
    span: f32,
    mesh: &feathrise_map_terrain::TerrainMeshInput,
) -> Result<MapTerrainObject, String> {
    if object.terrain.kind != MapTerrainKind::Heightfield || !span.is_finite() || span <= 0.0 {
        return Err("只有高度网格可调整有限正数的作者高度域".into());
    }
    let center = f64::from(object.terrain.base_height) + f64::from(object.terrain.height_scale) * 0.5;
    let minimum = (center - f64::from(span) * 0.5) as f32;
    let maximum = minimum + span;
    if !minimum.is_finite() || !maximum.is_finite() || minimum >= maximum || mesh.positions.is_empty() {
        return Err("作者高度域或当前作者几何无效".into());
    }
    if mesh.positions.iter().any(|point| !point[1].is_finite() || !(minimum..=maximum).contains(&point[1])) {
        return Err(format!("作者高度域{minimum:.3}～{maximum:.3}m必须包含当前全部已雕刻几何；顶点保持原样"));
    }
    object.terrain.base_height = minimum;
    object.terrain.height_scale = span;
    Ok(object)
}
