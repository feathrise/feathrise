//! 模式连接与普通启动共用沿边亮段；绝对定位与绘制变换不参与文字布局。

use super::components::{ConnectionBorderSegment, ConnectionStatus, LaunchStatus};
use bevy::prelude::*;
use feathrise_editor_ui::colors as c;

const SEGMENTS: u8 = 16;
const SEGMENT_SIZE: f32 = 2.0;

pub(super) fn mount(button: &mut ChildSpawnerCommands) {
    mount_for(button, ConnectionStatus, "工程3D忙碌边框");
}

pub(super) fn mount_launch(button: &mut ChildSpawnerCommands) {
    mount_for(button, LaunchStatus, "启动准备边框");
}

fn mount_for(button: &mut ChildSpawnerCommands, marker: impl Component, name: &'static str) {
    button.spawn((
        marker,
        Name::new(name),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            // 绝对定位相对父按钮的边框内侧；向外补回既有1px边框，覆盖原轮廓。
            left: px(-1), right: px(-1), top: px(-1), bottom: px(-1),
            ..default()
        },
        Pickable::IGNORE,
    )).with_children(|border| {
        for index in 0..SEGMENTS {
            border.spawn((
                ConnectionBorderSegment(index),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0), top: px(0), width: px(SEGMENT_SIZE), height: px(SEGMENT_SIZE),
                    border_radius: BorderRadius::all(px(SEGMENT_SIZE * 0.5)),
                    ..default()
                },
                BackgroundColor(c::TEXT_ACCENT.with_alpha(1.0 - index as f32 / SEGMENTS as f32 * 0.85)),
                UiTransform::default(),
                Pickable::IGNORE,
            ));
        }
    });
}

/// 沿1px边框中心的圆角矩形取点；尺寸使用实际布局，兼容DPI与字体变化。
fn border_point(size: Vec2, phase: f32, behind: f32) -> Vec2 {
    let radius = 2.5_f32.min((size.min_element() - 1.0) * 0.5);
    let straight = size - Vec2::splat(1.0 + radius * 2.0);
    let arc = radius * std::f32::consts::FRAC_PI_2;
    let perimeter = 2.0 * (straight.x + straight.y) + 4.0 * arc;
    let mut distance = (phase * perimeter - behind).rem_euclid(perimeter);
    let starts = [
        Vec2::new(0.5 + radius, 0.5),
        Vec2::new(size.x - 0.5, 0.5 + radius),
        Vec2::new(size.x - 0.5 - radius, size.y - 0.5),
        Vec2::new(0.5, size.y - 0.5 - radius),
    ];
    let directions = [Vec2::X, Vec2::Y, Vec2::NEG_X, Vec2::NEG_Y];
    for side in 0..4 {
        let length = if side % 2 == 0 { straight.x } else { straight.y };
        if distance <= length { return starts[side] + directions[side] * distance; }
        distance -= length;
        let next = (side + 1) % 4;
        if distance <= arc {
            let center = starts[next] - directions[side] * radius;
            let angle = (side as f32 - 1.0) * std::f32::consts::FRAC_PI_2 + distance / radius;
            return center + Vec2::new(angle.cos(), angle.sin()) * radius;
        }
        distance -= arc;
    }
    starts[0]
}

pub(super) fn animate(
    time: Res<Time<Real>>,
    borders: Query<(&Node, &ComputedNode), Or<(With<ConnectionStatus>, With<LaunchStatus>)>>,
    mut segments: Query<(&ConnectionBorderSegment, &ChildOf, &mut UiTransform)>,
) {
    // 真实时间每1.2秒绕一周，玩法暂停仍连续；每个按钮16个固定节点，无逐帧布局位置写入。
    let phase = (time.elapsed_secs_f64().rem_euclid(1.2) / 1.2) as f32;
    for (segment, parent, mut transform) in &mut segments {
        let Ok((node, computed)) = borders.get(parent.parent()) else { continue; };
        let size = computed.size() * computed.inverse_scale_factor;
        if node.display == Display::None || !size.is_finite() || size.min_element() <= 2.0 { continue; }
        let point = border_point(size, phase, segment.0 as f32 * 1.25) - Vec2::splat(SEGMENT_SIZE * 0.5);
        transform.translation = Val2::px(point.x, point.y);
    }
}
