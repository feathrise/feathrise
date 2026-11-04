//! 连续水网作者工具共享的纯选择、端点与编辑期校验函数。

use std::collections::BTreeSet; // 稳定ID与控制点预算使用确定性集合校验。

use bevy::prelude::*; // Vec2、Vec3与有限浮点运算。
use feathrise_map_content::{
    MAX_MAP_WATER_FLOW_SPEED, MAX_MAP_WATER_SPLINE_DEPTH_METERS, MAX_MAP_WATER_SPLINE_WIDTH_METERS, MIN_MAP_WATER_SPLINE_DEPTH_METERS, MIN_MAP_WATER_SPLINE_WIDTH_METERS, MapId, MapWaterAsset,
    MapWaterSplinePoint,
}; // 编辑期仍严格复用正式预算和数值范围，只放宽未完成点数与连接。

use super::connections::endpoint_height; // 宽松引用校验复用统一端点解析，不维护第二套连接语义。
use crate::authoring::resources::{EditorAuthoringSession, EditorCommand}; // 水网完整快照进入统一撤销与预览修订。
use crate::resources::{MapEditorState, MapEditorViewportState}; // 当前工具选择与固定像素命中换算。
use crate::types::EditorTool; // 河流和瀑布拥有独立作者工具。

pub(super) const DEFAULT_POINT_WIDTH: f32 = 6.0; // 新控制点默认完整宽度六米。
pub(super) const DEFAULT_POINT_DEPTH: f32 = 1.5; // 新控制点默认玩法水深一点五米。
pub(super) const DEFAULT_POINT_SPEED: f32 = 2.0; // 新控制点默认作者流速二米每秒。
pub(super) const POINT_PICK_RADIUS_PIXELS: f32 = 14.0; // 三维控制点命中统一使用屏幕像素，不受透视深度影响。

/// 分配包含作者修订和会话序号的稳定ID，并显式跳过重新打开地图后已有的全部水体与点ID。
pub(super) fn allocate_water_id(
    session: &mut EditorAuthoringSession,
    water: &MapWaterAsset,
    prefix: &str,
) -> String {
    loop {
        session.next_serial = session.next_serial.wrapping_add(1); // 与模型、传送点共用会话唯一递增源。
        let candidate = format!("{prefix}-{}-{}", session.revision, session.next_serial);
        let in_use = water.layers.iter().any(|layer| layer.id == candidate)
            || water.rivers.iter().any(|river| {
                river.id == candidate || river.points.iter().any(|point| point.id == candidate)
            })
            || water.waterfalls.iter().any(|waterfall| {
                waterfall.id == candidate
                    || waterfall.points.iter().any(|point| point.id == candidate)
            }); // 进程重启会把会话序号归零，不能让已有稳定ID迫使作者反复点击碰运气。
        if !in_use {
            return candidate; // 正式水网规模固定有界，最多跳过现有数百个候选后必然收敛。
        }
    }
}

/// 原子采用一份编辑期受检水网，并登记完整前后快照、重做分支和现有预览修订。
pub(super) fn commit_water_change(
    session: &mut EditorAuthoringSession,
    editor: &mut MapEditorState,
    before: MapWaterAsset,
    after: MapWaterAsset,
    status: impl Into<String>,
) {
    let Some(draft) = session.draft.as_mut() else {
        return; // 草稿换代时迟到操作不得创建新的水域替身。
    };
    let status = status.into(); // 无变化与真实提交共用调用方已经生成的明确动作说明。
    if before == after {
        editor.status = format!("{status}；水网定义未变化");
        return; // 端点循环或连接吸附抵消参数变化时不得压空撤销、清重做或推进预览修订。
    }
    draft.water = Some(after.clone()); // 先把完整定义作为当前作者权威原子采用。
    session.push_bounded_water_history(EditorCommand::WaterDefinitionsChanged { before, after }); // 一个按钮、落点或完整拖动只对应一条受限撤销。
    session.mark_water_definition_changed(editor); // 复用现有single-flight水预览和正式保存脏门。
    editor.status = status; // 操作结果同步进入底栏。
}

/// 返回当前工具选中的控制点只读切片。
pub(super) fn active_points<'a>(
    water: &'a MapWaterAsset,
    editor: &MapEditorState,
) -> Option<&'a [MapWaterSplinePoint]> {
    match editor.tool {
        EditorTool::RiverSpline => water
            .rivers
            .get(editor.selected_water_river)
            .map(|river| river.points.as_slice()),
        EditorTool::Waterfall => water
            .waterfalls
            .get(editor.selected_waterfall)
            .map(|waterfall| waterfall.points.as_slice()),
        _ => None,
    }
}

/// 返回当前工具选中的控制点可变向量。
pub(super) fn active_points_mut<'a>(
    water: &'a mut MapWaterAsset,
    editor: &MapEditorState,
) -> Option<&'a mut Vec<MapWaterSplinePoint>> {
    match editor.tool {
        EditorTool::RiverSpline => water
            .rivers
            .get_mut(editor.selected_water_river)
            .map(|river| &mut river.points),
        EditorTool::Waterfall => water
            .waterfalls
            .get_mut(editor.selected_waterfall)
            .map(|waterfall| &mut waterfall.points),
        _ => None,
    }
}

/// 返回当前特征稳定ID和瀑布标记，供跨帧拖动冻结身份。
pub(super) fn active_feature_identity(
    water: &MapWaterAsset,
    editor: &MapEditorState,
) -> Option<(String, bool)> {
    match editor.tool {
        EditorTool::RiverSpline => water
            .rivers
            .get(editor.selected_water_river)
            .map(|river| (river.id.clone(), false)),
        EditorTool::Waterfall => water
            .waterfalls
            .get(editor.selected_waterfall)
            .map(|waterfall| (waterfall.id.clone(), true)),
        _ => None,
    }
}

/// 按稳定特征和点ID读取位置，避免拖动松手时旧数组索引误指另一点。
pub(super) fn point_position_by_id(
    water: &MapWaterAsset,
    feature_id: &str,
    point_id: &str,
    waterfall: bool,
) -> Option<[f32; 3]> {
    let points = if waterfall {
        &water
            .waterfalls
            .iter()
            .find(|item| item.id == feature_id)?
            .points
    } else {
        &water
            .rivers
            .iter()
            .find(|item| item.id == feature_id)?
            .points
    };
    points
        .iter()
        .find(|point| point.id == point_id)
        .map(|point| point.position)
}

/// 按稳定特征和点ID修改控制点，供跨帧拖动拒绝数组重排。
pub(super) fn point_mut_by_id<'a>(
    water: &'a mut MapWaterAsset,
    feature_id: &str,
    point_id: &str,
    waterfall: bool,
) -> Option<&'a mut MapWaterSplinePoint> {
    let points = if waterfall {
        &mut water
            .waterfalls
            .iter_mut()
            .find(|item| item.id == feature_id)?
            .points
    } else {
        &mut water
            .rivers
            .iter_mut()
            .find(|item| item.id == feature_id)?
            .points
    };
    points.iter_mut().find(|point| point.id == point_id)
}

/// 把约十四个视口像素换算为世界XZ命中半径。
pub(super) fn point_pick_radius(viewport: &MapEditorViewportState) -> f32 {
    let height = viewport.render_size.y.max(1) as f32; // RTT未完成尺寸同步时仍避免除零。
    (viewport.extent / height * POINT_PICK_RADIUS_PIXELS).clamp(0.25, 12.0) // Gizmo世界半径与屏幕命中规格保持同源。
}

/// 在当前路径内按实际投影像素查找最近控制点；距离相同时保持数组顺序确定性。
pub(super) fn nearest_projected_point(
    points: &[MapWaterSplinePoint],
    cursor: Vec2,
    radius_pixels: f32,
    mut project: impl FnMut(Vec3) -> Option<Vec2>,
) -> Option<usize> {
    if !cursor.is_finite() || !radius_pixels.is_finite() || radius_pixels < 0.0 {
        return None; // 非有限投影输入不得把任意三维点误判为命中。
    }
    points
        .iter()
        .enumerate()
        .filter_map(|(index, point)| {
            let position = project(Vec3::from_array(point.position))?; // 相机负责拒绝视锥后方或不可投影的三维点。
            let distance = position.distance(cursor); // 透视与正交都按同一十四像素交互半径比较。
            (distance.is_finite() && distance <= radius_pixels).then_some((index, distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
}

/// 求鼠标世界射线与指定Y水平面的前向交点；拖动据此保持控制点始终位于自身高度。
pub(super) fn horizontal_plane_intersection(
    ray_origin: Vec3,
    ray_direction: Vec3,
    plane_y: f32,
) -> Option<Vec3> {
    if !ray_origin.is_finite()
        || !ray_direction.is_finite()
        || !plane_y.is_finite()
        || ray_direction.y.abs() <= f32::EPSILON
    {
        return None; // 平行射线、NaN或无穷输入没有稳定的水平面交点。
    }
    let distance = (plane_y - ray_origin.y) / ray_direction.y;
    if !distance.is_finite() || distance < 0.0 {
        return None; // 相机背后的交点不能用于直接操控。
    }
    let point = ray_origin + ray_direction * distance;
    point.is_finite().then_some(point) // 只发布有限世界坐标，避免污染作者快照。
}

/// 编辑期校验允许空路径、单点路径和未连接瀑布，但拒绝坏身份、坏引用、越界预算和NaN。
pub(crate) fn validate_intermediate_water(
    water: &MapWaterAsset,
    expected_map_id: &MapId,
) -> Result<(), String> {
    if &water.map_id != expected_map_id {
        return Err("水网草稿地图身份已变化".to_owned());
    }
    let mut feature_ids = BTreeSet::new();
    for layer in &water.layers {
        if layer.id.is_empty() || !feature_ids.insert(layer.id.as_str()) {
            return Err("静水、河流与瀑布稳定ID必须非空且全局唯一".to_owned());
        }
    }
    for river in &water.rivers {
        if river.id.is_empty() || !feature_ids.insert(river.id.as_str()) {
            return Err("静水、河流与瀑布稳定ID必须非空且全局唯一".to_owned());
        }
        if !valid_water_material(river.base_color_srgba, river.perceptual_roughness) {
            return Err(format!("河流{}包含无效材质参数", river.label));
        }
        for layer_id in [
            river.source_layer_id.as_ref(),
            river.target_layer_id.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if !water.layers.iter().any(|layer| &layer.id == layer_id) {
                return Err(format!("河流{}引用了不存在的静水体", river.label));
            }
        }
        validate_points(&river.points, &river.label)?;
    }
    for waterfall in &water.waterfalls {
        if waterfall.id.is_empty() || !feature_ids.insert(waterfall.id.as_str()) {
            return Err("静水、河流与瀑布稳定ID必须非空且全局唯一".to_owned());
        }
        if !waterfall.foam_strength.is_finite()
            || !(0.0..=1.0).contains(&waterfall.foam_strength)
            || !valid_water_material(waterfall.base_color_srgba, waterfall.perceptual_roughness)
        {
            return Err(format!("瀑布{}包含无效泡沫或材质参数", waterfall.label));
        }
        for endpoint in [waterfall.source.as_ref(), waterfall.target.as_ref()]
            .into_iter()
            .flatten()
        {
            if endpoint_height(water, endpoint).is_none() {
                return Err(format!(
                    "瀑布{}引用了不存在或尚无控制点的端点",
                    waterfall.label
                ));
            }
        }
        validate_points(&waterfall.points, &waterfall.label)?;
    }
    Ok(())
}

/// 校验标准水材质颜色和粗糙度范围。
fn valid_water_material(color: [f32; 4], roughness: f32) -> bool {
    color
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        && roughness.is_finite()
        && (0.0..=1.0).contains(&roughness)
}

/// 校验单条中间路径控制点身份和数值范围，不要求正式最少点数或相邻段长度。
fn validate_points(points: &[MapWaterSplinePoint], label: &str) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for point in points {
        if point.id.is_empty() || !ids.insert(point.id.as_str()) {
            return Err(format!("{label}包含空白或重复控制点ID"));
        }
        if !point.position.iter().all(|value| value.is_finite())
            || !point.width.is_finite()
            || !(MIN_MAP_WATER_SPLINE_WIDTH_METERS..=MAX_MAP_WATER_SPLINE_WIDTH_METERS)
                .contains(&point.width)
            || !point.depth.is_finite()
            || !(MIN_MAP_WATER_SPLINE_DEPTH_METERS..=MAX_MAP_WATER_SPLINE_DEPTH_METERS)
                .contains(&point.depth)
            || !point.flow_speed.is_finite()
            || !(0.0..=MAX_MAP_WATER_FLOW_SPEED).contains(&point.flow_speed)
        {
            return Err(format!("{label}包含无效控制点位置、宽度、深度或流速"));
        }
    }
    Ok(())
}
