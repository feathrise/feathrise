//! 读取 XYZ 固定子控件的真实布局轮廓，保留透明根和负轴圆环内部的工程画面。

use feathrise_editor_ui::InteractionButton;
use super::ViewportControlShape;
use crate::components::{MapEditorOrientationAxisEndpoint, MapEditorOrientationAxisLine,
    MapEditorOrientationAxisRoot};
use bevy::{ecs::system::SystemParam, prelude::*};
use std::cmp::Ordering;

const MAX_SHAPES: usize = 10; // 三条轴线、六个端点和一个中心，不能扩展为任意 UI 树扫描。

#[derive(SystemParam)]
pub(super) struct AxisOcclusion<'w, 's> {
    roots: Query<'w, 's, (&'static Node, &'static InheritedVisibility, &'static Children),
        With<MapEditorOrientationAxisRoot>>,
    nodes: Query<'w, 's, (&'static Node, &'static ComputedNode, &'static UiGlobalTransform,
        &'static InheritedVisibility, Option<&'static MapEditorOrientationAxisLine>,
        Option<&'static MapEditorOrientationAxisEndpoint>, Has<InteractionButton>)>,
}

impl AxisOcclusion<'_, '_> {
    pub(super) fn capture(&self, viewport: Rect) -> Result<Vec<ViewportControlShape>, String> {
        let mut shapes = Vec::new();
        for (index, (root, visible, children)) in self.roots.iter().enumerate() {
            if index != 0 || children.len() > MAX_SHAPES {
                return Err("XYZ 方向轴超过一个根或10个固定子控件，已隐藏工程视口".into());
            }
            if root.display == Display::None || !visible.get() { continue; }
            let mut centers = 0;
            for child in children.iter() {
                let Ok((node, computed, transform, visible, line, endpoint, button)) = self.nodes.get(child) else {
                    continue; // 布局尚未生成时不猜测轮廓，下一帧读取完整的正式节点。
                };
                if node.display == Display::None || !visible.get() || computed.is_empty() { continue; }
                let hollow = match (line, endpoint, button) {
                    (Some(_), None, _) => None,
                    (None, Some(endpoint), _) => Some(!endpoint.positive),
                    (None, None, true) => {
                        centers += 1;
                        if centers > 1 { return Err("XYZ 方向轴出现重复中心控件，已隐藏工程视口".into()); }
                        Some(false)
                    }
                    _ => return Err("XYZ 方向轴子控件身份不符，已隐藏工程视口".into()),
                };
                let shape = node_shape(computed, transform, hollow)?;
                // 只剔除完全在视口外的轮廓；边缘圆/斜线交由最终窗口区域裁切，
                // 提前裁小外接框会把原来的圆弧改变为另一条椭圆。
                if intersects(&shape, viewport) { shapes.push(shape); }
            }
        }
        // 固定十项按几何排序，ECS/子节点迭代顺序变化不能使静止窗口每帧重建区域。
        shapes.sort_by(shape_order);
        shapes.dedup();
        Ok(shapes)
    }
}

fn node_shape(computed: &ComputedNode, transform: &UiGlobalTransform,
    hollow: Option<bool>) -> Result<ViewportControlShape, String> {
    let local = Rect::from_center_size(Vec2::ZERO, computed.size());
    let corners = transformed_corners(local, transform);
    if corners.iter().any(|point| !point.is_finite()) {
        return Err("XYZ 方向轴布局包含非法坐标，已隐藏工程视口".into());
    }
    let Some(hollow) = hollow else { return Ok(ViewportControlShape::Quad { points: corners }); };
    let outer = bounds(corners);
    let inner = if hollow {
        let border = computed.border;
        if !border.min_inset.is_finite() || !border.max_inset.is_finite()
            || border.min_inset.min_element() < 0. || border.max_inset.min_element() < 0. {
            return Err("XYZ 方向轴圆环包含非法边宽，已隐藏工程视口".into());
        }
        let minimum = local.min + border.min_inset;
        let maximum = local.max - border.max_inset;
        minimum.cmplt(maximum).all().then(|| bounds(transformed_corners(
            Rect::from_corners(minimum, maximum), transform)))
    } else { None };
    Ok(ViewportControlShape::Ellipse { outer, inner })
}

fn transformed_corners(rect: Rect, transform: &UiGlobalTransform) -> [Vec2; 4] {
    [rect.min, Vec2::new(rect.max.x, rect.min.y), rect.max, Vec2::new(rect.min.x, rect.max.y)]
        .map(|point| transform.affine().transform_point2(point))
}

fn bounds(points: [Vec2; 4]) -> Rect {
    Rect::from_corners(points.into_iter().fold(Vec2::splat(f32::INFINITY), Vec2::min),
        points.into_iter().fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max))
}

fn intersects(shape: &ViewportControlShape, viewport: Rect) -> bool {
    let rect = match shape {
        ViewportControlShape::Ellipse { outer, .. } => *outer,
        ViewportControlShape::Quad { points } => bounds(*points),
    };
    rect.min.max(viewport.min).cmplt(rect.max.min(viewport.max)).all()
}

fn shape_order(a: &ViewportControlShape, b: &ViewportControlShape) -> Ordering {
    let coordinates = |shape: &ViewportControlShape| match shape {
        ViewportControlShape::Ellipse { outer, inner } => {
            let inner = inner.unwrap_or(Rect::EMPTY);
            [0., outer.min.x, outer.min.y, outer.max.x, outer.max.y,
                inner.min.x, inner.min.y, inner.max.x, inner.max.y]
        }
        ViewportControlShape::Quad { points: [a, b, c, d] } =>
            [1., a.x, a.y, b.x, b.y, c.x, c.y, d.x, d.y],
    };
    coordinates(a).into_iter().zip(coordinates(b))
        .map(|(a, b)| a.total_cmp(&b)).find(|order| *order != Ordering::Equal).unwrap_or(Ordering::Equal)
}

#[cfg(test)]
#[path = "viewport_axis_occlusion_tests.rs"]
mod tests;
