//! 在业务语义色上统一叠加按钮反馈；只处理绘制背景，不写回逻辑色或交互状态。

use crate::UiInteraction;
use super::resources::{EditorPalette, key};
use crate::colors;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use feathrise_ui_widgets::ButtonColors;

#[derive(Clone, Copy)]
pub(super) struct ButtonFeedback {
    pub interaction: UiInteraction,
    pub disabled: bool,
    pub background: LinearRgba,
    pub colors: Option<ButtonColors>,
}

/// Bevy按压捕获可跨越按钮边界；明确移出后取消按压视觉，输入取消由共享点击契约处理。
pub(super) fn interaction_for_cursor(
    interaction: UiInteraction,
    cursor: Option<&RelativeCursorPosition>,
) -> UiInteraction {
    if interaction == UiInteraction::Pressed && cursor.is_some_and(|cursor| !cursor.cursor_over()) {
        UiInteraction::None
    } else {
        interaction
    }
}

impl ButtonFeedback {
    pub fn resolve(self, palette: &EditorPalette) -> LinearRgba {
        let mut base = self.background;
        // 原生三键等控件的普通态透明，显式三态颜色本身就是完整反馈（关闭键为红色）。
        // 不能把红色状态归一成透明黑色再增亮；普通态/禁用也应保持透明，不遗留按压底色。
        if let Some(colors) = self.colors
            && colors.normal.is_fully_transparent()
            && [colors.normal, colors.hovered, colors.pressed]
                .iter()
                .any(|color| color.to_linear() == base)
        {
            let color = if self.disabled {
                colors.normal
            } else {
                match self.interaction {
                    UiInteraction::None => colors.normal,
                    UiInteraction::Hovered => colors.hovered,
                    UiInteraction::Pressed => colors.pressed,
                }
            };
            return palette.resolve(color).to_linear();
        }
        // 公共控件可能已经着色，业务也可能刚覆盖回普通/选中色；先恢复底色，避免双重增亮。
        if let Some(colors) = self.colors
            && [colors.hovered, colors.pressed]
                .iter()
                .any(|color| key(color.to_linear()) == key(base))
        {
            base = colors.normal.to_linear().with_alpha(base.alpha);
        } else {
            for (normal, hovered, pressed) in [
                (
                    colors::BUTTON_DEV_SMALL_NORMAL,
                    colors::BUTTON_DEV_SMALL_HOVERED,
                    colors::BUTTON_DEV_SMALL_PRESSED,
                ),
                (
                    colors::SELECT_BG,
                    colors::SELECT_BG_HOVER,
                    colors::SELECT_BG_PRESSED,
                ),
            ] {
                if [hovered, pressed]
                    .iter()
                    .any(|color| key(color.to_linear()) == key(base))
                {
                    base = normal.to_linear().with_alpha(base.alpha);
                    break;
                }
            }
        }
        let mapped = palette.resolve_linear(base);
        if self.disabled {
            return mapped;
        }
        let lift = match self.interaction {
            UiInteraction::None => return mapped,
            UiInteraction::Hovered => 0.07,
            UiInteraction::Pressed => 0.13,
        };
        // 等量提升显示亮度，保留选中蓝灰、危险红及自定义底色之间的色相差异和透明强度。
        let source = Color::LinearRgba(mapped).to_srgba();
        Color::srgba(
            (source.red + lift).min(1.0),
            (source.green + lift).min(1.0),
            (source.blue + lift).min(1.0),
            source.alpha,
        )
        .to_linear()
    }
}
