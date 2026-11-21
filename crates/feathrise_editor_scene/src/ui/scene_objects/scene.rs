//! 右侧属性栏的场景对象管理控件，沿用编辑器的中文输入样式。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::components::*;
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineHeight};
use feathrise_scene_content::MAX_PLACEMENT_EDITOR_NAME_CHARS;

fn button(label: &'static str, action: ObjectAction) -> impl Scene {
    bsn! {
        InteractionButton
        ObjectButton(action)
        Node { min_height: px(25), padding: UiRect::axes(px(5), px(3)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), flex_shrink: 0.0 }
        BackgroundColor(colors::SLOT_BG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [Text(label) @feathrise_editor_ui::localization::label_scene(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)]
    }
}

fn field(label: &'static str, kind: ObjectField) -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Row,
            column_gap: px(4), align_items: AlignItems::Center, flex_shrink: 0.0 }
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_HINT)
                Node { width: px(36), min_width: px(0), flex_shrink: 0.0 }--

                // 命中、键盘焦点和文字绘制共用实体，点击文字或边缘都能聚焦。
                InteractionButton ObjectInput(kind) crate::ui::text_input::EditorTextInput ~{TabIndex(0)}
                ~{crate::ui::text_input::single_line_input("", MAX_PLACEMENT_EDITOR_NAME_CHARS)}
                bevy::text::EditableTextFilter::new(|character| !character.is_control())
                bevy::text::TextCursorStyle::default() TextLayout::no_wrap() ~{LineHeight::Px(18.0)}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY)
                Node { flex_grow: 1.0, min_width: px(0), height: px(26), padding: UiRect::axes(px(5), px(3)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)), overflow: Overflow::clip_x() }
                BackgroundColor(colors::INPUT_BG)
                BorderColor::all(colors::COPPER_BORDER_WEAK)
        ]
    }
}

/// 所选属性只挂载在右侧；复用原控件与作者事务。
pub(crate) fn properties_panel() -> impl Scene {
    bsn! { ~{crate::ui::scene_inspector::InspectorSection::ObjectManagement} Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [
            ~{crate::ui::scene_inspector::InspectorSection::ObjectName}
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    @field("名称", ObjectField::Name)--
                    @button("确认名称", ObjectAction::Rename)
                ]--
            @field("分组", ObjectField::Group)--

                Node { width: percent(100), min_width: px(0), flex_wrap: FlexWrap::Wrap, column_gap: px(3), row_gap: px(3) }
                Children [
                    @button("归组", ObjectAction::AssignGroup)--
                    @button("移出组", ObjectAction::Ungroup)--
                    @button("选择组", ObjectAction::SelectGroup)--
                    @button("显隐", ObjectAction::ToggleHidden)--
                    @button("锁定", ObjectAction::ToggleLocked)
                ]
            --

                ObjectSummary
                Text("选择对象或目录后进行管理") @feathrise_editor_ui::localization::label_scene("选择对象或目录后进行管理")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_HINT)
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }

        ]
    }
}
