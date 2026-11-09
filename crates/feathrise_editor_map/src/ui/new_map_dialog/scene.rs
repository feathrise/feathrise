//! 资料优先的地图模态布局；所有作者字段仍使用原生输入实体和原确认动作。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineBreak, LineHeight};
use bevy::ui::RelativeCursorPosition;

use super::components::*;
use crate::components::*;
use crate::types::EditorAction;
use crate::ui::scene::FONT_PATH;
use crate::ui::theme as colors;

pub(super) fn panel() -> impl Scene {
    bsn! {
        NewMapDialogPanel
        Node {
            width: px(520), max_width: percent(100), max_height: percent(100),
            min_width: px(0), min_height: px(0), align_self: AlignSelf::Center,
            flex_shrink: 1.0, flex_direction: FlexDirection::Column, row_gap: px(8),
            padding: UiRect::all(px(12)), border: px(1),
            border_radius: BorderRadius::all(px(6)),
        }
        // 模态必须不透明，地图底层的文字和铜边不能穿透资料字段。
        BackgroundColor(Color::srgb(0.035, 0.030, 0.024))
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [

                NewMapDialogTitle
                Text("新建地图") @feathrise_editor_ui::localization::label_scene("新建地图")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_BODY) }
                TextColor(colors::TEXT_ACCENT)
                Node { flex_shrink: 0.0 }
            --

                NewMapDialogBodyScroll
                ScrollPosition::default()
                RelativeCursorPosition::default()
                Node {
                    width: percent(100), min_height: px(0), flex_shrink: 1.0,
                    flex_direction: FlexDirection::Column, row_gap: px(6),
                    padding: UiRect::right(px(3)), overflow: Overflow::scroll_y(),
                }
                Children [
                    @section_title("基础资料")--
                    @caption("地图名称 · 必填")--

                        // 唯一官方输入根保留Unicode、选区、光标及IME，外层不加截获点击的Button。
                        NewMapNameText
                        bevy::text::EditableText { allow_newlines: false }
                        crate::ui::text_input::EditorTextInput
                        bevy::text::EditableTextFilter::new(|character| !character.is_control())
                        bevy::text::TextCursorStyle::default()
                        TextLayout::no_wrap()
                        ~{LineHeight::Px(18.0)}
                        Node {
                            width: percent(100), height: px(26), flex_shrink: 0.0,
                            // 12px中文字体预留18px行框，26px外框扣除边框后上下各留3px。
                            padding: UiRect::axes(px(6), px(3)), border: px(1),
                            overflow: Overflow::clip_x(),
                        }
                        BackgroundColor(colors::INPUT_BG)
                        BorderColor::all(colors::INPUT_ACTIVE_BORDER)
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_PRIMARY)
                    --
                    @caption("备注 · 可选")--

                        ScrollPosition::default()
                        Node {
                            width: percent(100), height: px(64), flex_shrink: 0.0,
                            padding: UiRect::axes(px(6), px(4)), border: px(1),
                            overflow: Overflow::scroll_y(),
                        }
                        BackgroundColor(colors::INPUT_BG)
                        BorderColor::all(colors::INPUT_ACTIVE_BORDER)
                        Children [
                            NewMapNotesText
                            bevy::text::EditableText { allow_newlines: true }
                            crate::ui::text_input::EditorTextInput
                            bevy::text::EditableTextFilter::new(|character| character == '\n' || !character.is_control())
                            bevy::text::TextCursorStyle::default()
                            TextLayout::linebreak(LineBreak::WordOrCharacter)
                            Node { width: percent(100), min_height: px(54) }
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_PRIMARY)
                        ]
                    --

                        NewMapDialogHint
                        Text("")
                        TextLayout::linebreak(LineBreak::AnyCharacter)
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                        TextColor(colors::TEXT_HINT)
                        Node { width: percent(100), flex_shrink: 0.0 }

                ]
            --

                // 完整事务错误独立滚动、最多60px；错误再长也不把确认与取消推出屏幕。
                DialogFeedbackScroll
                ScrollPosition::default()
                RelativeCursorPosition::default()
                Node {
                    width: percent(100), min_height: px(28), max_height: px(60), flex_shrink: 0.0,
                    padding: UiRect::axes(px(6), px(4)), overflow: Overflow::scroll_y(),
                    border_radius: BorderRadius::all(px(3)),
                }
                BackgroundColor(colors::PANEL_BG_SECTION)
                Children [
                    NewMapStatusText
                    Text("请输入地图名称，可填写备注") @feathrise_editor_ui::localization::label_scene("请输入地图名称，可填写备注")
                    TextLayout::linebreak(LineBreak::AnyCharacter)
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                    TextColor(colors::TEXT_PRIMARY)
                    Node { width: percent(100), flex_shrink: 0.0 }
                ]
            --

                DialogFooter
                Node { width: percent(100), flex_shrink: 0.0, justify_content: JustifyContent::FlexEnd, flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(4) }
                Children [
                    @crate::ui::scene::editor_button("取消", EditorAction::CancelNewMap, 78.0)--
                    @confirm_button()
                ]

        ]
    }
}

fn confirm_button() -> impl Scene {
    bsn! {
        InteractionButton
        MapEditorActionButton(EditorAction::ConfirmNewMap)
        Node {
            width: px(110), height: px(26), flex_shrink: 0.0,
            border: px(1), border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(5)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            NewMapConfirmText Text("创建空白地图") @feathrise_editor_ui::localization::label_scene("创建空白地图")
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

fn caption(label: &str) -> impl Scene {
    bsn! {
        Text(label) @feathrise_editor_ui::localization::label_scene(label)
        TextLayout::linebreak(LineBreak::AnyCharacter)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
        TextColor(colors::TEXT_MUTED)
        Node { width: percent(100), flex_shrink: 0.0 }
    }
}

fn section_title(label: &str) -> impl Scene {
    bsn! {
        Text(label) @feathrise_editor_ui::localization::label_scene(label)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_ACCENT)
        Node { width: percent(100), margin: UiRect::top(px(3)), flex_shrink: 0.0 }
    }
}
