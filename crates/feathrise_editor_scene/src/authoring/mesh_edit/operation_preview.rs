//! 建模手势的屏幕参数到受检拓扑配方；每次从冻结的before求完整结果，避免累计舍入。

use super::{geometry::*, topology_ops, types::*};
use bevy::prelude::*;
use feathrise_scene_content::ScenePlacementRecord;

pub(super) fn operation_label(movement: &ElementMove) -> &'static str {
    movement
        .topology
        .map_or_else(|| movement.tool.label(), TopologyOperation::label)
}

pub(super) fn topology_preview(
    movement: &ElementMove,
    mesh: &WorldMesh,
    kind: MeshElementKind,
    cursor: Vec2,
    control: bool,
) -> Result<ScenePlacementRecord, String> {
    let operation = movement.topology.ok_or("当前不是拓扑操作")?;
    let start = movement.start_cursor.unwrap_or(cursor);
    let pixels = (cursor - start) * movement.camera.size * movement.camera.inverse_scale;
    let handle = movement
        .camera
        .handle_length(movement.origin)
        .ok_or("操作中心不在有效视口内")?;
    let edge = mesh
        .edges()
        .iter()
        .map(|&[a, b]| mesh.vertices[a].distance(mesh.vertices[b]))
        .fold(f32::INFINITY, f32::min);
    let base = (edge * 0.12).clamp(0.001, 1.0);
    let mut width = base + pixels.x / 80.0 * handle;
    if movement.snapping.active(control) {
        let step = movement.snapping.translation_meters;
        width = (width / step).round() * step;
    }
    match operation {
        TopologyOperation::Extrude { face, normal } => {
            let mut distance = if let (Some(a), Some(b)) = (
                movement.camera.project(movement.origin),
                movement.camera.project(movement.origin + normal * handle),
            ) {
                let axis = (b - a) * movement.camera.size * movement.camera.inverse_scale;
                if axis.length() >= 12.0 {
                    pixels.dot(axis.normalize()) / axis.length() * handle
                } else {
                    pixels.x / 80.0 * handle
                }
            } else {
                return Err("挤出方向越过相机裁剪面".into());
            };
            if movement.snapping.active(control) {
                distance = (distance / movement.snapping.translation_meters).round()
                    * movement.snapping.translation_meters;
            }
            feathrise_scene_content::extruded_placement_record(&movement.before, face, distance)
        }
        TopologyOperation::Inset => {
            topology_ops::inset_record(&movement.before, kind, &movement.selected_elements, width)
        }
        TopologyOperation::Bevel => {
            topology_ops::bevel_record(&movement.before, kind, &movement.selected_elements, width)
        }
        TopologyOperation::LoopCut => topology_ops::loop_cut_record(
            &movement.before,
            kind,
            &movement.selected_elements,
            (0.5 + pixels.x / 200.0).clamp(0.05, 0.95),
        ),
        TopologyOperation::Knife => {
            if pixels.length() < 8.0 {
                return Err("在视口拖出切割线，再确认；Esc取消".into());
            }
            let a = constraint_point(
                movement.camera,
                start,
                movement.origin,
                MoveConstraint::View,
            )
            .ok_or("切割起点无效")?;
            let b = constraint_point(
                movement.camera,
                cursor,
                movement.origin,
                MoveConstraint::View,
            )
            .ok_or("切割终点无效")?;
            let ray = movement.camera.ray(start).ok_or("切割视图无效")?;
            let normal = (b - a)
                .cross(*ray.direction)
                .try_normalize()
                .ok_or("切割线退化")?;
            topology_ops::knife_record(&movement.before, a, normal)
        }
        TopologyOperation::Subdivide(_) => unreachable!("细分由参数面板刷新"),
    }
}

pub(super) fn pointer_preview(
    movement: &ElementMove,
    mesh: &WorldMesh,
    kind: MeshElementKind,
    cursor: Vec2,
    control: bool,
) -> Result<ScenePlacementRecord, String> {
    let start = movement.start_cursor.ok_or("操作起点无效")?;
    let camera = movement.camera;
    let origin = movement.origin;
    let constraint = movement.constraint;
    let snapping = movement.snapping;
    let before = &movement.before;
    let selected = &movement.selected_vertices;
    if movement.topology.is_some() {
        return topology_preview(movement, mesh, kind, cursor, control);
    }
    match movement.tool {
        MeshEditTool::Scale => {
            let factor = scale_factor(camera, start, cursor, origin, constraint, snapping, control)?;
            scaled_record(before, selected, origin, constraint, factor)
        }
        MeshEditTool::Rotate => {
            let (axis, mut radians) = rotation_delta(camera, start, cursor, origin, constraint)?;
            // 元素没有独立欧拉角；围绕选区中心对相对角度吸附。
            if snapping.active(control) {
                let step = snapping.rotation_degrees.to_radians();
                radians = (radians / step).round() * step;
            }
            rotated_record(before, selected, origin, axis, radians)
        }
        _ => {
            let start = constraint_point(camera, start, origin, constraint);
            let point = constraint_point(camera, cursor, origin, constraint);
            let (start, point) = start.zip(point).ok_or("当前视角与约束轴平行，请取消后调整视角")?;
            translated_record(before, selected, snapped_delta(
                origin, point - start, constraint, snapping, control,
            ))
        }
    }
}
