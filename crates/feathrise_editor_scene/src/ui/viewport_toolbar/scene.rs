//! 场景工具条固定一排：视图与参数同排横滚，切换工具不改变场景视口矩形。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use bevy::ui::RelativeCursorPosition;

use super::components::{ViewportToolbar, ViewportToolbarScroll, ViewportViewControls};
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

pub(in crate::ui) fn toolbar_layout(display: Display) -> impl Scene {
    bsn! {
        ViewportToolbarScroll
        RelativeCursorPosition::default()
        ScrollPosition::default()
        Node {
            display: display,
            width: percent(100),
            min_width: px(0),
            height: px(32), // 一排26px、底部留白5px和弱分隔线1px，任何工具共用此预算。
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            column_gap: px(8), // 视图与参数保持弱分组，不占用第二排。
            padding: UiRect::bottom(px(5)),
            border: UiRect::bottom(px(1)),
            overflow: Overflow::scroll_x(),
        } // 不能因字段数或窄窗换行推动RTT视口边界；不叠到场景画面或复制相机。
        BorderColor::all(colors::COPPER_BORDER_WEAK)
    }
}

pub(in crate::ui) fn control_group() -> impl Scene {
    bsn! {
        Node { min_width: px(0), height: px(26), align_items: AlignItems::Center, column_gap: px(2), flex_shrink: 0.0 }
    } // 保留自然宽度，只由整条工具栏横滚。
}

pub(in crate::ui) fn view_label() -> impl Scene {
    bsn! {
        Text("视图") @feathrise_editor_ui::localization::label_scene("视图")
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(ui_metrics::FONT_HINT),
        }
        TextColor(colors::TEXT_MUTED)
        TextLayout::no_wrap()
        Node { margin: UiRect::right(px(6)), flex_shrink: 0.0 }
    }
}

pub(in crate::ui) fn viewport_toolbar() -> impl Scene {
    bsn! {
        @toolbar_layout(Display::Flex)
        ViewportToolbar
        Children [
                ViewportViewControls
                @control_group()
                Children [
                    @view_label()--
                    @view_button("自由", EditorAction::ViewPerspective)--
                    @view_button("顶视", EditorAction::ViewTop)--
                    @view_button("前视", EditorAction::ViewFront)--
                    @view_button("侧视", EditorAction::ViewSide)--
                    // 正交只切换投影，与观察方向同排；窄视口继续由整条工具栏横滚。
                    @view_button("正交", EditorAction::ToggleOrthographic)--
                    @view_button("聚焦所选", EditorAction::FocusSelected)--
                    @view_button("查看全图", EditorAction::FrameScene)
                ]
            --
            @crate::viewport::snapping::toolbar()--
            @brush_controls_panel() // 保持原四个稳定槽位；无适用参数时仅收起该组，整条工具栏仍保持32px。
        ]
    }
}
