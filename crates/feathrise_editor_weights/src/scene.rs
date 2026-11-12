//! 权重右栏的静态 BSN 结构。按钮标记由场景插件消费，动态文案由原会话回显。

use bevy::{prelude::*, text::FontSourceTemplate, ui::FocusPolicy};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::{ButtonColors, InteractionButton, colors, metrics};
use feathrise_editor_weights_model::{
    components::{Action, EditControls, Entry, Label},
    types::Tool,
};

const NORMAL: ButtonColors = ButtonColors::new(
    colors::BUTTON_DEV_SMALL_NORMAL,
    colors::BUTTON_DEV_SMALL_HOVERED,
    colors::BUTTON_DEV_SMALL_PRESSED,
);
fn label(role: Label) -> impl Scene {
    let layout = if matches!(role, Label::Button(_)) {
        TextLayout::new(Justify::Center, bevy::text::LineBreak::NoWrap)
    } else {
        TextLayout::new(Justify::Left, bevy::text::LineBreak::AnyCharacter)
    };
    bsn! {
        ~{role} Text("") ~{FocusPolicy::Pass}
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(metrics::FONT_CONTROL)
        }
        TextColor(colors::TEXT_PRIMARY)
        ~{layout}
        Node { min_width: px(0), flex_shrink: 0.0, overflow: Overflow::clip_x() }
    }
}

fn button(action: Action) -> impl Scene {
    bsn! {
        InteractionButton ~{action} ~{NORMAL}
        Node {
            min_height: px(24), min_width: px(0), flex_grow: 1.0, flex_basis: px(0),
            justify_content: JustifyContent::Center, align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(4)), border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)), overflow: Overflow::clip_x()
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@label(Label::Button(action))]
    }
}

fn row() -> impl Scene {
    bsn! { Node { width: percent(100), min_width: px(0), flex_shrink: 0.0, column_gap: px(3) } }
}

pub(super) fn entry() -> impl Scene {
    bsn! {
        Entry
        Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column,
            row_gap: px(3), flex_shrink: 0.0 }
        Children [@button(Action::Open)--@label(Label::EntryNotice)]
    }
}

fn edit_controls() -> impl Scene {
    bsn! {
        EditControls
        Node { display: Display::None, flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [
            @row() Children [@button(Action::Mesh(-1))--@button(Action::Mesh(1))]--
            @row() Children [@button(Action::Lock)]--
            @row() Children [@button(Action::Mode(Tool::Point))--@button(Action::Mode(Tool::Box))--@button(Action::Clear)]--
            @row() Children [@button(Action::Mode(Tool::Add))--@button(Action::Mode(Tool::Subtract))--@button(Action::Mode(Tool::Smooth))]--
            @row() Children [@button(Action::Visible)--@button(Action::Mirror)--@button(Action::Mask)]--
            @row() Children [@button(Action::Apply)--@button(Action::Remove)--@button(Action::Assign)]--
            @label(Label::Selection)--
            @label(Label::Legend)--
            @row() Children [@button(Action::Compare)--@button(Action::Rest)]--
            @row() Children [@button(Action::OpenSaved)]
        ]
    }
}

pub(super) fn sidebar_page() -> impl Scene {
    bsn! {
        Node {
            display: Display::None, width: percent(100), min_width: px(0), min_height: px(0),
            flex_grow: 1.0, flex_basis: px(0), flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(), row_gap: px(4), padding: UiRect::all(px(3))
        }
        Children [
            @label(Label::Target)--
            @label(Label::Notice)--
            @label(Label::Geometry)--
            @label(Label::Joint)--
            @edit_controls()--
            @row() Children [@button(Action::Heat)--@button(Action::Skeleton)]--
            @row() Children [@button(Action::Discard)]
        ]
    }
}
