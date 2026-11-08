//! 地图参数、世界调试与全局默认的静态环境页；原天体字段与折叠身份保持不变。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use bevy::ui::RelativeCursorPosition;

use super::components::{EnvironmentMapControls, EnvironmentSidebarText};
use super::types::EnvironmentTextField;
use crate::authoring::{
    CelestialColorSwatch, CelestialNumericInputSlot, CelestialNumericKey, CelestialNumericScope,
    CelestialOverrideProperty, CelestialOverrideToggle, CelestialOverrideToggleLabel,
    CelestialSectionBody, CelestialSectionToggle, CelestialSectionToggleLabel, CelestialValueField,
    EnvironmentNumericInputSlot, EnvironmentNumericKind,
};
use crate::components::{MapEditorEnvironmentText, MapEditorRightPanelScroll};
use crate::types::EditorAction;
use crate::ui::scene::{FONT_PATH, flex_editor_button, inspector_section_title};
use crate::ui::sidebars::{EditorRightSidebarTab, EditorSidebarTab, MapEditorSidebarPage};
use crate::ui::theme as colors;
use crate::world_environment_controls::WorldEnvironmentControlsSlot;

/// 两种天空模式与资源动作复用原作者动作；短文案在窄栏仍保持同排居中。
fn two_button_row(
    first_label: &str,           // 左列按钮文字。
    first_action: EditorAction,  // 左列结构化动作。
    second_label: &str,          // 右列按钮文字。
    second_action: EditorAction, // 右列结构化动作。
) -> impl Scene {
    bsn! {
        Node {
            width: percent(100), min_width: px(0),
            column_gap: px(3),
            flex_shrink: 0.0,
        }
        Children [
            (@flex_editor_button(first_label, first_action)),
            (@flex_editor_button(second_label, second_action)),
        ]
    }
}

/// 来源、范围说明与失败文本自然换行；不固定文本高度或叠加第二层滚动。
fn field_text(field: EnvironmentTextField) -> impl Scene {
    let node = bsn! {
        EnvironmentSidebarText(field)
        Text("")
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
        TextColor(colors::TEXT_HINT)
        TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    };
    let source = match field {
        EnvironmentTextField::MapFeedback => Some("地图环境"),
        EnvironmentTextField::GlobalFeedback => Some("全局环境"),
        EnvironmentTextField::MapHint => Some("环境作用范围"),
        _ => None,
    };
    // 实时计算值仍属属性；加载/保存/约束诊断保留原Text，由底栏集中采集和隐藏。
    (node, source.map(|source| bsn! { ~{crate::ui::status_bar::PanelStatusText(source)} }))
}

/// 一个环境专业数值框的整行挂载槽。
fn environment_numeric_slot(kind: EnvironmentNumericKind) -> impl Scene {
    bsn! {
        EnvironmentNumericInputSlot(kind)
        Node {
            width: percent(100),
            min_width: px(0), // 标签、实际值、单位与进度条均在300px右栏完整显示。
            height: px(24),
            flex_shrink: 0.0,
        }
    }
}

/// 全局或地图天体光照标量的公共数值挂载槽。
fn celestial_numeric_slot(scope: CelestialNumericScope, field: CelestialValueField) -> impl Scene {
    bsn! {
        CelestialNumericInputSlot(CelestialNumericKey::new(scope, field))
        Node {
            width: percent(100),
            min_width: px(0),
            height: px(24),
            flex_shrink: 0.0,
        }
    }
}

/// RGB共用一条属性行；三个原数值根仍独立持有颜色通道、拖动捕获及提交身份。
fn celestial_color_slots(scope: CelestialNumericScope, day: bool) -> impl Scene {
    let fields = if day { [CelestialValueField::DayRed, CelestialValueField::DayGreen, CelestialValueField::DayBlue] }
        else { [CelestialValueField::NightRed, CelestialValueField::NightGreen, CelestialValueField::NightBlue] };
    let slots = fields.into_iter().map(|field| bsn! {
        CelestialNumericInputSlot(CelestialNumericKey::new(scope, field))
        Node { min_width: px(0), flex_grow: 1.0, flex_basis: px(0), height: px(24), flex_shrink: 0.0 }
    }).collect::<Vec<_>>();
    bsn! { Node { width: percent(100), min_width: px(0), column_gap: px(ui_metrics::GAP), flex_shrink: 0.0 }
        Children [{slots}] }
}

/// 两个天体光照折叠区共用的铜边标题按钮。
fn celestial_section_header(scope: CelestialNumericScope) -> impl Scene {
    let title = match scope {
        CelestialNumericScope::Global => "▼ 太阳与月光 · 全局默认",
        CelestialNumericScope::Map => "▼ 太阳与月光 · 地图覆盖",
    };
    bsn! {
        InteractionButton
        CelestialSectionToggle(scope)
        Node {
            width: percent(100),
            min_width: px(0),
            height: px(26),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(6)),
            border: UiRect::top(px(1)),
            border_radius: BorderRadius::all(px(2)),
        }
        BackgroundColor(colors::PANEL_BG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            CelestialSectionToggleLabel(scope)
            Text(title) @feathrise_editor_ui::localization::label_scene(title)
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                font_size: px(ui_metrics::FONT_CONTROL),
            }
            TextColor(colors::TEXT_ACCENT)
        )]
    }
}

fn celestial_property_title(label: &str) -> impl Scene {
    bsn! {
        Text(label) @feathrise_editor_ui::localization::label_scene(label)
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(ui_metrics::FONT_HINT),
        }
        TextColor(colors::TEXT_MUTED)
        Node {
            width: percent(100),
            min_width: px(0),
            margin: UiRect::top(px(2)),
        }
    }
}

fn celestial_color_title(label: &str, scope: CelestialNumericScope, day: bool) -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            min_width: px(0),
            height: px(24),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            margin: UiRect::top(px(2)),
        }
        Children [
            (
                Text(label) @feathrise_editor_ui::localization::label_scene(label)
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(ui_metrics::FONT_HINT),
                }
                TextColor(colors::TEXT_MUTED)
            ),
            (
                CelestialColorSwatch { scope, day }
                Node {
                    width: px(38),
                    flex_shrink: 0.0,
                    height: px(13),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(2)),
                }
                BackgroundColor(Color::BLACK)
                BorderColor::all(colors::COPPER_BORDER_WEAK)
            ),
        ]
    }
}

fn celestial_override_title(property: CelestialOverrideProperty) -> impl Scene {
    let label = property.label();
    bsn! {
        Node {
            width: percent(100),
            min_width: px(0),
            height: px(24),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            margin: UiRect::top(px(2)),
        }
        Children [
            (
                Text(label) @feathrise_editor_ui::localization::label_scene(label)
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(ui_metrics::FONT_HINT),
                }
                TextColor(colors::TEXT_MUTED)
            ),
            (
                InteractionButton
                CelestialOverrideToggle(property)
                Node {
                    width: px(78),
                    flex_shrink: 0.0,
                    height: px(26),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                }
                BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
                BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [(
                    CelestialOverrideToggleLabel(property)
                    Text("跟随全局") @feathrise_editor_ui::localization::label_scene("跟随全局")
                    TextFont {
                        font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                        font_size: px(ui_metrics::FONT_CONTROL),
                    }
                    TextColor(colors::TEXT_PRIMARY)
                )]
            ),
        ]
    }
}

fn celestial_override_color_title(property: CelestialOverrideProperty, day: bool) -> impl Scene {
    let label = property.label();
    bsn! {
        Node {
            width: percent(100),
            min_width: px(0),
            min_height: px(24),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: px(4),
            margin: UiRect::top(px(2)),
        }
        Children [
            (
                Text(label) @feathrise_editor_ui::localization::label_scene(label)
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(ui_metrics::FONT_HINT),
                }
                TextColor(colors::TEXT_MUTED)
                Node { flex_grow: 1.0, min_width: px(0) }
            ),
            (
                CelestialColorSwatch {
                    scope: CelestialNumericScope::Map,
                    day,
                }
                Node {
                    width: px(30),
                    flex_shrink: 0.0,
                    height: px(13),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(2)),
                }
                BackgroundColor(Color::BLACK)
                BorderColor::all(colors::COPPER_BORDER_WEAK)
            ),
            (
                InteractionButton
                CelestialOverrideToggle(property)
                Node {
                    width: px(78),
                    flex_shrink: 0.0,
                    height: px(26),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                }
                BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
                BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [(
                    CelestialOverrideToggleLabel(property)
                    Text("跟随全局") @feathrise_editor_ui::localization::label_scene("跟随全局")
                    TextFont {
                        font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                        font_size: px(ui_metrics::FONT_CONTROL),
                    }
                    TextColor(colors::TEXT_PRIMARY)
                )]
            ),
        ]
    }
}

fn global_celestial_body() -> impl Scene {
    bsn! {
        CelestialSectionBody(CelestialNumericScope::Global)
        Node {
            display: Display::Flex,
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            padding: UiRect::horizontal(px(3)),
            flex_shrink: 0.0,
        }
        Children [
            (@celestial_property_title("日间基准照度")),
            (@celestial_numeric_slot(CelestialNumericScope::Global, CelestialValueField::DayIlluminance)),
            (@celestial_color_title("日间基准颜色 · sRGB 0–1", CelestialNumericScope::Global, true)),
            (@celestial_color_slots(CelestialNumericScope::Global, true)),
            (@celestial_property_title("月亮午夜峰值照度")),
            (@celestial_numeric_slot(CelestialNumericScope::Global, CelestialValueField::NightIlluminance)),
            (@celestial_color_title("夜间基准颜色 · sRGB 0–1", CelestialNumericScope::Global, false)),
            (@celestial_color_slots(CelestialNumericScope::Global, false)),
            (@celestial_property_title("天体方位角偏移")),
            (@celestial_numeric_slot(CelestialNumericScope::Global, CelestialValueField::Azimuth)),
            (@celestial_property_title("光源角直径")),
            (@celestial_numeric_slot(CelestialNumericScope::Global, CelestialValueField::AngularDiameter)),
        ]
    }
}

fn map_celestial_body() -> impl Scene {
    bsn! {
        CelestialSectionBody(CelestialNumericScope::Map)
        Node {
            display: Display::Flex,
            width: percent(100),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            padding: UiRect::horizontal(px(3)),
            flex_shrink: 0.0,
        }
        Children [
            (@celestial_override_title(CelestialOverrideProperty::DayIlluminance)),
            (@celestial_numeric_slot(CelestialNumericScope::Map, CelestialValueField::DayIlluminance)),
            (@celestial_override_color_title(CelestialOverrideProperty::DayColor, true)),
            (@celestial_color_slots(CelestialNumericScope::Map, true)),
            (@celestial_override_title(CelestialOverrideProperty::NightIlluminance)),
            (@celestial_numeric_slot(CelestialNumericScope::Map, CelestialValueField::NightIlluminance)),
            (@celestial_override_color_title(CelestialOverrideProperty::NightColor, false)),
            (@celestial_color_slots(CelestialNumericScope::Map, false)),
            (@celestial_override_title(CelestialOverrideProperty::Azimuth)),
            (@celestial_numeric_slot(CelestialNumericScope::Map, CelestialValueField::Azimuth)),
            (@celestial_override_title(CelestialOverrideProperty::AngularDiameter)),
            (@celestial_numeric_slot(CelestialNumericScope::Map, CelestialValueField::AngularDiameter)),
        ]
    }
}

/// 所有内容仍属于原环境页唯一滚动宿主；地图区域、世界调试和全局默认按原顺序排列。
pub(in crate::ui) fn environment_page() -> impl Scene {
    bsn! {
        MapEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::Environment))
        MapEditorRightPanelScroll
        RelativeCursorPosition::default()
        ScrollPosition::default()
        Node {
            display: Display::None,
            width: percent(100), min_width: px(0),
            flex_grow: 1.0, min_height: px(0),
            flex_direction: FlexDirection::Column, row_gap: px(3),
            padding: UiRect::top(px(3)), overflow: Overflow::scroll_y(),
        }
        Children [
            (@crate::ui::context_help::section_header("当前地图 · 持久参数", "此区写入当前动态地图。太阳、月光未启用覆盖的属性跟随全局默认；固定场景没有动态地图环境草稿。来源和实际计算值仍在相应字段中显示。")),
            (
                MapEditorEnvironmentText
                EnvironmentSidebarText(EnvironmentTextField::MapContext)
                Text("未进入地图") @feathrise_editor_ui::localization::label_scene("未进入地图")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            ),
            (@field_text(EnvironmentTextField::MapSource)),
            (@field_text(EnvironmentTextField::MapHint)),
            (
                EnvironmentMapControls
                Node {
                    display: Display::None, width: percent(100), min_width: px(0),
                    flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 0.0,
                }
                Children [
                    (@inspector_section_title("天空与环境光")),
                    (@two_button_row("动态天空", EditorAction::SkyModeDynamic, "天空盒", EditorAction::SkyModeSkybox)),
                    (@two_button_row("使用所选资源", EditorAction::SetSelectedSkybox, "清除资源", EditorAction::ClearSkybox)),
                    (@environment_numeric_slot(EnvironmentNumericKind::SkyboxBrightness)),
                    (@environment_numeric_slot(EnvironmentNumericKind::EnvironmentDiffuseIntensity)),
                    (@environment_numeric_slot(EnvironmentNumericKind::EnvironmentSpecularIntensity)),
                    (@environment_numeric_slot(EnvironmentNumericKind::SkyboxYaw)),
                    (@field_text(EnvironmentTextField::CurrentValues)),
                    (@celestial_section_header(CelestialNumericScope::Map)),
                    (@map_celestial_body()),
                ]
            ),
            (@field_text(EnvironmentTextField::MapFeedback)),
            (@crate::ui::context_help::section_header("世界调试 · 当前运行", "时刻、月份与天气覆盖只调整当前运行。时间暂停及晴天草尖基准摆幅沿用全局设置保存，不写入当前地图环境参数。")),
            (
                WorldEnvironmentControlsSlot
                Node {
                    width: percent(100), min_width: px(0),
                    flex_direction: FlexDirection::Column, row_gap: px(3),
                    flex_shrink: 0.0,
                }
            ),
            (@crate::ui::context_help::section_header("全局默认 · 所有动态地图", "地图未启用覆盖的属性跟随此处。全局草稿与地图草稿由顶部保存统一提交。")),
            (@field_text(EnvironmentTextField::GlobalFeedback)),
            (@celestial_section_header(CelestialNumericScope::Global)),
            (@global_celestial_body()),
        ]
    }
}
