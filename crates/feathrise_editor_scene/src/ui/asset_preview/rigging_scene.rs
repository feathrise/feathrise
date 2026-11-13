//! 模型画布共用固定关节点，模型属性通过单个入口打开左侧工程动作目录。

use bevy::{prelude::*, text::FontSourceTemplate, ui::{FocusPolicy, RelativeCursorPosition, Val2}};
use feathrise_editor_ui::{
    ButtonColors,
    InteractionButton,
    localization::LocalizedText,
    metrics as ui_metrics,
};

use super::types::*;
use crate::ui::{scene::FONT_PATH, theme as colors};
use crate::ui::sidebars::{EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarPage};

fn button(action: RigAction) -> impl Scene {
    let colors = if matches!(action, RigAction::Bind | RigAction::ChooseProjectMotion) {
        ButtonColors::new(colors::SELECT_BG, colors::SELECT_BG_HOVER, colors::SELECT_BG_PRESSED)
    } else {
        ButtonColors::new(colors::BUTTON_DEV_SMALL_NORMAL, colors::BUTTON_DEV_SMALL_HOVERED, colors::BUTTON_DEV_SMALL_PRESSED)
    };
    bsn! {
        InteractionButton
        ~{action}
        ~{colors}
        Node {
            display: Display::None,
            min_height: px(24), min_width: px(0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(6)), overflow: Overflow::clip_x(),
            border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)),
        }
        ~{BackgroundColor(colors.normal)}
        BorderColor::all(colors::COPPER_BORDER_WEAK)
    }
}

fn button_text(label: &str, key: &'static str) -> impl Scene {
    bsn! {
        Text(label) ~{LocalizedText::new(key)}
        ~{FocusPolicy::Pass}
        TextColor(colors::TEXT_PRIMARY)
        TextLayout::no_wrap()
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(ui_metrics::FONT_CONTROL),
        }
    }
}

fn label(role: RigText, color: Color, single_line: bool) -> impl Scene {
    let layout = if single_line {
        TextLayout::no_wrap()
    } else {
        TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter, ..default() }
    };
    bsn! {
        ~{role}
        ~{FocusPolicy::Pass}
        Text("")
        ~{layout}
        TextColor(color)
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(ui_metrics::FONT_CONTROL),
        }
        Node { min_width: px(0), overflow: Overflow::clip_x() }
    }
}

fn fixed_button(action: RigAction, caption: &str, key: &'static str) -> impl Scene {
    bsn! {
        @button(action)
        Children [@button_text(caption, key)]
    }
}

pub(in crate::ui::asset_preview) fn launch() -> impl Scene {
    bsn! {
        @button(RigAction::Launch)
        Children [@label(RigText::Launch, colors::TEXT_PRIMARY, true)]
    }
}

#[derive(Component, Clone, Copy, Default)]
pub(super) struct RigToolHost;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct RigMotionHost;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct RigMotionHelp;

pub(in crate::ui::asset_preview) fn motion_properties_host() -> impl Scene {
    bsn! {
        RigMotionHost
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0,
            flex_direction: FlexDirection::Column }
    }
}

pub(crate) fn sidebar_page() -> impl Scene {
    bsn! {
        RigToolHost
        SceneEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::Rigging))
        crate::components::SceneEditorRightPanelScroll
        RelativeCursorPosition::default() ScrollPosition::default()
        Node {
            display: Display::None, width: percent(100), min_width: px(0), min_height: px(0),
            flex_grow: 1.0, flex_basis: px(0), flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
        }
        Children [@panel()]
    }
}

fn panel() -> impl Scene {
    bsn! {
        RigPanel
        Node {
            display: Display::None,
            width: percent(100), min_width: px(0), flex_shrink: 0.0,
            flex_direction: FlexDirection::Column, row_gap: px(4),
            padding: UiRect::all(px(6)),
            border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)),
        }
        BackgroundColor(colors::PANEL_BG_STRONG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Node {
                align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap,
                column_gap: px(5), row_gap: px(3), min_width: px(0),
            }
            Children [
                Node { flex_grow: 1.0, min_width: px(0) }
                Children [@label(RigText::Title, colors::TEXT_PRIMARY, true)]
                --
                @button(RigAction::SwitchBackend)
                Children [@label(RigText::Backend, colors::TEXT_PRIMARY, true)]
                --
                @fixed_button(RigAction::Retry, "重新检查", "rig.retry")--
                @button(RigAction::Close)
                Children [@label(RigText::Close, colors::TEXT_PRIMARY, true)]--
                RigMotionHelp
                Node { display: Display::None, width: px(18), height: px(18), flex_shrink: 0.0 }
                Children [@crate::ui::context_help::help_button("工程动作", "点击「选择动画」打开活动文件选择窗口。单击文件只在窗口内选中候选，点击文件箭头展开片段列表。选中片段后点击「选择」，在当前模型上播放。点击「取消」或关闭窗口结束选择。")]
            ]
            --
            @label(RigText::Notice, colors::TEXT_VALUE, false)--
            @label(RigText::Warning, colors::TEXT_HINT, false)--
            @fixed_button(RigAction::ChooseProjectMotion, "选择动画", "rig.motion.choose_project")--
            RigMarkerTools
            Node { display: Display::None, width: percent(100), flex_direction: FlexDirection::Column }
            Children [
                @label(RigText::SelectedMarker, colors::TEXT_MUTED, false)--
                @label(RigText::Reference, colors::TEXT_VALUE, false)
            ]
            --
            RigMarkerTools
            Node { column_gap: px(5), row_gap: px(3), flex_wrap: FlexWrap::Wrap }
            Children [
                @button(RigAction::ToggleMirror)
                Children [@label(RigText::Mirror, colors::TEXT_PRIMARY, true)]
                --
                @fixed_button(RigAction::ResetMarks, "恢复关节点", "rig.reset_marks")--
                @fixed_button(RigAction::Bind, "生成已绑骨 GLB", "rig.bind")
            ]
        ]
    }
}

fn dot(index: usize) -> impl Scene {
    bsn! {
        ~{RigLandmarkDot(index)}
        Pickable::IGNORE
        ~{FocusPolicy::Pass}
        Node {
            display: Display::None, position_type: PositionType::Absolute,
            width: px(12), height: px(12), border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(6)),
        }
        UiTransform { translation: Val2::px(-6.0, -6.0) }
        BackgroundColor(colors::TEXT_ACCENT)
        BorderColor::all(colors::PANEL_BG_STRONG)
    }
}

/// 标记为同一画布内的投影覆盖，子标记不抢拾取，保持右键环绕与中键平移。
pub(in crate::ui::asset_preview) fn dots() -> impl Scene {
    bsn! {
        Pickable::IGNORE
        ~{FocusPolicy::Pass}
        Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100), height: percent(100) }
        Children [
            @dot(0)-- @dot(1)-- @dot(2)-- @dot(3)-- @dot(4)-- @dot(5)--
            @dot(6)-- @dot(7)-- @dot(8)-- @dot(9)-- @dot(10)-- @dot(11)--
            @dot(12)-- @dot(13)-- @dot(14)-- @dot(15)-- @dot(16)
        ]
    }
}
