//! 白模GLB与实时预览共用的规则高度场几何构建器。

use bevy::math::Vec3; // 用同一向量实现校验平滑法线与真实三角面朝向的一致性。

/// 浮点误差范围内也不允许共享顶点法线落入任一相邻三角面的背半球。
const TERRAIN_NORMAL_FACE_EPSILON: f32 = 1.0e-6; // 正值让修正后的射线起点明确位于高度场外侧。

/// 一次完整构建后可直接编码GLB或采用为Bevy Mesh的拥有数据。
pub(crate) struct TerrainGeometry {
    pub(crate) positions: Vec<[f32; 3]>,
    pub(crate) normals: Vec<[f32; 3]>,
    pub(crate) indices: Vec<u32>,
    pub(crate) min: [f32; 3],
    pub(crate) max: [f32; 3],
}

/// 从16位高度场构建一个网格、受三角面半球约束的平滑法线和朝外三角形绕序。
pub(crate) fn build_terrain_geometry(
    heights: &[u16],
    resolution: [u32; 2],
    world_min: [f32; 2],
    world_max: [f32; 2],
    base_height: f32,
    height_scale: f32,
) -> Result<TerrainGeometry, String> {
    let [width, height] = resolution;
    if width < 2 || height < 2 || width > 1025 || height > 1025 {
        return Err("地形分辨率必须位于2×2至1025×1025".to_owned());
    }
    let vertex_count = width as usize * height as usize;
    if heights.len() != vertex_count {
        return Err(format!(
            "高度样本{}与分辨率{}×{}不一致",
            heights.len(),
            width,
            height
        ));
    }
    if !world_min
        .iter()
        .chain(world_max.iter())
        .all(|value| value.is_finite())
        || world_min[0] >= world_max[0]
        || world_min[1] >= world_max[1]
        || !base_height.is_finite()
        || !height_scale.is_finite()
        || height_scale <= 0.0
    {
        return Err("地形世界范围或高度比例无效".to_owned());
    }

    let x_step = (world_max[0] - world_min[0]) / (width - 1) as f32;
    let z_step = (world_max[1] - world_min[1]) / (height - 1) as f32;
    let sample_height = |x: u32, z: u32| -> f32 {
        let raw = heights[(z * width + x) as usize] as f32 / u16::MAX as f32;
        base_height + raw * height_scale
    };
    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for z in 0..height {
        for x in 0..width {
            let y = sample_height(x, z);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            positions.push([
                world_min[0] + x as f32 * x_step,
                y,
                world_min[1] + z as f32 * z_step,
            ]);
            let left = sample_height(x.saturating_sub(1), z);
            let right = sample_height((x + 1).min(width - 1), z);
            let down = sample_height(x, z.saturating_sub(1));
            let up = sample_height(x, (z + 1).min(height - 1));
            let dx_divisor = if x == 0 || x + 1 == width {
                x_step
            } else {
                2.0 * x_step
            };
            let dz_divisor = if z == 0 || z + 1 == height {
                z_step
            } else {
                2.0 * z_step
            };
            let dx = (right - left) / dx_divisor;
            let dz = (up - down) / dz_divisor;
            let length = (dx * dx + 1.0 + dz * dz).sqrt();
            normals.push([-dx / length, 1.0 / length, -dz / length]);
        }
    }
    let mut indices = Vec::with_capacity((width - 1) as usize * (height - 1) as usize * 6);
    for z in 0..height - 1 {
        for x in 0..width - 1 {
            let a = z * width + x;
            let b = a + 1;
            let c = a + width;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    keep_normals_in_heightfield_face_hemispheres(&positions, &indices, &mut normals); // 锐折处修正法线且不拆点，保留正式顶点与载荷预算。
    Ok(TerrainGeometry {
        positions,
        normals,
        indices,
        min: [world_min[0], min_y, world_min[1]],
        max: [world_max[0], max_y, world_max[1]],
    })
}

/// 中央差分跨越锐折时可能指向某个相邻面的内侧；该情况只把共享顶点退回安全的世界上方向。
fn keep_normals_in_heightfield_face_hemispheres(
    positions: &[[f32; 3]],   // 规则高度场的唯一顶点位置表。
    indices: &[u32],          // 已按朝上绕序生成的完整三角索引。
    normals: &mut [[f32; 3]], // 原地修正以避免拆点或改变GLB作者拓扑。
) {
    // 完成法线修正入口定义。
    for triangle in indices.chunks_exact(3) {
        // 每个索引三元组独立提供真实几何面法线。
        let first_index = triangle[0] as usize; // 解析当前三角形第一个共享顶点。
        let second_index = triangle[1] as usize; // 解析当前三角形第二个共享顶点。
        let third_index = triangle[2] as usize; // 解析当前三角形第三个共享顶点。
        let first = Vec3::from_array(positions[first_index]); // 读取第一个世界空间位置。
        let second = Vec3::from_array(positions[second_index]); // 读取第二个世界空间位置。
        let third = Vec3::from_array(positions[third_index]); // 读取第三个世界空间位置。
        let face_normal = (second - first).cross(third - first).normalize(); // 规则网格保证三角形非退化且绕序朝上。
        for &vertex_index in triangle {
            // 同一面必须逐角验证插值法线端点。
            let vertex_index = vertex_index as usize; // 转为位置与法线切片索引。
            let smooth_normal = Vec3::from_array(normals[vertex_index]); // 读取可能跨越局部锐折的中央差分法线。
            if smooth_normal.dot(face_normal) <= TERRAIN_NORMAL_FACE_EPSILON {
                // 背半球法线会把Solari主表面射线偏移到地形内部。
                normals[vertex_index] = Vec3::Y.to_array(); // 高度场全部面朝上，世界+Y对所有相邻面都是安全公共半球。
            } // 完成当前共享顶点的必要修正。
        } // 完成当前三角形三个顶点检查。
    } // 完成全部相邻面约束检查。
} // 完成原地法线修正。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_grid_has_up_normals_and_counter_clockwise_top_faces() {
        let geometry =
            build_terrain_geometry(&[0; 9], [3, 3], [-1.0, -1.0], [1.0, 1.0], 0.0, 8.0).unwrap();
        assert!(
            geometry
                .normals
                .iter()
                .all(|normal| *normal == [0.0, 1.0, 0.0])
        );
        assert_eq!(&geometry.indices[0..6], &[0, 3, 1, 1, 3, 4]);
    }

    #[test] // 真实锐折回归样本必须避免中央差分法线穿入相邻三角面。
    fn sharp_heightfield_keeps_every_shared_normal_in_the_incident_face_hemisphere() {
        let heights = [
            // 截取自复现场景中曾产生负面法线夹角的三乘三R16邻域。
            34_403, 35_818, 33_731, // 第一行保留原始北侧高度关系。
            37_469, 40_333,
            32_761, // 中心高点与东侧陡降触发原中央差分异常。
            34_451, 32_767, 32_714, // 第三行保留原始南侧高度关系。
        ]; // 完成固定回归高度源。
        let geometry = build_terrain_geometry(
            // 使用复现场景相同单元米制比例构建候选。
            &heights,         // 传入固定锐折R16样本。
            [3, 3],           // 三乘三足以覆盖中心顶点全部相邻三角面。
            [0.0, 0.0],       // 局部世界范围从零开始不改变坡度。
            [13.375, 13.125], // 两轴步长分别保持原图的6.6875m与6.5625m。
            -80.0,            // 沿用复现场景的高度基线。
            160.0,            // 沿用复现场景的完整R16高度量程。
        ) // 完成共享构建器调用。
        .unwrap(); // 固定合法输入必须成功构建。
        assert_eq!(geometry.positions.len(), heights.len()); // 修正不得通过拆点改变动态地形顶点顺序。
        assert_eq!(geometry.normals.len(), heights.len()); // 每个共享位置仍只对应一条法线。
        for triangle in geometry.indices.chunks_exact(3) {
            // 验证所有真实面与三个插值端点。
            let first = Vec3::from_array(geometry.positions[triangle[0] as usize]); // 读取面首顶点。
            let second = Vec3::from_array(geometry.positions[triangle[1] as usize]); // 读取面次顶点。
            let third = Vec3::from_array(geometry.positions[triangle[2] as usize]); // 读取面末顶点。
            let face_normal = (second - first).cross(third - first).normalize(); // 重建与生产路径同绕序的面法线。
            for &vertex_index in triangle {
                // 每个角都必须位于当前面的正半球。
                let normal = Vec3::from_array(geometry.normals[vertex_index as usize]); // 读取最终采用法线。
                assert!(
                    // 负值会重新引入视角相关的Solari地形自相交。
                    normal.dot(face_normal) > TERRAIN_NORMAL_FACE_EPSILON, // 要求保留明确正向余量。
                    "顶点{vertex_index}的法线落入相邻三角面背半球", // 失败时直接指出异常共享顶点。
                ); // 完成当前角的半球断言。
            } // 完成当前三角形三个角验证。
        } // 完成全部三角形验证。
    } // 完成锐折高度场回归测试。
}
