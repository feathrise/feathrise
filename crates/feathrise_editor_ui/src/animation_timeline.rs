//! 角色和场景共用时间轴绘制与鼠标换算。片段、选区和写入由各作者域持有。

use bevy::{prelude::*, text::FontSourceTemplate, ui::{FocusPolicy, RelativeCursorPosition}};
use feathrise_editor_core::FONT_PATH;
use crate::{UiInteraction, colors, metrics};

pub const KEY_SLOTS: usize = 64;

pub fn cursor_time(window_start: f32, window_span: f32, centered_x: f32) -> f32 {
    window_start + (centered_x + 0.5) * window_span
}

/// 定位使用已排序真实键列，不依赖当前分配给菱形控件的可见槽位。
pub fn nearest_key<T>(keys: &[T], time: f32, tolerance: f32, time_of: impl Fn(&T) -> f32) -> Option<usize> {
    let right = keys.partition_point(|key| time_of(key) < time);
    let distance = |index: usize| (time_of(&keys[index]) - time).abs();
    [right.checked_sub(1), (right < keys.len()).then_some(right)]
        .into_iter().flatten().filter(|&index| distance(index) <= tolerance)
        .min_by(|&a, &b| distance(a).total_cmp(&distance(b)))
}

pub fn playhead() -> impl Scene {
    bsn! { ~{FocusPolicy::Pass}
        Node { position_type: PositionType::Absolute, left: percent(0), top: px(0),
            width: px(1), height: percent(100) }
        BackgroundColor(colors::TEXT_ACCENT)
    }
}

pub fn tick_node(index: usize) -> Node {
    // 首末帧采用同一时间映射；边缘刻度的文字仍在画布内。
    let last = index == 8;
    Node {
        position_type: PositionType::Absolute,
        left: if last { Val::Auto } else { percent(index as f32 * 12.5) },
        right: if last { px(0) } else { Val::Auto },
        top: px(0), bottom: px(0), width: px(34),
        border: if last { UiRect::right(px(1)) } else { UiRect::left(px(1)) },
        padding: UiRect::horizontal(px(3)),
        justify_content: if last { JustifyContent::FlexEnd } else { JustifyContent::FlexStart },
        overflow: Overflow::clip_x(),
        ..default()
    }
}

pub fn key_marker() -> impl Scene {
    bsn! { UiInteraction::default() ~{FocusPolicy::Pass}
        Node { display: Display::None, position_type: PositionType::Absolute,
            left: percent(0), top: px(2), width: px(14), height: px(20),
            margin: UiRect::left(px(-7)), align_items: AlignItems::Center,
            justify_content: JustifyContent::Center }
        BackgroundColor(Color::NONE)
        Children [
            Text("◆") ~{FocusPolicy::Pass}
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                font_size: px(metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_VALUE)
        ]
    }
}

pub fn lane(ruler: bool) -> impl Scene {
    let node = Node {
        position_type: PositionType::Relative, flex_grow: 1.0, flex_basis: px(0),
        min_width: px(0), height: percent(100), overflow: Overflow::clip(),
        border: if ruler { UiRect::bottom(px(1)) } else { UiRect::ZERO },
        ..default()
    };
    let background = if ruler { colors::PANEL_BG_STRONG } else { colors::TRACK_BG };
    bsn! { RelativeCursorPosition::default() UiInteraction::default() ~{FocusPolicy::Block}
        ~{node} BackgroundColor(background) BorderColor::all(colors::COPPER_BORDER_WEAK)
    }
}
