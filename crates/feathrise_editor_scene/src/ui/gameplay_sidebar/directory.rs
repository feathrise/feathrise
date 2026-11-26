//! 交互目录固定为首行四项、次行两项，目录留在右栏的正文滚动区外。

use super::{components::*, types::GameplaySidebarPage as Page};
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::{prelude::*, text::FontSourceTemplate};
use feathrise_editor_ui::{InteractionButton, localization::label_scene, metrics};

fn label(value: &str) -> impl Scene {
    bsn! {
        Text(value) @label_scene(value) Pickable::IGNORE
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()
        Node { flex_shrink: 0.0 }
    }
}

fn entry(page: Page) -> impl Scene {
    let title = page.label();
    bsn! {
        ~{Name::new(format!("交互目录条目{title}"))} InteractionButton GameplaySidebarTab(page)
        Node { height: px(28), min_width: px(0),
            padding: UiRect::horizontal(px(3)), border: px(1), border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center, overflow: Overflow::clip() }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@label(title)]
    }
}

pub(super) fn directory() -> impl Scene {
    // 四字按钮留足 4 × 12px，在现有 260px 最小栏宽保持同一排。
    bsn! {
        ~{Name::new("交互工具目录")}
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0, display: Display::Grid,
            grid_template_columns: vec![RepeatedGridTrack::flex(4, 1.0)],
            row_gap: px(4), column_gap: px(3), padding: UiRect::all(px(4)), border: px(1) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            @entry(Page::Portal)-- @entry(Page::Boundary)--
            @entry(Page::MotionTrigger)-- @entry(Page::Locations)--
            @entry(Page::Variables)-- @entry(Page::Rules)
        ]
    }
}
