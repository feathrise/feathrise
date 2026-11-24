//! 场景树与文件窗口共用右键面板和行项样式，各自保留尺寸与命令规则。

use super::theme as colors;
use bevy::{prelude::*, ui::FocusPolicy};
use feathrise_editor_ui::{InteractionButton, UiInteraction};

pub(super) const ROW_HEIGHT: f32 = 26.;
pub(super) const PANEL_BORDER: f32 = 1.;
const ROW_GAP: f32 = 2.;
const PANEL_PADDING: f32 = 4.;
const ROW_PADDING: f32 = 8.;
const ROW_BORDER: f32 = 1.;
pub(super) const PANEL_INSET: f32 = PANEL_PADDING + PANEL_BORDER;
const ARROW_SIZE: f32 = 12.;
const ARROW_GAP: f32 = 4.;
pub(super) const ARROW_SPACE: f32 = ARROW_SIZE + ARROW_GAP;
pub(super) const ARROW_ICON: &str = "icons/editor/tree_expand.png";

pub(super) fn row_pitch() -> f32 { ROW_HEIGHT + ROW_GAP }

pub(super) fn panel_height(rows: usize) -> f32 {
    2. * PANEL_INSET + rows as f32 * ROW_HEIGHT + rows.saturating_sub(1) as f32 * ROW_GAP
}

/// 按字体实测宽度计算文件窗菜单时，计入面板与按钮两侧各自的边框和内距。
pub(super) fn measured_width(label_width: f32) -> f32 {
    (label_width + 2. * (PANEL_INSET + ROW_PADDING + ROW_BORDER)).ceil()
}

pub(super) fn panel() -> Node {
    Node {
        position_type: PositionType::Absolute,
        flex_direction: FlexDirection::Column,
        row_gap: px(ROW_GAP),
        padding: UiRect::all(px(PANEL_PADDING)),
        border: UiRect::all(px(PANEL_BORDER)),
        border_radius: BorderRadius::all(px(6.)),
        overflow: Overflow::scroll_y(),
        ..default()
    }
}

/// 主菜单与侧开菜单共用完整可交互菜单项，命令身份由调用方添加。
pub(super) fn item() -> impl Bundle {
    let (color, border) = item_colors(true, UiInteraction::None);
    (
        InteractionButton,
        Node {
            width: percent(100.), min_width: px(0.), height: px(ROW_HEIGHT),
            flex_shrink: 0., padding: UiRect::horizontal(px(ROW_PADDING)), align_items: AlignItems::Center,
            column_gap: px(ARROW_GAP), border: UiRect::all(px(ROW_BORDER)),
            border_radius: BorderRadius::all(px(3.)), overflow: Overflow::clip_x(),
            ..default()
        },
        BackgroundColor(color),
        border,
    )
}

/// 复用真实箭头图标。它贴行右缘，命中仍由所属菜单项处理。
pub(super) fn arrow(image: &Handle<Image>) -> impl Bundle {
    (
        ImageNode { image: image.clone(), color: colors::TEXT_PRIMARY, ..default() },
        Node { width: px(ARROW_SIZE), height: px(ARROW_SIZE), flex_shrink: 0.,
            margin: UiRect::left(Val::Auto), ..default() },
        Pickable::IGNORE,
    )
}

/// 行文字继续交还行命中，依同一字体、字号和截断规则排版。
pub(super) fn label(text: impl Bundle, font: &Handle<Font>, color: Color) -> impl Bundle {
    (
        text,
        TextFont { font: font.clone().into(), font_size: 11.0.into(), ..default() },
        TextColor(color), TextLayout::no_wrap(), FocusPolicy::Pass, Pickable::IGNORE,
        Node { min_width: px(0), overflow: Overflow::clip_x(), ..default() },
    )
}

pub(super) fn item_colors(enabled: bool, interaction: UiInteraction) -> (Color, BorderColor) {
    let background = if enabled {
        match interaction {
            UiInteraction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
            UiInteraction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
            UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL,
        }
    } else { colors::INPUT_BG };
    let border = if enabled && interaction != UiInteraction::None { colors::COPPER_BORDER } else { colors::COPPER_BORDER_WEAK };
    (background, BorderColor::all(border))
}
