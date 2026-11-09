//! 根直属的四节点提示池；绝对定位与鼠标穿透确保不挤动工具条、标签或正文。

use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineBreak};
use bevy::ui::FocusPolicy;

use super::components::{TooltipPopup, TooltipText};
use crate::ui::{scene::FONT_PATH, theme as colors};

pub(in crate::ui) fn tooltip_popup() -> impl Scene {
    bsn! {
        TooltipPopup Pickable::IGNORE ~{FocusPolicy::Pass}
        Node { display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: px(280), max_width: percent(95), min_width: px(0),
            flex_direction: FlexDirection::Column, row_gap: px(3), padding: px(7),
            border: px(1), border_radius: BorderRadius::all(px(4)) }
        // 高于主体但低于右键菜单及模态；没有透明输入遮罩或第二套鼠标所有权。
        GlobalZIndex(425) BackgroundColor(colors::PANEL_BG_STRONG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            @tooltip_text(TooltipText::Title, colors::TEXT_ACCENT, 11.0)--
            @tooltip_text(TooltipText::Description, colors::TEXT_PRIMARY, 10.5)--
            @tooltip_text(TooltipText::Reason, colors::DANGER_TEXT, 10.5)
        ]
    }
}

fn tooltip_text(kind: TooltipText, color: Color, size: f32) -> impl Scene {
    bsn! {
        ~{kind} Pickable::IGNORE ~{FocusPolicy::Pass}
        Text("") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(size) }
        TextColor(color) TextShadow { offset: Vec2::splat(1.0) }
        TextLayout::linebreak(LineBreak::AnyCharacter)
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}
