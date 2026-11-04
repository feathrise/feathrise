//! 水网面板选择、结构、参数和端点连接动作。

use bevy::prelude::*; // ECS资源与有限浮点钳制。
use feathrise_map_content::{
    DEFAULT_MAP_WATER_PERCEPTUAL_ROUGHNESS, MapWaterEndpoint, MapWaterRiver, MapWaterSegmentKind, MapWaterfall,
}; // 连续水网正式类型与统一数值范围。

use super::connections::{
    endpoint_references_river, sync_water_network_connections, waterfall_endpoint_choices,
}; // 结构动作只通过唯一连接模块清理引用并同步端点。
use super::helpers::{
    active_points, active_points_mut, allocate_water_id, commit_water_change,
    validate_intermediate_water,
}; // 一帧动作只组合共享纯函数与原子事务。
use crate::authoring::resources::{EditorAuthoringSession, WaterNetworkEditState}; // 当前草稿、撤销栈和拖动互斥门。
use crate::resources::MapEditorState; // 当前河流、瀑布、点选择和一帧请求。
use crate::types::{EditorTool, WaterNetworkAction}; // 工具决定动作操作河流还是瀑布。

/// 返回当前静水层的颜色、粗糙度与稳定ID，空静水地图使用统一水材质默认值。
fn selected_still_water_style(
    water: &feathrise_map_content::MapWaterAsset,
    selected_layer: usize,
) -> ([f32; 4], f32, Option<String>) {
    water.layers.get(selected_layer).map_or(
        (
            [0.055, 0.24, 0.29, 0.72],
            DEFAULT_MAP_WATER_PERCEPTUAL_ROUGHNESS,
            None,
        ), // 空静水图创建河流或瀑布时直接采用清晰天空反射默认值。
        |layer| {
            (
                layer.base_color_srgba,
                layer.perceptual_roughness,
                Some(layer.id.clone()),
            )
        },
    )
}

/// 从固定选项中循环到下一项；只跳过与另一端相同的真实连接，`None`始终允许显式断开。
fn next_distinct_choice<T: Clone + PartialEq>(
    choices: &[Option<T>],
    current: &Option<T>,
    other: &Option<T>,
) -> Option<T> {
    if choices.is_empty() {
        return None; // 调用方正常总提供断开项，空集合仍保持安全无连接。
    }
    let index = choices.iter().position(|choice| choice == current);
    let mut candidate = choices[index.map_or(0, |index| (index + 1) % choices.len())].clone();
    for _ in 0..choices.len() {
        if candidate.is_none() || &candidate != other {
            return candidate; // 两端都未连接是合法中间态，只有相同Some端点需要跳过。
        }
        let index = choices
            .iter()
            .position(|choice| choice == &candidate)
            .unwrap_or(0);
        candidate = choices[(index + 1) % choices.len()].clone();
    }
    candidate // 只有冲突真实端点时保留有界循环结果，调用方的无变化门不会压空撤销。
}

/// 循环可选静水层稳定ID；河流冻结格式只允许静水源和静水目标。
fn next_still_layer_id(
    water: &feathrise_map_content::MapWaterAsset,
    current: &Option<String>,
    other: &Option<String>,
) -> Option<String> {
    let mut choices = vec![None]; // 第一项明确断开河流与静水体的连接。
    choices.extend(water.layers.iter().map(|layer| Some(layer.id.clone())));
    next_distinct_choice(&choices, current, other) // 河流与瀑布共用完全相同的None和冲突Some语义。
}

/// 消费水网面板的一帧动作；选择不制造撤销，真实定义变化全部原子提交。
pub(crate) fn apply_water_network_actions(
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    edit: Res<WaterNetworkEditState>,
) {
    let Some(action) = editor.water_network_action_requested.take() else {
        return; // 稳定帧不克隆水域文档。
    };
    if session
        .draft
        .as_ref()
        .is_some_and(|draft| !draft.terrain_manifest().terrain.kind.supports_spline_water())
    {
        editor.status = "当前地形类型不支持河流、急流与瀑布编辑".to_owned();
        return; // 一帧请求即使绕过UI也不能修改只读样条数据。
    }
    if edit.dragging() {
        editor.status = "请先松开正在拖动的水网控制点".to_owned();
        return; // 拖动中间态不得与按钮事务并发。
    }
    if session.saving {
        editor.status = "地图保存期间暂不接受水网修改".to_owned();
        return; // 保存快照与作者定义不能并发分叉。
    }
    let Some(before) = session.draft.as_ref().and_then(|draft| draft.water.clone()) else {
        editor.status = "当前地图没有可编辑水域草稿".to_owned();
        return;
    };

    match action {
        WaterNetworkAction::PointPrevious => {
            let Some(points) = active_points(&before, &editor) else {
                editor.status = "请先选择河流或瀑布".to_owned();
                return;
            };
            if points.is_empty() {
                editor.status = "当前路径还没有控制点".to_owned();
                return;
            }
            let current = editor
                .selected_water_control_point
                .unwrap_or(points.len())
                .min(points.len());
            editor.selected_water_control_point = Some(if current == 0 {
                points.len() - 1
            } else {
                current - 1
            });
            editor.status = format!(
                "已选择第{}个水网控制点",
                editor.selected_water_control_point.unwrap_or(0) + 1
            );
            return; // 点选择同样不修改作者定义。
        }
        _ => {} // 其余动作继续构造完整前后快照。
    }

    let map_id = before.map_id.clone(); // 校验身份在可变会话借用前冻结。
    let mut after = before.clone();
    let status = match action {
        WaterNetworkAction::RiverAdd => {
            let id = allocate_water_id(&mut session, &after, "water-river");
            let (color, roughness, source_layer_id) =
                selected_still_water_style(&after, editor.selected_water_layer);
            let number = after.rivers.len() + 1;
            after.rivers.push(MapWaterRiver {
                id,
                label: format!("河流_{number}"),
                source_layer_id,
                target_layer_id: None,
                points: Vec::new(),
                base_color_srgba: color,
                perceptual_roughness: roughness,
            });
            editor.selected_water_river = after.rivers.len() - 1;
            editor.selected_water_control_point = None;
            editor.tool = EditorTool::RiverSpline;
            "已新建空河流；请在顶视或透视中从上游到下游逐点点击".to_owned()
        }
        WaterNetworkAction::RiverDelete => {
            if after.rivers.is_empty() {
                editor.status = "当前没有可删除河流".to_owned();
                return;
            }
            let index = editor.selected_water_river.min(after.rivers.len() - 1);
            let removed = after.rivers.remove(index);
            for waterfall in &mut after.waterfalls {
                if endpoint_references_river(&waterfall.source, &removed.id) {
                    waterfall.source = None; // 删除河流必须同次清理瀑布上游悬空引用。
                }
                if endpoint_references_river(&waterfall.target, &removed.id) {
                    waterfall.target = None; // 下游引用也在同一撤销快照内清理。
                }
            }
            editor.selected_water_river = index.min(after.rivers.len().saturating_sub(1));
            editor.selected_water_control_point = None;
            format!("已删除河流“{}”并断开相关瀑布连接", removed.label)
        }
        WaterNetworkAction::WaterfallAdd => {
            let id = allocate_water_id(&mut session, &after, "waterfall");
            let (color, roughness, source_layer_id) =
                selected_still_water_style(&after, editor.selected_water_layer);
            let number = after.waterfalls.len() + 1;
            after.waterfalls.push(MapWaterfall {
                id,
                label: format!("瀑布_{number}"),
                source: source_layer_id.map(|layer_id| MapWaterEndpoint::StillWater { layer_id }),
                target: None,
                points: Vec::new(),
                foam_strength: 1.0,
                base_color_srgba: color,
                perceptual_roughness: roughness,
            });
            editor.selected_waterfall = after.waterfalls.len() - 1;
            editor.selected_water_control_point = None;
            editor.tool = EditorTool::Waterfall;
            "已新建瀑布；请从瀑布口到落水区至少绘制两个控制点".to_owned()
        }
        WaterNetworkAction::WaterfallDelete => {
            if after.waterfalls.is_empty() {
                editor.status = "当前没有可删除瀑布".to_owned();
                return;
            }
            let index = editor.selected_waterfall.min(after.waterfalls.len() - 1);
            let removed = after.waterfalls.remove(index);
            editor.selected_waterfall = index.min(after.waterfalls.len().saturating_sub(1));
            editor.selected_water_control_point = None;
            format!("已删除瀑布“{}”", removed.label)
        }
        WaterNetworkAction::PointDelete => {
            let selected = editor.selected_water_control_point;
            let Some(points) = active_points_mut(&mut after, &editor) else {
                editor.status = "请先选择河流或瀑布".to_owned();
                return;
            };
            let Some(index) = selected.filter(|index| *index < points.len()) else {
                editor.status = "请先在视口或“上一控制点”选择一个控制点".to_owned();
                return;
            };
            points.remove(index);
            editor.selected_water_control_point =
                (!points.is_empty()).then_some(index.min(points.len().saturating_sub(1)));
            format!("已删除第{}个水网控制点", index + 1)
        }
        WaterNetworkAction::ToggleSegmentKind => {
            let selected = editor.selected_water_control_point;
            let Some(points) = active_points_mut(&mut after, &editor) else {
                editor.status = "请先选择河流或瀑布".to_owned();
                return;
            };
            let Some(point) = selected.and_then(|index| points.get_mut(index)) else {
                editor.status = "请先选择一个水网控制点".to_owned();
                return;
            };
            point.outgoing = match point.outgoing {
                MapWaterSegmentKind::River => MapWaterSegmentKind::Rapid,
                MapWaterSegmentKind::Rapid => MapWaterSegmentKind::River,
            }; // 离散段类型继续使用按钮，四项连续参数已由公共专业字段处理。
            format!(
                "控制点参数：Y {:.2}m · 宽 {:.2}m · 深 {:.2}m · 速 {:.2}m/s",
                point.position[1], point.width, point.depth, point.flow_speed
            )
        }
        WaterNetworkAction::SourceNext | WaterNetworkAction::TargetNext => {
            let source = action == WaterNetworkAction::SourceNext;
            match editor.tool {
                EditorTool::RiverSpline => {
                    let index = editor
                        .selected_water_river
                        .min(after.rivers.len().saturating_sub(1));
                    let Some(river) = after.rivers.get(index) else {
                        editor.status = "当前没有可连接的河流".to_owned();
                        return;
                    };
                    let (current, other) = if source {
                        (&river.source_layer_id, &river.target_layer_id)
                    } else {
                        (&river.target_layer_id, &river.source_layer_id)
                    };
                    let next = next_still_layer_id(&after, current, other);
                    let river = &mut after.rivers[index];
                    if source {
                        river.source_layer_id = next.clone();
                    } else {
                        river.target_layer_id = next.clone();
                    }
                    let label = next
                        .as_ref()
                        .and_then(|layer_id| {
                            after
                                .layers
                                .iter()
                                .find(|layer| &layer.id == layer_id)
                                .map(|layer| layer.label.as_str())
                        })
                        .unwrap_or("未连接");
                    format!("河流{}连接：{label}", if source { "源" } else { "目标" })
                }
                EditorTool::Waterfall => {
                    let index = editor
                        .selected_waterfall
                        .min(after.waterfalls.len().saturating_sub(1));
                    let Some(waterfall) = after.waterfalls.get(index) else {
                        editor.status = "当前没有可连接的瀑布".to_owned();
                        return;
                    };
                    let (current, other) = if source {
                        (&waterfall.source, &waterfall.target)
                    } else {
                        (&waterfall.target, &waterfall.source)
                    };
                    let choices = waterfall_endpoint_choices(&after);
                    let next = next_distinct_choice(&choices, current, other);
                    let waterfall = &mut after.waterfalls[index];
                    if source {
                        waterfall.source = next;
                    } else {
                        waterfall.target = next;
                    }
                    format!(
                        "瀑布{}连接已循环并吸附端点高度",
                        if source { "源" } else { "目标" }
                    )
                }
                _ => {
                    editor.status = "请先选择河流样条或瀑布工具".to_owned();
                    return;
                }
            }
        }
        WaterNetworkAction::PointPrevious => unreachable!("纯选择动作已提前返回"),
    };

    sync_water_network_connections(&mut after); // 参数、增删点或端点变化同事务维护静水/河流/瀑布几何连续。
    if let Err(error) = validate_intermediate_water(&after, &map_id) {
        editor.status = format!("水网修改未采用：{error}");
        return; // 宽松校验仍失败时保持原草稿和撤销栈原样。
    }
    commit_water_change(&mut session, &mut editor, before, after, status);
}

#[cfg(test)]
mod tests {
    use super::*; // 端点循环是纯逻辑，不启动Bevy App或资产系统。

    #[test]
    fn river_endpoint_can_disconnect_while_the_other_end_is_none() {
        let choices = vec![None, Some("lake".to_owned())]; // 单静水地图仍必须能从连接回到断开。
        let current = Some("lake".to_owned());
        assert_eq!(next_distinct_choice(&choices, &current, &None), None);
    }

    #[test]
    fn waterfall_endpoint_can_disconnect_while_the_other_end_is_none() {
        let endpoint = MapWaterEndpoint::RiverEnd {
            river_id: "river".to_owned(), // 使用河尾证明瀑布全部端点类别共享断开语义。
        };
        let choices = vec![None, Some(endpoint.clone())];
        assert_eq!(next_distinct_choice(&choices, &Some(endpoint), &None), None);
    }

    #[test]
    fn endpoint_cycle_skips_only_an_equal_connected_some_value() {
        let choices = vec![None, Some("upper".to_owned()), Some("lower".to_owned())];
        let next = next_distinct_choice(&choices, &None, &Some("upper".to_owned()));
        assert_eq!(next.as_deref(), Some("lower")); // 相同真实端点被跳过，但None仍保留在循环中。
    }
}
