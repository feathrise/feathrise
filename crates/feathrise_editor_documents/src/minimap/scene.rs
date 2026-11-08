//! 代码小地图固定栏：轮廓、视区框跟随编辑器配色，仅在中央编辑时显示。

use feathrise_editor_ui::metrics as ui_metrics;
use bevy::{
    prelude::*,
    text::FontSourceTemplate,
    ui::{FocusPolicy, RelativeCursorPosition},
};

use super::super::{components::*, constants::*};
use feathrise_editor_core::FONT_PATH;
use feathrise_editor_ui::colors;

pub(crate) fn panel() -> impl Scene {
    let thumb_background = colors::TEXT_ACCENT.with_alpha(0.16);
    bsn! {
        DocumentMinimapRoot
        Node { display: Display::None, width: px(MINIMAP_PANEL_WIDTH), height: percent(100),
            min_height: px(0), flex_shrink: 0.0, flex_direction: FlexDirection::Column,
            row_gap: px(4), padding: px(4), border: px(1), overflow: Overflow::clip() }
        BackgroundColor(colors::INPUT_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (@feathrise_editor_ui::context_help::section_header("代码小地图", "点击或拖动轮廓可定位正文，视区框表示当前可见范围；不会修改源码。轮廓生成和失败信息见底栏详情。", FONT_PATH)),
            (
                DocumentMinimapRail RelativeCursorPosition::default() Interaction::default()
                ~{FocusPolicy::Block}
                Node { width: percent(100), min_height: px(0), flex_grow: 1.0,
                    flex_basis: px(0), overflow: Overflow::clip() }
                Children [
                    (
                        DocumentMinimapImage
                        ImageNode { image_mode: bevy::ui::widget::NodeImageMode::Stretch, color: colors::TEXT_MUTED }
                        ~{FocusPolicy::Pass}
                        Node { display: Display::None, position_type: PositionType::Absolute,
                            top: px(0), left: px(0), width: percent(100), height: px(0) }
                    ),
                    (
                        super::super::navigation::components::NavigationOverview
                        ImageNode { image_mode: bevy::ui::widget::NodeImageMode::Stretch, color: colors::TEXT_ACCENT }
                        ~{FocusPolicy::Pass}
                        Node { display: Display::None, position_type: PositionType::Absolute,
                            top: px(0), right: px(0), width: px(4), height: px(0) }
                        ZIndex(2)
                    ),
                    (
                        DocumentMinimapThumb ~{FocusPolicy::Pass}
                        Node { position_type: PositionType::Absolute, top: px(0), left: px(0),
                            width: percent(100), height: px(0), border: px(1) }
                        BackgroundColor(thumb_background)
                        BorderColor::all(colors::COPPER_BORDER)
                        ZIndex(1)
                    ),
                ]
            ),
            (
                DocumentMinimapLabel feathrise_editor_ui::PanelStatusText("代码小地图") feathrise_editor_ui::feedback::GlobalPanelStatus Text("") ~{FocusPolicy::Pass}
                Node { display: Display::None }
                TextLayout::no_wrap()
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_HINT)
            ),
        ]
    }
}
