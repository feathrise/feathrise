//! F4 编辑器场景监控面板的纯布局配置。
//! 【功能设计】布局值逐项复刻 F3 资源监控；两者只在快捷键、层级和数据源上区分。

use bevy::prelude::*;

#[cfg(test)]
pub(super) const TARGET_VIEWPORT_WIDTH: f32 = 1_920.0;
#[cfg(test)]
pub(super) const TARGET_VIEWPORT_HEIGHT: f32 = 1_080.0;
pub(super) const PANEL_WIDTH: f32 = 1_760.0;
pub(super) const PANEL_TOP: f32 = 28.0;
pub(super) const PANEL_RIGHT: f32 = 10.0;
pub(super) const PANEL_MAX_WIDTH_PERCENT: f32 = 96.0;
pub(super) const PANEL_MAX_HEIGHT_PERCENT: f32 = 92.0;
pub(super) const PANEL_PADDING: f32 = 8.0;
pub(super) const PANEL_GAP: f32 = 6.0;
pub(super) const CONTENT_PADDING: f32 = 8.0;
pub(super) const TAB_GAP: f32 = 6.0;
pub(super) const DIAGNOSTIC_FONT_SIZE: f32 = 12.0;
pub(super) const DIAGNOSTIC_LINE_HEIGHT: f32 = 1.1;

/// 构造与 F3 完全相同的右上监控面板布局。
pub(super) fn resource_monitor_panel_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        top: Val::Px(PANEL_TOP),
        right: Val::Px(PANEL_RIGHT),
        width: Val::Px(PANEL_WIDTH),
        max_width: Val::Percent(PANEL_MAX_WIDTH_PERCENT),
        max_height: Val::Percent(PANEL_MAX_HEIGHT_PERCENT),
        padding: UiRect::all(Val::Px(PANEL_PADDING)),
        border: UiRect::all(Val::Px(1.0)),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(PANEL_GAP),
        overflow: Overflow::clip_y(),
        ..default()
    }
}

/// 构造与 F3 相同的横向页签行。
pub(super) fn resource_monitor_tabs_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(TAB_GAP),
        ..default()
    }
}

/// 构造与 F3 相同的等分顶层组按钮。
pub(super) fn resource_monitor_tab_node() -> Node {
    Node {
        min_width: Val::Px(0.0),
        flex_grow: 1.0,
        height: Val::Px(28.0),
        padding: UiRect::horizontal(Val::Px(10.0)),
        border: UiRect::all(Val::Px(1.0)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

/// 构造与 F3 相同的固定宽度二级页签按钮。
pub(super) fn resource_monitor_subtab_node(is_visible: bool) -> Node {
    Node {
        width: Val::Px(132.0),
        height: Val::Px(26.0),
        padding: UiRect::horizontal(Val::Px(10.0)),
        border: UiRect::all(Val::Px(1.0)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        display: resource_monitor_page_display(is_visible),
        ..default()
    }
}

/// 构造与 F3 相同的单页正文分区。
pub(super) fn resource_monitor_content_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        min_width: Val::Px(0.0),
        padding: UiRect::all(Val::Px(CONTENT_PADDING)),
        border: UiRect::all(Val::Px(1.0)),
        flex_direction: FlexDirection::Column,
        ..default()
    }
}

/// 非当前页必须退出布局，不能用 Hidden 留下累积高度。
pub(super) fn resource_monitor_page_display(is_selected: bool) -> Display {
    if is_selected {
        Display::Flex
    } else {
        Display::None
    }
}

/// 构造与 F3 相同的全宽正文文字节点。
pub(super) fn resource_monitor_text_node(is_selected: bool) -> Node {
    Node {
        width: Val::Percent(100.0),
        display: resource_monitor_page_display(is_selected),
        ..default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_layout_matches_f3_and_fits_target_1080p_viewport() {
        let panel = resource_monitor_panel_node();
        assert_eq!(panel.width, Val::Px(PANEL_WIDTH));
        assert_eq!(panel.max_width, Val::Percent(PANEL_MAX_WIDTH_PERCENT));
        assert_eq!(panel.max_height, Val::Percent(PANEL_MAX_HEIGHT_PERCENT));
        let Val::Px(panel_width) = panel.width else {
            panic!("F4 监控面板宽度必须使用像素值");
        };
        let Val::Px(panel_right) = panel.right else {
            panic!("F4 监控面板右边距必须使用像素值");
        };
        let Val::Px(panel_top) = panel.top else {
            panic!("F4 监控面板顶部偏移必须使用像素值");
        };
        assert!(panel_width + panel_right < TARGET_VIEWPORT_WIDTH);
        assert!(panel_top < TARGET_VIEWPORT_HEIGHT);
    }

    #[test]
    fn tabs_are_horizontal_and_content_uses_full_width() {
        assert_eq!(
            resource_monitor_tabs_node().flex_direction,
            FlexDirection::Row
        );
        assert_eq!(resource_monitor_content_node().width, Val::Percent(100.0));
    }

    #[test]
    fn inactive_page_uses_display_none() {
        assert_eq!(resource_monitor_text_node(true).display, Display::Flex);
        assert_eq!(resource_monitor_text_node(false).display, Display::None);
    }
}
