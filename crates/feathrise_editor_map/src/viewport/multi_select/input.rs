//! 选择模式区分短点击和拖框；松手后固定预算捕获作者原点，完整选集才发布。

use super::resources::{
    PlacementSelectionSet, SelectedMember, SelectionGesture, SelectionRefresh, SelectionScan,
};
use crate::authoring::EditorAuthoringSession;
use crate::authoring::placement_pose::{is_automatic_volume, placement_record_world_position};
use crate::components::MapEditorViewport;
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState, MapEditorViewportState};
use crate::ui::gltf_browser::EditorGltfBrowserState;
use crate::viewport::placement_move::{MoveCamera, MoveInputContext, PlacementMoveState};
use crate::viewport::{
    LayerCandidateState, LayerTarget,
    modes::{SelectionFilter, ViewportModeState},
};
use bevy::{prelude::*, ui::RelativeCursorPosition};

pub(crate) fn box_filter(filter: SelectionFilter) -> bool {
    matches!(filter, SelectionFilter::All | SelectionFilter::Placements)
}

pub(crate) fn handle_multi_select_pointer(
    mut selection: ResMut<PlacementSelectionSet>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    modes: Res<ViewportModeState>,
    context: MoveInputContext,
    move_state: Res<PlacementMoveState>,
    viewport: Option<Res<MapEditorViewportState>>,
    nodes: Query<(&RelativeCursorPosition, &ComputedNode), With<MapEditorViewport>>,
) {
    selection.click_ready = false;
    if !mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
        && !mouse.any_just_released([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
    {
        selection.owns_mouse = false;
    }
    let node = nodes.single().ok();
    let camera = viewport.as_ref().and_then(|viewport| {
        node.and_then(|(_, node)| MoveCamera::new(&editor, viewport, node.inverse_scale_factor()))
    });
    let available = context.selection_available(&editor)
        && !session.saving
        && !move_state.owns_mouse()
        && box_filter(modes.filter);
    let chord = mouse.any_pressed([MouseButton::Right, MouseButton::Middle]);
    let escaped = keyboard.just_pressed(KeyCode::Escape);
    if let Some(mut gesture) = selection.gesture.take() {
        let current = available
            && session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.map_id == gesture.map)
            && viewport
                .as_ref()
                .is_some_and(|viewport| viewport.framed_map_id.as_ref() == Some(&gesture.map))
            && session.revision == gesture.author_revision
            && session.placement_structure_revision == gesture.structure_revision
            && session.terrain_revision == gesture.terrain_revision
            && session.placement_derived_pose_revision == gesture.derived_pose_revision
            && modes.filter == gesture.filter
            && camera == Some(gesture.camera)
            && node.is_some_and(|(_, node)| node.size() == gesture.ui_size);
        if !current || escaped || chord {
            selection.owns_mouse = true;
            keyboard.clear();
            return; // 放弃整次手势，不把取消或和弦当成短点击。
        }
        if let Some(point) = node
            .and_then(|(cursor, _)| cursor.normalized)
            .filter(|p| p.is_finite())
        {
            gesture.current = point.clamp(Vec2::splat(-0.5), Vec2::splat(0.5)); // Bevy相对坐标以节点中心为零。
        }
        let delta =
            (gesture.current - gesture.start) * gesture.camera.size * gesture.camera.inverse_scale;
        if delta.length_squared() > 16.0 {
            gesture.dragging = true;
            selection.owns_mouse = true;
        }
        if gesture.dragging {
            keyboard.clear();
        }
        if mouse.just_released(MouseButton::Left) {
            if gesture.dragging {
                selection.scan = Some(SelectionScan {
                    gesture,
                    cursor: 0,
                    members: Vec::new(),
                });
                editor.status = "正在框选普通资产…".into();
            } else {
                selection.click_ready = true;
                selection.click_additive = gesture.additive;
            }
        } else {
            selection.gesture = Some(gesture);
        }
        return;
    }
    if !available
        || guard.blocked()
        || context.pointer_over_ui()
        || escaped
        || chord
        || !mouse.just_pressed(MouseButton::Left)
    {
        return;
    }
    let (Some(draft), Some(camera), Some((cursor, node))) = (session.draft.as_ref(), camera, node)
    else {
        return;
    };
    if viewport
        .as_ref()
        .is_none_or(|viewport| viewport.framed_map_id.as_ref() != Some(&draft.map_id))
    {
        return;
    }
    let Some(start) = cursor.normalized.filter(|_| cursor.cursor_over()) else {
        return;
    };
    selection.scan = None;
    if mouse.just_released(MouseButton::Left) {
        selection.click_ready = true;
        selection.click_additive = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        return; // 按下和松开落在同一帧的快速单击也必须交给正式拾取，不能留下无松手的手势。
    }
    selection.gesture = Some(SelectionGesture {
        map: draft.map_id.clone(),
        author_revision: session.revision,
        structure_revision: session.placement_structure_revision,
        terrain_revision: session.terrain_revision,
        derived_pose_revision: session.placement_derived_pose_revision,
        camera,
        ui_size: node.size(),
        filter: modes.filter,
        start,
        current: start,
        additive: keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        dragging: false,
    });
}

/// 多选时收起单物体属性，避免只操作其中一项；单项选择仍走原完整实例链。
pub(crate) fn publish_primary(
    selection: &PlacementSelectionSet,
    browser: &mut EditorGltfBrowserState,
    candidates: &mut LayerCandidateState,
) {
    browser.selection = None;
    browser.selected_placement_id = if selection.members.len() == 1 {
        Some(selection.members[0].id.clone())
    } else {
        None
    };
    candidates.selected = selection
        .members
        .last()
        .map(|member| LayerTarget::Placement(member.id.clone()));
    candidates.selected_author_index = selection.members.last().map(|member| member.index);
    candidates.hover = None;
    candidates.hover_author_index = None;
    candidates.clear_objects = false;
}

pub(crate) fn advance_multi_selection(
    mut selection: ResMut<PlacementSelectionSet>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    modes: Res<ViewportModeState>,
    viewport: Option<Res<MapEditorViewportState>>,
    nodes: Query<&ComputedNode, With<MapEditorViewport>>,
    mut ui: ParamSet<(
        MoveInputContext,
        (ResMut<EditorGltfBrowserState>, ResMut<LayerCandidateState>),
    )>,
    move_state: Res<PlacementMoveState>,
    standalone: Option<Res<crate::standalone::StandaloneEditorMode>>,
) {
    let input_available = ui.p0().selection_available(&editor) && !session.saving;
    let (mut browser, mut candidates) = ui.p1();
    let hidden_for_play = !editor.open
        && standalone.as_ref().is_some_and(|mode| mode.embedded_playing());
    let Some(draft) = session
        .draft
        .as_ref()
        .filter(|draft| (editor.open || hidden_for_play)
            && editor.selected_map_id.as_ref() == Some(&draft.map_id))
    else {
        selection.clear();
        selection.gesture = None;
        selection.scan = None;
        selection.map = None;
        return;
    };
    if selection
        .map
        .as_ref()
        .is_some_and(|map| map != &draft.map_id)
    {
        selection.clear();
        selection.scan = None;
        selection.map = Some(draft.map_id.clone());
    }
    if hidden_for_play {
        // 常驻试玩只收起作者呈现：保留同图完整选集，不用隐藏期间的输入/面板重解释选择。
        // 真正关闭或地图身份失效仍经过上方原清理入口，返回后继续原有有界身份复核。
        return;
    }
    if !modes.mode.transforms_placement() && !move_state.owns_mouse() && !selection.box_busy() {
        if let Some(id) = browser
            .selected_placement_id
            .as_ref()
        {
            if let Some((index, record)) = browser
                .placement_record_index(id, session.placement_structure_revision)
                .and_then(|index| {
                    draft
                        .placements
                        .placements
                        .get(index)
                        .map(|record| (index, record))
                })
                .filter(|(_, record)| record.id == *id && record.editor.selectable())
            {
                if record.building_preset_id.is_some() || is_automatic_volume(record) {
                    if !selection.members.is_empty() || selection.ready || selection.refresh.is_some() {
                        selection.clear();
                    } // 同ID主项也重验作者归属；空闲帧不重复分配空集或发布Changed。
                } else if selection.members.len() != 1 || selection.members[0].id != *id {
                    let position = match placement_record_world_position(draft, record) {
                        Ok(position) => position,
                        Err(error) => {
                            selection.clear();
                            publish_primary(&selection, &mut browser, &mut candidates);
                            editor.status = error;
                            return; // 源失效时撤销整项编辑资格，不能以局部点重新激活旧主选择。
                        }
                    };
                    selection.publish(
                        vec![SelectedMember {
                            id: id.clone(),
                            index,
                            position,
                        }],
                        &session,
                    );
                }
            }
        } else if browser.selected_placement_id.is_none()
            && candidates
                .selected
                .as_ref()
                .is_some_and(|target| !matches!(target, LayerTarget::Placement(_)))
        {
            selection.clear(); // 复制/其他作者入口的正式选择变化同样同步选集，不能保留隐藏的旧整组。
        }
    }
    if selection
        .gesture
        .as_ref()
        .is_some_and(|gesture| gesture.dragging)
        || selection.scan.is_some()
    {
        candidates.freeze_selection(); // 本次框选淘汰未完成单击和空命中，保留旧选集直到完整采用。
    }
    if let Some(mut scan) = selection.scan.take() {
        let node = nodes.single().ok();
        let camera = viewport.as_ref().and_then(|viewport| {
            node.and_then(|node| MoveCamera::new(&editor, viewport, node.inverse_scale_factor()))
        });
        if !input_available
            || modes.filter != scan.gesture.filter
            || session.revision != scan.gesture.author_revision
            || viewport
                .as_ref()
                .is_none_or(|viewport| viewport.framed_map_id.as_ref() != Some(&scan.gesture.map))
            || session.placement_structure_revision != scan.gesture.structure_revision
            || session.terrain_revision != scan.gesture.terrain_revision
            || session.placement_derived_pose_revision != scan.gesture.derived_pose_revision
            || scan.gesture.map != draft.map_id
            || camera != Some(scan.gesture.camera)
            || !node.is_some_and(|node| node.size() == scan.gesture.ui_size)
        {
            return;
        }
        let min = scan.gesture.start.min(scan.gesture.current);
        let max = scan.gesture.start.max(scan.gesture.current);
        let end = (scan.cursor + super::CAPTURE_BUDGET).min(draft.placements.placements.len());
        for (index, record) in draft.placements.placements[scan.cursor..end]
            .iter()
            .enumerate()
        {
            if record.building_preset_id.is_some() || !record.editor.selectable() || is_automatic_volume(record) {
                continue;
            }
            // 三维地形的作者平移属于源空间；框选与世界手柄必须共享同一实际原点。
            let Ok(position) = placement_record_world_position(draft, record) else { continue; };
            if scan
                .gesture
                .camera
                .project(position)
                .is_some_and(|p| p.cmpge(min).all() && p.cmple(max).all())
            {
                if scan.members.len() == super::MAX_SELECTED {
                    editor.status = format!(
                        "框选超过{}个普通资产，请缩小范围；原选择已保留",
                        super::MAX_SELECTED
                    );
                    return;
                }
                scan.members.push(SelectedMember {
                    id: record.id.clone(),
                    index: scan.cursor + index,
                    position,
                });
            }
        }
        scan.cursor = end;
        if end == draft.placements.placements.len() {
            if scan.gesture.additive && !selection.members.is_empty() && !selection.ready_for(&session) {
                editor.status = "多选对象位置正在更新，请稍后再追加框选".into();
                return; // 旧选集可能仍在16项/帧复核；拒绝追加，保持原刷新游标及完整采用边界。
            }
            let mut members = if scan.gesture.additive {
                selection.members.as_ref().clone()
            } else {
                Vec::new()
            };
            for member in scan.members {
                if !members.iter().any(|old| old.id == member.id) {
                    members.push(member);
                }
            }
            if members.len() > super::MAX_SELECTED {
                editor.status = "追加框选超过多选上限，原选择已保留".into();
                return;
            }
            selection.publish(members, &session);
            publish_primary(&selection, &mut browser, &mut candidates);
            editor.status = format!(
                "已选择{}个普通资产；切到移动模式可整组拖动",
                selection.members.len()
            );
        } else {
            selection.scan = Some(scan);
        }
        return;
    }
    if move_state.owns_mouse() || selection.members.is_empty() || selection.ready_for(&session) {
        return;
    }
    let restart = selection.refresh.as_ref().is_none_or(|refresh| {
        refresh.revision != session.revision
            || refresh.structure != session.placement_structure_revision
            || refresh.terrain != session.terrain_revision
            || refresh.derived_pose != session.placement_derived_pose_revision
    });
    if restart {
        selection.ready = false;
        selection.refresh = Some(SelectionRefresh {
            revision: session.revision,
            structure: session.placement_structure_revision,
            terrain: session.terrain_revision,
            derived_pose: session.placement_derived_pose_revision,
            cursor: 0,
            members: Vec::new(),
        });
    }
    let mut refresh = selection.refresh.take().expect("已建立局部复核");
    if !browser.placement_index_ready(session.placement_structure_revision) {
        selection.refresh = Some(refresh);
        return; // 索引未完整采用时不推进或重复复制局部批次。
    }
    let end = (refresh.cursor + super::REFRESH_BUDGET).min(selection.members.len());
    for member in &selection.members[refresh.cursor..end] {
        let Some(index) =
            browser.placement_record_index(&member.id, session.placement_structure_revision)
        else {
            continue;
        };
        if let Some(record) = draft
            .placements
            .placements
            .get(index)
            .filter(|record| record.id == member.id && record.editor.selectable()
                && record.building_preset_id.is_none() && !is_automatic_volume(record))
        {
            let position = match placement_record_world_position(draft, record) {
                Ok(position) => position,
                Err(error) => { editor.status = error; continue; }
            };
            refresh.members.push(SelectedMember {
                id: member.id.clone(),
                index,
                position,
            });
        }
    }
    refresh.cursor = end;
    if end == selection.members.len() {
        selection.publish(refresh.members, &session);
        // 结构历史可能把多选缩为单项；恢复其完整实例目标，但同ID已选子网格仍保留精确身份。
        let primary_matches = selection.members.len() == 1
            && browser.selected_placement_id.as_deref() == Some(selection.members[0].id.as_str());
        if !primary_matches {
            publish_primary(&selection, &mut browser, &mut candidates);
        }
    } else {
        selection.refresh = Some(refresh);
    }
}
