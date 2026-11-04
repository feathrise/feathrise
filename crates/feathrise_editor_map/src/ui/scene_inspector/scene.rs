//! 右侧全高的对象属性、概览与反馈，独立于左侧场景树滚动。

use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineBreak};

use super::components::{SceneInspectorOverview, SceneInspectorProperties, SceneInspectorText};
use super::types::SceneInspectorField;
use crate::ui::scene::FONT_PATH;
use crate::ui::sidebars::{EditorSidebarHelp, MapEditorSidebarHelpBody, sidebar_help_button};
use crate::ui::theme as colors;

fn inspector_text(marker: SceneInspectorField, size: f32, color: Color) -> impl Scene {
    let feedback = (marker == SceneInspectorField::Hint).then(|| bsn! {
        template_value(crate::ui::status_bar::PanelStatusText("所选对象"))
    });
    (bsn! {
        SceneInspectorText(marker)
        Text("")
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(size.max(feathrise_editor_ui::metrics::FONT_HINT)),
        }
        TextColor(color)
        TextLayout { linebreak: LineBreak::AnyCharacter }
        Node {
            width: percent(100),
            min_width: px(0), // 无分隔符的长来源路径也不能撑宽检查器。
            flex_shrink: 0.0,
        }
    }, feedback)
}

pub(in crate::ui) fn scene_inspector_overview() -> impl Scene {
    bsn! {
        SceneInspectorOverview
        Node {
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: px(4),
        }
        Children [
            (inspector_text(SceneInspectorField::Name, 11.5, colors::TEXT_PRIMARY)),
            (super::components::SelectedToolPropertiesHost
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, flex_shrink: 0.0, row_gap: px(4) }),
        ]
    }
}

pub(in crate::ui) fn scene_inspector_feedback() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: px(4),
            padding: UiRect::bottom(px(5)),
        }
        Children [
            // 这里只保留本对象诊断；全局操作结果统一由底部状态栏显示，避免重复占用属性高度。
            (inspector_text(SceneInspectorField::Hint, 9.5, colors::TEXT_HINT)),
            (super::components::SceneInspectorMetadata
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0, flex_direction: FlexDirection::Column }
                Children [
                    (sidebar_help_button(EditorSidebarHelp::Inspector)),
                    (MapEditorSidebarHelpBody(EditorSidebarHelp::Inspector)
                        Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                        Children [(inspector_text(SceneInspectorField::Details, 9.5, colors::TEXT_HINT))]),
                ]),
        ]
    }
}

/// 两个原属性树只创建一次；容器显隐不改变选中身份、数值焦点或作者事务。
pub(in crate::ui) fn scene_inspector_properties() -> impl Scene {
    bsn! {
        SceneInspectorProperties
        Node {
            display: Display::None,
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: px(4),
        }
        Children [
            (crate::ui::placement_transform::placement_transform_panel()),
            (crate::ui::placement_water::placement_water_properties_panel()),
        ]
    }
}
