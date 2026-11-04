//! 固定轮廓纹理的量化与视区映射；所有交互均以真实物理布局为准。

use bevy::prelude::*;

use super::super::{constants::*, types::MinimapMark};

#[derive(Clone, Copy, Debug)]
pub(super) struct OverviewMetrics {
    pub height: f32,
    pub thumb_height: f32,
    pub thumb_top: f32,
    pub maximum: f32,
}

impl OverviewMetrics {
    pub fn new(extent: Vec2, view: Vec2, rail: Vec2, offset: f32, scale: f32) -> Option<Self> {
        if !extent.is_finite()
            || !view.is_finite()
            || !rail.is_finite()
            || extent.x < 0.0
            || extent.y <= 0.0
            || view.min_element() <= 0.0
            || rail.min_element() <= 0.0
            || !offset.is_finite()
            || !scale.is_finite()
            || scale <= 0.0
        {
            return None;
        }
        // 短文档按宽度等比缩小，长文档压到轨道高度；不把几行文字拉长成整页。
        let height = (extent.y * rail.x / view.x).min(rail.y).max(1.0);
        let maximum = (extent.y - view.y).max(0.0);
        let thumb_height = (height * (view.y / extent.y).min(1.0))
            .max(MINIMAP_MIN_THUMB_HEIGHT * scale)
            .min(height);
        let travel = height - thumb_height;
        let thumb_top = if maximum > 0.0 {
            offset.clamp(0.0, maximum) / maximum * travel
        } else {
            0.0
        };
        Some(Self {
            height,
            thumb_height,
            thumb_top,
            maximum,
        })
    }

    pub fn scroll_for_top(self, top: f32) -> f32 {
        let travel = self.height - self.thumb_height;
        if travel <= 0.0 || !top.is_finite() {
            return 0.0;
        }
        (top / travel).clamp(0.0, 1.0) * self.maximum
    }
}

pub(super) fn mark(position: Vec2, glyph_size: Vec2, extent: Vec2) -> Option<MinimapMark> {
    if !position.is_finite()
        || !glyph_size.is_finite()
        || !extent.is_finite()
        || glyph_size.min_element() <= 0.0
        || extent.min_element() <= 0.0
    {
        return None;
    }
    let factor = Vec2::new((MINIMAP_WIDTH - 4) as f32, MINIMAP_HEIGHT as f32) / extent;
    let min = ((position - glyph_size * 0.5) * factor + Vec2::new(2.0, 0.0)).floor();
    let max = ((position + glyph_size * 0.5) * factor + Vec2::new(2.0, 0.0)).ceil();
    let limit = Vec2::new(MINIMAP_WIDTH as f32, MINIMAP_HEIGHT as f32);
    let min = min.clamp(Vec2::ZERO, limit);
    let max = max.clamp(Vec2::ZERO, limit);
    (max.cmpgt(min).all()).then_some(MinimapMark {
        min: [min.x as u16, min.y as u16],
        max: [max.x as u16, max.y as u16],
    })
}
