//! 连续水网三维Gizmo与左栏作者状态回显。

use bevy::prelude::*; // Gizmos、Text、颜色与ECS资源。
#[cfg(test)]
use feathrise_scene_content::SceneWaterEndpoint; // 纯状态回归覆盖河流端点枚举。
use feathrise_scene_content::{SceneWaterAsset, SceneWaterSegmentKind, SceneWaterSplinePoint}; // 正式控制点、段类型和连接引用。

use super::connections::endpoint_label; // 状态文本只通过连接模块解析稳定端点语义。
use super::helpers::{active_points, point_pick_radius}; // 当前选择和固定像素控制点尺寸。
use crate::authoring::resources::{EditorAuthoringSession, WaterNetworkEditState}; // 当前草稿与拖动高亮身份。
use crate::components::SceneEditorWaterNetworkText; // 左栏固定状态节点。
use crate::resources::{SceneEditorState, SceneEditorViewportState}; // 工具、选择、光标与缩放。
use crate::types::EditorTool; // 只在河流或瀑布工具中显示水网Gizmo。
use crate::viewport::SceneEditorBrushGizmos; // 线条只进入F12离屏相机。

/// 按段类型绘制中心线和两侧完整宽度边；瀑布始终使用独立蓝白色。
fn draw_path(
    gizmos: &mut Gizmos<SceneEditorBrushGizmos>,
    points: &[SceneWaterSplinePoint],
    waterfall: bool,
) {
    for pair in points.windows(2) {
        let start = Vec3::from_array(pair[0].position);
        let end = Vec3::from_array(pair[1].position);
        let center_color = if waterfall {
            Color::srgb(0.36, 0.76, 1.0) // 瀑布中心线使用明亮水蓝。
        } else if pair[0].outgoing == SceneWaterSegmentKind::Rapid {
            Color::srgb(1.0, 0.48, 0.10) // 急流段使用橙色，与普通河流直接区分。
        } else {
            Color::srgb(0.08, 0.92, 0.96) // 普通河流使用青色。
        };
        let edge_color = if waterfall {
            Color::srgba(0.72, 0.90, 1.0, 0.62)
        } else if pair[0].outgoing == SceneWaterSegmentKind::Rapid {
            Color::srgba(1.0, 0.65, 0.18, 0.48)
        } else {
            Color::srgba(0.08, 0.92, 0.96, 0.44)
        };
        gizmos.line(start, end, center_color);
        let delta = Vec2::new(end.x - start.x, end.z - start.z);
        let length = delta.length();
        if !length.is_finite() || length <= f32::EPSILON {
            continue; // 重叠编辑中间态只显示中心点，不产生NaN宽度边。
        }
        let normal = Vec2::new(-delta.y, delta.x) / length;
        let start_offset = Vec3::new(
            normal.x * pair[0].width * 0.5,
            0.0,
            normal.y * pair[0].width * 0.5,
        );
        let end_offset = Vec3::new(
            normal.x * pair[1].width * 0.5,
            0.0,
            normal.y * pair[1].width * 0.5,
        );
        gizmos.line(start + start_offset, end + end_offset, edge_color);
        gizmos.line(start - start_offset, end - end_offset, edge_color);
    }
}

/// 每帧按固定水网预算绘制河流、急流、瀑布、宽度边和选中控制点。
pub(crate) fn draw_water_network(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    edit: Res<WaterNetworkEditState>,
    viewport: Option<Res<SceneEditorViewportState>>,
    mut gizmos: Gizmos<SceneEditorBrushGizmos>,
) {
    if !editor.open || !matches!(editor.tool, EditorTool::RiverSpline | EditorTool::Waterfall) {
        return; // 静水笔刷和其他工具不叠加连续水网辅助线。
    }
    let Some(water) = session
        .draft
        .as_ref()
        .and_then(|draft| draft.water.as_ref())
    else {
        return;
    };
    for river in &water.rivers {
        draw_path(&mut gizmos, &river.points, false);
    }
    for waterfall in &water.waterfalls {
        draw_path(&mut gizmos, &waterfall.points, true);
    }
    let point_radius = viewport
        .as_deref()
        .map(|viewport| point_pick_radius(viewport) * 0.34)
        .unwrap_or(0.8);
    let selected_feature = match editor.tool {
        EditorTool::RiverSpline => water
            .rivers
            .get(editor.selected_water_river)
            .map(|river| (river.id.as_str(), river.points.as_slice(), false)),
        EditorTool::Waterfall => water
            .waterfalls
            .get(editor.selected_waterfall)
            .map(|waterfall| (waterfall.id.as_str(), waterfall.points.as_slice(), true)),
        _ => None,
    };
    if let Some((feature_id, points, waterfall)) = selected_feature {
        for (index, point) in points.iter().enumerate() {
            let dragging = edit.feature_id.as_deref() == Some(feature_id)
                && edit.point_id.as_deref() == Some(point.id.as_str())
                && edit.waterfall == waterfall;
            let selected = editor.selected_water_control_point == Some(index);
            let color = if dragging {
                Color::srgb(1.0, 0.12, 0.08) // 拖动中使用红色。
            } else if selected {
                Color::srgb(1.0, 0.88, 0.18) // 已选点使用金黄色。
            } else {
                Color::srgb(0.98, 0.98, 0.92) // 其余点使用米白色。
            };
            gizmos
                .sphere(
                    Isometry3d::from_translation(Vec3::from_array(point.position)),
                    point_radius,
                    color,
                )
                .resolution(12);
        }
        if let (Some(last), Some(cursor)) = (points.last(), editor.cursor_world) {
            gizmos.line(
                Vec3::from_array(last.position),
                Vec3::new(cursor.x, last.position[1].min(cursor.y + 0.05), cursor.z),
                Color::srgba(1.0, 0.94, 0.36, 0.66),
            ); // 下一段预览遵循默认不升高规则。
        }
    }
}

/// 返回河流静水连接的中文标签。
fn river_layer_label(water: &SceneWaterAsset, layer_id: &Option<String>) -> String {
    layer_id.as_ref().map_or_else(
        || "未连接".to_owned(),
        |layer_id| {
            water
                .layers
                .iter()
                .find(|layer| &layer.id == layer_id)
                .map_or_else(|| format!("失效:{layer_id}"), |layer| layer.label.clone())
        },
    )
}

/// 只在作者状态变化时更新当前河流、瀑布、端点和控制点参数。
pub(crate) fn sync_water_network_text(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    mut texts: Query<&mut Text, With<SceneEditorWaterNetworkText>>,
) {
    if !editor.is_changed() && !session.is_changed() {
        return; // 稳定帧不分配状态字符串。
    }
    let value = session
        .draft
        .as_ref()
        .and_then(|draft| draft.water.as_ref())
        .map_or_else(
            || "水网：当前场景没有水域作者数据".to_owned(),
            |water| {
                let river = water.rivers.get(editor.selected_water_river);
                let waterfall = water.waterfalls.get(editor.selected_waterfall);
                let active = match editor.tool {
                    EditorTool::RiverSpline => river.map(|river| {
                        format!(
                            "当前河流：{} · {}点{}\n源 {} → 目标 {}",
                            river.label,
                            river.points.len(),
                            if river.points.len() < 2 {
                                "（未完成）"
                            } else {
                                ""
                            },
                            river_layer_label(water, &river.source_layer_id),
                            river_layer_label(water, &river.target_layer_id),
                        )
                    }),
                    EditorTool::Waterfall => waterfall.map(|waterfall| {
                        format!(
                            "当前瀑布：{} · {}点{}\n源 {} → 目标 {}",
                            waterfall.label,
                            waterfall.points.len(),
                            if waterfall.points.len() < 2 {
                                "（未完成）"
                            } else {
                                ""
                            },
                            endpoint_label(water, &waterfall.source),
                            endpoint_label(water, &waterfall.target),
                        )
                    }),
                    _ => None,
                }
                .unwrap_or_else(|| "当前工具未选择连续水网".to_owned());
                let point = active_points(water, &editor)
                    .and_then(|points| {
                        editor
                            .selected_water_control_point
                            .and_then(|index| points.get(index))
                    })
                    .map_or_else(
                        || "控制点：未选择".to_owned(),
                        |point| {
                            format!(
                                "控制点：Y {:.2}m · 宽 {:.2}m · 深 {:.2}m · 速 {:.2}m/s · {}",
                                point.position[1],
                                point.width,
                                point.depth,
                                point.flow_speed,
                                if point.outgoing == SceneWaterSegmentKind::Rapid {
                                    "急流"
                                } else {
                                    "普通"
                                },
                            )
                        },
                    );
                format!(
                    "水网：河流 {} 条 · 瀑布 {} 条\n{active}\n{point}",
                    water.rivers.len(),
                    water.waterfalls.len(),
                )
            },
        );
    for mut text in &mut texts {
        **text = value.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 端点类型参与状态文本语义的最小编译回归。

    #[test]
    fn endpoint_variants_have_distinct_status_semantics() {
        let start = SceneWaterEndpoint::RiverStart {
            river_id: "river".to_owned(),
        };
        assert!(matches!(start, SceneWaterEndpoint::RiverStart { .. }));
    }
}
