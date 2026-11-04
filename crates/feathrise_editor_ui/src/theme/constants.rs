//! 保留原暖墨常量作为稳定语义身份；两套新主题集中映射，透明强调沿用原比例。
use crate::colors as c;
use bevy::prelude::*;

pub(super) const fn rgba(rgb: u32, alpha: f32) -> Color {
    Color::srgba(
        ((rgb >> 16) & 255) as f32 / 255.0,
        ((rgb >> 8) & 255) as f32 / 255.0,
        (rgb & 255) as f32 / 255.0,
        alpha,
    )
}

// 原逻辑色 / 中性深灰 / 石墨灰+雾蓝；别名共用同一语义，不在业务页复制色板。
pub(super) const COLORS: &[(Color, Color, Color)] = &[
    (c::OVERLAY_BG, rgba(0x1e1e1e, 1.0), rgba(0x181a1f, 1.0)),
    (c::PANEL_BG, rgba(0x262626, 1.0), rgba(0x22252b, 1.0)),
    (c::PANEL_BG_STRONG, rgba(0x262626, 1.0), rgba(0x1e2127, 1.0)),
    (
        c::PANEL_BG_SECTION,
        rgba(0x262626, 1.0),
        rgba(0x2b3038, 1.0),
    ),
    (c::SLOT_BG, rgba(0x282828, 1.0), rgba(0x202329, 1.0)),
    (c::SLOT_BG_FILLED, rgba(0x303030, 1.0), rgba(0x292e36, 1.0)),
    (
        c::SLOT_BG_DISABLED,
        rgba(0x242424, 0.62),
        rgba(0x1e2126, 0.62),
    ),
    (
        c::MODEL_PREVIEW_BG,
        rgba(0x1e1e1e, 1.0),
        rgba(0x181a1f, 1.0),
    ),
    (c::COPPER_BORDER, rgba(0x999999, 1.0), rgba(0x86b7e8, 1.0)),
    (
        c::COPPER_BORDER_WEAK,
        rgba(0x3a3a3a, 1.0),
        rgba(0x353b45, 1.0),
    ),
    (c::DANGER_BG, rgba(0x542929, 0.9), rgba(0x542929, 0.9)),
    (c::DANGER_BORDER, rgba(0xc87777, 0.8), rgba(0xc87777, 0.8)),
    (c::DANGER_TEXT, rgba(0xe1b0b0, 1.0), rgba(0xe1b0b0, 1.0)),
    (c::TEXT_PRIMARY, rgba(0xd4d4d4, 1.0), rgba(0xd6dae1, 1.0)),
    (c::TEXT_MUTED, rgba(0xa0a0a0, 1.0), rgba(0xa2aab6, 1.0)),
    (c::TEXT_DIM, rgba(0xa0a0a0, 0.48), rgba(0xa2aab6, 0.48)),
    (c::TEXT_HINT, rgba(0x909090, 1.0), rgba(0x929baa, 1.0)),
    (c::TEXT_ACCENT, rgba(0xd4d4d4, 1.0), rgba(0x86b7e8, 1.0)),
    (c::TEXT_VALUE, rgba(0xd4d4d4, 1.0), rgba(0xd6dae1, 1.0)),
    (c::TEXT_GREEN, rgba(0xa3b89a, 1.0), rgba(0x9dbf94, 1.0)),
    (c::SYNTAX_KEYWORD, rgba(0xb8bbcf, 1.0), rgba(0xb9a7d5, 1.0)),
    (c::SYNTAX_STRING, rgba(0xadbfa0, 1.0), rgba(0xa7c08a, 1.0)),
    (c::SYNTAX_NUMBER, rgba(0xc1b5cb, 1.0), rgba(0xc4a5d0, 1.0)),
    (c::SYNTAX_COMMENT, rgba(0x91988e, 1.0), rgba(0x919b89, 1.0)),
    (c::SYNTAX_TYPE, rgba(0xb4c5c0, 1.0), rgba(0x96c6bf, 1.0)),
    (
        c::BUTTON_SETTINGS_HOVERED,
        rgba(0x343434, 0.9),
        rgba(0x2b3542, 0.9),
    ),
    (
        c::BUTTON_SETTINGS_PRESSED,
        rgba(0x404040, 0.95),
        rgba(0x354960, 0.95),
    ),
    (
        c::BUTTON_DEV_SMALL_NORMAL,
        rgba(0x262626, 1.0),
        rgba(0x2b3038, 1.0),
    ),
    (
        c::BUTTON_DEV_SMALL_HOVERED,
        rgba(0x393939, 1.0),
        rgba(0x343e4a, 1.0),
    ),
    (
        c::BUTTON_DEV_SMALL_PRESSED,
        rgba(0x454545, 1.0),
        rgba(0x3d5167, 1.0),
    ),
    (c::TAB_ACTIVE_BG, rgba(0x394751, 1.0), rgba(0x2b3c4f, 1.0)),
    (c::TAB_HOVERED_BG, rgba(0x3e3e3e, 1.0), rgba(0x34485d, 1.0)),
    (c::TAB_PRESSED_BG, rgba(0x484848, 1.0), rgba(0x3d5269, 1.0)),
    (c::INPUT_BG, rgba(0x353535, 1.0), rgba(0x191c21, 1.0)),
    (c::INPUT_ACTIVE_BG, rgba(0x3b3b3b, 1.0), rgba(0x243447, 1.0)),
    (
        c::INPUT_DISABLED_BORDER,
        rgba(0x3a3a3a, 0.5),
        rgba(0x353b45, 0.5),
    ),
    (c::THUMB_BG, rgba(0xaaaaaa, 1.0), rgba(0x86b7e8, 1.0)),
    (c::SELECT_BG, rgba(0x394751, 1.0), rgba(0x2b3c4f, 1.0)), // 中性主题采用用户参考图的蓝灰选中背景。
    (c::SELECT_BG_HOVER, rgba(0x404040, 1.0), rgba(0x34485d, 1.0)),
    (
        c::SELECT_BG_PRESSED,
        rgba(0x484848, 1.0),
        rgba(0x3d5269, 1.0),
    ),
    (
        c::COMPACT_BUTTON_BG,
        rgba(0x303030, 1.0),
        rgba(0x252a32, 1.0),
    ),
];
