//! 六项工具固定分在两组两列，目录留在右栏的正文滚动区外。

use super::{components::*, types::GameplaySidebarPage as Page};
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::{prelude::*, text::FontSourceTemplate};
use feathrise_editor_ui::{InteractionButton, localization::label_scene, metrics};

fn label(value: &str, color: Color) -> impl Scene {
    bsn! {
        Text(value) @label_scene(value) Pickable::IGNORE
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(metrics::FONT_CONTROL) }
        TextColor(color) TextLayout::no_wrap()
        Node { flex_shrink: 0.0 }
    }
}

fn entry(page: Page) -> impl Scene {
    let title = page.label();
    bsn! {
        ~{Name::new(format!("交互目录条目{title}"))} InteractionButton GameplaySidebarTab(page)
        Node { height: px(28), min_width: px(0), flex_basis: px(0), flex_grow: 1.0,
            padding: UiRect::horizontal(px(6)), border: px(1), border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center, overflow: Overflow::clip() }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@label(title, colors::TEXT_PRIMARY)]
    }
}

fn group(title: &'static str) -> impl Scene {
    bsn! {
        ~{Name::new(format!("交互目录{title}分组"))}
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0,
            flex_direction: FlexDirection::Column, row_gap: px(4) }
    }
}

fn row(left: Page, right: Page) -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), column_gap: px(4), flex_shrink: 0.0 }
        Children [@entry(left)-- @entry(right)]
    }
}

fn heading() -> impl Scene {
    bsn! {
        Node { height: px(22), align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween }
        Children [
            @label("工程", colors::TEXT_ACCENT)--
            ~{Name::new("工程规则模板菜单")} InteractionButton TemplateMenuButton
            Node { width: px(24), height: px(22), justify_content: JustifyContent::Center, align_items: AlignItems::Center }
            Children [@label("···", colors::TEXT_MUTED)]--
            TemplateMenu ~{Visibility::Hidden} GlobalZIndex(422) // 盖住同组下一行，层级低于独立文件窗口。
            Node { display: Display::None, position_type: PositionType::Absolute, top: px(22), right: px(0) }
            Children [@super::project_rules::sample_button()]
        ]
    }
}

pub(super) fn directory() -> impl Scene {
    // 各行都由同一对等宽单元构成，拖动分隔条只改变单元宽度。
    bsn! {
        ~{Name::new("交互工具目录")}
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0, flex_direction: FlexDirection::Column,
            row_gap: px(7), padding: UiRect::all(px(5)), border: px(1) }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            @group("场景")
                Children [
                    Node { height: px(22), align_items: AlignItems::Center }
                    Children [@label("场景", colors::TEXT_ACCENT)]--
                    @row(Page::Portal, Page::Boundary)--
                    @row(Page::MotionTrigger, Page::Locations)
                ]--
            @group("工程")
                Children [@heading()-- @row(Page::Variables, Page::Rules)]
        ]
    }
}
