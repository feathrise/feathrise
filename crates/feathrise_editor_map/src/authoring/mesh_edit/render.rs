//! 元素线框和可见点标记，沿用编辑器专用 Gizmo 层与正式相机投影。

use super::{
    geometry::{WorldMesh, selection_center, world_mesh},
    resources::MeshEditState,
    transform_math::{move_arrows, rotation_rings, scale_handles},
    types::{MeshEditTool, MeshElementKind, MoveConstraint},
};
use crate::{
    components::MapEditorViewport,
    resources::{MapEditorState, MapEditorViewportState},
    viewport::{
        MapEditorBrushGizmos,
        placement_move::{MoveAxis, MoveCamera},
    },
};
use bevy::prelude::*;

/// 拓扑预览已经改变元素编号，只描出完整候选的真实面轮廓；确认前不把旧选择套到新边上。
pub(super) fn display_selection(
    state: &MeshEditState,
    mesh: &WorldMesh,
) -> (MeshElementKind, u128, Option<usize>) {
    if state
        .movement
        .as_ref()
        .is_some_and(|movement| movement.topology.is_some())
    {
        (MeshElementKind::Face, mesh.all(MeshElementKind::Face), None)
    } else {
        (state.kind, state.selected, state.hover)
    }
}

pub(super) fn draw_elements(
    state: Res<MeshEditState>,
    editor: Res<MapEditorState>,
    viewport: Option<Res<MapEditorViewportState>>,
    nodes: Query<&ComputedNode, With<MapEditorViewport>>,
    mut gizmos: Gizmos<MapEditorBrushGizmos>,
) {
    if !editor.open || editor.close_requested || editor.map_transition_in_progress {
        return;
    }
    let Some(target) = &state.target else {
        return;
    };
    let record = state
        .movement
        .as_ref()
        .map_or(&target.record, |movement| &movement.preview);
    let Some(mesh) = world_mesh(record) else {
        return;
    };
    let vertices = &mesh.vertices;
    let Some(camera) = viewport
        .as_deref()
        .zip(nodes.single().ok())
        .and_then(|(viewport, node)| {
            MoveCamera::new(&editor, viewport, node.inverse_scale_factor())
        })
    else {
        return;
    };
    let dim = Color::srgb(0.20, 0.24, 0.29);
    let selected = Color::srgb(1.0, 0.51, 0.12);
    let hovered = Color::srgb(1.0, 0.81, 0.36);
    let (kind, selected_elements, element_hover) = display_selection(&state, &mesh);
    for (index, &[a, b]) in mesh.edges.iter().enumerate() {
        if !mesh.visible(camera, (vertices[a] + vertices[b]) * 0.5) {
            continue;
        }
        // 同一条真实边只提交一次；面选择直接决定边色，避免重叠橙线被深度测试丢弃。
        let face_contains_edge = |face: &Vec<usize>| {
            (0..face.len()).any(|side| {
                let x = face[side];
                let y = face[(side + 1) % face.len()];
                (x == a && y == b) || (x == b && y == a)
            })
        };
        let chosen = kind == MeshElementKind::Edge && selected_elements & (1 << index) != 0
            || kind == MeshElementKind::Face
                && mesh.faces.iter().enumerate().any(|(face, vertices)| {
                    selected_elements & (1 << face) != 0 && face_contains_edge(vertices)
                });
        let hover = kind == MeshElementKind::Edge && element_hover == Some(index)
            || kind == MeshElementKind::Face
                && element_hover
                    .is_some_and(|face| mesh.faces.get(face).is_some_and(face_contains_edge));
        gizmos.line(
            vertices[a],
            vertices[b],
            if hover {
                hovered
            } else if chosen {
                selected
            } else {
                dim
            },
        );
    }
    if kind == MeshElementKind::Vertex {
        for (index, &point) in vertices.iter().enumerate() {
            if !mesh.visible(camera, point) {
                continue;
            }
            let color = if element_hover == Some(index) {
                hovered
            } else if selected_elements & (1 << index) != 0 {
                selected
            } else {
                dim
            };
            if let Some(corners) = camera.screen_square(point, 3.0) {
                for i in 0..4 {
                    gizmos.line(corners[i], corners[(i + 1) % 4], color);
                }
                gizmos.line(corners[0], corners[2], color);
                gizmos.line(corners[1], corners[3], color);
            }
        }
    }
    if kind == MeshElementKind::Face {
        for index in 0..mesh.faces.len() {
            let Some(center) = mesh.visible_representative(MeshElementKind::Face, index, camera)
            else {
                continue;
            };
            let hover = element_hover == Some(index);
            if !hover && selected_elements & (1 << index) == 0 {
                continue;
            }
            let color = if hover { hovered } else { selected };
            if let Some(corners) = camera.screen_square(center, 4.0) {
                for i in 0..4 {
                    gizmos.line(corners[i], corners[(i + 1) % 4], color);
                }
            }
        }
    }
    if let Some(selection) = &state.box_selection {
        if let (Some(start), Some(end)) = (selection.start, selection.current) {
            let corners = [
                start,
                Vec2::new(end.x, start.y),
                end,
                Vec2::new(start.x, end.y),
            ];
            // 位于相机近面前方的小平面，不让框线被正在选择的实体遮住。
            let points = corners.map(|corner| camera.ray(corner).map(|ray| ray.get_point(0.01)));
            for side in 0..4 {
                if let (Some(a), Some(b)) = (points[side], points[(side + 1) % 4]) {
                    gizmos.line(a, b, hovered);
                }
            }
        }
        return;
    }
    let tool = state
        .movement
        .as_ref()
        .map_or(state.tool, |movement| movement.tool);
    let origin = if let Some(movement) = &state.movement {
        if movement.tool == MeshEditTool::Move {
            // 移动箭头跟随当前预览的选中点；旋转环保持本笔冻结的共同枢轴。
            selection_center(&vertices, movement.selected_vertices).or(Some(movement.origin))
        } else {
            Some(movement.origin)
        }
    } else {
        selection_center(
            &vertices,
            mesh.selected_vertices(state.kind, state.selected),
        )
    };
    if tool != MeshEditTool::Select
        && let Some(origin) = origin
    {
        let color = |constraint| {
            if state.handle_hover == Some(constraint)
                || state
                    .movement
                    .as_ref()
                    .is_some_and(|movement| movement.constraint == constraint)
            {
                return hovered;
            }
            match constraint {
                MoveConstraint::X => MoveAxis::X.color(),
                MoveConstraint::Y => MoveAxis::Y.color(),
                MoveConstraint::Z => MoveAxis::Z.color(),
                MoveConstraint::View => Color::srgb(0.78, 0.80, 0.84),
            }
        };
        match tool {
            MeshEditTool::Move => {
                for arrow in move_arrows(camera, origin).into_iter().flatten() {
                    let [start, tip, left, right] = arrow.world;
                    let color = color(arrow.constraint);
                    gizmos.line(start, tip, color);
                    gizmos.line(tip, left, color);
                    gizmos.line(tip, right, color);
                }
                if let Some(corners) = camera.screen_square(origin, 6.0) {
                    for side in 0..4 {
                        gizmos.line(
                            corners[side],
                            corners[(side + 1) % 4],
                            color(MoveConstraint::View),
                        );
                    }
                }
            }
            MeshEditTool::Rotate => {
                for ring in rotation_rings(camera, origin).into_iter().flatten() {
                    for segment in ring.world.windows(2) {
                        gizmos.line(segment[0], segment[1], color(ring.constraint));
                    }
                }
            }
            MeshEditTool::Scale => {
                for handle in scale_handles(camera, origin).into_iter().flatten() {
                    let [start, tip, a, b, c, d] = handle.world;
                    let color = color(handle.constraint);
                    gizmos.line(start, tip, color);
                    for (a, b) in [(a, b), (b, c), (c, d), (d, a)] {
                        gizmos.line(a, b, color);
                    }
                }
                if let Some(corners) = camera.screen_square(origin, 7.0) {
                    for side in 0..4 {
                        gizmos.line(
                            corners[side],
                            corners[(side + 1) % 4],
                            color(MoveConstraint::View),
                        );
                    }
                }
            }
            MeshEditTool::Select => {}
        }
    }
}
