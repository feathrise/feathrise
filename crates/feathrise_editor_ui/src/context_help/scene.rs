//! 不占正文高度的问号与有效分组标题；构造器只生成静态UI，不注册另一套浮层或输入系统。

use crate::InteractionButton;
use super::components::*;
use crate::{colors, metrics};
use crate::localization::{editor_label, label_scene};
use bevy::{
    prelude::*,
    text::{FontSourceTemplate, LineBreak},
    ui::FocusPolicy,
};

/// 插入已有分组Node即可；正文是问号原子树的一部分，默认完全退出布局。
pub fn help_button(title: &str, copy: &str, font_path: &'static str) -> impl Scene {
    // 文案先拥有数据，BSN保存的延迟模板只移动owned值，不延长调用方&str的生命周期。
    let button = ContextHelpButton(title.to_owned());
    let body = Text::new(copy);
    let body_label = label_scene(copy);
    bsn! {
        InteractionButton ~{button}
        Node { position_type: PositionType::Absolute, right: px(0), top: px(0),
            width: px(18), height: px(18), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        Children [
            Text("?") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(font_path)), font_size: px(metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_HINT) Pickable::IGNORE ~{FocusPolicy::Pass}--
            ContextHelpBody ~{body} @body_label
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(font_path)), font_size: px(metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY) TextLayout::linebreak(LineBreak::WordOrCharacter)
                Pickable::IGNORE ~{FocusPolicy::Pass}
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
        ]
    }
}

/// 有标题的分组共用一行标题与问号；右侧预留命中区，长标题裁切而不遮住帮助入口。
pub fn section_header(title: &str, copy: &str, font_path: &'static str) -> impl Scene {
    let help = help_button(title, copy, font_path);
    let title_label = label_scene(title);
    let title_text = Text::new(title);
    bsn! {
        Node { width: percent(100), min_width: px(0), height: px(24), flex_shrink: 0.0,
            align_items: AlignItems::Center, padding: UiRect::right(px(24)) }
        Children [
            ~{title_text} @title_label TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(font_path)), font_size: px(metrics::FONT_BODY) }
                TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap()
                Node { min_width: px(0), overflow: Overflow::clip_x() } Pickable::IGNORE ~{FocusPolicy::Pass}--
            @help
        ]
    }
}

/// 动态作者工厂复用已加载字体，生成与BSN完全相同的契约，不另建浮层或读文件。
pub fn spawn_help_button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    title: &str,
    copy: &str,
) {
    parent
        .spawn((
            InteractionButton,
            ContextHelpButton(title.to_owned()),
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(0),
                width: px(18),
                height: px(18),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 0.0,
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new("?"),
                TextFont {
                    font: font.clone().into(),
                    font_size: metrics::FONT_CONTROL.into(),
                    ..default()
                },
                TextColor(colors::TEXT_HINT),
                Pickable::IGNORE, // 现代拾取显式穿透装饰文字，问号仍由父按钮受理。
                FocusPolicy::Pass,
            ));
            button.spawn((
                ContextHelpBody,
                editor_label(copy),
                TextFont {
                    font: font.clone().into(),
                    font_size: metrics::FONT_CONTROL.into(),
                    ..default()
                },
                TextColor(colors::TEXT_PRIMARY),
                TextLayout::linebreak(LineBreak::WordOrCharacter),
                Pickable::IGNORE,
                FocusPolicy::Pass,
                Node {
                    display: Display::None,
                    width: percent(100),
                    min_width: px(0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
        });
}
