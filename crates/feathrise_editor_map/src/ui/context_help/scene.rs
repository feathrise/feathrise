//! 十八像素问号与根直属弱灰帮助窗口；静态说明只创建一次，默认完全退出正文布局。

use feathrise_editor_ui::InteractionButton;
use super::components::*;
use crate::ui::{scene::FONT_PATH, theme as colors};
use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_ui::metrics as ui_metrics;

/// 插入现有分组即可；绝对定位不为问号或隐藏说明新增空白行。
pub(crate) fn help_button(title: &str, copy: &str) -> impl Scene {
    feathrise_editor_ui::context_help::help_button(title, copy, FONT_PATH)
}

/// 需要分组名时复用同一有效标题行，问号不会另占一行或遮住标题。
pub(crate) fn section_header(title: &str, copy: &str) -> impl Scene {
    feathrise_editor_ui::context_help::section_header(title, copy, FONT_PATH)
}

pub(in crate::ui) fn popup() -> impl Scene {
    let close = feathrise_editor_ui::dialog_close_button_style();
    bsn! {
        ContextHelpPopup RelativeCursorPosition::default() Interaction::default()
        ~{FocusPolicy::Block}
        // 帮助与设置、详情及日志使用同一外壳；来源锚点和65%阅读高度仍由原定位流程维护。
        ~{Node { display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: px(420), max_height: percent(65),
            ..feathrise_editor_ui::dialog_panel_node() }}
        GlobalZIndex(950) // 高于设置900、低于切图等更高模态；普通帮助在设置打开时先收起。
        BackgroundColor(feathrise_editor_ui::colors::DIALOG_BG)
        BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
        Children [
            (~{feathrise_editor_ui::dialog_header_node()}
                BackgroundColor(feathrise_editor_ui::colors::DIALOG_HEADER_BG)
                BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
                Children [
                    (ContextHelpTitle Text("操作说明") @feathrise_editor_ui::localization::label_scene("操作说明")
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_BODY) }
                        TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()
                        Node { min_width: px(0), flex_grow: 1.0, overflow: Overflow::clip_x() }
                        ~{FocusPolicy::Pass}),
                    (InteractionButton ContextHelpClose
                        ~{close.node} @feathrise_editor_ui::dialog_button_colors_scene(false)
                        ~{BackgroundColor(close.colors.normal)}
                        BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
                        Children [(Text("×")
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_PRIMARY) ~{FocusPolicy::Pass})]),
                ]),
            (ContextHelpScroll RelativeCursorPosition::default() ScrollPosition::default()
                ~{FocusPolicy::Block}
                ~{Node { flex_shrink: 1.0, overflow: Overflow::scroll_y(),
                    ..feathrise_editor_ui::dialog_body_node() }}),
        ]
    }
}
