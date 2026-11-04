//! 当前单个有界网格的世界拓扑与选择；遮挡、面拾取及框选复用正式三角化。

use super::{
    geometry::{triangle_distance, world_vertices},
    types::MeshElementKind,
};
use crate::viewport::placement_move::MoveCamera;
use bevy::prelude::*;
use feathrise_map_content::MapPlacementRecord;

pub(super) struct WorldMesh {
    pub vertices: Vec<Vec3>,
    pub edges: Vec<[usize; 2]>,
    pub faces: Vec<Vec<usize>>,
    pub triangles: Vec<(usize, [usize; 3])>,
}

pub(super) fn world_mesh(record: &MapPlacementRecord) -> Option<WorldMesh> {
    let geometry = record.asset_target.editable_geometry()?;
    // 冻结目标时已做完整闭壳校验；逐帧视图仍守住数量与索引边界，防止坏作者数据越界。
    // 此处不重复执行二次三角自交检查，只检查本次派生将实际索引的固定容量。
    if !(4..=64).contains(&geometry.vertex_count)
        || !(4..=64).contains(&geometry.face_count)
        || geometry.faces().iter().any(|face| {
            !(3..=16).contains(&face.vertex_count)
                || face
                    .indices()
                    .iter()
                    .any(|&index| index >= geometry.vertex_count)
        })
    {
        return None;
    }
    let vertices = world_vertices(record)?;
    if vertices.iter().any(|point| !point.is_finite()) {
        return None;
    }
    let edges = geometry.edges();
    let triangles = geometry.triangles_with_faces();
    if edges.len() > 126 || triangles.is_empty() || triangles.len() > 124 {
        return None;
    }
    Some(WorldMesh {
        vertices,
        edges,
        faces: geometry
            .faces()
            .iter()
            .map(|face| face.indices().iter().map(|&v| usize::from(v)).collect())
            .collect(),
        triangles,
    })
}

impl WorldMesh {
    pub fn count(&self, kind: MeshElementKind) -> usize {
        match kind {
            MeshElementKind::Vertex => self.vertices.len(),
            MeshElementKind::Edge => self.edges.len(),
            MeshElementKind::Face => self.faces.len(),
        }
    }
    pub fn all(&self, kind: MeshElementKind) -> u128 {
        let count = self.count(kind);
        if count == 128 {
            u128::MAX
        } else {
            (1u128 << count) - 1
        }
    }
    pub fn element_mask(&self, kind: MeshElementKind, index: usize) -> u64 {
        match kind {
            MeshElementKind::Vertex => 1u64 << index,
            MeshElementKind::Edge => self.edges[index]
                .iter()
                .fold(0, |mask, &v| mask | (1u64 << v)),
            MeshElementKind::Face => self.faces[index]
                .iter()
                .fold(0, |mask, &v| mask | (1u64 << v)),
        }
    }
    pub fn selected_vertices(&self, kind: MeshElementKind, selected: u128) -> u64 {
        (0..self.count(kind))
            .filter(|&i| selected & (1u128 << i) != 0)
            .fold(0, |mask, i| mask | self.element_mask(kind, i))
    }
    pub fn convert_selection(
        &self,
        old: MeshElementKind,
        new: MeshElementKind,
        selected: u128,
    ) -> u128 {
        let vertices = self.selected_vertices(old, selected);
        (0..self.count(new))
            .filter(|&i| {
                let mask = self.element_mask(new, i);
                mask & vertices == mask
            })
            .fold(0, |bits, i| bits | (1u128 << i))
    }
    pub fn nearest_face(&self, ray: Ray3d) -> Option<(usize, f32)> {
        self.triangles
            .iter()
            .filter_map(|&(face, [a, b, c])| {
                triangle_distance(ray, self.vertices[a], self.vertices[b], self.vertices[c])
                    .map(|distance| (face, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
    pub fn visible(&self, camera: MoveCamera, point: Vec3) -> bool {
        let Some(ray) = camera.project(point).and_then(|cursor| camera.ray(cursor)) else {
            return false;
        };
        let distance = (point - ray.origin).dot(*ray.direction);
        let epsilon = 0.00005 + distance.abs() * f32::EPSILON * 2.0;
        self.nearest_face(ray)
            .is_none_or(|(_, hit)| hit + epsilon >= distance)
    }
    pub fn center(&self, kind: MeshElementKind, index: usize) -> Vec3 {
        let mask = self.element_mask(kind, index);
        super::geometry::selection_center(&self.vertices, mask).unwrap_or(Vec3::ZERO)
    }
    /// 面标记与框选共用实际可见三角的重心；凹面均值可能落在面外，非平面均值也未必在表面。
    pub fn visible_representative(
        &self,
        kind: MeshElementKind,
        index: usize,
        camera: MoveCamera,
    ) -> Option<Vec3> {
        if kind == MeshElementKind::Face {
            self.triangles
                .iter()
                .filter(|&&(face, _)| face == index)
                .map(|&(_, [a, b, c])| {
                    (self.vertices[a] + self.vertices[b] + self.vertices[c]) / 3.0
                })
                .find(|&point| self.visible(camera, point))
        } else {
            let point = self.center(kind, index);
            self.visible(camera, point).then_some(point)
        }
    }
    pub fn pick(&self, kind: MeshElementKind, camera: MoveCamera, cursor: Vec2) -> Option<usize> {
        if kind == MeshElementKind::Face {
            let ray = camera.ray(cursor)?;
            let (face, distance) = self.nearest_face(ray)?;
            // 射线从近面出发但没有远限；只复核实际命中位置，不能让正交远面外的壳被点选，
            // 也不能要求整个面或面中心可投影，部分裁剪后仍可见的表面应继续接受点击。
            return camera.project(ray.get_point(distance)).map(|_| face);
        }
        let logical = camera.size * camera.inverse_scale;
        let pointer = cursor * logical;
        let mut nearest = None;
        for index in 0..self.count(kind) {
            let (point, distance) = if kind == MeshElementKind::Vertex {
                let point = self.vertices[index];
                let Some(screen) = camera.project(point) else {
                    continue;
                };
                (point, pointer.distance(screen * logical))
            } else {
                let [a, b] = self.edges[index];
                let (Some(start), Some(end)) = (
                    camera.project(self.vertices[a]),
                    camera.project(self.vertices[b]),
                ) else {
                    continue;
                };
                let edge = (end - start) * logical;
                if edge.length_squared() < 0.01 {
                    continue;
                }
                let t =
                    ((pointer - start * logical).dot(edge) / edge.length_squared()).clamp(0.0, 1.0);
                let Some(ray) = camera.ray(start + (end - start) * t) else {
                    continue;
                };
                let direction = (self.vertices[b] - self.vertices[a]).normalize_or_zero();
                let ad = direction.dot(*ray.direction);
                let w = ray.origin - self.vertices[a];
                let denominator = 1.0 - ad * ad;
                let length = self.vertices[a].distance(self.vertices[b]);
                let along = if denominator > 1e-6 {
                    ((direction.dot(w) - ad * ray.direction.dot(w)) / denominator)
                        .clamp(0.0, length)
                } else {
                    t * length
                };
                (
                    self.vertices[a] + direction * along,
                    pointer.distance(start * logical + edge * t),
                )
            };
            if distance <= 10.0
                && self.visible(camera, point)
                && nearest.is_none_or(|(_, best)| distance < best)
            {
                nearest = Some((index, distance));
            }
        }
        nearest.map(|(i, _)| i)
    }
    /// 框内点、边中点或可见面标记才被纳入；面标记与绘制共用同一真实表面代表点。
    pub fn in_box(
        &self,
        kind: MeshElementKind,
        camera: MoveCamera,
        start: Vec2,
        end: Vec2,
    ) -> u128 {
        let min = start.min(end);
        let max = start.max(end);
        (0..self.count(kind))
            .filter(|&index| {
                self.visible_representative(kind, index, camera)
                    .and_then(|point| camera.project(point))
                    .is_some_and(|screen| screen.cmpge(min).all() && screen.cmple(max).all())
            })
            .fold(0, |bits, index| bits | (1u128 << index))
    }
}
