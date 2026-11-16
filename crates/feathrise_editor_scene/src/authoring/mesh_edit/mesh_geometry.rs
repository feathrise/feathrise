//! 当前网格的世界坐标缓存与选择。遮挡沿用作者几何共享的局部三角加速索引。

mod spatial;

use self::spatial::{Bounds, ScreenIndex};
use super::{selection::MeshSelection, types::MeshElementKind};
use crate::viewport::placement_move::MoveCamera;
use bevy::{math::{DQuat, DVec3}, prelude::*};
use feathrise_scene_content::{EditableMeshGeometry, MeshFace, ScenePlacementAssetTarget, ScenePlacementRecord};

pub(super) struct WorldMesh {
    pub vertices: Vec<Vec3>,
    source: ScenePlacementAssetTarget,
    geometry: EditableMeshGeometry,
    translation: Vec3,
    yaw_degrees: f32,
    scale: Vec3,
    inverse_rotation: DQuat,
    screen: [Option<ScreenIndex>; 3],
}

pub(super) fn world_mesh(record: &ScenePlacementRecord) -> Option<WorldMesh> {
    let geometry = record.asset_target.editable_geometry()?;
    let translation = Vec3::from_array(record.translation);
    let scale = Vec3::from_array(record.scale);
    if !translation.is_finite() || !scale.is_finite() || scale.min_element() <= 0.0
        || !record.yaw_degrees.is_finite() {
        return None;
    }
    let radians = record.yaw_degrees.to_radians();
    let rotation = Quat::from_rotation_y(radians);
    let vertices: Vec<_> = geometry.vertices().iter().map(|point| {
        let local = Vec3::from_array(point.map(|value| value as f32 * 0.0005));
        translation + rotation * (local * scale)
    }).collect();
    if vertices.iter().any(|point| !point.is_finite()) {
        return None;
    }

    // 视口候选索引仅随冻结的几何/对象姿态刷新；移动镜头不重复构建或复制拓扑。
    let screen = [
        ScreenIndex::new(vertices.len(), |i| Bounds::point(vertices[i])),
        ScreenIndex::new(geometry.edges().len(), |i| {
            let [a, b] = geometry.edges()[i];
            Bounds::point(vertices[a]).include(vertices[b])
        }),
        ScreenIndex::new(geometry.face_count(), |i| {
            let face = geometry.faces()[i].indices();
            face.iter().fold(Bounds::point(vertices[face[0] as usize]), |bounds, &v| {
                bounds.include(vertices[v as usize])
            })
        }),
    ];
    Some(WorldMesh {
        vertices,
        source: record.asset_target.clone(),
        geometry,
        translation,
        yaw_degrees: record.yaw_degrees,
        scale,
        inverse_rotation: DQuat::from_rotation_y(-f64::from(radians)),
        screen,
    })
}

impl WorldMesh {
    pub fn matches_record(&self, record: &ScenePlacementRecord) -> bool {
        self.translation.to_array() == record.translation
            && self.yaw_degrees == record.yaw_degrees
            && self.scale.to_array() == record.scale
            && match &record.asset_target {
                ScenePlacementAssetTarget::EditableMesh { geometry, .. } => self.geometry.same_storage(geometry),
                source => self.source == *source,
            }
    }
    pub fn edges(&self) -> &[[usize; 2]] {
        self.geometry.edges()
    }
    pub fn faces(&self) -> &[MeshFace] {
        self.geometry.faces()
    }
    pub fn edge_faces(&self, edge: usize) -> impl Iterator<Item = usize> {
        self.geometry.adjacent_faces(edge)
    }
    pub fn count(&self, kind: MeshElementKind) -> usize {
        match kind {
            MeshElementKind::Vertex => self.vertices.len(),
            MeshElementKind::Edge => self.edges().len(),
            MeshElementKind::Face => self.faces().len(),
        }
    }
    pub fn all(&self, kind: MeshElementKind) -> MeshSelection {
        MeshSelection::all(self.count(kind))
    }
    #[cfg(test)]
    pub fn element_mask(&self, kind: MeshElementKind, index: usize) -> MeshSelection {
        let mut selection = MeshSelection::default();
        self.for_each_element_vertex(kind, index, |vertex| selection.insert(vertex));
        selection
    }
    fn for_each_element_vertex(&self, kind: MeshElementKind, index: usize, f: impl FnMut(usize)) {
        match kind {
            MeshElementKind::Vertex => std::iter::once(index).for_each(f),
            MeshElementKind::Edge => self.edges()[index].into_iter().for_each(f),
            MeshElementKind::Face => self.faces()[index].indices().iter().map(|&v| v as usize).for_each(f),
        }
    }
    pub fn selected_vertices(&self, kind: MeshElementKind, selected: &MeshSelection) -> MeshSelection {
        let count = self.count(kind);
        if kind == MeshElementKind::Vertex {
            // 生产选择始终来自当前拓扑；仍丢弃来自旧夹具或失效状态的越界 ID。
            if selected.iter().all(|i| i < count) {
                return selected.clone();
            }
            return selected.iter().take_while(|&i| i < count).collect();
        }
        let mut vertices = MeshSelection::default();
        for index in selected.iter().take_while(|&i| i < count) {
            self.for_each_element_vertex(kind, index, |vertex| vertices.insert(vertex));
        }
        vertices
    }
    pub fn convert_selection(
        &self,
        old: MeshElementKind,
        new: MeshElementKind,
        selected: &MeshSelection,
    ) -> MeshSelection {
        let vertices = self.selected_vertices(old, selected);
        match new {
            MeshElementKind::Vertex => vertices,
            MeshElementKind::Edge => self.edges().iter().enumerate().filter_map(|(index, &[a, b])| {
                (vertices.contains(a) && vertices.contains(b)).then_some(index)
            }).collect(),
            MeshElementKind::Face => self.faces().iter().enumerate().filter_map(|(index, face)| {
                face.indices().iter().all(|&v| vertices.contains(v as usize)).then_some(index)
            }).collect(),
        }
    }
    fn nearest_face_within(&self, ray: Ray3d, max_distance: f32) -> Option<(usize, f32)> {
        let half_mm = 2_000.0 / self.scale.as_dvec3();
        // 射线逆变换后方向保留尺度，交距参数继续以世界米计，非均匀缩放也可比较遮挡。
        let origin = (self.inverse_rotation * (ray.origin.as_dvec3() - self.translation.as_dvec3())) * half_mm;
        let direction = (self.inverse_rotation * ray.direction.as_dvec3()) * half_mm;
        let (triangle, distance) = self.geometry.nearest_triangle(origin, direction, f64::from(max_distance))?;
        Some((self.geometry.triangles_with_faces()[triangle].0, distance as f32))
    }
    pub fn nearest_face(&self, ray: Ray3d) -> Option<(usize, f32)> {
        self.nearest_face_within(ray, f32::INFINITY)
    }
    pub fn visible(&self, camera: MoveCamera, point: Vec3) -> bool {
        camera.project(point).is_some_and(|screen| self.visible_at(camera, point, screen))
    }
    fn visible_at(&self, camera: MoveCamera, point: Vec3, screen: Vec2) -> bool {
        let Some(ray) = camera.ray(screen) else {
            return false;
        };
        let distance = (point - ray.origin).dot(*ray.direction);
        // 世界标记由 f32 投影，局部射线使用 f64。量级舍入容差避免把原表面的边判为隐藏。
        let epsilon = 0.00005 + distance.abs().max(point.abs().max_element()) * f32::EPSILON * 2.0;
        let limit = distance - epsilon;
        limit <= 0.0 || self.nearest_face_within(ray, limit).is_none_or(|(_, hit)| hit >= limit)
    }
    pub fn center(&self, kind: MeshElementKind, index: usize) -> Vec3 {
        let mut sum = DVec3::ZERO;
        let mut count = 0;
        self.for_each_element_vertex(kind, index, |vertex| {
            sum += self.vertices[vertex].as_dvec3();
            count += 1;
        });
        (sum / f64::from(count)).as_vec3()
    }
    /// 面标记与框选共用实际可见三角的重心；凹面均值可能落在面外。
    pub fn visible_representative(
        &self,
        kind: MeshElementKind,
        index: usize,
        camera: MoveCamera,
    ) -> Option<Vec3> {
        if kind == MeshElementKind::Face {
            self.geometry.face_triangles(index)?
                .iter()
                .map(|&[a, b, c]| (self.vertices[a] + self.vertices[b] + self.vertices[c]) / 3.0)
                .find(|&point| self.visible(camera, point))
        } else {
            let point = self.center(kind, index);
            self.visible(camera, point).then_some(point)
        }
    }
    pub fn pick(&self, kind: MeshElementKind, camera: MoveCamera, cursor: Vec2) -> Option<usize> {
        if !cursor.is_finite() {
            return None;
        }
        if kind == MeshElementKind::Face {
            let ray = camera.ray(cursor)?;
            let (face, distance) = self.nearest_face(ray)?;
            // 只核对真实命中的远裁面，部分裁剪后仍可见的表面继续接受点击。
            return camera.project(ray.get_point(distance)).map(|_| face);
        }
        let logical = camera.size * camera.inverse_scale;
        let pointer = cursor * logical;
        let radius = Vec2::splat(10.0) / logical;
        let mut nearest = None;
        self.visit_candidates(kind, camera, cursor - radius, cursor + radius, |index| {
            let candidate = match kind {
                MeshElementKind::Vertex => {
                    let point = self.vertices[index];
                    camera.project(point).map(|screen| (point, screen))
                }
                MeshElementKind::Edge => self.edge_pick_point(index, camera, logical, pointer),
                MeshElementKind::Face => unreachable!(),
            };
            let Some((point, screen)) = candidate else { return; };
            let distance = pointer.distance(screen * logical);
            if distance <= 10.0
                && nearest.is_none_or(|(best_index, best)| (distance, index) < (best, best_index))
                && self.visible_at(camera, point, screen)
            {
                nearest = Some((index, distance));
            }
        });
        nearest.map(|(i, _)| i)
    }
    fn edge_pick_point(
        &self,
        index: usize,
        camera: MoveCamera,
        logical: Vec2,
        pointer: Vec2,
    ) -> Option<(Vec3, Vec2)> {
        let [start, end] = self.edges()[index].map(|vertex| self.vertices[vertex]);
        let screen_start = camera.project(start)?;
        let edge = (camera.project(end)? - screen_start) * logical;
        let squared = edge.length_squared();
        if squared < 0.01 {
            return None;
        }
        let t = ((pointer - screen_start * logical).dot(edge) / squared).clamp(0.0, 1.0);
        let cursor = screen_start + edge / logical * t;
        if pointer.distance_squared(cursor * logical) > 100.0 {
            return None;
        }
        // 透视下屏幕参数不等于世界参数，从当前射线还原边上的真实遮挡点。
        let ray = camera.ray(cursor)?;
        let direction = (end - start).normalize_or_zero();
        let ad = direction.dot(*ray.direction);
        let w = ray.origin - start;
        let denominator = 1.0 - ad * ad;
        let length = start.distance(end);
        let along = if denominator > 1e-6 {
            ((direction.dot(w) - ad * ray.direction.dot(w)) / denominator).clamp(0.0, length)
        } else {
            t * length
        };
        Some((start + direction * along, cursor))
    }
    /// 框内点、边中点或可见面标记才被纳入；与绘制共用真实面片代表点。
    pub fn in_box(
        &self,
        kind: MeshElementKind,
        camera: MoveCamera,
        start: Vec2,
        end: Vec2,
    ) -> MeshSelection {
        if !start.is_finite() || !end.is_finite() {
            return MeshSelection::default();
        }
        let min = start.min(end);
        let max = start.max(end);
        let mut selection = MeshSelection::default();
        self.visit_candidates(kind, camera, min, max, |index| {
            // 点/边先筛投影框再查遮挡；大网格的小框不为框外元素发射射线。
            let in_box = |screen: Vec2| screen.cmpge(min).all() && screen.cmple(max).all();
            let match_selection = if kind == MeshElementKind::Face {
                self.visible_representative(kind, index, camera)
                    .and_then(|point| camera.project(point)).is_some_and(in_box)
            } else {
                let point = self.center(kind, index);
                camera.project(point).is_some_and(|screen| {
                    in_box(screen) && self.visible_at(camera, point, screen)
                })
            };
            if match_selection {
                selection.insert(index);
            }
        });
        selection
    }
    fn visit_candidates(
        &self,
        kind: MeshElementKind,
        camera: MoveCamera,
        min: Vec2,
        max: Vec2,
        callback: impl FnMut(usize),
    ) {
        let index = match kind {
            MeshElementKind::Vertex => 0,
            MeshElementKind::Edge => 1,
            MeshElementKind::Face => 2,
        };
        if let Some(tree) = &self.screen[index] {
            tree.visit(camera, min, max, callback);
        } else {
            (0..self.count(kind)).for_each(callback);
        }
    }
}
