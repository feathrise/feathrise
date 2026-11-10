//! 编辑器暖墨搜索栏；表单保持固定实体，结果仅创建16个可复用行槽。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use super::{components::*, constants::*};
use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::colors;

fn button(label: &str, action: SearchAction) -> impl Scene {
    bsn! {
        InteractionButton ~{action}
        Node { height: px(24), padding: UiRect::horizontal(px(5)), border: px(1), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) ~{FocusPolicy::Pass} TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

fn field(kind: SearchField, max: usize) -> impl Scene {
    bsn! {
        ~{kind} feathrise_editor_core::EditorTextInput
        ~{feathrise_editor_ui::single_line_input("", max)}
        bevy::text::EditableTextFilter::new(|ch| !ch.is_control())
        bevy::text::TextCursorStyle::default() TextLayout::no_wrap() ~{FocusPolicy::Block}
        Node { height: px(24), min_width: px(0), flex_grow: 1.0, flex_basis: px(0),
            padding: UiRect::horizontal(px(4)), border: px(1), overflow: Overflow::clip() }
        BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::INPUT_ACTIVE_BORDER)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
    }
}

fn path_submit() -> impl Scene {
    bsn! {
        InteractionButton ~{SearchAction::Submit}
        Node { height: px(24), padding: UiRect::horizontal(px(5)), border: px(1), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            ~{SearchLabel::PathSubmit} Text("重新查找") @feathrise_editor_ui::localization::label_scene("重新查找") ~{FocusPolicy::Pass} TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

fn editor_choice(editor: feathrise_editor_core::ExternalEditor) -> impl Scene {
    let label = editor.name();
    bsn! {
        InteractionButton ~{SearchAction::ChooseEditor(editor)} ~{SearchEditorOption(editor)}
        Node { display: Display::None, height: px(24), padding: UiRect::horizontal(px(5)), border: px(1), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center }
        BackgroundColor(colors::SLOT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) ~{FocusPolicy::Pass} TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

pub(crate) fn panel() -> impl Scene {
    bsn! {
        Node { width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
        Children [

                // 窄侧栏按字宽将缺失检查折到下一行，保留右侧问号的命中空间。
                Node { width: percent(100), column_gap: px(3), row_gap: px(3),
                    flex_wrap: FlexWrap::Wrap, padding: UiRect::right(px(22)) }
                Children [
                    InteractionButton ~{SearchAction::Toggle}
                    Node { min_height: px(24), max_width: percent(100), flex_grow: 1.0,
                        padding: UiRect::horizontal(px(5)), align_items: AlignItems::Center }
                    BackgroundColor(colors::SLOT_BG)
                    Children [
                        ~{SearchLabel::Toggle} Text("内容搜索 Ctrl+Shift+F") @feathrise_editor_ui::localization::label_scene("内容搜索 Ctrl+Shift+F") ~{FocusPolicy::Pass}
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_ACCENT)
                    ]
                -- @button("缺失检查", SearchAction::MissingAssets)--
                    @feathrise_editor_ui::context_help::help_button("资产搜索", "输入文字后按Enter搜索；文件类型可填json、rs或*.wgsl，留空搜索全部受支持文本。点击结果在内置编辑器定位；外部打开使用磁盘版本，“打开工程”会同时打开工程目录。直接引用检查config/scenes JSON和当前工程的 Rust 源码中的完整路径，JSON优先使用草稿；不判断运行分支、拼接路径、预设ID或模型内部依赖，0处不代表未使用。缺失检查只核对明确路径的物理文件，子资源检查其来源文件。", FONT_PATH)]
            --

                SearchBody
                Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0 }
                Children [

                        SearchReferenceControls
                        Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column, row_gap: px(3) }
                        Children [
                            ~{SearchLabel::ReferenceTarget} Text("") ~{FocusPolicy::Pass}
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_PRIMARY)
                        --
                            Node { width: percent(100), column_gap: px(3) }
                            Children [@path_submit()-- @button("取消", SearchAction::Cancel)--
                                @button("内容搜索", SearchAction::TextSearch)]
                        ]
                    --

                        SearchTextControls
                        Node { width: percent(100), column_gap: px(3) }
                        Children [@field(SearchField::Query, QUERY_CHARS)-- @button("搜索", SearchAction::Submit)-- @button("取消", SearchAction::Cancel)]
                    --

                        SearchTextControls
                        Node { width: percent(100), column_gap: px(3), flex_wrap: FlexWrap::Wrap, row_gap: px(3) }
                        Children [@button("全部资产", SearchAction::AllAssets)-- @button("当前目录", SearchAction::CurrentFolder)--
                            @button("大小写", SearchAction::CaseSensitive)-- @button("全词", SearchAction::WholeWord)]
                    --

                        ~{SearchLabel::Scope} Text("范围：assets/（递归）") @feathrise_editor_ui::localization::label_scene("范围：assets/（递归）") ~{FocusPolicy::Pass}
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_MUTED)
                    --

                        SearchTextControls
                        Node { width: percent(100), column_gap: px(4), align_items: AlignItems::Center }
                        Children [
                            Text("文件类型") @feathrise_editor_ui::localization::label_scene("文件类型") ~{FocusPolicy::Pass} TextLayout::no_wrap()
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_MUTED)
                        -- @field(SearchField::Filter, FILTER_CHARS)]
                    --

                        ~{SearchLabel::Status} feathrise_editor_ui::PanelStatusText("资产搜索") feathrise_editor_ui::feedback::GlobalPanelStatus Text("") ~{FocusPolicy::Pass}
                        Node { display: Display::None, width: percent(100), min_width: px(0) }
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) } TextColor(colors::TEXT_ACCENT)
                    --

                        SearchTextControls
                        Node { width: percent(100), flex_direction: FlexDirection::Column }
                        Children [@super::replacement::sidebar()]
                    --

                        SearchEditorControls
                        Node { display: Display::None, width: percent(100), column_gap: px(3), row_gap: px(3),
                            flex_wrap: FlexWrap::Wrap, align_items: AlignItems::Center }
                        Children [
                            Text("外部编辑器") @feathrise_editor_ui::localization::label_scene("外部编辑器") ~{FocusPolicy::Pass} TextLayout::no_wrap()
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_MUTED)
                        -- @editor_choice(feathrise_editor_core::ExternalEditor::VsCode)--
                            @editor_choice(feathrise_editor_core::ExternalEditor::Zed)--
                            @editor_choice(feathrise_editor_core::ExternalEditor::RustRover)]
                    --

                        SearchResults RelativeCursorPosition::default() ScrollPosition::default()
                        Node { display: Display::None, width: percent(100), height: px(0), flex_shrink: 0.0, overflow: Overflow::scroll_y() }
                        BackgroundColor(colors::INPUT_BG)
                        Children [
                            SearchContent
                            Node { width: percent(100), height: px(0), flex_shrink: 0.0 }
                        ]

                ]

        ]
    }
}
