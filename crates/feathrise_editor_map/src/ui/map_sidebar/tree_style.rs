//! 根行与虚拟行池共用交互优先级，选中、悬停和有效落点不互相掩盖。

use feathrise_editor_ui::UiInteraction;
use crate::ui::theme as colors;
use bevy::prelude::*;

pub(crate) fn tree_row_colors(
    selected: bool,
    drop_target: bool,
    interaction: UiInteraction,
) -> (Color, BorderColor) {
    let background = if drop_target {
        colors::SELECT_BG_HOVER
    } else {
        match interaction {
            UiInteraction::Pressed => colors::SELECT_BG_PRESSED,
            UiInteraction::Hovered => colors::SELECT_BG_HOVER,
            UiInteraction::None if selected => colors::SELECT_BG,
            UiInteraction::None => colors::PANEL_BG,
        }
    };
    let border = BorderColor::all(
        if drop_target || selected || interaction != UiInteraction::None {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        },
    );
    (background, border)
}
