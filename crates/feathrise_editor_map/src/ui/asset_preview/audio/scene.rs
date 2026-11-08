//! 紧凑音频播放器在原画布内迁移，沿用三种编辑器主题，不增加预览View。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, types::*};
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};

fn label(text: &str, kind: AudioLabel) -> impl Scene {
    let width = if matches!(kind, AudioLabel::Volume) {
        px(82)
    } else {
        Val::Auto
    };
    bsn! { ~{kind} Text(text) ~{FocusPolicy::Pass}
        Node { width, flex_shrink: 0.0 }
    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
    TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap() }
}

fn button(text: &str, action: AudioAction) -> impl Scene {
    bsn! { InteractionButton ~{AudioButton { action, key: None }}
    Node { height: px(26), padding: UiRect::horizontal(px(9)), flex_shrink: 0.0,
        align_items: AlignItems::Center, justify_content: JustifyContent::Center,
        border: px(1), border_radius: BorderRadius::all(px(3)) }
    BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
    Children [(
        ~{AudioLabel::Button(action)} Text(text) ~{FocusPolicy::Pass}
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_PRIMARY)
    )] }
}

fn slider(field: SliderField) -> impl Scene {
    bsn! { ~{AudioTrack { field, key: None }} RelativeCursorPosition::default()
    Interaction::default() ~{FocusPolicy::Block}
    Node { width: percent(100), height: px(22), min_width: px(0), flex_shrink: 1.0,
        position_type: PositionType::Relative, align_items: AlignItems::Center,
        border: px(1), border_radius: BorderRadius::all(px(3)), overflow: Overflow::clip() }
    BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
    Children [(
        ~{AudioFill(field)} ~{FocusPolicy::Pass}
        Node { width: percent(0), height: percent(100), flex_shrink: 0.0 }
        BackgroundColor(colors::SELECT_BG)
    )] }
}

pub(in super::super) fn panel() -> impl Scene {
    bsn! { AudioPanel
    Node { display: Display::None, width: percent(100), max_width: px(720), min_width: px(0),
        flex_shrink: 0.0, flex_direction: FlexDirection::Column, row_gap: px(8),
        padding: UiRect::all(px(10)) }
    Children [
        (Node { width: percent(100), min_width: px(0), height: px(24), align_items: AlignItems::Center,
            padding: UiRect::right(px(22)) }
            Children [(@label("音频试听", AudioLabel::Playback)),
                (@crate::ui::context_help::help_button("音频试听", "拖动进度试听片段；试听音量叠加全局音量。播放和停止只影响当前文件，切换文件或离开预览不会自动播放。"))]),
        (
            Node { width: percent(100), min_width: px(0), align_items: AlignItems::Center,
                column_gap: px(6), row_gap: px(4), flex_wrap: FlexWrap::Wrap }
            Children [(@button("播放", AudioAction::Toggle)), (@button("停止", AudioAction::Stop)),
                (@button("重新载入", AudioAction::Reload))]
        ),
        (Node { width: percent(100), min_width: px(0), align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween, column_gap: px(4) }
            Children [(@label("00:00 / 00:00", AudioLabel::Time)), (@label("", AudioLabel::Metadata))]),
        (@slider(SliderField::Position)),
        (
            Node { width: percent(100), min_width: px(0), align_items: AlignItems::Center, column_gap: px(8) }
            Children [
                (@label("音量 75%", AudioLabel::Volume)),
                (@slider(SliderField::Volume)),
            ]
        ),
        (
            ~{AudioLabel::Details} crate::ui::status_bar::PanelStatusText("音频输出") feathrise_editor_ui::feedback::GlobalPanelStatus
            Text("") ~{FocusPolicy::Pass}
            TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
            TextColor(colors::TEXT_MUTED) Node { display: Display::None }
        ),
    ] }
}
