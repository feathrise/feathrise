//! 地图和 GLB 共用单行工具栏和右端的多行展开入口。

use feathrise_editor_ui::{ButtonColors, InteractionButton};
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use bevy::ui::RelativeCursorPosition;

use super::components::{ViewportToolbar, ViewportToolbarExpandButton, ViewportToolbarScroll, ViewportViewControls};
use crate::components::SceneEditorActionButton;
use crate::types::EditorAction;
use crate::ui::{brush_controls::brush_controls_panel, scene::FONT_PATH, theme as colors};

/// 自然文字宽度的视图切换项；继续由原动作分派和选中样式读取EditorView。
fn view_button(label: &str, action: EditorAction) -> impl Scene {
    let tooltip = crate::ui::tooltips::TooltipAction::Editor(action);
    bsn! {
        @control_surface()
        SceneEditorActionButton(action)
        crate::ui::tooltips::EditorTooltipTarget(tooltip)
        Children [
            @control_text(label, ui_metrics::FONT_CONTROL) @feathrise_editor_ui::localization::label_scene(label)
        ]
    }
}

/// 场景和中央模型预览共用按钮尺寸和暖墨表面，各自绑定独立业务动作。
pub(in crate::ui) fn control_surface() -> impl Scene {
    bsn! {
        InteractionButton
        Node {
            min_width: px(0),
            max_width: percent(100),
            height: px(26),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(8)),
            border: UiRect::bottom(px(2)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
    }
}

pub(in crate::ui) fn control_text(label: &str, font_size: f32) -> impl Scene {
    bsn! {
        Text(label)
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(font_size),
        }
        TextColor(colors::TEXT_PRIMARY)
        TextLayout::no_wrap()
    }
}

pub(in crate::ui) fn toolbar_layout(display: Display, controls: impl Scene) -> impl Scene {
    let row = bsn! {
        ViewportToolbarScroll
        RelativeCursorPosition::default()
        ScrollPosition::default()
        Node {
            min_width: px(0),
            min_height: px(32),
            height: px(32), // 一排26px、底部留白5px和弱分隔线1px。
            align_items: AlignItems::Center,
            align_content: AlignContent::FlexStart,
            flex_grow: 1.0,
            flex_basis: px(0),
            column_gap: px(8),
            padding: UiRect::bottom(px(5)),
            border: UiRect::bottom(px(1)),
            overflow: Overflow::scroll_x(),
        }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        @controls
    };
    bsn! {
        Node {
            display: display, width: percent(100), min_width: px(0), min_height: px(32),
            flex_shrink: 0.0, align_items: AlignItems::FlexStart, column_gap: px(3),
        }
        Children [@row--@expand_button()]
    }
}

fn expand_button() -> impl Scene {
    let name = Name::new("工具栏换行");
    let palette = ButtonColors::new(
        colors::BUTTON_DEV_SMALL_NORMAL,
        colors::BUTTON_DEV_SMALL_HOVERED,
        colors::BUTTON_DEV_SMALL_PRESSED,
    );
    bsn! {
        ViewportToolbarExpandButton
        InteractionButton ~{palette} ~{name}
        Node {
            display: Display::None, width: px(26), height: px(26), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@control_text("▼", ui_metrics::FONT_HINT) Pickable::IGNORE]
    }
}

pub(in crate::ui) fn control_group() -> impl Scene {
    bsn! {
        Node { min_width: px(0), height: px(26), align_items: AlignItems::Center, column_gap: px(2), flex_shrink: 0.0 }
    } // 保留自然宽度，只由整条工具栏横滚。
}

pub(in crate::ui) fn viewport_toolbar() -> impl Scene {
    let controls = bsn! {
        ViewportToolbar
        Children [
                ViewportViewControls
                @control_group()
                Children [
                    @view_button("自由", EditorAction::ViewPerspective)--
                    @view_button("顶视", EditorAction::ViewTop)--
                    @view_button("前视", EditorAction::ViewFront)--
                    @view_button("侧视", EditorAction::ViewSide)--
                    // 正交只切换投影，排列在观察方向后面。
                    @view_button("正交", EditorAction::ToggleOrthographic)--
                    @view_button("聚焦所选", EditorAction::FocusSelected)--
                    @view_button("查看全图", EditorAction::FrameScene)
                ]
            --
            @crate::viewport::snapping::toolbar()--
            @brush_controls_panel()
        ]
    };
    toolbar_layout(Display::Flex, controls)
}
