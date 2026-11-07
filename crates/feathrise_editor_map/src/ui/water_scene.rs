//! 水域横向工具页固定在滚动正文上方，统一对象列表与原作者属性始终常驻。

use feathrise_editor_ui::metrics as ui_metrics;
use crate::ui::theme as colors;
use bevy::prelude::*; // BSN Scene与显式等宽布局。
use bevy::text::FontSourceTemplate; // 水域摘要与状态使用项目中文字体。
use bevy::ui::RelativeCursorPosition; // 水域页保持水墨铜木配色。

use crate::authoring::{WaterNetworkNumericInputSlot, WaterNetworkNumericKind}; // 河流与瀑布四项专业数值框挂载槽。
use crate::components::{
    MapEditorRightPanelScroll, MapEditorWaterFeatureList, MapEditorWaterFeatureListTitle,
    MapEditorWaterLevelInputSlot, MapEditorWaterNetworkText,
}; // 统一对象行池、摘要、静水位和动态状态marker。
use crate::types::EditorAction; // 全部作者操作使用既有结构化动作。

use super::scene::{FONT_PATH, flex_editor_button}; // 侧栏按钮严格等宽，不再依赖混合宽度换行。
use super::sidebars::{
    MapEditorStillWaterProperties, MapEditorWaterListActions,
    MapEditorWaterNetworkProperties,
};
use super::water_preset_editor::{water_preset_feedback, water_preset_panel};
use super::water_sidebar::*;

fn tab(page: WaterSidebarPage) -> impl Scene {
    let label = page.label();
    bsn! {
        Button
        WaterSidebarTab(page)
        Node {
            height: px(24), flex_grow: 1.0, flex_basis: px(0), min_width: px(0),
            border: UiRect::bottom(px(1)), align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

fn heading(label: &str) -> impl Scene {
    bsn! {
        Text(label) feathrise_editor_ui::localization::label_scene(label)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_ACCENT)
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}

fn feedback_text(kind: WaterSidebarFeedbackKind, label: &str) -> impl Scene {
    let node = bsn! {
        WaterSidebarFeedbackText(kind)
        Text(label) feathrise_editor_ui::localization::label_scene(label)
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
        TextColor(colors::TEXT_MUTED)
        TextLayout { linebreak: LineBreak::AnyCharacter }
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    };
    let source = match kind {
        WaterSidebarFeedbackKind::Availability => Some("水域工具"),
        WaterSidebarFeedbackKind::Message => Some("水域最近操作"),
        _ => None,
    };
    (node, source.map(|source| bsn! { template_value(crate::ui::status_bar::PanelStatusText(source)) }))
}

fn create(page: WaterSidebarPage, label: &str, action: EditorAction) -> impl Scene {
    bsn! {
        WaterSidebarPanel(page)
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
        Children [(flex_editor_button(label, action))]
    }
}

/// 构造严格等宽双列按钮行。
fn two_button_row(
    first_label: &str,           // 左列动作标签。
    first_action: EditorAction,  // 左列作者动作。
    second_label: &str,          // 右列动作标签。
    second_action: EditorAction, // 右列作者动作。
) -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            column_gap: px(3),
            flex_shrink: 0.0,
        }
        Children [
            (flex_editor_button(first_label, first_action)),
            (flex_editor_button(second_label, second_action)),
        ]
    }
}

/// 单个水网控制点专业数值框槽。
fn water_network_numeric_slot(kind: WaterNetworkNumericKind) -> impl Scene {
    bsn! {
        WaterNetworkNumericInputSlot(kind)
        Node {
            flex_basis: px(122), // 两个字段有足够空间才同排；窄栏整枚换行，不压缩值和单位。
            flex_grow: 1.0,
            min_width: px(0),
            height: px(24),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
    }
}

/// 构造两列水网连续属性行。
fn water_network_numeric_pair(
    first_kind: WaterNetworkNumericKind,  // 左列参数身份。
    second_kind: WaterNetworkNumericKind, // 右列参数身份。
) -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            min_width: px(0),
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            column_gap: px(3),
            row_gap: px(3),
            flex_shrink: 0.0,
        }
        Children [
            (water_network_numeric_slot(first_kind)),
            (water_network_numeric_slot(second_kind)),
        ]
    }
}

/// 正文所有作者节点常驻；工具页只改变适用入口，不过滤或复制统一对象列表。
fn water_authoring_body() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            min_width: px(0),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
        }
        Children [
            (
                // 静水外观目录不适用于样条水；管理入口仍打开原中央文档，不改目录或引用规则。
                WaterSidebarPanel(WaterSidebarPage::Still)
                Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, flex_shrink: 0.0 }
                Children [
                    (crate::ui::context_help::help_button("静水预设库", "选择预设后可应用到当前静水体；新建或编辑在中央打开完整波浪标签。全局预设编辑与地图水域对象是不同的编辑对象。")),
                    (water_preset_panel()),
                ]
            ),
            (crate::ui::context_help::section_header("当前地图水域", "选择模式只读；工具页只准备类别，进入视口编辑模式后绘制。静水用笔刷绘制R8覆盖，Shift擦除；水位不自动修改河床。\n河流/瀑布：顶视或透视左键空白逐点，拖点只改XZ并保留Y；连接端点高度由系统决定。瀑布正式保存需要不同且有效的源/目标，终点低于起点。\n专业数值框支持左拖实际值、右拖灵敏度、中键精确输入。")),
            (
                MapEditorWaterFeatureListTitle
                Text("水域对象 · 等待地图作者数据") feathrise_editor_ui::localization::label_scene("水域对象 · 等待地图作者数据")
                TextFont {
                    font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                    font_size: px(ui_metrics::FONT_CONTROL),
                }
                TextColor(colors::TEXT_VALUE)
                TextLayout { linebreak: LineBreak::AnyCharacter }
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            ),
            (
                MapEditorWaterFeatureList
                Node {
                    display: Display::None, // 零对象时六行池完全退出布局。
                    width: percent(100),
                    height: px(0), // 一项起按当前页真实行数增长。
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    padding: UiRect::all(px(0)),
                    flex_shrink: 0.0,
                }
            ),
            (
                MapEditorWaterListActions
                Node {
                    display: Display::None, // 有对象后才保留分页与统一删除操作。
                    width: percent(100),
                    column_gap: px(3),
                    flex_shrink: 0.0,
                }
                Children [
                    (flex_editor_button("前页首项", EditorAction::WaterListPreviousPage)),
                    (flex_editor_button("后页首项", EditorAction::WaterListNextPage)),
                    (flex_editor_button("删除所选", EditorAction::WaterDeleteSelected)),
                ]
            ),
            (create(WaterSidebarPage::Still, "新建静水体", EditorAction::WaterAddLayer)),
            (create(WaterSidebarPage::River, "新建河流", EditorAction::RiverAdd)),
            (create(WaterSidebarPage::Waterfall, "新建瀑布", EditorAction::WaterfallAdd)),
            (feedback_text(WaterSidebarFeedbackKind::Target, "当前工具目标：尚未选择或创建")),
            (
                Node {
                    width: percent(100), min_width: px(0), flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column, row_gap: px(4),
                    padding: UiRect::top(px(8)), border: UiRect::top(px(1)),
                }
                BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [
                    (feedback_text(WaterSidebarFeedbackKind::Mode, "选择 · 预备静水")),
                    (feedback_text(WaterSidebarFeedbackKind::Availability, "等待当前地图作者数据")),
                    (
                        WaterSidebarPanel(WaterSidebarPage::Still)
                        Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, flex_shrink: 0.0 }
                        Children [(water_preset_feedback())]
                    ),
                    (feedback_text(WaterSidebarFeedbackKind::Message, "")),
                ]
            ),
        ]
    }
}

/// 顶部导航不进入滚动宿主；低窗口仍能直接选择三种准备工具。
pub(super) fn water_authoring_panel() -> impl Scene {
    bsn! {
        WaterSidebarRoot
        Node {
            width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0,
            flex_direction: FlexDirection::Column, row_gap: px(4),
        }
        Children [
            (
                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                Children [(tab(WaterSidebarPage::Still)), (tab(WaterSidebarPage::River)), (tab(WaterSidebarPage::Waterfall))]
            ),
            (
                WaterSidebarScroll
                MapEditorRightPanelScroll
                RelativeCursorPosition::default()
                ScrollPosition::default()
                Node {
                    width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0,
                    flex_direction: FlexDirection::Column, overflow: Overflow::scroll_y(),
                }
                Children [(water_authoring_body())]
            ),
        ]
    }
}

/// 所选属性只挂载在右侧；复用原控件与作者事务。
pub(crate) fn water_properties_panel() -> impl Scene {
    bsn! { template_value(crate::ui::scene_inspector::InspectorSection::Water) Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(4), flex_shrink: 0.0 }
        Children [
            (
                MapEditorStillWaterProperties
                Node {
                    display: Display::None, // 只有当前高亮静水体时才显示该上下文。
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(3),
                    flex_shrink: 0.0,
                }
                Children [
                    (heading("所选静水属性")),
                    (flex_editor_button("复制静水体", EditorAction::WaterDuplicateLayer)),
                    (
                        Node {
                            width: percent(100),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: px(3),
                            flex_shrink: 0.0,
                        }
                        Children [
                            (
                                MapEditorWaterLevelInputSlot
                                Node {
                                    width: percent(100), // 水位独占整行，取地形水位不再挤占数字空间。
                                    min_width: px(0),
                                    height: px(24),
                                    flex_shrink: 0.0,
                                }
                            ),
                            (flex_editor_button("取地形水位", EditorAction::WaterLevelFromCursor)),
                        ]
                    ),
                ]
            ),
            (
                MapEditorWaterNetworkProperties
                Node {
                    display: Display::None, // 只有河流或瀑布选择时显示控制点与连接属性。
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(3),
                    flex_shrink: 0.0,
                }
                Children [
                    (heading("所选控制点与连接")),
                    (two_button_row("上一控制点", EditorAction::WaterPointPrevious, "删除控制点", EditorAction::WaterPointDelete)),
                    (water_network_numeric_pair(WaterNetworkNumericKind::Height, WaterNetworkNumericKind::Width)),
                    (water_network_numeric_pair(WaterNetworkNumericKind::Depth, WaterNetworkNumericKind::FlowSpeed)),
                    (flex_editor_button("普通/急流", EditorAction::WaterSegmentKindToggle)),
                    (two_button_row("循环源连接", EditorAction::WaterSourceNext, "循环目标连接", EditorAction::WaterTargetNext)),
                    (
                        MapEditorWaterNetworkText
                        Text("水域：等待地图作者数据") feathrise_editor_ui::localization::label_scene("水域：等待地图作者数据")
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_VALUE)
                        TextLayout { linebreak: LineBreak::AnyCharacter }
                        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                    ),
                ]
            ),
        ]
    }
}
