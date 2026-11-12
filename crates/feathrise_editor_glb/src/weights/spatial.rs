//! 静置姿势下的网格空间索引。选择、可见性与镜像命中均在后台复用同一份索引。

use bevy::prelude::*;
use feathrise_editor_core::view_projection::ViewProjection;
use feathrise_editor_glb_model::weights::MirrorPlane;
use std::{cell::Cell, ops::Range};

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Bounds {
    fn point(point: Vec3) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    fn extend(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    fn center(self) -> Vec3 {
        self.min.midpoint(self.max)
    }

    fn ray(self, ray: Ray3d, limit: f32) -> bool {
        let mut enter = 0.0_f32;
        let mut exit = limit;
        for axis in 0..3 {
            let origin = ray.origin[axis];
            let direction = ray.direction[axis];
            if direction.abs() < 1e-9 {
                if origin < self.min[axis] || origin > self.max[axis] {
                    return false;
                }
            } else {
                let a = (self.min[axis] - origin) / direction;
                let b = (self.max[axis] - origin) / direction;
                enter = enter.max(a.min(b));
                exit = exit.min(a.max(b));
                if enter > exit {
                    return false;
                }
            }
        }
        true
    }

    fn projected(self, view: ViewProjection, low: Vec2, high: Vec2) -> bool {
        let mut minimum = Vec2::splat(f32::INFINITY);
        let mut maximum = -minimum;
        for x in [self.min.x, self.max.x] {
            for y in [self.min.y, self.max.y] {
                for z in [self.min.z, self.max.z] {
                    // 跨近裁剪面时继续向下测试，避免把可见的一部分误剪掉。
                    let Some(point) = view.project(Vec3::new(x, y, z)) else {
                        return true;
                    };
                    minimum = minimum.min(point);
                    maximum = maximum.max(point);
                }
            }
        }
        minimum.cmple(high).all() && maximum.cmpge(low).all()
    }
}

struct Branch {
    bounds: Bounds,
    children: Option<[usize; 2]>,
    entries: Range<usize>,
}

struct Bvh {
    order: Vec<u32>,
    branches: Vec<Branch>,
}

impl Bvh {
    fn new(count: usize, bounds: impl Fn(u32) -> Bounds) -> Self {
        let mut tree = Self {
            order: (0..count as u32).collect(),
            branches: Vec::new(),
        };
        if count > 0 {
            tree.split(0..count, &bounds);
        }
        tree
    }

    fn split(&mut self, entries: Range<usize>, bounds: &impl Fn(u32) -> Bounds) -> usize {
        let node = self.branches.len();
        let merged = self.order[entries.clone()]
            .iter()
            .copied()
            .map(bounds)
            .reduce(Bounds::extend)
            .expect("分支至少含一个元素");
        self.branches.push(Branch {
            bounds: merged,
            children: None,
            entries: entries.clone(),
        });
        if entries.len() > 12 {
            let span = merged.max - merged.min;
            let axis = if span.x >= span.y && span.x >= span.z {
                0
            } else if span.y >= span.z {
                1
            } else {
                2
            };
            let middle = entries.len() / 2;
            self.order[entries.clone()].select_nth_unstable_by(middle, |&a, &b| {
                bounds(a).center()[axis]
                    .total_cmp(&bounds(b).center()[axis])
                    .then(a.cmp(&b))
            });
            let split = entries.start + middle;
            let left = self.split(entries.start..split, bounds);
            let right = self.split(split..entries.end, bounds);
            self.branches[node].children = Some([left, right]);
        }
        node
    }

    fn visit(&self, index: usize, visible: &impl Fn(Bounds) -> bool, hit: &mut impl FnMut(u32)) {
        let Some(branch) = self.branches.get(index) else {
            return;
        };
        if !visible(branch.bounds) {
            return;
        }
        if let Some([left, right]) = branch.children {
            self.visit(left, visible, hit);
            self.visit(right, visible, hit);
        } else {
            for &entry in &self.order[branch.entries.clone()] {
                hit(entry);
            }
        }
    }
}

pub(super) struct Points {
    pub positions: Vec<Vec3>,
    neighbors: Vec<Vec<u32>>,
    tree: Bvh,
}

impl Points {
    /// 动作姿态只做拾取，邻接关系仍由静置图元复用。
    pub fn for_selection(positions: Vec<Vec3>) -> Self {
        let tree = Bvh::new(positions.len(), |i| Bounds::point(positions[i as usize]));
        Self {
            positions,
            neighbors: Vec::new(),
            tree,
        }
    }

    pub fn new(positions: Vec<Vec3>, indices: &[u32]) -> Self {
        let tree = Bvh::new(positions.len(), |i| Bounds::point(positions[i as usize]));
        let mut neighbors = vec![Vec::new(); positions.len()];
        for face in indices.chunks_exact(3) {
            for corner in 0..3 {
                let entries = &mut neighbors[face[corner] as usize];
                entries.extend([face[(corner + 1) % 3], face[(corner + 2) % 3]]);
            }
        }
        for (index, adjacent) in neighbors.iter_mut().enumerate() {
            adjacent.sort_unstable();
            adjacent.dedup();
            adjacent.retain(|&other| other as usize != index);
        }
        Self {
            positions,
            neighbors,
            tree,
        }
    }

    pub fn neighbors(&self, index: u32) -> &[u32] {
        &self.neighbors[index as usize]
    }

    pub fn projected(
        &self,
        view: ViewProjection,
        min: Vec2,
        max: Vec2,
        mut hit: impl FnMut(u32, Vec2, Vec3),
    ) {
        self.tree.visit(
            0,
            &|bounds| bounds.projected(view, min, max),
            &mut |index| {
                let position = self.positions[index as usize];
                if let Some(point) = view.project(position)
                    && point.cmpge(min).all()
                    && point.cmple(max).all()
                {
                    hit(index, point, position);
                }
            },
        );
    }

    fn nearest(&self, wanted: Vec3, limit: f32) -> Option<u32> {
        let distance = Cell::new(limit * limit);
        let mut selected = None;
        self.tree.visit(
            0,
            &|bounds| {
                wanted.distance_squared(wanted.clamp(bounds.min, bounds.max)) <= distance.get()
            },
            &mut |index| {
                let length = wanted.distance_squared(self.positions[index as usize]);
                if length < distance.get()
                    || length == distance.get() && selected.is_some_and(|old| index < old)
                {
                    distance.set(length);
                    selected = Some(index);
                }
            },
        );
        selected
    }

    pub fn reflected(
        &self,
        index: u32,
        plane: MirrorPlane,
        tolerance: f32,
        mut hit: impl FnMut(u32),
    ) {
        let point = self.positions[index as usize];
        let seam = tolerance * 0.001;
        if (point - plane.center).dot(plane.axis).abs() <= seam {
            return;
        }
        let Some(other) = self
            .nearest(plane.reflect(point), tolerance)
            .filter(|&other| other != index)
        else {
            return;
        };
        let anchor = self.positions[other as usize];
        let seam_distance = seam * seam;
        let Some(back) = self.nearest(plane.reflect(anchor), tolerance) else {
            return;
        };
        if point.distance_squared(self.positions[back as usize]) > seam_distance {
            return;
        }
        // 只采用相互最近的另一侧点组。位置重合的 UV 接缝顶点一起采用，避免蒙皮撕缝。
        self.tree.visit(
            0,
            &|bounds| {
                anchor.distance_squared(anchor.clamp(bounds.min, bounds.max)) <= seam_distance
            },
            &mut |other| {
                if other != index
                    && anchor.distance_squared(self.positions[other as usize]) <= seam_distance
                {
                    hit(other);
                }
            },
        );
    }
}

pub(super) struct Occlusion {
    triangles: Vec<[Vec3; 3]>,
    tree: Bvh,
}

impl Occlusion {
    pub fn new(triangles: Vec<[Vec3; 3]>) -> Self {
        let tree = Bvh::new(triangles.len(), |index| {
            let [a, b, c] = triangles[index as usize];
            Bounds {
                min: a.min(b).min(c),
                max: a.max(b).max(c),
            }
        });
        Self { triangles, tree }
    }

    pub fn visible(&self, ray: Ray3d, point: Vec3, tolerance: f32) -> bool {
        let limit = (point - ray.origin).dot(*ray.direction) - tolerance;
        if limit <= 0.0 {
            return true;
        }
        let occluded = Cell::new(false);
        self.tree.visit(
            0,
            &|bounds| !occluded.get() && bounds.ray(ray, limit),
            &mut |index| {
                if hit_triangle(ray, self.triangles[index as usize])
                    .is_some_and(|depth| depth < limit)
                {
                    occluded.set(true);
                }
            },
        );
        !occluded.get()
    }
}

fn hit_triangle(ray: Ray3d, [a, b, c]: [Vec3; 3]) -> Option<f32> {
    let edge_ab = b - a;
    let edge_ac = c - a;
    let cross = ray.direction.cross(edge_ac);
    let determinant = edge_ab.dot(cross);
    if determinant.abs() <= 1e-10 {
        return None;
    }
    let reciprocal = determinant.recip();
    let local = ray.origin - a;
    let u = local.dot(cross) * reciprocal;
    let v_cross = local.cross(edge_ab);
    let v = ray.direction.dot(v_cross) * reciprocal;
    let depth = edge_ac.dot(v_cross) * reciprocal;
    (u >= -1e-5 && v >= -1e-5 && u + v <= 1.0 + 1e-5 && depth >= 0.0).then_some(depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_triangle_blocks_hidden_vertices_but_not_its_own_vertices() {
        let index = Occlusion::new(vec![[
            Vec3::new(-1.0, -1.0, 1.0),
            Vec3::new(1.0, -1.0, 1.0),
            Vec3::new(0.0, 1.0, 1.0),
        ]]);
        let ray = Ray3d::new(Vec3::ZERO, Dir3::Z);
        assert!(index.visible(ray, Vec3::Z, 0.001));
        assert!(!index.visible(ray, Vec3::Z * 2.0, 0.001));
        let clear_ray = Ray3d::new(Vec3::X * 3.0, Dir3::Z);
        assert!(index.visible(clear_ray, Vec3::X * 3.0 + Vec3::Z * 2.0, 0.001));
    }

    #[test]
    fn mirror_pairs_coincident_seams_and_rejects_unmatched_points() {
        let positions = vec![
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.005, 0.0),
        ];
        let points = Points::new(positions, &[]);
        let plane = MirrorPlane {
            center: Vec3::ZERO,
            axis: Vec3::X,
        };
        let mut mirrored = Vec::new();
        points.reflected(0, plane, 0.01, |index| mirrored.push(index));
        assert_eq!(mirrored, [1, 2]);
        mirrored.clear();
        points.reflected(3, plane, 0.01, |index| mirrored.push(index));
        assert!(mirrored.is_empty());
    }
}
