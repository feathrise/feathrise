//! 右侧属性栏的场景对象管理控件，沿用编辑器的中文输入样式。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::components::*;
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineHeight};
use feathrise_scene_content::MAX_PLACEMENT_EDITOR_NAME_CHARS;

fn button(label: &'static str, marker: impl Component + Clone + Unpin) -> impl Scene {
    bsn! {
        InteractionButton
        ~{marker}
        Node { min_height: px(25), padding: UiRect::axes(px(5), px(3)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), flex_shrink: 0.0 }
        BackgroundColor(colors::SLOT_BG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [Text(label) @feathrise_editor_ui::localization::label_scene(label) TextLayout::no_wrap() TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)]
    }
}

pub(in crate::ui) fn name_row(input: impl Component + Clone + Unpin, confirm: impl Component + Clone + Unpin) -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Row,
            column_gap: px(4), align_items: AlignItems::Center, flex_shrink: 0.0 }
        Children [
            Text("名称") @feathrise_editor_ui::localization::label_scene("名称") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_HINT)
                Node { width: px(36), min_width: px(0), flex_shrink: 0.0 }--

                // 命中、键盘焦点和文字绘制共用实体，点击文字或边缘都能聚焦。
                InteractionButton ~{input} crate::ui::text_input::EditorTextInput ~{TabIndex(0)}
                ~{crate::ui::text_input::single_line_input("", MAX_PLACEMENT_EDITOR_NAME_CHARS)}
                bevy::text::EditableTextFilter::new(|character| !character.is_control())
                bevy::text::TextCursorStyle::default() TextLayout::no_wrap() ~{LineHeight::Px(18.0)}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY)
                // 名称输入占标签与确认按钮以外的宽度，长名称只在输入框内裁切。
                Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), height: px(26), padding: UiRect::axes(px(5), px(3)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)), overflow: Overflow::clip_x() }
                BackgroundColor(colors::INPUT_BG)
                BorderColor::all(colors::COPPER_BORDER_WEAK)
            --
                @button("确认名称", confirm)
        ]
    }
}

/// 右侧属性编辑对象名称。归类、显隐和锁定都在场景树操作。
pub(crate) fn properties_panel() -> impl Scene {
    bsn! { ~{crate::ui::scene_inspector::InspectorSection::ObjectManagement} Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [
            ~{crate::ui::scene_inspector::InspectorSection::ObjectName}
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [@name_row(ObjectInput(ObjectField::Name), ObjectButton(ObjectAction::Rename))]--
                ObjectSummary
                Text("选择对象或文件夹后进行管理") @feathrise_editor_ui::localization::label_scene("选择对象或文件夹后进行管理")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_HINT)
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }

        ]
    }
}
