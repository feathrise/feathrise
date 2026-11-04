//! 独立空气墙的点编辑：单墙快照、开放/闭合、近边插点和单笔拖动撤销。
use super::helpers::{
    nearest_segment, point_pick_radius, selected_path_index, validate_path_geometry,
};
use super::path_history::commit_path_change;
use crate::authoring::resources::{BoundaryEditState, EditorAuthoringSession};
use crate::components::MapEditorViewport;
use crate::resources::{MapEditorState, MapEditorViewportState};
use crate::types::EditorTool;
use crate::viewport::modes::ViewportModeState;
use crate::viewport::vegetation_ground::{GroundCursorTarget, VegetationGroundCursor};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use feathrise_editor_ui::ActiveNumericInput;
use feathrise_map_content::{MapBoundaryPath, MapBoundaryRole};
use feathrise_scene::{ClipmapGroundHit, DynamicClipmapGroundSurface};

/// 所有编辑快照只复制当前墙，不随全图墙体数量复制控制点。
pub(super) fn editable_path(session: &EditorAuthoringSession) -> Option<(usize, MapBoundaryPath)> {
    let index = selected_path_index(session)?; // 只在输入边沿解析稳定选择。
    let path = session
        .draft
        .as_ref()?
        .boundaries
        .as_ref()?
        .paths
        .get(index)?
        .clone();
    Some((index, path))
}

/// 单墙动作共用的错误回显与历史提交。
fn submit(
    session: &mut EditorAuthoringSession,
    editor: &mut MapEditorState,
    index: usize,
    before: MapBoundaryPath,
    after: MapBoundaryPath,
    status: &str,
) {
    if let Err(error) =
        commit_path_change(session, editor, index, Some(before), Some(after), status)
    {
        editor.status = error; // 身份冲突不能覆盖其他修改。
    }
}

/// 中间点保存在输入资源里，取消既不遗留无历史修改，也不覆盖并发新草稿。
fn cancel_drag(edit: &mut BoundaryEditState) {
    edit.clear_drag();
}

/// 松手（包括丢失release边沿）只提交最后一个已受理位置，不等待在途射线补移点。
fn finish_drag(
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    edit: &mut BoundaryEditState,
) {
    let index = edit.path_index.unwrap();
    let point = edit.point_index.unwrap();
    let next = edit.preview_point;
    let before = edit.before.take().unwrap();
    let moved = edit.moved;
    edit.clear_drag(); // 保留selected_point供Delete使用。
    if !moved {
        return;
    }
    let draft = session.draft.as_ref().unwrap();
    let mut after = before.clone();
    if let Some(next) = next {
        after.points[point] = next;
    }
    let validation = draft
        .manifest
        .world_bounds()
        .ok_or_else(|| "地图范围无效".to_owned())
        .and_then(|bounds| validate_path_geometry(&after, &draft.map_id, bounds));
    if let Err(error) = validation {
        editor.status = format!("控制点位置无效，已恢复：{error}");
        return;
    }
    submit(
        session,
        editor,
        index,
        before,
        after,
        "控制点已移动；本次拖动可一次撤销",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_map_content::MapBoundariesAsset;

    fn app() -> App {
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        let mut document =
            MapBoundariesAsset::rectangular_default(&draft.map_id, [0.0; 2], [10.0; 2], 0.0, 20.0)
                .unwrap();
        let mut internal = document.paths[0].clone();
        internal.id = "internal".into();
        internal.role = MapBoundaryRole::Internal;
        document.paths.push(internal);
        draft.boundaries = Some(document);
        let selected = Some((draft.map_id.clone(), "internal".into()));
        let mut app = App::new();
        app.insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            selected_boundary_path: selected,
            ..default()
        });
        app.init_resource::<MapEditorState>()
            .init_resource::<BoundaryEditState>()
            .add_systems(Update, apply_boundary_actions);
        app
    }

    #[test]
    fn open_internal_wall_leaves_outer_wall_unchanged() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .boundary_close_requested = true;
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        let paths = &session
            .draft
            .as_ref()
            .unwrap()
            .boundaries
            .as_ref()
            .unwrap()
            .paths;
        assert!(paths[0].closed); // 外框不被内部墙按钮误写。
        assert!(!paths[1].closed); // 内部围挡可以打开。
        assert_eq!(session.undo.len(), 1); // 只产生一条对象事务。
    }

    #[test]
    fn delete_removes_selected_point_and_preserves_closed_shape() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<BoundaryEditState>()
            .selected_point = Some(("internal".into(), 1));
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .boundary_remove_last_requested = true;
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        let paths = &session
            .draft
            .as_ref()
            .unwrap()
            .boundaries
            .as_ref()
            .unwrap()
            .paths;
        assert_eq!(paths[0].points.len(), 4); // 保留主墙。
        assert_eq!(paths[1].points, vec![[2.0, 2.0], [8.0, 8.0], [2.0, 8.0]]); // 删除实际选中点而非末点。
        assert!(paths[1].closed); // 合法围挡删点无需强制重开。
    }
}

/// 重画、删除所选点，以及内部墙的开口/闭合切换。
pub(crate) fn apply_boundary_actions(
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut edit: ResMut<BoundaryEditState>,
) {
    let redraw = std::mem::take(&mut editor.boundary_redraw_requested);
    let remove = std::mem::take(&mut editor.boundary_remove_last_requested);
    let toggle = std::mem::take(&mut editor.boundary_close_requested);
    if !redraw && !remove && !toggle {
        return;
    } // 稳定帧不复制墙。
    if session.saving {
        editor.status = "保存期间暂不修改空气墙".into();
        return;
    }
    let Some((index, before)) = editable_path(&session) else {
        return;
    };
    edit.clear_drag(); // 按钮动作不会继续旧拖动。
    let mut after = before.clone();
    let status;
    if redraw {
        after.points.clear(); // 保留名称、角色、显隐和墙体参数。
        after.closed = false;
        edit.selected_point = None;
        status = "已开始重画；逐点点击，内部挡墙两点即可，外框请闭合";
    } else if remove {
        let point = edit
            .selected_point
            .as_ref()
            .filter(|(id, _)| *id == before.id)
            .map(|(_, index)| *index);
        let Some(point) = point else {
            editor.status = "请先点击选择要删除的控制点".into();
            return;
        };
        if let Err(error) = after.remove_point(point) {
            editor.status = error;
            return;
        }
        edit.selected_point = None;
        status = "已删除所选控制点";
    } else {
        if after.closed && after.role == MapBoundaryRole::Outer {
            editor.status = "地图外框必须闭合；需要开放挡墙请新建内部墙".into();
            return;
        }
        if !after.closed && after.points.len() < 3 {
            editor.status = "闭合围挡至少需要三个控制点".into();
            return;
        }
        after.closed = !after.closed; // 内部墙可在开放折线和闭合围挡间切换。
        let Some(draft) = session.draft.as_ref() else {
            return;
        };
        let Some(bounds) = draft.manifest.world_bounds() else {
            return;
        };
        if let Err(error) = validate_path_geometry(&after, &draft.map_id, bounds) {
            editor.status = error;
            return;
        }
        status = if after.closed {
            "空气墙已闭合"
        } else {
            "内部空气墙已打开"
        };
    }
    submit(&mut session, &mut editor, index, before, after, status);
}

/// 左键选点拖动，Ctrl左键在线段插点，Delete只删除当前明确选择的控制点。
#[allow(clippy::too_many_arguments)]
pub(crate) fn edit_boundary_from_viewport(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    numeric: Option<Res<ActiveNumericInput>>,
    text_inputs: Query<(), With<bevy::text::EditableText>>,
    viewport_node: Query<&RelativeCursorPosition, With<MapEditorViewport>>,
    viewport: Option<Res<MapEditorViewportState>>,
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut edit: ResMut<BoundaryEditState>,
    surface: Option<Res<DynamicClipmapGroundSurface>>,
    mut ground: Option<ResMut<VegetationGroundCursor>>,
    mode: Option<Res<ViewportModeState>>,
    guards: (
        Option<Res<crate::resources::MapEditorNumericScrubMouseGuard>>,
        Option<Res<crate::resources::MapBrowserState>>,
        Option<Res<crate::resources::MapBrowserDragState>>,
        Option<Res<crate::authoring::TerrainExternalChangeState>>,
    ),
) {
    let map = session.draft.as_ref().map(|draft| draft.map_id.clone());
    if edit.map_id != map {
        edit.clear_drag();
        edit.selected_point = None;
        edit.map_id = map.clone();
    } // 换图释放所有点身份。
    if !editor.open
        || editor.new_map_dialog_open
        || editor.tool != EditorTool::Boundary
        || editor.close_requested
        || editor.save_requested
        || session.saving
        || editor.selected_map_id.as_ref() != map.as_ref()
    {
        if editor.tool == EditorTool::Boundary || edit.dragging() {
            if let Some(ground) = ground.as_deref_mut() {
                ground.cancel_boundary_click();
            }
            cancel_drag(&mut edit);
        }
        return;
    }
    let clipmap = session.draft.as_ref().is_some_and(|draft| {
        draft.manifest.terrain.kind == feathrise_map_content::MapTerrainKind::SparseClipmap
    });
    let snapshot = surface
        .as_ref()
        .and_then(|surface| surface.snapshot.as_ref());
    if edit.dragging() {
        let valid = session.revision == edit.expected_revision
            && session
                .selected_boundary_path
                .as_ref()
                .is_some_and(|(selected_map, id)| {
                    Some(selected_map) == map.as_ref()
                        && edit.before.as_ref().is_some_and(|before| &before.id == id)
                })
            && session
                .draft
                .as_ref()
                .and_then(|draft| draft.boundaries.as_ref())
                .and_then(|doc| edit.path_index.and_then(|index| doc.paths.get(index)))
                .is_some_and(|path| {
                    edit.before
                        .as_ref()
                        .is_some_and(|before| path.id == before.id)
                        && edit
                            .point_index
                            .is_some_and(|point| point < path.points.len())
                })
            && edit.clipmap_content.is_none_or(|content| {
                clipmap
                    && edit.terrain_revision == session.terrain_revision
                    && snapshot.is_none_or(|snapshot| {
                        Some(&snapshot.map_id) == map.as_ref()
                            && snapshot.content_revision_words() == content
                    })
            });
        if !valid {
            cancel_drag(&mut edit);
            if let Some(ground) = ground.as_deref_mut() {
                ground.cancel_boundary_click();
            }
            editor.status = "空气墙选择、作者或高度来源已变化，本次拖动已取消".into();
            return;
        }
        if !mouse.pressed(MouseButton::Left) {
            finish_drag(&mut editor, &mut session, &mut edit);
            if let Some(ground) = ground.as_deref_mut() {
                ground.cancel_boundary_click();
            }
            return;
        }
    }
    let hovered = viewport_node
        .single()
        .is_ok_and(|cursor| cursor.cursor_over());
    let text_focused = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .is_some_and(|entity| text_inputs.contains(entity))
        || numeric.as_ref().is_some_and(|input| input.entity.is_some()); // 输入文字时Delete属于文本框。
    let blocked = editor.suppress_world_pick_this_frame
        || mode.as_ref().is_some_and(|mode| mode.pointer_over_controls)
        || guards.0.as_ref().is_some_and(|guard| guard.blocked())
        || guards
            .1
            .as_ref()
            .is_some_and(|browser| browser.dialog.is_some())
        || guards
            .2
            .as_ref()
            .is_some_and(|drag| drag.blocks_other_mouse())
        || guards
            .3
            .as_ref()
            .is_some_and(|terrain| terrain.modal_open());
    if !hovered || text_focused || blocked {
        if let Some(ground) = ground.as_deref_mut() {
            ground.cancel_boundary_click();
        }
        return; // 阻塞帧仍运行取消逻辑，不能在解锁后消费旧点击。
    }
    if keys.just_pressed(KeyCode::Delete) && !edit.dragging() {
        editor.boundary_remove_last_requested = true; // 统一按钮动作在下一作者阶段收口。
    }
    let Some(viewport) = viewport else {
        return;
    };
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !edit.dragging() {
        let world3 = if clipmap {
            let target = super::cursor::clipmap_cursor_target(&session, Some(&edit), ctrl);
            match (snapshot, ground.as_deref_mut(), target) {
                (Some(snapshot), Some(ground), Some(target)) => {
                    match ground.take_author_click_result(
                        snapshot,
                        map.as_ref().unwrap(),
                        target,
                        session.revision,
                    ) {
                        Some(ClipmapGroundHit::Hit(point)) => Some(point),
                        Some(ClipmapGroundHit::Miss) => {
                            editor.status = "本次空气墙点击未命中地形".into();
                            None
                        }
                        Some(ClipmapGroundHit::Failed) => {
                            editor.status = "空气墙地形查询失败，本次点击未落点".into();
                            None
                        }
                        _ => None,
                    }
                }
                _ => None,
            } // 每帧只消费原点击一次，允许缺页点击在松键后完成；不读显示保留点。
        } else {
            mouse
                .just_pressed(MouseButton::Left)
                .then_some(editor.cursor_world)
                .flatten()
        };
        let Some(world3) = world3 else {
            return;
        };
        let Some((index, before)) = editable_path(&session) else {
            return;
        };
        let world = Vec2::new(world3.x, world3.z);
        let radius = point_pick_radius(viewport.extent, viewport.render_size);
        if ctrl {
            let Some((point_index, point)) = nearest_segment(&before, world, radius) else {
                editor.status = "Ctrl点击空气墙线段可插入控制点".into();
                return;
            };
            let mut after = before.clone();
            if let Err(error) = after.insert_point(point_index, point) {
                editor.status = error;
                return;
            }
            edit.selected_point = Some((before.id.clone(), point_index));
            submit(
                &mut session,
                &mut editor,
                index,
                before,
                after,
                "已在线段插入控制点",
            );
            return;
        }
        let nearest = before
            .points
            .iter()
            .enumerate()
            .filter_map(|(index, point)| {
                let distance = Vec2::from_array(*point).distance_squared(world);
                (distance <= radius * radius).then_some((index, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index); // 只在按下边沿扫描当前墙。
        if let Some(point) = nearest {
            edit.path_index = Some(index);
            edit.point_index = Some(point);
            edit.selected_point = Some((before.id.clone(), point));
            edit.before = Some(before);
            edit.expected_revision = session.revision;
            edit.clipmap_content = clipmap.then(|| snapshot.unwrap().content_revision_words());
            edit.terrain_revision = session.terrain_revision;
            edit.moved = false;
            if let Some(ground) = ground.as_deref_mut() {
                ground.cancel_boundary_click();
            }
            editor.status = format!("已选择第{}点；拖动移动，Delete删除", point + 1);
            if !mouse.pressed(MouseButton::Left) {
                edit.clear_drag(); // 延迟点击松键后只选择既有点，不创建无release可结束的拖动。
            }
            return;
        }
        if before.closed {
            editor.status = "点击控制点拖动；Ctrl点击线段插点".into();
            return;
        }
        let mut after = before.clone();
        let point = after.points.len();
        if let Err(error) = after.insert_point(point, world.to_array()) {
            editor.status = error;
            return;
        }
        edit.selected_point = Some((before.id.clone(), point));
        submit(
            &mut session,
            &mut editor,
            index,
            before,
            after,
            "已添加控制点；开放挡墙两点即可，闭合围挡至少三点",
        );
    }
    if !edit.dragging() {
        return;
    }
    let point = edit.point_index.unwrap();
    if !ctrl {
        let world = if clipmap {
            snapshot.and_then(|snapshot| {
                ground.as_deref().and_then(|ground| {
                    ground.accepted_paint_hit(
                        snapshot,
                        map.as_ref().unwrap(),
                        &GroundCursorTarget::BoundaryDrag {
                            path_id: edit.before.as_ref().unwrap().id.clone(),
                            point_index: point,
                        },
                        edit.terrain_revision,
                    )
                })
            }) // 只使用本帧完成的原射线，等待期间不拿旧显示点重复移动。
        } else {
            editor.cursor_world
        };
        if let Some(world) = world {
            let next = [world.x, world.z];
            if edit
                .preview_point
                .unwrap_or(edit.before.as_ref().unwrap().points[point])
                != next
            {
                edit.preview_point = Some(next);
                edit.moved = true;
            }
            // 实时只更新固定大小预览点，作者文档和碰撞在完整单墙事务提交后变化。
        }
    }
}

#[cfg(test)]
#[path = "edit/clipmap_tests.rs"]
mod clipmap_tests;
