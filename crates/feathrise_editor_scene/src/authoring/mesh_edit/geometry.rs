//! 网格世界位移的半毫米逆算与共享投影工具；保留旧八点测试夹具的固定拓扑辅助函数。

pub(super) use super::mesh_geometry::*;
pub(super) use super::transform_math::{
    pick_transform_handle, rotated_record, rotation_delta, scale_factor, scaled_record,
};
#[cfg(test)]
use super::types::MeshElementKind;
use super::types::MoveConstraint;
use super::selection::MeshSelection;
use crate::viewport::placement_move::MoveCamera;
use bevy::prelude::*;
use feathrise_scene_content::ScenePlacementRecord;
#[cfg(test)]
use feathrise_scene_content::{EDITABLE_CUBOID_EDGES, EDITABLE_CUBOID_FACES};

#[cfg(test)]
pub(super) fn world_vertices(record: &ScenePlacementRecord) -> Option<Vec<Vec3>> {
    let geometry = record.asset_target.editable_geometry()?;
    Some(
        geometry
            .vertices()
            .iter()
            .map(|&point| world_vertex(record, point))
            .collect(),
    )
}

pub(super) fn world_vertex(record: &ScenePlacementRecord, point: [i32; 3]) -> Vec3 {
    let local = Vec3::from_array(point.map(|value| value as f32 * 0.0005));
    Vec3::from_array(record.translation)
        + Quat::from_rotation_y(record.yaw_degrees.to_radians())
            * (local * Vec3::from_array(record.scale))
}

#[cfg(test)]
pub(super) fn element_vertices(kind: MeshElementKind, index: usize) -> u64 {
    match kind {
        MeshElementKind::Vertex => 1 << index,
        MeshElementKind::Edge => EDITABLE_CUBOID_EDGES[index]
            .iter()
            .fold(0, |bits, &v| bits | (1 << v)),
        MeshElementKind::Face => EDITABLE_CUBOID_FACES[index]
            .iter()
            .fold(0, |bits, &v| bits | (1 << v)),
    }
}

#[cfg(test)]
pub(super) fn selected_vertices(kind: MeshElementKind, selected: u128) -> u64 {
    (0..kind.count())
        .filter(|index| selected & (1 << index) != 0)
        .fold(0, |bits, index| bits | element_vertices(kind, index))
}

#[cfg(test)]
pub(super) fn selection_for_kind(
    old: MeshElementKind,
    new: MeshElementKind,
    selected: u128,
) -> u128 {
    let vertices = selected_vertices(old, selected);
    (0..new.count())
        .filter(|&index| {
            let mask = element_vertices(new, index);
            vertices & mask == mask
        })
        .fold(0, |bits, index| bits | (1 << index))
}

/// 双面三角射线取最近实体表面；背面不能穿过前面继续命中隐藏元素。
#[cfg(test)]
fn triangle_distance(ray: Ray3d, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let ab = b - a;
    let ac = c - a;
    let p = ray.direction.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inverse = det.recip();
    let t = ray.origin - a;
    let u = t.dot(p) * inverse;
    if !(-1e-5..=1.00001).contains(&u) {
        return None;
    }
    let q = t.cross(ab);
    let v = ray.direction.dot(q) * inverse;
    if v < -1e-5 || u + v > 1.00001 {
        return None;
    }
    let distance = ac.dot(q) * inverse;
    (distance.is_finite() && distance >= 0.0).then_some(distance)
}

#[cfg(test)]
pub(super) fn nearest_face(ray: Ray3d, vertices: &[Vec3]) -> Option<(usize, f32)> {
    let mut nearest = None;
    for (face, &[a, b, c, d]) in EDITABLE_CUBOID_FACES.iter().enumerate() {
        for [a, b, c] in [[a, b, c], [a, c, d]] {
            let Some(distance) = triangle_distance(ray, vertices[a], vertices[b], vertices[c])
            else {
                continue;
            };
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((face, distance));
            }
        }
    }
    nearest
}

#[cfg(test)]
pub(super) fn visible_point(camera: MoveCamera, point: Vec3, vertices: &[Vec3]) -> bool {
    let Some(ray) = camera.project(point).and_then(|cursor| camera.ray(cursor)) else {
        return false;
    };
    let distance = (point - ray.origin).dot(*ray.direction);
    // 面边与顶点正好在三角形边界上，只容忍射线计算的几次浮点舍入，不能穿透薄面。
    let epsilon = 0.00005 + distance.abs() * f32::EPSILON * 2.0;
    nearest_face(ray, vertices).is_none_or(|(_, hit)| hit + epsilon >= distance)
}

#[cfg(test)]
pub(super) fn pick_element(
    kind: MeshElementKind,
    camera: MoveCamera,
    cursor: Vec2,
    vertices: &[Vec3],
) -> Option<usize> {
    if kind == MeshElementKind::Face {
        return nearest_face(camera.ray(cursor)?, vertices).map(|(face, _)| face);
    }
    let logical = camera.size * camera.inverse_scale;
    let pointer = cursor * logical;
    let mut nearest = None;
    for index in 0..kind.count() {
        let (point, distance) = match kind {
            MeshElementKind::Vertex => {
                let point = vertices[index];
                let Some(screen) = camera.project(point) else {
                    continue;
                };
                (point, pointer.distance(screen * logical))
            }
            MeshElementKind::Edge => {
                let [a, b] = EDITABLE_CUBOID_EDGES[index];
                let (Some(start), Some(end)) =
                    (camera.project(vertices[a]), camera.project(vertices[b]))
                else {
                    continue;
                };
                let segment = (end - start) * logical;
                if segment.length_squared() < 0.01 {
                    continue;
                }
                let t = ((pointer - start * logical).dot(segment) / segment.length_squared())
                    .clamp(0.0, 1.0);
                // 透视下屏幕参数不是世界参数：从射线和线段求最近世界点再验证遮挡。
                let Some(ray) = camera.ray(start + (end - start) * t) else {
                    continue;
                };
                let edge = vertices[b] - vertices[a];
                let direction = edge.normalize_or_zero();
                let ad = direction.dot(*ray.direction);
                let w = ray.origin - vertices[a];
                let denominator = 1.0 - ad * ad;
                let world_t = if denominator > 1e-6 {
                    ((direction.dot(w) - ad * ray.direction.dot(w)) / denominator)
                        .clamp(0.0, edge.length())
                } else {
                    t * edge.length()
                };
                (
                    vertices[a] + direction * world_t,
                    pointer.distance(start * logical + segment * t),
                )
            }
            MeshElementKind::Face => unreachable!(),
        };
        if distance <= 10.0
            && visible_point(camera, point, vertices)
            && nearest.is_none_or(|(_, best)| distance < best)
        {
            nearest = Some((index, distance));
        }
    }
    nearest.map(|(index, _)| index)
}

pub(super) fn selection_center(vertices: &[Vec3], selected: &MeshSelection) -> Option<Vec3> {
    let mut sum = bevy::math::DVec3::ZERO;
    let mut count = 0;
    for index in selected.iter() {
        sum += vertices.get(index)?.as_dvec3();
        count += 1;
    }
    (count > 0).then(|| (sum / f64::from(count)).as_vec3())
}

pub(super) fn constraint_point(
    camera: MoveCamera,
    cursor: Vec2,
    origin: Vec3,
    constraint: MoveConstraint,
) -> Option<Vec3> {
    let ray = camera.ray(cursor)?;
    if let Some(axis) = constraint.axis() {
        let ad = axis.dot(*ray.direction);
        let denominator = 1.0 - ad * ad;
        if denominator <= 0.0001 {
            return None;
        } // 端视轴拒绝极端位移，切换视角后再开始移动。
        let w = ray.origin - origin;
        let scalar = (axis.dot(w) - ad * ray.direction.dot(w)) / denominator;
        return scalar.is_finite().then_some(origin + axis * scalar);
    }
    let normal = *camera.ray(Vec2::ZERO)?.direction;
    let denominator = ray.direction.dot(normal);
    if denominator.abs() <= 0.0001 {
        return None;
    }
    let distance = (origin - ray.origin).dot(normal) / denominator;
    (distance.is_finite() && distance >= 0.0).then(|| ray.get_point(distance))
}

pub(super) fn snapped_delta(
    origin: Vec3,
    delta: Vec3,
    constraint: MoveConstraint,
    snap: feathrise_editor_core::EditorSnapSettings,
    control: bool,
) -> Vec3 {
    if !snap.active(control) {
        return delta;
    }
    let mut result = delta;
    for i in 0..3 {
        if constraint.axis().is_some_and(|axis| axis[i] == 0.0) {
            continue;
        }
        let phase = if snap.mode == feathrise_editor_core::EditorSnapMode::Relative {
            origin[i]
        } else {
            0.0
        };
        result[i] = ((origin[i] + delta[i] - phase) / snap.translation_meters).round()
            * snap.translation_meters
            + phase
            - origin[i];
    }
    result
}

/// 世界位移经根的逆旋转/逆缩放后写局部坐标，选边/面只移动其关联顶点。
pub(super) fn translated_record(
    before: &ScenePlacementRecord,
    selected: &MeshSelection,
    world_delta: Vec3,
) -> Result<ScenePlacementRecord, String> {
    if !world_delta.is_finite() {
        return Err("移动结果不是有限坐标".into());
    }
    let mut after = before.clone();
    let geometry = before
        .asset_target
        .editable_geometry()
        .ok_or("当前对象不支持点边面编辑")?;
    let local = Quat::from_rotation_y(-before.yaw_degrees.to_radians()) * world_delta
        / Vec3::from_array(before.scale);
    let mut vertices = geometry.vertices().to_vec();
    for index in selected.iter() {
        let vertex = vertices.get_mut(index).ok_or("顶点选择已经超出当前拓扑")?;
        for axis in 0..3 {
            let value = vertex[axis] as f64 + f64::from(local[axis]) * 2000.0;
            if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
                return Err("顶点坐标超过可保存范围".into());
            }
            vertex[axis] = value.round() as i32;
        }
    }
    // 零位移不把参数化立方体无意义地转换为编辑网格，也不产生撤销或脏标记。
    if vertices == geometry.vertices() {
        return Ok(after);
    }
    // 含越界、翻转或自交的候选由共享几何构造入口拒绝。
    after.asset_target = before.asset_target.with_editable_geometry(geometry.with_vertices(vertices)?)?;
    Ok(after)
}
