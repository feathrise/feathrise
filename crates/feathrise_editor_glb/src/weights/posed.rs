//! 在后台用冻结的关节矩阵重建动作帧的实际点位与遮挡。

use super::{
    asset::Document,
    spatial::{Occlusion, Points},
};
use crate::files::active;
use bevy::prelude::*;
use feathrise_editor_glb_model::weights::{PoseSample, VertexWeights};
use std::sync::atomic::AtomicBool;

pub(super) fn deform(
    pose: Vec<PoseSample>,
    doc: &Document,
    selected_surface: usize,
    visible: bool,
    cancel: &AtomicBool,
) -> Result<(Points, Option<Occlusion>), String> {
    let mut selected = None;
    let mut triangles = Vec::new();
    for sample in pose {
        active(cancel)?;
        let (surface, positions, indices) = match &sample {
            PoseSample::Skin { surface, .. } => {
                let (positions, indices) = doc.surfaces[*surface].geometry();
                (Some(*surface), positions, indices)
            }
            PoseSample::Rigid(index, _) => {
                let shape = &doc.rigid[*index];
                (None, shape.positions.as_slice(), shape.indices.as_slice())
            }
        };
        if surface != Some(selected_surface) && !visible {
            continue;
        }
        let mut points = Vec::with_capacity(positions.len());
        for (group, block) in positions.chunks(2048).enumerate() {
            active(cancel)?;
            for (offset, &position) in block.iter().enumerate() {
                let point = Vec3::from_array(position);
                points.push(match &sample {
                    PoseSample::Skin {
                        matrices, weights, ..
                    } => skin_point(point, weights[group * 2048 + offset], matrices),
                    PoseSample::Rigid(_, matrix) => matrix.transform_point3(point),
                });
            }
        }
        if points.iter().any(|point| !point.is_finite()) {
            return Err("问题帧的蒙皮坐标无效".into());
        }
        if visible {
            triangles.extend(indices.chunks_exact(3).map(|face| {
                [
                    points[face[0] as usize],
                    points[face[1] as usize],
                    points[face[2] as usize],
                ]
            }));
        }
        if surface == Some(selected_surface) {
            if selected.is_some() {
                return Err("动作容器复用了当前蒙皮图元".into());
            }
            selected = Some(points);
        }
    }
    active(cancel)?;
    let points = selected.ok_or("动作容器没有当前蒙皮图元")?;
    Ok((
        Points::for_selection(points),
        visible.then(|| Occlusion::new(triangles)),
    ))
}

// 与 Bevy 蒙皮一致，使用关节世界矩阵乘逆绑定矩阵，保持 GLB 顶点下标。
fn skin_point(point: Vec3, row: VertexWeights, matrices: &[Mat4]) -> Vec3 {
    row.joints
        .into_iter()
        .zip(row.weights)
        .filter(|(_, weight)| *weight > 0.0)
        .map(|(joint, weight)| matrices[joint as usize].transform_point3(point) * weight)
        .sum()
}
