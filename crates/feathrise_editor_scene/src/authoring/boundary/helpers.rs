//! 区域边界编辑中的点位校验、命中半径和贴地显示纯函数。

use bevy::prelude::*; // Rect、UVec2、Vec2与Vec3。
use feathrise_scene_content::{
    MAX_SCENE_BOUNDARY_PATHS, MAX_SCENE_BOUNDARY_POINTS_PER_PATH, MAX_SCENE_BOUNDARY_POINTS_TOTAL,
    MIN_SCENE_BOUNDARY_SEGMENT_METERS, SceneBoundariesAsset, SceneId,
}; // 编辑中间态沿用正式预算，但允许主路径尚未闭合。

use crate::authoring::resources::EditorSceneDraft; // 贴地显示读取当前GLB派生高度与清单解码参数。

/// 所选路径不存在时失败关闭；没有显式选择才沿用主路径。
pub(super) fn selected_path_index(
    session: &crate::authoring::EditorAuthoringSession,
) -> Option<usize> {
    let draft = session.draft.as_ref()?;
    let document = draft.boundaries.as_ref()?;
    if let Some((map, id)) = session
        .selected_boundary_path
        .as_ref()
        .filter(|(map, _)| *map == draft.scene_id)
    {
        let _ = map; // 完整场景身份已验证。
        document.paths.iter().position(|path| path.id == *id) // 仅在编辑输入边沿解析稳定ID。
    } else {
        None // 未选择时不隐式修改第一面墙。
    }
}

/// 控制点在当前视图中的选择半径，保持约十四像素可点击面积。
pub(super) fn point_pick_radius(extent: f32, render_size: UVec2) -> f32 {
    let pixels_y = render_size.y.max(1) as f32; // 防止尚未建立RTT时除零。
    (extent / pixels_y * 14.0).clamp(0.5, 24.0) // 大场景全景不让一个点吞掉过大世界范围。
}

/// 校验F12绘制中间态；允许零点或开放路径，但拒绝越界、退化和超预算数据。
pub(super) fn validate_edit_geometry(
    document: &SceneBoundariesAsset, // 当前小型完整草稿。
    expected_scene_id: &SceneId,       // 草稿必须属于当前场景。
    _world_bounds: Rect,           // 兼容旧调用；人工墙可以位于任意有限世界坐标。
) -> Result<(), String> {
    if &document.scene_id != expected_scene_id {
        return Err("区域边界草稿所属场景与当前场景不一致".to_owned()); // 跨场景拖点必须拒绝。
    }
    if document.paths.len() > MAX_SCENE_BOUNDARY_PATHS {
        return Err(format!(
            "区域边界路径数量必须为1..={MAX_SCENE_BOUNDARY_PATHS}"
        )); // 重画仍保留一条空主路径。
    }
    let mut total_points = 0usize; // 场景级预算不能通过多路径绕过。
    for path in &document.paths {
        if path.points.len() > MAX_SCENE_BOUNDARY_POINTS_PER_PATH {
            return Err(format!(
                "区域边界{}控制点超过{MAX_SCENE_BOUNDARY_POINTS_PER_PATH}",
                path.id
            )); // 单路径固定预算保护Gizmo与物理实体数量。
        }
        if !path.bottom_y.is_finite()
            || !path.top_y.is_finite()
            || path.top_y <= path.bottom_y
            || !(path.top_y - path.bottom_y).is_finite()
            || !path.thickness.is_finite()
            || !(0.25..=32.0).contains(&path.thickness)
        {
            return Err(format!("区域边界{}墙高或厚度无效", path.id)); // 编辑中间态也不能产生NaN物理规格。
        }
        total_points = total_points.saturating_add(path.points.len()); // 饱和累加避免异常大输入溢出。
        for point in &path.points {
            let point = Vec2::from_array(*point); // 统一转换为世界X/Z向量。
            if !point.is_finite() {
                return Err(format!("区域边界{}存在非有限控制点", path.id));
            }
        }
        for pair in path.points.windows(2) {
            if Vec2::from_array(pair[0]).distance(Vec2::from_array(pair[1]))
                < MIN_SCENE_BOUNDARY_SEGMENT_METERS
            {
                return Err(format!("区域边界{}包含过短或重复线段", path.id)); // 退化盒体旋转不稳定。
            }
        }
        if path.closed && path.points.len() >= 2 {
            let last = Vec2::from_array(*path.points.last().expect("闭合路径至少两个点")); // 长度门保证安全读取。
            let first = Vec2::from_array(path.points[0]); // 首尾额外形成闭合段。
            if last.distance(first) < MIN_SCENE_BOUNDARY_SEGMENT_METERS {
                return Err(format!("区域边界{}闭合段过短", path.id)); // 不允许重复保存首点。
            }
        }
    }
    if total_points > MAX_SCENE_BOUNDARY_POINTS_TOTAL {
        return Err(format!(
            "区域边界总控制点超过{MAX_SCENE_BOUNDARY_POINTS_TOTAL}"
        )); // 场景级硬预算保持固定。
    }
    Ok(()) // 当前中间态可以继续绘制或拖动。
}

/// 插点命中只在Ctrl按下边沿扫描所选墙，返回线段投影位置和插入编号。
pub(super) fn nearest_segment(
    path: &feathrise_scene_content::SceneBoundaryPath,
    world: Vec2,
    radius: f32,
) -> Option<(usize, [f32; 2])> {
    let count = path.points.len(); // 开放末点没有下一段。
    if count < 2 {
        return None;
    }
    let segments = if path.closed { count } else { count - 1 }; // 闭环补上末段。
    (0..segments)
        .filter_map(|index| {
            let start = Vec2::from_array(path.points[index]); // 当前段起点。
            let end = Vec2::from_array(path.points[(index + 1) % count]); // 当前段终点。
            let direction = end - start; // 投影到真实线段，不能改变已有墙轮廓。
            let length = direction.length_squared();
            if length < MIN_SCENE_BOUNDARY_SEGMENT_METERS.powi(2) {
                return None;
            }
            let projected =
                start + direction * ((world - start).dot(direction) / length).clamp(0.0, 1.0);
            let distance = projected.distance_squared(world); // 平方距离避免逐段开方。
            (distance <= radius * radius).then_some((index + 1, projected.to_array(), distance))
        })
        .min_by(|left, right| left.2.total_cmp(&right.2))
        .map(|(index, point, _)| (index, point))
}

/// 单墙提交只检查该墙，不扫描整个场景的其他对象。
pub(super) fn validate_path_geometry(
    path: &feathrise_scene_content::SceneBoundaryPath,
    map: &SceneId,
    bounds: Rect,
) -> Result<(), String> {
    let document = SceneBoundariesAsset {
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        scene_id: map.clone(),
        paths: vec![path.clone()],
    }; // 只复制正在编辑的一面墙。
    validate_edit_geometry(&document, map, bounds) // 绘制中的开放外框仍允许继续编辑。
}

/// 把二维控制点放到当前正式三角剖分高度上方，确保边界线在坡面视口可见。
pub(super) fn display_point(draft: &EditorSceneDraft, point: [f32; 2]) -> Vec3 {
    feathrise_scene_content::boundary_display_point(
        draft.heights.as_deref().map(|heights| heights.as_slice()), // 不复制当前草稿高度。
        &draft.manifest, // 游戏与编辑器共用高度参数和0.35米偏移。
        point,           // 原作者端点。
    )
}

#[cfg(test)]
mod tests {
    use super::*; // 回归验证缩放命中半径和开放中间态校验。
    use feathrise_scene_content::SceneBoundariesAsset; // 使用正式默认构造器建立夹具。

    #[test] // 控制点选择半径应随视图缩放变化并保持明确上下限。
    fn point_pick_radius_tracks_view_scale_with_fixed_limits() {
        assert_eq!(point_pick_radius(10.0, UVec2::new(100, 100)), 1.4); // 十四像素对应一点四米。
        assert_eq!(point_pick_radius(1.0, UVec2::new(10_000, 10_000)), 0.5); // 极近视图保持半米最小命中。
        assert_eq!(point_pick_radius(10_000.0, UVec2::new(100, 100)), 24.0); // 极远视图不超过二十四米。
    }

    #[test] // 重画后的空开放主路径必须允许继续落点但不能冒充正式闭合文件。
    fn edit_geometry_accepts_an_empty_open_path() {
        let scene_id = SceneId::parse("MAP-BOUNDARY-EDIT").expect("测试场景ID应有效"); // 创建稳定测试身份。
        let mut document = SceneBoundariesAsset::rectangular_default(
            &scene_id,        // 文件所属场景。
            [-50.0, -50.0], // 合法最小角。
            [50.0, 50.0],   // 合法最大角。
            -80.0,          // 合法最低高度。
            160.0,          // 合法高度跨度。
        )
        .expect("默认边界应构造成功");
        document.paths[0].points.clear(); // 模拟点击“重画边界”。
        document.paths[0].closed = false; // 主路径正在开放绘制。
        let bounds = Rect::from_corners(Vec2::splat(-50.0), Vec2::splat(50.0)); // 当前场景范围。
        assert!(validate_edit_geometry(&document, &scene_id, bounds).is_ok()); // F12中间态可以继续工作。
        assert!(document.validate(&scene_id, bounds).is_err()); // 保存与退出仍必须拒绝未闭合路径。
    }
}
