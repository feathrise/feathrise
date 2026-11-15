//! 元素线框和可见点标记，沿用编辑器专用 Gizmo 层与正式相机投影。

use super::{
    geometry::{WorldMesh, selection_center},
    resources::MeshEditState,
    selection::MeshSelection,
    transform_math::{move_arrows, rotation_rings, scale_handles},
    types::{MeshEditTool, MeshElementKind, MoveConstraint},
};
use crate::{
    components::SceneEditorViewport,
    resources::{SceneEditorState, SceneEditorViewportState},
    viewport::{
        SceneEditorBrushGizmos,
        placement_move::{MoveAxis, MoveCamera},
    },
};
use bevy::prelude::*;

/// 拓扑预览使用候选编号，细分可直接显示按当前点／边／面模式映射后的新选择。
pub(super) fn display_selection(
    state: &MeshEditState,
    mesh: &WorldMesh,
) -> (MeshElementKind, MeshSelection, Option<usize>) {
    if let Some(movement) = &state.movement {
        if let Some(selected) = &movement.result_selection {
            return (state.kind, selected.clone(), None);
        }
        if movement.topology.is_some() && movement.visual_mesh.is_some() {
            return (MeshElementKind::Face, mesh.all(MeshElementKind::Face), None);
        }
    }
    (state.kind, state.selected.clone(), state.hover)
}

pub(super) fn draw_elements(
    state: Res<MeshEditState>,
    editor: Res<SceneEditorState>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<&ComputedNode, With<SceneEditorViewport>>,
    mut gizmos: Gizmos<SceneEditorBrushGizmos>,
) {
    if !editor.open || editor.close_requested || editor.scene_transition_in_progress {
        return;
    }
    if state.target.is_none() {
        return;
    }
    let mesh = state
        .movement
        .as_ref()
        .and_then(|movement| movement.visual_mesh.as_deref())
        .or(state.world_mesh.as_deref());
    #[cfg(test)]
    let fixture_mesh = mesh.is_none().then(|| {
        let record = state.movement.as_ref().map_or(&state.target.as_ref().unwrap().record, |movement| &movement.preview);
        super::geometry::world_mesh(record)
    }).flatten(); // 旧 GPU 夹具直接挂记录，生产走冻结快照。
    #[cfg(test)]
    let mesh = mesh.or(fixture_mesh.as_ref());
    let Some(mesh) = mesh else {
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
    let (kind, selected_elements, element_hover) = display_selection(&state, mesh);
    // 大网格保留精确拾取，CPU 标记按固定预算绘制，完整网格边仍由线框着色负责。
    let edge_hover = element_hover.filter(|_| kind == MeshElementKind::Edge);
    let edges = mesh.edges();
    for index in overlay_indices(edges.len(), 1024, edge_hover, &selected_elements, kind == MeshElementKind::Edge) {
        let [a, b] = edges[index];
        if !mesh.visible(camera, (vertices[a] + vertices[b]) * 0.5) {
            continue;
        }
        // 同一条真实边只提交一次；面选择直接决定边色，避免重叠橙线被深度测试丢弃。
        let adjacent = mesh.edge_faces(index);
        let chosen = kind == MeshElementKind::Edge && selected_elements.contains(index)
            || kind == MeshElementKind::Face
                && adjacent.into_iter().any(|face| selected_elements.contains(face));
        let hover = edge_hover == Some(index)
            || (kind == MeshElementKind::Face && element_hover.is_some_and(|face| adjacent.contains(&face)));
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
        for index in overlay_indices(vertices.len(), 512, element_hover, &selected_elements, true) {
            let point = vertices[index];
            if !mesh.visible(camera, point) {
                continue;
            }
            let color = if element_hover == Some(index) {
                hovered
            } else if selected_elements.contains(index) {
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
        for index in overlay_indices(mesh.faces().len(), 512, element_hover, &selected_elements, true) {
            let hover = element_hover == Some(index);
            if !hover && !selected_elements.contains(index) {
                continue;
            }
            let Some(center) = mesh.visible_representative(MeshElementKind::Face, index, camera)
            else {
                continue;
            };
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
    if tool == MeshEditTool::Select {
        return;
    }
    let origin = if let Some(movement) = &state.movement {
        // 移动手柄采用已准备候选的中心，避免密集选区在渲染系统里逐帧求和。
        movement.visual_origin.or(Some(movement.origin))
    } else if let Some((kind, selected, origin)) = &state.pivot
        && *kind == state.kind && selected.same_storage(&state.selected)
    {
        *origin
    } else if vertices.len() <= super::jobs::IMMEDIATE_VERTICES {
        selection_center(
            vertices,
            &mesh.selected_vertices(state.kind, &state.selected),
        )
    } else {
        None
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

/// 抽样时手动选中的小选区与悬停元素仍绘制，确保密集拓扑可逐点、逐边继续编辑。
fn overlay_indices(
    count: usize,
    budget: usize,
    hover: Option<usize>,
    selected: &MeshSelection,
    add_selected: bool,
) -> impl Iterator<Item = usize> + '_ {
    let stride = count.div_ceil(budget).max(1);
    let add_selected = add_selected && stride > 1 && selected.len() <= budget;
    (0..count).step_by(stride)
        .chain(selected.iter().take(if add_selected { budget } else { 0 })
            .filter(move |&index| index < count && index % stride != 0))
        .chain(hover.filter(move |&index| index < count && index % stride != 0
            && !(add_selected && selected.contains(index))))
}
