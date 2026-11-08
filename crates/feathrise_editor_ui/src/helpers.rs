//! 编辑器阴影和字段主题；输入法与数值算法复用无主题控件。
use crate::colors::*;
use bevy::prelude::*;
pub use feathrise_ui_widgets::helpers::*;

/// 用实际变换后的祖先裁切计算可见边界，供滚动和只排版可见行的工具使用。
pub fn clipped_ui_bounds(clip: &bevy::ui::CalculatedClip, bounds: Rect) -> Option<Rect> {
    let corners = [
        (bounds.min, ()),
        (Vec2::new(bounds.max.x, bounds.min.y), ()),
        (bounds.max, ()),
        (Vec2::new(bounds.min.x, bounds.max.y), ()),
    ];
    // 与 Bevy 绘制路径共用裁切，保留旋转祖先、单轴裁切及完全不可见的语义。
    let vertices = bevy::ui_render::clipping::clip_polygon(Some(clip), &corners, |(), (), _| ());
    let mut points = vertices.into_iter().map(|(position, ())| position);
    let first = points.next()?;
    Some(points.fold(Rect::from_corners(first, first), |bounds, point| Rect {
        min: bounds.min.min(point),
        max: bounds.max.max(point),
    }))
}

/// 原浮层入口继续兼容，但外壳由同一对话框配方决定，避免无框和亮框各自演变。
pub fn floating_panel_node() -> Node {
    dialog_panel_node()
}

/// 日志、详情、设置及工程关闭共用低对比1px边界；调用方只提供业务所需的尺寸和锚点。
pub fn dialog_panel_node() -> Node {
    Node {
        min_width: px(0), min_height: px(0), flex_direction: FlexDirection::Column,
        border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)),
        overflow: Overflow::clip(), ..default()
    }
}

pub fn dialog_header_node() -> Node {
    Node {
        width: percent(100), min_width: px(0), min_height: px(36), flex_shrink: 0.,
        align_items: AlignItems::Center, column_gap: px(8),
        padding: UiRect::axes(px(12), px(3)), border: UiRect::bottom(px(1)), ..default()
    }
}

/// 带多项工具动作的标题栏保留同一外观，只为固定关闭按钮预留命中区。
pub fn dialog_toolbar_header_node() -> Node {
    Node {
        flex_wrap: FlexWrap::Wrap, row_gap: px(4),
        padding: UiRect { right: px(48), ..dialog_header_node().padding },
        ..dialog_header_node()
    }
}

pub fn dialog_body_node() -> Node {
    Node {
        width: percent(100), min_width: px(0), min_height: px(0),
        flex_direction: FlexDirection::Column, row_gap: px(8),
        padding: UiRect::all(px(12)), ..default()
    }
}

pub fn dialog_footer_node() -> Node {
    Node {
        width: percent(100), min_width: px(0), min_height: px(40), flex_shrink: 0.,
        align_items: AlignItems::Center, justify_content: JustifyContent::FlexEnd,
        flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(4),
        padding: UiRect::axes(px(12), px(5)), border: UiRect::top(px(1)), ..default()
    }
}

/// 极小逻辑视口统一降低密度，保留正文、标题和操作的共同结构，不能由各弹窗另设一套皮肤。
pub fn dialog_uses_compact_layout(available: Vec2) -> bool {
    available.x < 220. || available.y < 180.
}

pub fn dialog_compact_header_node() -> Node {
    Node { min_height: px(24), padding: UiRect::horizontal(px(6)), column_gap: px(4), ..dialog_header_node() }
}

pub fn dialog_compact_body_node() -> Node {
    Node { padding: UiRect::axes(px(6), px(2)), row_gap: px(4), ..dialog_body_node() }
}

pub fn dialog_compact_footer_node() -> Node {
    Node { min_height: px(24), padding: UiRect::horizontal(px(6)), column_gap: px(4), ..dialog_footer_node() }
}

pub fn text_shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::new(1.0, 1.0),
        color: TEXT_SHADOW_COLOR,
    }
}

/// 字段主题随组件参与焦点刷新，另一界面的主题不会覆盖当前字段。
pub fn numeric_input_palette() -> feathrise_ui_widgets::NumericInputPalette {
    feathrise_ui_widgets::NumericInputPalette {
        normal_bg: INPUT_BG,
        normal_border: INPUT_BORDER,
        hovered_bg: COMPACT_BUTTON_BG,
        active_bg: INPUT_ACTIVE_BG,
        active_border: INPUT_ACTIVE_BORDER,
        disabled_bg: INPUT_DISABLED_BG,
        disabled_border: INPUT_DISABLED_BORDER,
    }
}
