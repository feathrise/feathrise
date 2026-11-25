//! 曲线图沿用时间轴秒制与 GLB 的真实采样器。拖切线只临时复制相邻键。

use bevy::prelude::*;
use feathrise_editor_animation_model::data::Channel;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Plot {
    pub start: f32,
    pub span: f32,
    pub low: f32,
    pub high: f32,
}

impl Plot {
    pub fn point(self, time: f32, value: f32) -> Vec2 {
        Vec2::new(
            (time - self.start) / self.span,
            (self.high - value) / (self.high - self.low),
        )
    }
    pub fn sample(self, cursor: Vec2) -> Vec2 {
        Vec2::new(
            self.start + cursor.x * self.span,
            self.high - cursor.y * (self.high - self.low),
        )
    }
}

pub(super) fn handle(
    channel: &Channel,
    key: usize,
    axis: usize,
    outgoing: bool,
    span: f32,
) -> Option<Vec2> {
    if channel.interpolation != "CUBICSPLINE" {
        return None;
    }
    let current = channel.keys.get(key)?;
    let (neighbor, tangent) = if outgoing {
        (channel.keys.get(key + 1)?, &current.out_tangent)
    } else {
        (channel.keys.get(key.checked_sub(1)?)?, &current.in_tangent)
    };
    // 手柄是梯度示意线。时间距离随相邻键和视区调整，显示长度不写入 glTF。
    let delta = neighbor.time - current.time;
    let dt = delta.signum() * (delta.abs() / 3.0).min(span / 8.0);
    Some(Vec2::new(
        current.time + dt,
        current.value[axis] + dt * tangent[axis],
    ))
}
