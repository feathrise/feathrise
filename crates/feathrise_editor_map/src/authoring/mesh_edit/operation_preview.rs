//! 建模手势的屏幕参数到受检拓扑配方；每次从冻结的before求完整结果，避免累计舍入。

use super::{geometry::*, topology_ops, types::*};
use bevy::prelude::*;
use feathrise_map_content::MapPlacementRecord;

pub(super) fn operation_label(movement: &ElementMove) -> &'static str {
    movement
        .topology
        .map_or_else(|| movement.tool.label(), TopologyOperation::label)
}

pub(super) fn topology_preview(
    movement: &ElementMove,
    kind: MeshElementKind,
    cursor: Vec2,
    control: bool,
) -> Result<MapPlacementRecord, String> {
    let operation = movement.topology.ok_or("当前不是拓扑操作")?;
    if matches!(operation, TopologyOperation::Subdivide) {
        // 细分不依赖屏幕距离或手柄可见性；工具栏的即时预览和后续帧使用同一纯运算。
        return topology_ops::subdivide_record(&movement.before, kind, movement.selected_elements);
    }
    let start = movement.start_cursor.unwrap_or(cursor);
    let pixels = (cursor - start) * movement.camera.size * movement.camera.inverse_scale;
    let handle = movement
        .camera
        .handle_length(movement.origin)
        .ok_or("操作中心不在有效视口内")?;
    let mesh = world_mesh(&movement.before).ok_or("当前来源不支持网格操作")?;
    let edge = mesh
        .edges
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
            feathrise_map_content::extruded_placement_record(&movement.before, face, distance)
        }
        TopologyOperation::Inset => {
            topology_ops::inset_record(&movement.before, kind, movement.selected_elements, width)
        }
        TopologyOperation::Bevel => {
            topology_ops::bevel_record(&movement.before, kind, movement.selected_elements, width)
        }
        TopologyOperation::LoopCut => topology_ops::loop_cut_record(
            &movement.before,
            kind,
            movement.selected_elements,
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
        TopologyOperation::Subdivide => unreachable!("无参数细分已提前返回"),
    }
}
