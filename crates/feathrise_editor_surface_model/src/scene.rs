//! 地形工具页中的全局PBR预设库与无分页连续列表，诊断由独立反馈宿主呈现。

use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use feathrise_editor_ui::colors;

use super::components::*;
use super::types::SurfacePresetAction;
use feathrise_editor_core::FONT_PATH;

fn button(label: &str, action: SurfacePresetAction) -> impl Scene {
    bsn! {
        Button
        SurfacePresetButton(action)
        Node {
            height: px(24),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(3)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(9.2) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

pub fn surface_preset_panel() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            flex_shrink: 0.0,
        }
        Children [
            (
                Text("全局预设库") feathrise_editor_ui::localization::label_scene("全局预设库")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(10.5) }
                TextColor(colors::TEXT_ACCENT)
            ),
            (
                Text("跨地图共用的 PBR 材质；选中后在下方新建地图层。") feathrise_editor_ui::localization::label_scene("跨地图共用的 PBR 材质；选中后在下方新建地图层。")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(9.5) }
                TextColor(colors::TEXT_MUTED)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0) }
            ),
            (
                Node { width: percent(100), column_gap: px(3), flex_shrink: 0.0 }
                Children [
                    (button("新建预设", SurfacePresetAction::Create)),
                    (button("编辑所选", SurfacePresetAction::EditSelected)),
                    (button("删除预设", SurfacePresetAction::DeleteSelected)),
                ]
            ),
            (
                SurfacePresetSummaryText
                Text("当前预设：尚未选择") feathrise_editor_ui::localization::label_scene("当前预设：尚未选择")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(9.0) }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0) }
            ),
            (
                SurfacePresetListTitle
                Text("地表预设 · 等待目录") feathrise_editor_ui::localization::label_scene("地表预设 · 等待目录")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(9.2) }
                TextColor(colors::TEXT_VALUE)
            ),
            (
                SurfacePresetList
                Node {
                    width: percent(100),
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(3),
                    row_gap: px(3),
                    flex_shrink: 0.0,
                }
            ),
        ]
    }
}

/// 原预设域仍拥有诊断内容；侧栏仅把唯一状态实体放入共享反馈区。
pub fn surface_preset_feedback() -> impl Scene {
    bsn! {
        SurfacePresetStatusText
        Text("选择预设后显式新建地图层；新建或编辑预设在中央标签完成") feathrise_editor_ui::localization::label_scene("选择预设后显式新建地图层；新建或编辑预设在中央标签完成")
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(9) }
        TextColor(colors::TEXT_MUTED)
        TextLayout { linebreak: LineBreak::AnyCharacter }
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}
