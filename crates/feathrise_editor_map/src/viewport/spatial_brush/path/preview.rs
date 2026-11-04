//! 有界的完整弧长轮廓和所选节点；真实截面数据与后台密度使用同一姿态。
use super::PathState;
use crate::viewport::spatial_brush::types::SpatialBrushGizmos;
use bevy::prelude::*;
use feathrise_map_terrain::volume::{VolumeBrush, VolumeBrushFrame, VolumeBrushShape};

fn ring(brush: VolumeBrush) -> ([Vec3; 16], usize) {
    let frame = brush.frame.unwrap();
    let q = Quat::from_array(frame.rotation);
    let half = Vec3::from_array(frame.half_extents);
    let center = Vec3::from_array(brush.from);
    let mut vertices = [center; 16];
    let count = if brush.shape == VolumeBrushShape::Box {
        4
    } else {
        16
    };
    for (i, vertex) in vertices.iter_mut().take(count).enumerate() {
        let local = if count == 4 {
            Vec3::new(
                if i == 0 || i == 3 { -half.x } else { half.x },
                if i < 2 { -half.y } else { half.y },
                0.,
            )
        } else {
            let angle = i as f32 / 16. * std::f32::consts::TAU;
            Vec3::new(angle.cos() * half.x, 0., angle.sin() * half.z)
        };
        *vertex = center + q * local;
    }
    (vertices, count)
}

pub(in crate::viewport::spatial_brush) fn draw(
    state: &PathState,
    center: Vec3,
    gizmos: &mut Gizmos<SpatialBrushGizmos>,
) {
    let color = if state.busy() {
        Color::srgb(1., 0.75, 0.2)
    } else {
        Color::srgb(0.18, 0.95, 1.)
    };
    let mut previous: Option<([Vec3; 16], usize)> = None;
    for (index, brush) in state.preview.iter().copied().enumerate() {
        let (vertices, count) = ring(brush);
        if index % 4 == 0 || index + 1 == state.preview.len() {
            for i in 0..count {
                gizmos.line(vertices[i], vertices[(i + 1) % count], color);
            }
        }
        if let Some((before, previous_count)) = previous {
            for i in 0..count.min(previous_count) {
                gizmos.line(before[i], vertices[i], color);
            }
        }
        previous = Some((vertices, count));
    }
    // 只有当前节点附近的固定小组标记，不在稳定帧遍历整条作者控制点数组。
    if !state.nodes.is_empty() {
        for i in state.visible_nodes().into_iter().flatten() {
            let point = Vec3::from_array(state.nodes[i]);
            let tint = if Some(i) == state.selected {
                Color::srgb(1., 0.7, 0.2)
            } else {
                Color::srgb(0.4, 1., 0.4)
            };
            gizmos.sphere(Isometry3d::from_translation(point), 0.12, tint);
        }
    }
    let parameters = state.parameters;
    let brush = if let Some(plan) = &state.plan {
        let brush = state
            .selected
            .and_then(|i| plan.node_brush(i))
            .unwrap_or_else(|| {
                plan.brush_at(
                    plan.length(),
                    feathrise_map_terrain::volume::VolumeBrushKind::Dig,
                    1.,
                    0.25,
                )
            });
        let frame = brush.frame.unwrap();
        let floor = Vec3::from_array(brush.from)
            - Vec3::from_array(frame.plane_normal) * (parameters.height * 0.5);
        brush.translated(center - floor)
    } else {
        VolumeBrush {
            shape: VolumeBrushShape::Box,
            from: (center + Vec3::Y * parameters.height * 0.5).to_array(),
            frame: Some(VolumeBrushFrame {
                rotation: Quat::IDENTITY.to_array(),
                half_extents: [parameters.width * 0.5, parameters.height * 0.5, 0.25],
                plane_normal: Vec3::Y.to_array(),
                plane_distance: center.y,
            }),
            ..Default::default()
        }
    };
    let (vertices, count) = ring(brush);
    for i in 0..count {
        gizmos.line(
            vertices[i],
            vertices[(i + 1) % count],
            Color::srgb(1., 0.8, 0.2),
        );
    }
}

/// 拾取与显示共用端点/相邻节点的固定小集合，地下标记不依赖地形射线。
pub(in crate::viewport::spatial_brush) fn pick(
    state: &PathState,
    camera: crate::viewport::placement_move::MoveCamera,
    cursor: Vec2,
) -> Option<usize> {
    state
        .visible_nodes()
        .into_iter()
        .flatten()
        .filter_map(|i| {
            let screen = camera.project(Vec3::from_array(state.nodes[i]))? + Vec2::splat(0.5);
            let distance =
                ((screen - cursor) * camera.size * camera.inverse_scale).length_squared();
            (distance <= 8. * 8.).then_some((i, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}
