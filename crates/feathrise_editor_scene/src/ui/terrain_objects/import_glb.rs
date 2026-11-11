//! 读取完整Scene0的真实范围；节点变换、多个primitive、材质和外部资产引用仍由正式GLB加载器保留。
use bevy::prelude::*;
use feathrise_scene_content::SceneTerrainObject;

pub(crate) fn read(path: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if !path.to_ascii_lowercase().ends_with(".glb") { return Err("网格模型请选择GLB文件".into()); }
    let source = crate::authoring::safe_asset_path(&crate::authoring::workspace_asset_root(), path)?;
    let file = std::fs::File::open(source).map_err(|e| format!("读取GLB失败：{e}"))?;
    let mut bytes = Vec::new(); file.take(128 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 128 * 1024 * 1024 { return Err("GLB超过128MiB读取预算".into()); }
    Ok(bytes)
}
pub(super) fn bounds(bytes: &[u8]) -> Result<(Vec3, Vec3), String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("GLB格式无效：{e}"))?;
    let scene = gltf.scenes().next().ok_or("GLB缺少Scene0")?;
    let mut stack = scene.nodes().map(|node| (node, Mat4::IDENTITY)).collect::<Vec<_>>();
    let mut min = Vec3::splat(f32::INFINITY); let mut max = Vec3::splat(f32::NEG_INFINITY);
    while let Some((node, parent)) = stack.pop() {
        if node.name().is_some_and(|name| name.starts_with("规划_")) { continue; }
        let matrix = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if !matrix.is_finite() || !matrix.inverse().is_finite() { return Err("GLB节点变换无效".into()); }
        if node.skin().is_some() { return Err("当前GLB网格入口需要静态几何；蒙皮角色请使用普通资产摆放".into()); }
        if let Some(mesh) = node.mesh() { for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles { return Err("GLB网格需要三角形primitive".into()); }
            let bound = primitive.bounding_box();
            for i in 0..8 {
                let point = matrix.transform_point3(Vec3::new(if i & 1 == 0 { bound.min[0] } else { bound.max[0] },
                    if i & 2 == 0 { bound.min[1] } else { bound.max[1] }, if i & 4 == 0 { bound.min[2] } else { bound.max[2] }));
                min = min.min(point); max = max.max(point);
            }
        } }
        stack.extend(node.children().map(|node| (node, matrix)));
    }
    if !min.is_finite() || !max.is_finite() || min.x >= max.x || min.z >= max.z {
        return Err("GLB缺少具有有效XZ面积的网格几何".into());
    }
    Ok((min, max))
}
pub(super) fn configure(object: &mut SceneTerrainObject, path: String, (min, max): (Vec3, Vec3), creation: [f32; 8]) -> Result<(), String> {
    let size = (max - min).max(Vec3::splat(0.01));
    object.terrain.glb_path = path;
    object.terrain.resolution = [0, 0];
    object.world_min = [min.x, min.z]; object.world_max = [max.x, max.z];
    object.terrain.base_height = min.y; object.terrain.height_scale = size.y;
    object.display_name = "GLB网格".into();
    object.transform.scale = [creation[3] / size.x, if max.y - min.y <= 0.001 { 1.0 } else { creation[4] / size.y }, creation[5] / size.z];
    object.transform.validate()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_generated_glb_reads_actual_scene_extent() {
        let bytes = feathrise_terrain::build_flat_terrain_glb(feathrise_terrain::TerrainSampleSpec {
            resolution: [3, 3], world_min: [-2.0, -4.0], world_max: [2.0, 4.0], base_height: 0.0, height_scale: 8.0 }, 3.0).unwrap();
        let (min, max) = bounds(&bytes).unwrap();
        assert_eq!(min, Vec3::new(-2.0, 3.0, -4.0));
        assert_eq!(max, Vec3::new(2.0, 3.0, 4.0));
    }
}
