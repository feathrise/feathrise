//! 冻结世界点、边和面的包围盒，缩小每次点击、悬停和框选的准确判断范围。

use crate::viewport::placement_move::MoveCamera;
use bevy::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Bounds {
    pub(super) fn point(point: Vec3) -> Self {
        Self { min: point, max: point }
    }

    pub(super) fn include(self, point: Vec3) -> Self {
        Self { min: self.min.min(point), max: self.max.max(point) }
    }

    fn intersects(self, camera: MoveCamera, min: Vec2, max: Vec2) -> bool {
        let mut outside = u8::MAX;
        for x in [self.min.x, self.max.x] {
            for y in [self.min.y, self.max.y] {
                for z in [self.min.z, self.max.z] {
                    let p = camera.clip(Vec3::new(x, y, z));
                    if !p.is_finite() {
                        return true;
                    }
                    // 齐次裁剪保留跨近面的包围盒；只有所有角落同落在同一平面外才剔除。
                    let border = p.abs().max_element().max(1.0) * f32::EPSILON * 8.0;
                    let sides = [
                        p.x - 2.0 * min.x * p.w, 2.0 * max.x * p.w - p.x,
                        p.y + 2.0 * max.y * p.w, -2.0 * min.y * p.w - p.y,
                        p.z, p.w - p.z, p.w,
                    ];
                    let mask = sides.into_iter().enumerate().fold(0, |mask, (index, side)| {
                        mask | (u8::from(side < -border) << index)
                    });
                    outside &= mask;
                    if outside == 0 {
                        return true;
                    }
                }
            }
        }
        false
    }
}

struct Element {
    bounds: Bounds,
    index: usize,
}

struct Node {
    bounds: Bounds,
    kind: NodeKind,
}

enum NodeKind {
    Leaf(std::ops::Range<usize>),
    Branch([usize; 2]),
}

pub(super) struct ScreenIndex {
    order: Vec<usize>,
    nodes: Vec<Node>,
}

impl ScreenIndex {
    pub(super) fn new(count: usize, bounds: impl Fn(usize) -> Bounds) -> Option<Self> {
        // 少量几何直接遍历成本更低；较大对象只在冻结世界几何时构造一次树。
        if count <= 64 {
            return None;
        }
        let mut elements: Vec<_> = (0..count).map(|index| Element { bounds: bounds(index), index }).collect();
        let mut nodes = Vec::new();
        Self::build(&mut nodes, &mut elements, 0);
        Some(Self { order: elements.into_iter().map(|item| item.index).collect(), nodes })
    }

    fn build(nodes: &mut Vec<Node>, items: &mut [Element], start: usize) -> usize {
        let bounds = items.iter().fold(items[0].bounds, |bounds, item| {
            bounds.include(item.bounds.min).include(item.bounds.max)
        });
        let root = nodes.len();
        nodes.push(Node { bounds, kind: NodeKind::Leaf(start..start + items.len()) });
        if items.len() > 12 {
            let span = bounds.max - bounds.min;
            let axis = if span.x >= span.y && span.x >= span.z { 0 } else if span.y >= span.z { 1 } else { 2 };
            let middle = items.len() / 2;
            items.select_nth_unstable_by(middle, |a, b| {
                let center = |item: &Element| item.bounds.min[axis] * 0.5 + item.bounds.max[axis] * 0.5;
                center(a).total_cmp(&center(b)).then_with(|| a.index.cmp(&b.index))
            });
            let (a, b) = items.split_at_mut(middle);
            let left = Self::build(nodes, a, start);
            let right = Self::build(nodes, b, start + middle);
            nodes[root].kind = NodeKind::Branch([left, right]);
        }
        root
    }

    pub(super) fn visit(&self, camera: MoveCamera, min: Vec2, max: Vec2, mut found: impl FnMut(usize)) {
        self.visit_node(0, camera, min, max, &mut found);
    }

    fn visit_node(&self, index: usize, camera: MoveCamera, min: Vec2, max: Vec2, found: &mut impl FnMut(usize)) {
        let node = &self.nodes[index];
        if !node.bounds.intersects(camera, min, max) {
            return;
        }
        match &node.kind {
            NodeKind::Leaf(range) => self.order[range.clone()].iter().copied().for_each(found),
            NodeKind::Branch([a, b]) => {
                self.visit_node(*a, camera, min, max, found);
                self.visit_node(*b, camera, min, max, found);
            }
        }
    }
}
