//! 复用常规设置的墨色卡片、弱铜边与中文排版；LOD状态与禁用物理明确区分。

use feathrise_editor_ui::metrics as ui_metrics;
use super::components::*;
use bevy::{prelude::*, text::{FontSourceTemplate, LineBreak}};
use crate::ui::{scene::FONT_PATH, theme as colors};
use super::super::{components::SettingsGroup, scene::group_node};

fn button(label: &str, action: Action) -> impl Scene {
    bsn! {
        Button ActionButton(action)
        Node { height: px(28), width: percent(100), min_width: px(0), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(3)), padding: UiRect::horizontal(px(4)) }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(ActionLabel(action) Text(label) TextLayout::linebreak(LineBreak::AnyCharacter)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY))]
    }
}

fn slot(field: MeshLodField) -> impl Scene {
    bsn! { Slot(field) Node { width: percent(100), min_width: px(0), height: px(26), flex_shrink: 0.0 } }
}

pub(crate) fn panel() -> impl Scene {
    bsn! {
        SettingsGroup template_value(group_node())
        BackgroundColor(colors::PANEL_BG_SECTION) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (crate::ui::context_help::section_header("全局网格 LOD", "当前通用策略用于普通静态GLB、可雕刻网格及建筑外观；Clipmap与体素保留各自的高级分层。显示按屏幕误差选择精度；碰撞独立，按近场区域驻留，32米内保持原始精度。关闭LOD保留原始网格和碰撞，独立对象覆盖也受总开关约束。")),
            (button("显示 LOD：开启", Action::Display)),
            (slot(MeshLodField::ScreenError)), (slot(MeshLodField::Hysteresis)),
            (button("碰撞 LOD：原始", Action::Collision)),
            (slot(MeshLodField::SafeNear)), (slot(MeshLodField::CollisionError)),
            (button("恢复网格 LOD 默认", Action::Reset)),
        ]
    }
}
