//! 建筑占地与真实地形命中层的后台联合整平算法。
//!
//! 【功能设计】平台覆盖占地并外扩至少一个地形单元对角线，外侧再用一个单元
//! 对角线作有限过渡；所有目标只抬高不削低，并同步同位置的GLB属性拆点。

use std::sync::Arc; // 输入浅共享当前网格，后台生成独立完整结果。

use bevy::prelude::*; // Vec2和凸四边形距离计算只使用纯数学值。
use feathrise_map_content::BuildingWorldQuad; // 平台边界严格复用建筑实际旋转占地。
use feathrise_map_terrain::{TerrainEditingIndex, TerrainMeshInput}; // 真实拓扑分层和输出作者网格。

use super::super::EditorMapDraft; // 请求提交时从当前草稿冻结同一地形修订输入。
use super::super::HeightChange; // 结果直接进入联合撤销命令。
use super::BuildingTerrainHit; // 点击世界点与三角必须来自同一视口帧。
use super::types::{TerrainLevelingInput, TerrainLevelingOutput}; // 后台边界使用普通可发送类型。

const HEIGHT_EPSILON: f32 = 0.000_01; // 避免浮点插值制造不可见历史噪音。
const MAX_CELL_SPACING_EXPANSIONS: usize = 16; // 异形拓扑局部扩圈必须有确定上限，不能沿大三角无限扩大。

/// 从当前作者草稿冻结一次联合整平输入；缺少任何真实拓扑证据都失败关闭。
pub(super) fn input_from_selected_surface(
    draft: &EditorMapDraft,
    hit: Option<BuildingTerrainHit>,
    selected: Option<Vec3>,
) -> Result<TerrainLevelingInput, String> {
    let locate_hit_triangle = draft.manifest.terrains.is_some();
    let hit = match (hit, selected, locate_hit_triangle) {
        (Some(hit), _, false) => hit,
        (_, Some(position), true) => BuildingTerrainHit { position, triangle: 0 },
        _ => return Err("联合整平需要视口命中当前地形的真实三角".to_owned()),
    };
    let mesh = draft
        .terrain_mesh
        .clone()
        .ok_or_else(|| "当前地图没有可编辑GLB地形，不能执行联合整平".to_owned())?;
    let editing_index = draft
        .terrain_editing_index
        .clone()
        .ok_or_else(|| "当前地图尚未建立地形拓扑索引，不能执行联合整平".to_owned())?;
    let manifest = draft.terrain_manifest();
    let minimum_height = manifest.terrain.base_height;
    let maximum_height = minimum_height + manifest.terrain.height_scale;
    if !minimum_height.is_finite()
        || !maximum_height.is_finite()
        || minimum_height >= maximum_height
    {
        return Err("当前地图地形高度量程无效，不能执行联合整平".to_owned());
    }
    Ok(TerrainLevelingInput {
        locate_hit_triangle,
        mesh,                                  // Arc只浅克隆，完整positions副本留在后台任务内创建。
        editing_index,                         // 索引只读共享且必须与mesh拓扑同代。
        hit_position: draft.terrain_point(hit.position).to_array(), // 独立对象的原始作者顶点均处于源坐标。
        hit_triangle: hit.triangle,            // 原三角序号用于局部拓扑连通种子。
        resolution: manifest.terrain.resolution, // 高度采样尺度仅作下限，实际GLB三角边长决定整平外扩。
        world_min: manifest.world_min,           // 当前作者对象的源边界，不用地图容器范围。
        world_max: manifest.world_max,
        minimum_height,                                // 输出高度必须继续可由当前GLB保存协议编码。
        maximum_height,                                // 最高现有平台点天然不会越过已验证源量程。
    })
}

/// 在CPU池中生成只抬高当前命中拓扑层的完整新网格与精确撤销差分。
pub(super) fn build_leveling(
    mut input: TerrainLevelingInput,
    footprint: BuildingWorldQuad,
) -> Result<TerrainLevelingOutput, String> {
    if input.locate_hit_triangle {
        let origin = Vec3::from_array(input.hit_position) + Vec3::Y * 0.02;
        let hit = input.editing_index.ray_intersection(&input.mesh.positions, &input.mesh.indices,
            origin.to_array(), Vec3::NEG_Y.to_array()).ok_or("实际承托对象没有同层可编辑GLB三角")?;
        if (hit.position[1] - input.hit_position[1]).abs() > 0.05 {
            return Err("作者GLB尚未采用当前命中层，联合整平结果已保留等待重新确认".into());
        }
        input.hit_triangle = hit.triangle;
    } // 三角搜索仍在原CPU槽，不让主线程同步遍历网格。
    validate_input(&input)?; // 所有范围和拓扑长度在复制整图前完成失败关闭。
    let cell = terrain_cell_diagonal(&input)?; // 命中三角的实际边长不能被更密的高度采样分辨率冒充。
    let center = footprint.foundation_samples()[0]; // 占地仿射中心与正式五点采样一致。
    let footprint_radius = footprint
        .corners
        .iter()
        .map(|corner| corner.distance(center))
        .fold(0.0_f32, f32::max); // 圆形宽相位必须覆盖旋转四边形全部角。
    let hit_xz = Vec2::new(input.hit_position[0], input.hit_position[2]);
    let (cell, representatives) = connected_platform_vertices(
        &input,           // 只读共享索引和源GLB，扩圈计算始终在后台任务。
        center,           // 实际旋转占地中心用于局部空间查询。
        footprint_radius, // 视口网格吸附后仍覆盖全部四角。
        hit_xz,           // 原点击三角为洞穴层身份种子。
        cell,             // 从命中三角与高度采样下限开始自适应扩大。
    )?;
    let platform_padding = cell; // 内圈全部提升到统一最高点。
    let transition_width = cell; // 外圈在固定一格内线性收敛回原高度。

    let outer_distance = platform_padding + transition_width;
    let mut candidates = Vec::new(); // 只保存实际平台及过渡带的逻辑代表，不携带整图临时表。
    for (representative, _) in representatives {
        let position = input
            .mesh
            .positions
            .get(representative)
            .ok_or_else(|| "地形拓扑代表顶点已经失效".to_owned())?;
        let distance = distance_to_quad(Vec2::new(position[0], position[2]), footprint);
        if distance > outer_distance {
            continue; // 圆形宽相位角落不属于旋转平台的有限过渡带。
        }
        let height = logical_max_height(
            input.editing_index.as_ref(),
            input.mesh.as_ref(),
            representative,
        )?; // 属性拆点异常细缝也只会向最高成员收敛，绝不削低。
        candidates.push((representative, distance, height));
    }
    let platform_height = candidates
        .iter()
        .filter(|(_, distance, _)| *distance <= platform_padding)
        .map(|(_, _, height)| *height)
        .max_by(f32::total_cmp)
        .ok_or_else(|| "建筑平台范围内没有可整平的连通地形顶点".to_owned())?;
    if !platform_height.is_finite()
        || platform_height < input.minimum_height
        || platform_height > input.maximum_height
    {
        return Err("联合整平解析出的平台高度超出当前地形量程".to_owned());
    }

    let source_mesh_identity = Arc::as_ptr(&input.mesh) as usize; // 主线程采用时复核仍是这份作者源。
    let mut changes = Vec::new(); // 先只规划真实改变的源属性顶点，空差分不复制整图。
    for (representative, distance, current_height) in candidates {
        let factor = if distance <= platform_padding {
            1.0 // 平台内全部逻辑点严格达到统一高度。
        } else {
            (1.0 - (distance - platform_padding) / transition_width).clamp(0.0, 1.0)
        };
        let target = if platform_height > current_height {
            current_height + (platform_height - current_height) * factor // 外圈只抬升所需比例。
        } else {
            current_height // 已高于平台的过渡点保持原高，禁止削坡。
        }
        .clamp(input.minimum_height, input.maximum_height);
        for vertex in input.editing_index.coincident_vertices(representative) {
            let index = vertex as usize;
            let before = input
                .mesh
                .positions
                .get(index)
                .ok_or_else(|| "地形属性拆点下标已经失效".to_owned())?[1];
            if (target - before).abs() <= HEIGHT_EPSILON {
                continue; // 数值相同不扩大撤销命令或推进地形修订。
            }
            if target < before {
                return Err("联合整平内部目标意外要求削低地形，操作已取消".to_owned());
            }
            changes.push(HeightChange {
                index,         // 保持原GLB顶点顺序，撤销无需重新定位。
                before,        // 精确恢复操作前世界Y。
                after: target, // 重做恢复同一有限抬升结果。
            });
        }
    }
    let mesh = if changes.is_empty() {
        input.mesh // 已满足平台要求时浅复用源网格，不在后台制造无意义的整图副本。
    } else {
        let mut mesh = (*input.mesh).clone(); // 【帧关键路径】完整深复制只发生在本后台CPU任务。
        for change in &changes {
            mesh.positions[change.index][1] = change.after; // 同位置属性拆点按已验证计划一起写入。
        }
        Arc::new(mesh) // 主线程随后只执行一次Arc换指针。
    };
    Ok(TerrainLevelingOutput {
        source_mesh_identity, // 不持有旧Arc，完成值不会让后续撤销长期共享整图。
        mesh,                 // 空差分复用源Arc，真实差分持有后台生成的完整新网格。
        changes,              // 空差分表示地形原本已满足平台要求。
        platform_height,      // 建筑根直接对齐后台确定的平台，不等待派生高度场。
    })
}

/// 检查任务冻结的地形数组、命中三角和清单规格仍构成一份自洽输入。
fn validate_input(input: &TerrainLevelingInput) -> Result<(), String> {
    if input.mesh.positions.is_empty()
        || input.mesh.indices.len() % 3 != 0
        || input.mesh.indices.is_empty()
        || input
            .mesh
            .indices
            .iter()
            .any(|index| *index as usize >= input.mesh.positions.len())
    {
        return Err("联合整平收到的地形拓扑无效".to_owned());
    }
    if input.hit_triangle as usize >= input.mesh.indices.len() / 3 {
        return Err("联合整平的视口命中三角已经失效".to_owned());
    }
    if !input.hit_position.iter().all(|value| value.is_finite()) {
        return Err("联合整平的视口命中位置无效".to_owned());
    }
    Ok(())
}

/// 高度采样分辨率可以远大于GLB顶点密度；从真实命中面取得平台最小单元跨度。
fn terrain_cell_diagonal(input: &TerrainLevelingInput) -> Result<f32, String> {
    let [width, height] = input.resolution;
    if width < 2 || height < 2 {
        return Err("联合整平要求至少2×2地形分辨率".to_owned());
    }
    let extent_x = input.world_max[0] - input.world_min[0];
    let extent_z = input.world_max[1] - input.world_min[1];
    let step_x = extent_x / (width - 1) as f32;
    let step_z = extent_z / (height - 1) as f32;
    let mut diagonal = Vec2::new(step_x, step_z).length(); // 仅作安全下限，不代表实际GLB网格细分。
    if !diagonal.is_finite() || diagonal <= f32::EPSILON {
        return Err("联合整平无法从地图范围解析有效地形单元尺寸".to_owned());
    }
    let base = input.hit_triangle as usize * 3; // validate_input已确认面序号与顶点下标有效。
    let triangle = &input.mesh.indices[base..base + 3]; // 当前真实命中面属于将被整平的拓扑层。
    for edge in 0..3 {
        let first = input.mesh.positions[triangle[edge] as usize]; // 使用GLB真实世界X/Z而非R16栅格。
        let second = input.mesh.positions[triangle[(edge + 1) % 3] as usize]; // 包括三角斜边。
        let length = Vec2::new(first[0] - second[0], first[2] - second[2]).length(); // 粗网格不能留下跨建筑占地的低三角面。
        if !length.is_finite() {
            return Err("联合整平命中三角的地面跨度无效".to_owned()); // 异常几何不能发布不完整平台。
        }
        diagonal = diagonal.max(length); // 一格宽度至少覆盖当前面最远的两顶点。
    }
    Ok(diagonal)
}

/// 沿命中层局部扩圈，直到平台与过渡带使用的宽度覆盖邻接GLB实际边长。
fn connected_platform_vertices(
    input: &TerrainLevelingInput, // 完整源网格与同代邻接索引只在CPU任务内读取。
    center: Vec2,                 // 实际建筑占地中心。
    footprint_radius: f32,        // 旋转占地四角到中心的最大半径。
    hit_xz: Vec2,                 // 真实视口命中点用于确保种子三角进入查询圆。
    mut cell: f32,                // 命中三角提供的初始局部单元跨度。
) -> Result<(f32, Vec<(usize, f32)>), String> {
    for _ in 0..MAX_CELL_SPACING_EXPANSIONS {
        let search_radius = (footprint_radius + 2.0 * cell).max(hit_xz.distance(center) + cell); // 扩圈后同步覆盖平台和一格过渡带。
        let representatives = input.editing_index.connected_representatives_in_circle(
            &input.mesh.positions,    // 只访问当前作者GLB的实际顶点。
            &input.mesh.indices,      // 命中面身份固定为原三角序号。
            center.to_array(),        // 索引查询使用世界X/Z。
            search_radius,            // 圆形宽相位在桶索引中局部读取。
            input.hit_position,       // 洞穴顶底按真实命中高度分层。
            Some(input.hit_triangle), // 只遍历与点击面连通的地形层。
        );
        if representatives.is_empty() {
            return Err("联合整平范围内没有与命中三角连通的地形顶点".to_owned());
        }
        let mut widest_edge = cell; // 邻接大三角可能比命中面更粗，必须再扩大平台。
        for &(representative, _) in &representatives {
            let from = input.mesh.positions[representative]; // 逻辑顶点代表保留同一水平坐标。
            for neighbor in input.editing_index.neighbors(representative) {
                let to = input
                    .mesh
                    .positions
                    .get(neighbor as usize)
                    .ok_or_else(|| "联合整平邻接顶点已经失效".to_owned())?; // 损坏拓扑不得越界。
                let length = Vec2::new(from[0] - to[0], from[2] - to[2]).length(); // 只按水平跨度决定平台覆盖。
                if !length.is_finite() {
                    return Err("联合整平邻接地形跨度无效".to_owned()); // 无法保证平台连续时失败关闭。
                }
                widest_edge = widest_edge.max(length); // 更粗的相邻三角进入下一轮局部查询。
            }
        }
        if widest_edge <= cell + HEIGHT_EPSILON {
            return Ok((cell, representatives)); // 宽度稳定后才生成最终只抬高差分。
        }
        cell = widest_edge; // 单调增长，最多经过固定次数后明确失败。
    }
    Err("联合整平附近的地形网格跨度变化过大，无法安全形成完整平台".to_owned())
}

/// 读取逻辑地形点全部属性拆点中的最高世界Y，保证后续同步只抬不削。
fn logical_max_height(
    index: &TerrainEditingIndex,
    mesh: &TerrainMeshInput,
    representative: usize,
) -> Result<f32, String> {
    let mut maximum = None::<f32>; // 规则单成员与通用属性拆点共用零分配最高高度归约。
    for vertex in index.coincident_vertices(representative) {
        let height = mesh
            .positions
            .get(vertex as usize)
            .map(|position| position[1])
            .filter(|height| height.is_finite())
            .ok_or_else(|| "联合整平读取到无效地形顶点".to_owned())?; // 保留每个成员有限且存在的验证。
        maximum = Some(maximum.map_or(height, |current| current.max(height))); // 只抬高平台需读取全部副本中最高的原值。
    }
    maximum.ok_or_else(|| "联合整平的逻辑地形点没有属性顶点".to_owned())
}

/// 返回点到凸四边形闭区域的最短平面距离，内部和边界均为零。
pub(super) fn distance_to_quad(point: Vec2, quad: BuildingWorldQuad) -> f32 {
    if quad.contains(point) {
        return 0.0; // 实体占地内部属于完整平台核心。
    }
    (0..4)
        .map(|index| distance_to_segment(point, quad.corners[index], quad.corners[(index + 1) % 4]))
        .fold(f32::INFINITY, f32::min) // 四条边的最近距离就是凸矩形外部距离。
}

/// 返回点到有限线段的欧氏距离。
fn distance_to_segment(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let edge = end - start;
    let length_squared = edge.length_squared();
    if length_squared <= f32::EPSILON {
        return point.distance(start); // 防御损坏零长边，公共预设校验正常不会进入。
    }
    let factor = ((point - start).dot(edge) / length_squared).clamp(0.0, 1.0);
    point.distance(start + edge * factor)
}

#[cfg(test)]
mod tests {
    use super::*; // 纯后台算法回归不建立ECS或渲染世界。
    use feathrise_map_content::BuildingFootprintRect; // 构造旋转前单位平台。
    use feathrise_map_terrain::build_terrain_editing_index; // 使用生产索引验证洞穴分层。

    /// 构造一层规则网格及其确定TriangleList。
    fn grid(size: u32, height: impl Fn(u32, u32) -> f32) -> TerrainMeshInput {
        let mut positions = Vec::with_capacity((size * size) as usize);
        for z in 0..size {
            for x in 0..size {
                positions.push([x as f32, height(x, z), z as f32]); // 闭包按借用重复调用，不要求复制测试逻辑。
            }
        }
        let mut indices = Vec::new();
        for z in 0..size - 1 {
            for x in 0..size - 1 {
                let northwest = z * size + x;
                indices.extend_from_slice(&[
                    northwest,
                    northwest + size,
                    northwest + 1,
                    northwest + 1,
                    northwest + size,
                    northwest + size + 1,
                ]);
            }
        }
        TerrainMeshInput { positions, indices }
    }

    /// 为指定网格和首个三角建立完整后台输入。
    fn input(mesh: TerrainMeshInput, resolution: [u32; 2]) -> TerrainLevelingInput {
        let index = build_terrain_editing_index(&mesh).expect("规则测试网格应建立编辑索引");
        TerrainLevelingInput {
            locate_hit_triangle: false,
            mesh: Arc::new(mesh),
            editing_index: Arc::new(index),
            hit_position: [2.0, 0.0, 2.0],
            hit_triangle: 20,
            resolution,
            world_min: [0.0, 0.0],
            world_max: [4.0, 4.0],
            minimum_height: -10.0,
            maximum_height: 20.0,
        }
    }

    #[test]
    fn platform_raises_low_vertices_and_never_lowers_existing_high_ground() {
        let mesh = grid(5, |x, z| if x == 2 && z == 2 { 4.0 } else { 1.0 });
        let footprint = BuildingWorldQuad::from_local_rect(
            BuildingFootprintRect {
                center: [0.0, 0.0],
                half_extents: [0.6, 0.6],
            },
            [2.0, 0.0, 2.0],
            0.0,
            [1.0; 3],
        );
        let result = build_leveling(input(mesh.clone(), [5, 5]), footprint).unwrap();
        assert_eq!(result.platform_height, 4.0); // 平台采用命中层安全范围内的最高现有点。
        assert!(!result.changes.is_empty()); // 周围低点被实际抬升。
        assert!(
            result
                .changes
                .iter()
                .all(|change| change.after >= change.before)
        ); // 任一源属性顶点都不削低。
        assert_eq!(result.mesh.positions[12][1], 4.0); // 原最高中心保持不变。
    }

    #[test]
    fn coarse_glb_mesh_uses_real_triangle_spacing_instead_of_height_sample_resolution() {
        let mut mesh = grid(5, |x, z| if x == 2 && z == 2 { 4.0 } else { 1.0 });
        for position in &mut mesh.positions {
            position[0] *= 4.0; // GLB实际网格每格四米，而清单高度采样仍声明513点。
            position[2] *= 4.0; // 两种分辨率不能混作整平平台宽度。
        }
        let mut task_input = input(mesh, [513, 513]);
        task_input.world_max = [16.0, 16.0]; // 旧算法从清单错误算出约0.044米单元。
        task_input.hit_position = [8.0, 4.0, 8.0]; // 命中实际中央高点所在三角。
        let footprint = BuildingWorldQuad::from_local_rect(
            BuildingFootprintRect {
                center: [0.0, 0.0],
                half_extents: [1.0, 1.0],
            },
            [8.0, 0.0, 8.0],
            0.0,
            [1.0; 3],
        );

        let result =
            build_leveling(task_input, footprint).expect("粗GLB网格应整平整栋占地的真实三角");
        assert_eq!(result.platform_height, 4.0);
        for vertex in [12, 13, 17, 18] {
            assert_eq!(result.mesh.positions[vertex][1], 4.0); // 四角均抬平，建筑底部不跨低三角悬空。
        }
        assert!(
            result
                .changes
                .iter()
                .all(|change| change.after >= change.before)
        ); // 整平仍保持只抬高地形的合同。
    }

    #[test]
    fn disconnected_lower_cave_layer_is_not_modified() {
        let upper = grid(5, |x, z| if x == 2 && z == 2 { 3.0 } else { 1.0 });
        let upper_count = upper.positions.len() as u32;
        let mut positions = upper.positions.clone();
        positions.extend(
            upper
                .positions
                .iter()
                .map(|position| [position[0], -5.0, position[2]]),
        );
        let mut indices = upper.indices.clone();
        indices.extend(upper.indices.iter().map(|index| index + upper_count));
        let mesh = TerrainMeshInput { positions, indices };
        let mut task_input = input(mesh, [5, 5]);
        task_input.hit_position = [2.0, 3.0, 2.0]; // 命中上层中央附近。
        let footprint = BuildingWorldQuad::from_local_rect(
            BuildingFootprintRect {
                center: [0.0, 0.0],
                half_extents: [0.6, 0.6],
            },
            [2.0, 0.0, 2.0],
            0.0,
            [1.0; 3],
        );
        let result = build_leveling(task_input, footprint).unwrap();
        assert!(
            result
                .changes
                .iter()
                .all(|change| change.index < upper_count as usize)
        ); // 下层同X/Z顶点不进入差分。
        assert!(
            result.mesh.positions[upper_count as usize..]
                .iter()
                .all(|position| position[1] == -5.0)
        ); // 洞穴底层完整保持原高。
    }

    #[test]
    fn already_flat_platform_reuses_the_source_mesh_arc() {
        let mesh = grid(5, |_x, _z| 2.0); // 整个平台及过渡带都已经位于同一高度。
        let task_input = input(mesh, [5, 5]); // 生产输入以Arc共享完整源网格。
        let source = task_input.mesh.clone(); // 保留身份以确认后台没有复制空差分整图。
        let footprint = BuildingWorldQuad::from_local_rect(
            BuildingFootprintRect {
                center: [0.0, 0.0],       // 占地以实例根为中心。
                half_extents: [0.6, 0.6], // 与抬平测试使用相同平台范围。
            },
            [2.0, 0.0, 2.0], // 放在规则网格中央。
            0.0,             // 不旋转便于读取确定平台。
            [1.0; 3],        // 单位建筑缩放。
        );
        let result = build_leveling(task_input, footprint).expect("平坦平台应产生合法空差分");

        assert!(result.changes.is_empty()); // 已平坦地形不制造撤销噪音。
        assert!(Arc::ptr_eq(&result.mesh, &source)); // 空差分直接复用源Arc，避免后台整图复制。
    }
}
