//! 场景树上方的紧凑管理控件，复用正式中文EditableText与当前编辑器主题。

use feathrise_editor_ui::metrics as ui_metrics;
use super::components::*;
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;

fn button(label: &'static str, action: ObjectAction) -> impl Scene {
    bsn! {
        Button
        ObjectButton(action)
        Node { min_height: px(25), padding: UiRect::axes(px(5), px(3)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), flex_shrink: 0.0 }
        BackgroundColor(colors::SLOT_BG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(Text(label) feathrise_editor_ui::localization::label_scene(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY))]
    }
}

fn field(label: &'static str, kind: ObjectField) -> impl Scene {
    let search = kind == ObjectField::Search;
    let direction = FlexDirection::Row;
    let alignment = AlignItems::Center;
    let input_width = Val::Auto;
    let input_grow = 1.0;
    bsn! {
        Node { width: percent(100), min_width: px(0), flex_direction: direction,
            column_gap: px(4), row_gap: px(2), align_items: alignment, flex_shrink: 0.0 }
        Children [
            (Text(label) feathrise_editor_ui::localization::label_scene(label) TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_HINT)
                Node { width: px(if search { 28.0 } else { 36.0 }), min_width: px(0), flex_shrink: 0.0 }),
            (
                Button
                ObjectButton(ObjectAction::Focus(kind))
                Node { width: input_width, flex_grow: input_grow, min_width: px(0), height: px(26), align_items: AlignItems::Center, padding: UiRect::horizontal(px(5)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)), overflow: Overflow::clip_x() }
                BackgroundColor(colors::INPUT_BG)
                BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [(
                    ObjectInput(kind)
                    crate::ui::text_input::EditorTextInput
                    bevy::text::EditableText { allow_newlines: false }
                    bevy::text::EditableTextFilter::new(|character| !character.is_control())
                    bevy::text::TextCursorStyle::default()
                    TextLayout::no_wrap()
                    TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                    TextColor(colors::TEXT_PRIMARY)
                    Node { width: percent(100), min_width: px(0), height: px(18), overflow: Overflow::clip_x() }
                )]
            ),
        ]
    }
}

pub(crate) fn panel() -> impl Scene {
    bsn! {
        SceneAssetSearch Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
        Children [
            (field("搜索", ObjectField::Search)),
        ]
    }
}

/// 所选属性只挂载在右侧；复用原控件与作者事务。
pub(crate) fn properties_panel() -> impl Scene {
    bsn! { template_value(crate::ui::scene_inspector::InspectorSection::ObjectManagement) Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [
            (template_value(crate::ui::scene_inspector::InspectorSection::ObjectName)
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [
                    (field("名称", ObjectField::Name)),
                    (button("确认名称", ObjectAction::Rename)),
                ]),
            (field("分组", ObjectField::Group)),
            (
                Node { width: percent(100), min_width: px(0), flex_wrap: FlexWrap::Wrap, column_gap: px(3), row_gap: px(3) }
                Children [
                    (button("归组", ObjectAction::AssignGroup)),
                    (button("移出组", ObjectAction::Ungroup)),
                    (button("选择组", ObjectAction::SelectGroup)),
                    (button("显隐", ObjectAction::ToggleHidden)),
                    (button("锁定", ObjectAction::ToggleLocked)),
                ]
            ),
            (
                ObjectSummary
                Text("选择对象或目录后进行管理") feathrise_editor_ui::localization::label_scene("选择对象或目录后进行管理")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_HINT)
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            ),
        ]
    }
}
