//! 顶视/透视河流与瀑布逐点绘制和控制点拖动事务。

use bevy::prelude::*; // 鼠标输入、ECS资源与世界坐标。
use bevy::ui::RelativeCursorPosition; // 左键只在中央离屏视口内编辑水网。
#[cfg(test)]
use feathrise_map_content::MapWaterEndpoint; // 纯类型回归锁定端点仍可供首点吸附工作流使用。
use feathrise_map_content::{
    MIN_MAP_WATER_SPLINE_SEGMENT_METERS, MapWaterSegmentKind, MapWaterSplinePoint,
}; // 正式点数预算、最短段与冻结数据结构。
use feathrise_scene::ClipmapGroundHit; // Clipmap新增点只消费所选对象已采用的真实三角面。

use super::connections::{endpoint_height, sync_water_network_connections}; // 首点和每帧拖动共用唯一端点连续性派生。
use super::helpers::{
    DEFAULT_POINT_DEPTH, DEFAULT_POINT_SPEED, DEFAULT_POINT_WIDTH, POINT_PICK_RADIUS_PIXELS,
    active_feature_identity, active_points, active_points_mut, allocate_water_id,
    commit_water_change, horizontal_plane_intersection, nearest_projected_point, point_mut_by_id,
    point_position_by_id, validate_intermediate_water,
}; // 视口编辑复用稳定ID、命中、默认参数和原子事务。
use crate::authoring::resources::{EditorAuthoringSession, EditorCommand, WaterNetworkEditState}; // 当前水域草稿、统一撤销栈和跨帧拖动身份。
use crate::components::{MapEditorCamera, MapEditorViewport}; // 独立相机提供真实屏幕投影和鼠标世界射线。
use crate::resources::{MapEditorState, MapEditorViewportState}; // 工具、光标、选择与视口缩放。
use crate::types::{EditorTool, EditorView}; // 连续水网只在独立工具及顶视/透视中消费左键。

/// 返回新控制点Y：首点优先吸附源连接，其余点默认不高于上一点。
fn next_point_height(
    water: &feathrise_map_content::MapWaterAsset,
    editor: &MapEditorState,
    cursor_y: f32,
) -> f32 {
    if let Some(previous) = active_points(water, editor).and_then(|points| points.last()) {
        return (cursor_y + 0.05).min(previous.position[1]); // 默认略高于命中地形，同时沿绘制方向只平走或下降。
    }
    let source_height = match editor.tool {
        EditorTool::RiverSpline => water
            .rivers
            .get(editor.selected_water_river)
            .and_then(|river| river.source_layer_id.as_ref())
            .and_then(|layer_id| water.layers.iter().find(|layer| &layer.id == layer_id))
            .map(|layer| layer.water_level),
        EditorTool::Waterfall => water
            .waterfalls
            .get(editor.selected_waterfall)
            .and_then(|waterfall| waterfall.source.as_ref())
            .and_then(|endpoint| endpoint_height(water, endpoint)),
        _ => None,
    };
    source_height.unwrap_or(cursor_y + 0.05) // 无源连接时略高于当前地形，避免共面。
}

/// 拖动事务不依赖`just_released`边沿；只要按钮最终未按住就必须在本帧收口。
fn should_finish_drag(dragging: bool, left_pressed: bool) -> bool {
    dragging && !left_pressed // 同帧按下再释放时ButtonInput可能只有最终未按住状态。
}

/// 结束一次控制点拖动；实时修订已逐帧推进，松手只登记一条完整快照撤销。
fn finish_drag(
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    edit: &mut WaterNetworkEditState,
) {
    let Some(before) = edit.before.take() else {
        edit.clear_drag();
        return; // 缺少按下快照时不能构造半套撤销。
    };
    let feature_id = edit.feature_id.clone().unwrap_or_default();
    let point_id = edit.point_id.clone().unwrap_or_default();
    let waterfall = edit.waterfall;
    let moved = edit.moved;
    let expected_revision = edit.expected_revision;
    let dirty_before = edit.dirty_before;
    let map_matches = session
        .draft
        .as_ref()
        .is_some_and(|draft| Some(&draft.map_id) == edit.map_id.as_ref());
    let after = session.draft.as_ref().and_then(|draft| draft.water.clone());
    edit.clear_drag(); // 先释放输入互斥门，后续错误同样回到可编辑状态。
    if !map_matches || session.revision != expected_revision {
        editor.status = "水网拖动因地图或作者修订变化而取消".to_owned();
        return; // 并发变化存在时不得用旧完整快照覆盖新作者分支。
    }
    let Some(after) = after else {
        return; // 水域文档被移除时迟到松手无副作用。
    };
    let before_position = point_position_by_id(&before, &feature_id, &point_id, waterfall);
    let after_position = point_position_by_id(&after, &feature_id, &point_id, waterfall);
    if !moved || before_position == after_position {
        editor.dirty = dirty_before; // 拖回原点不制造保存要求或空撤销记录。
        editor.status = "水网控制点位置未改变".to_owned();
        return;
    }
    if let Err(error) = validate_intermediate_water(&after, &after.map_id) {
        if let Some(draft) = session.draft.as_mut() {
            draft.water = Some(before); // 非法完成值原子恢复拖动前完整定义。
        }
        session.mark_water_definition_changed(editor); // 恢复动作同样刷新可能已启动的实时预览。
        editor.status = format!("水网控制点位置无效，已恢复：{error}");
        return;
    }
    session.push_bounded_water_history(EditorCommand::WaterDefinitionsChanged { before, after }); // 整段逐帧拖动只占一条受限撤销记录。
    editor.status = "水网控制点XZ已移动；高度Y保持不变".to_owned();
}

/// 把一个已经通过当前地形后端验证的世界落点追加到当前河流或瀑布。
fn append_water_network_point(
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    cursor: Vec3,
) {
    let Some(before) = session.draft.as_ref().and_then(|draft| draft.water.clone()) else {
        editor.status = "当前地图没有可编辑水域草稿".to_owned();
        return;
    };
    let Some((_feature_id, _waterfall)) = active_feature_identity(&before, editor) else {
        editor.status = if editor.tool == EditorTool::RiverSpline {
            "请先点击“新建河流”".to_owned()
        } else {
            "请先点击“新建瀑布”".to_owned()
        };
        return;
    };
    let points = active_points(&before, editor).unwrap_or_default();
    let y = next_point_height(&before, editor, cursor.y);
    let candidate = Vec3::new(cursor.x, y, cursor.z); // 高度来自真实Clipmap三角或GLB命中，统一只加五厘米防共面。
    if points.last().is_some_and(|point| {
        Vec3::from_array(point.position).distance(candidate) < MIN_MAP_WATER_SPLINE_SEGMENT_METERS
    }) {
        editor.status =
            format!("新控制点必须与上一点相距至少{MIN_MAP_WATER_SPLINE_SEGMENT_METERS:.2}米");
        return;
    }
    let id = allocate_water_id(session, &before, "water-point");
    let mut after = before.clone();
    let count = {
        let Some(points) = active_points_mut(&mut after, editor) else {
            return;
        };
        points.push(MapWaterSplinePoint {
            id,
            position: candidate.to_array(),
            width: DEFAULT_POINT_WIDTH,
            depth: DEFAULT_POINT_DEPTH,
            flow_speed: DEFAULT_POINT_SPEED,
            outgoing: MapWaterSegmentKind::River,
        });
        points.len()
    };
    sync_water_network_connections(&mut after);
    if let Err(error) = validate_intermediate_water(&after, &after.map_id) {
        editor.status = format!("水网控制点未添加：{error}");
        return;
    }
    editor.selected_water_control_point = Some(count - 1);
    let guidance = if editor.tool == EditorTool::Waterfall && count < 2 {
        "；瀑布正式保存至少需要两个点"
    } else {
        ""
    };
    commit_water_change(
        session,
        editor,
        before,
        after,
        format!("已添加第{count}个水网控制点{guidance}"),
    );
}

/// 在选中路径上添加控制点，或按住已有控制点进行只改XZ的可撤销拖动。
pub(crate) fn edit_water_network_from_viewport(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>, // Ctrl+左键吸取完整水域对象，不得新增或拖动控制点。
    viewport_node: Query<&RelativeCursorPosition, With<MapEditorViewport>>,
    camera_query: Query<(&Camera, &Transform), With<MapEditorCamera>>,
    viewport: Option<Res<MapEditorViewportState>>,
    surface: Option<feathrise_scene::TerrainObjectClipmapGround>, // Clipmap异步点击只接受当前已采用真实三角面。
    mut ground: Option<ResMut<crate::viewport::VegetationGroundCursor>>, // 唯一single-flight查询保存冻结点击。
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut edit: ResMut<WaterNetworkEditState>,
    objects_ground: Option<Res<crate::authoring::terrain_ground::TerrainGroundCursor>>,
    mut pending_object: Local<Option<super::types::ObjectWaterPointClick>>,
) {
    let current_map_id = session.draft.as_ref().map(|draft| draft.map_id.clone());
    let clipmap = session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.uses_legacy_clipmap_cursor());
    let objects = session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.manifest.terrains.is_some());
    if edit.map_id.is_some() && edit.map_id != current_map_id {
        edit.clear_drag(); // 旧地图稳定ID绝不能进入新草稿。
    }
    if should_finish_drag(edit.dragging(), mouse.pressed(MouseButton::Left)) {
        finish_drag(&mut editor, &mut session, &mut edit); // 丢失释放事件或同帧短按短放也必须收口唯一撤销事务。
    }
    if session.draft.as_ref().is_some_and(|draft| {
        !draft
            .terrain_manifest()
            .terrain
            .kind
            .supports_spline_water()
    }) {
        if mouse.just_pressed(MouseButton::Left)
            && matches!(editor.tool, EditorTool::RiverSpline | EditorTool::Waterfall)
        {
            editor.status = "当前地形类型不支持河流、急流与瀑布编辑".to_owned();
        }
        edit.clear_drag();
        *pending_object = None;
        return; // 视口入口再次校验，旧文件中的样条内容不能被拖动或追加控制点。
    }
    let selecting_water =
        keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight); // 与Cursor阶段水面拾取使用同一按住语义。
    let active_tool = matches!(editor.tool, EditorTool::RiverSpline | EditorTool::Waterfall);
    let supported_view = matches!(editor.view, EditorView::Top | EditorView::Perspective);
    if !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || !active_tool
        || !supported_view
        || selecting_water
        || session.saving
    {
        *pending_object = None;
        return; // 前/侧视缺少稳定XZ地形命中，不消费连续水网左键。
    }
    let Some(viewport) = viewport else {
        return; // RTT尚未建立时无法计算固定像素命中范围。
    };
    let relative_cursor = viewport_node.single().ok(); // 同一UI坐标同时供屏幕点命中和相机射线使用。
    let hovered = relative_cursor.is_some_and(|cursor| cursor.cursor_over());
    let viewport_position =
        relative_cursor
            .and_then(|cursor| cursor.normalized)
            .map(|normalized| {
                Vec2::new(
                    (normalized.x + 0.5) * viewport.render_size.x as f32,
                    (normalized.y + 0.5) * viewport.render_size.y as f32,
                ) // 与视口发布地形射线使用完全相同的RTT像素换算。
            });

    if mouse.just_pressed(MouseButton::Left) {
        if !hovered {
            return; // 点击UI不能落点或开始拖动。
        }
        let Some(viewport_position) = viewport_position else {
            editor.status = "视口光标位置尚未就绪".to_owned();
            return;
        };
        let Ok((camera, camera_transform)) = camera_query.get(viewport.camera) else {
            editor.status = "地图编辑器相机尚未就绪".to_owned();
            return;
        };
        let Some(before) = session.draft.as_ref().and_then(|draft| draft.water.clone()) else {
            editor.status = "当前地图没有可编辑水域草稿".to_owned();
            return;
        };
        let Some((feature_id, waterfall)) = active_feature_identity(&before, &editor) else {
            editor.status = if editor.tool == EditorTool::RiverSpline {
                "请先点击“新建河流”".to_owned()
            } else {
                "请先点击“新建瀑布”".to_owned()
            };
            return;
        };
        let points = active_points(&before, &editor).unwrap_or_default();
        let camera_global = GlobalTransform::from(*camera_transform); // Cursor阶段刚写入的Transform不能等待PostUpdate传播。
        if let Some(point_index) = nearest_projected_point(
            points,
            viewport_position,
            POINT_PICK_RADIUS_PIXELS,
            |position| camera.world_to_viewport(&camera_global, position).ok(),
        ) {
            *pending_object = None;
            if clipmap && let Some(ground) = ground.as_deref_mut() {
                ground.cancel_spline_click(); // 控制点命中优先于同一按压冻结的新增点射线。
            }
            let point_id = points[point_index].id.clone();
            editor.selected_water_control_point = Some(point_index);
            edit.map_id = current_map_id;
            edit.feature_id = Some(feature_id);
            edit.point_id = Some(point_id);
            edit.waterfall = waterfall;
            edit.before = Some(before);
            edit.expected_revision = session.revision;
            edit.dirty_before = editor.dirty;
            edit.moved = false;
            editor.status = format!("正在拖动第{}个水网控制点；只修改XZ", point_index + 1);
            if should_finish_drag(edit.dragging(), mouse.pressed(MouseButton::Left)) {
                finish_drag(&mut editor, &mut session, &mut edit); // 同一输入帧已释放时把本次操作收口为纯选择。
            }
            return;
        }

        if objects {
            // 多网格严格点击会在后帧完成，不能拿按下前的悬停或零平面补控制点。
            *pending_object = objects_ground
                .as_deref()
                .and_then(|ground| super::object_click::capture(&editor, &session, ground));
            editor.status = "正在查询所选网格的真实表面；完成后添加水网控制点".into();
        } else if clipmap {
            let Some(snapshot) = surface
                .as_ref()
                .and_then(|surface| surface.snapshot.as_ref())
            else {
                editor.status = "正在等待Geometry Clipmap真实地表；本次点击尚未落点".to_owned();
                return;
            };
            let Some(ground) = ground.as_deref_mut() else {
                editor.status = "Geometry Clipmap地表查询尚未安装".to_owned();
                return;
            };
            if let Some(result) = ground.take_spline_click_result(
                snapshot,
                &before.map_id,
                &feature_id,
                waterfall,
                session.revision,
            ) {
                match result {
                    ClipmapGroundHit::Hit(cursor) => {
                        append_water_network_point(&mut editor, &mut session, cursor)
                    }
                    ClipmapGroundHit::WaitingForHeight => {
                        editor.status = "当前高度页尚未就绪；样条水控制点未添加".to_owned()
                    }
                    ClipmapGroundHit::Miss => {
                        editor.status = "点击位置没有命中Geometry Clipmap有限地表".to_owned()
                    }
                    ClipmapGroundHit::Failed => {
                        editor.status = "Geometry Clipmap地表射线无效；控制点未添加".to_owned()
                    }
                }
            } else if ground.spline_click_pending(
                &before.map_id,
                &feature_id,
                waterfall,
                session.revision,
                session.terrain_revision,
            ) {
                editor.status = "正在查询Geometry Clipmap真实三角面落点".to_owned();
            } else {
                editor.status = "视口光标没有命中当前地形".to_owned();
            }
            return; // 后台完成值由后续帧按同一冻结身份消费，不能读取平面回退。
        } else {
            let Some(cursor) = editor.cursor_world else {
                editor.status = "视口光标没有命中当前地形".to_owned();
                return;
            };
            append_water_network_point(&mut editor, &mut session, cursor);
        }
    }

    if objects
        && !edit.dragging()
        && let Some(point) = super::object_click::take(
            &mut pending_object,
            &mut editor,
            &session,
            objects_ground.as_deref(),
        )
    {
        append_water_network_point(&mut editor, &mut session, point);
    }

    // 点击释放后仍允许唯一冻结射线完成；地图、特征、作者修订或新点击变化会在查询资源内淘汰它。
    if clipmap && !edit.dragging() {
        let identity = session
            .draft
            .as_ref()
            .and_then(|draft| draft.water.as_ref().map(|water| (&draft.map_id, water)))
            .and_then(|(map_id, water)| {
                active_feature_identity(water, &editor)
                    .map(|(feature_id, waterfall)| (map_id.clone(), feature_id, waterfall))
            });
        if let (Some((map_id, feature_id, waterfall)), Some(snapshot), Some(ground)) = (
            identity,
            surface
                .as_ref()
                .and_then(|surface| surface.snapshot.as_ref()),
            ground.as_deref_mut(),
        ) && let Some(result) = ground.take_spline_click_result(
            snapshot,
            &map_id,
            &feature_id,
            waterfall,
            session.revision,
        ) {
            match result {
                ClipmapGroundHit::Hit(cursor) => {
                    append_water_network_point(&mut editor, &mut session, cursor)
                }
                ClipmapGroundHit::WaitingForHeight => {
                    editor.status = "当前高度页尚未就绪；样条水控制点未添加".to_owned()
                }
                ClipmapGroundHit::Miss => {
                    editor.status = "点击位置没有命中Geometry Clipmap有限地表".to_owned()
                }
                ClipmapGroundHit::Failed => {
                    editor.status = "Geometry Clipmap地表射线无效；控制点未添加".to_owned()
                }
            }
        }
    }

    if mouse.pressed(MouseButton::Left) && edit.dragging() && hovered {
        if session.revision != edit.expected_revision {
            edit.clear_drag();
            editor.status = "水网拖动因并发作者修改而取消".to_owned();
            return;
        }
        let Some(feature_id) = edit.feature_id.clone() else {
            return;
        };
        let Some(point_id) = edit.point_id.clone() else {
            return;
        };
        let Some(before_position) = session
            .draft
            .as_ref()
            .and_then(|draft| draft.water.as_ref())
            .and_then(|water| point_position_by_id(water, &feature_id, &point_id, edit.waterfall))
        else {
            edit.clear_drag(); // 稳定点在事务中消失时立即释放输入门。
            return;
        };
        let Some(viewport_position) = viewport_position else {
            return; // 光标短暂离开RTT时保持上一完整位置，重新进入后继续同一事务。
        };
        let Ok((camera, camera_transform)) = camera_query.get(viewport.camera) else {
            return; // 相机换代帧不伪造拖动坐标。
        };
        let Ok(ray) =
            camera.viewport_to_world(&GlobalTransform::from(*camera_transform), viewport_position)
        else {
            return; // 无法形成有限相机射线时保持当前控制点。
        };
        let Some(cursor) =
            horizontal_plane_intersection(ray.origin, *ray.direction, before_position[1])
        else {
            return; // 与控制点Y平面平行或交点在相机背后时不更新中间态。
        };
        let next = [cursor.x, before_position[1], cursor.z]; // 左拖只改变XZ，作者Y保持逐位不变。
        let changed = if next == before_position {
            false
        } else if let Some(water) = session
            .draft
            .as_mut()
            .and_then(|draft| draft.water.as_mut())
        {
            if let Some(point) = point_mut_by_id(water, &feature_id, &point_id, edit.waterfall) {
                point.position = next; // 先应用直接操控点，再统一推进所有依赖端点。
                sync_water_network_connections(water); // 河流端点拖动同帧带动所连接瀑布的完整XYZ。
                point_position_by_id(water, &feature_id, &point_id, edit.waterfall)
                    != Some(before_position) // 连接规则若把受约束端点吸回原位，则不制造虚假修订。
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            edit.moved = true;
            session.mark_water_definition_changed(&mut editor); // 每次真实中间态变化驱动现有single-flight实时预览。
            edit.expected_revision = session.revision; // 只允许本事务继续推进后的作者修订。
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 默认值和非上升规则保持纯逻辑测试。

    #[test]
    fn default_point_parameters_match_authoring_contract() {
        assert_eq!(DEFAULT_POINT_WIDTH, 6.0);
        assert_eq!(DEFAULT_POINT_DEPTH, 1.5);
        assert_eq!(DEFAULT_POINT_SPEED, 2.0);
    }

    #[test]
    fn endpoint_type_remains_available_for_source_height_workflow() {
        let endpoint = MapWaterEndpoint::StillWater {
            layer_id: "lake".to_owned(),
        };
        assert!(matches!(endpoint, MapWaterEndpoint::StillWater { .. }));
    }

    #[test]
    fn same_frame_press_and_release_finishes_a_started_drag() {
        assert!(should_finish_drag(true, false)); // 已建立快照但最终按钮未按住，必须立即收口。
        assert!(!should_finish_drag(true, true)); // 正常持续按住继续实时拖动。
        assert!(!should_finish_drag(false, false)); // 空闲帧不能伪造松手事务。
    }
}
