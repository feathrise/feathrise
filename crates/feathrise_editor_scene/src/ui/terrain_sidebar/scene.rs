//! 三个顶部工具页共享一个正文滚动宿主；原地表库、场景层与反馈实体始终常驻。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use bevy::ui::RelativeCursorPosition;

use super::{components::*, resources::TerrainSidebarPage};
use crate::components::{
    SceneEditorActionButton, SceneEditorCustomActionStyle, SceneEditorRightPanelScroll,
    SceneEditorSurfaceText,
};
use crate::types::{EditorAction, EditorTool};
use crate::ui::scene::FONT_PATH;
use crate::ui::context_help::section_header as help_header;
use crate::ui::status_bar::PanelStatusText;
use crate::ui::surface_layers::surface_authoring_panel;
use crate::ui::surface_preset_editor::{surface_preset_feedback, surface_preset_panel};
use crate::ui::theme as colors;

fn tab(page: TerrainSidebarPage) -> impl Scene {
    let label = page.label();
    bsn! {
        InteractionButton
        TerrainSidebarTab(page)
        Node {
            height: px(24),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            border: UiRect::bottom(px(1)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

fn tool(label: &str, action: EditorAction, tool: EditorTool) -> impl Scene {
    bsn! {
        InteractionButton
        SceneEditorActionButton(action)
        SceneEditorCustomActionStyle
        TerrainSidebarTool(tool)
        Node {
            height: px(24),
            min_width: px(0),
            flex_basis: px(0),
            flex_grow: 1.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

pub(crate) fn terrain_tools_panel() -> impl Scene {
    bsn! {
        TerrainSidebarRoot
        Node {
            width: percent(100),
            min_width: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
        }
        Children [

                // 塑形工具只操作明确所选网格；对象列表与变换属性留在场景/网格页。
                TerrainSidebarMeshContext
                Text("当前网格：尚未选择 · 从左侧场景树选择") @feathrise_editor_ui::localization::label_scene("当前网格：尚未选择 · 从左侧场景树选择")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED)
                TextLayout::linebreak(LineBreak::AnyCharacter)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            --

                // 导航不随正文滚动；低窗口仍能直接切换三个作者域。
                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                Children [
                    @tab(TerrainSidebarPage::Sculpt)--
                    @tab(TerrainSidebarPage::Surface)--
                    @tab(TerrainSidebarPage::Puddles)
                ]
            --

                // 模式状态紧邻常驻导航，避免藏在三维工具之后而把预备高亮误当成允许落笔。
                TerrainSidebarModeText
                Text("当前：选择 · 预备 抬高") @feathrise_editor_ui::localization::label_scene("当前：选择 · 预备 抬高")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_VALUE)
                TextLayout::no_wrap()
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0, overflow: Overflow::clip_x() }
            --

                TerrainSidebarScroll
                SceneEditorRightPanelScroll
                RelativeCursorPosition::default()
                ScrollPosition::default()
                Node {
                    width: percent(100),
                    min_width: px(0),
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(5),
                    overflow: Overflow::scroll_y(),
                }
                Children [

                        TerrainSidebarPanel(TerrainSidebarPage::Sculpt)
                        Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(5), flex_shrink: 0.0 }
                        Children [
                            @help_header("雕刻工具", "先选中高度网格和雕刻工具，再点击视口左侧“编辑”图标后左键连续雕刻；选择模式只预备工具。平滑降低局部高差，平坦区域不改变高度；压平以起笔高度为目标。四种工具共用雕刻速度，平滑和压平到达目标后停止，不越过目标。原始GLB保留资产几何。半径、强度和雕刻速度在视口工具条调整。数值左拖实时修改，中键精确输入，右拖只调整本字段灵敏度。")--

                                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                                Children [
                                    @tool("抬高", EditorAction::ToolRaise, EditorTool::Raise)--
                                    @tool("降低", EditorAction::ToolLower, EditorTool::Lower)
                                ]
                            --

                                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                                Children [
                                    @tool("平滑", EditorAction::ToolSmooth, EditorTool::Smooth)--
                                    @tool("压平", EditorAction::ToolFlatten, EditorTool::Flatten)
                                ]
                            --
                            @help_header("三维雕刻（原型）", "Clipmap与体素支持三维塑形。球形造洞穴；方盒做直墙、平底通道；圆柱开井。整平按首个落点高度削高补低。")--

                                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                                Children [
                                    @tool("三维挖掘", EditorAction::ToolVolumeDig, EditorTool::VolumeDig)--
                                    @tool("三维填充", EditorAction::ToolVolumeFill, EditorTool::VolumeFill)
                                ]
                            --

                                Node { width: percent(100), column_gap: px(4), flex_shrink: 0.0 }
                                Children [
                                    @tool("三维平滑", EditorAction::ToolVolumeSmooth, EditorTool::VolumeSmooth)--
                                    @tool("三维整平", EditorAction::ToolVolumeFlatten, EditorTool::VolumeFlatten)
                                ]
                            --
                            @super::volume_brush::panel()
                        ]
                    --

                        TerrainSidebarPanel(TerrainSidebarPage::Surface)
                        Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(5), flex_shrink: 0.0 }
                        Children [
                            @surface_preset_panel()--

                                ~{crate::ui::scene_inspector::SelectedToolProperties::new(crate::ui::scene_inspector::SelectedToolPropertiesKind::Surface)}
                                Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(5), border: UiRect::top(px(1)), padding: UiRect::top(px(8)), flex_shrink: 0.0 }
                                BorderColor::all(colors::COPPER_BORDER_WEAK)
                                Children [
                                    @help_header("当前场景", "选择预设后新建图层，切到编辑模式绘制；Shift擦除。删除场景图层不会删除全局预设。")--
                                    @surface_authoring_panel()--

                                        SceneEditorSurfaceText
                                        Text("等待场景作者数据") @feathrise_editor_ui::localization::label_scene("等待场景作者数据")
                                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                                        TextColor(colors::TEXT_VALUE)
                                        TextLayout { linebreak: LineBreak::AnyCharacter }
                                        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }

                                ]

                        ]
                    --

                        TerrainSidebarPanel(TerrainSidebarPage::Puddles)
                        Node { display: Display::None, width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(5), flex_shrink: 0.0 }
                        Children [
                            @help_header("独立积水潜力", "绘制地面的保水倾向，不是设置静水水位，不占地表或植被图层。左键绘制，Shift擦除；屋内、桥下保持零值。选择工具不创建画布，首次有效笔划才创建。切换工具可查看随天气变化的实际水洼。")--

                                TerrainSidebarPuddleTargetText
                                Text("等待场景作者数据") @feathrise_editor_ui::localization::label_scene("等待场景作者数据")
                                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                                TextColor(colors::TEXT_VALUE)
                                TextLayout { linebreak: LineBreak::AnyCharacter }
                                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }

                        ]
                    --

                        Node { width: percent(100), min_width: px(0), flex_direction: FlexDirection::Column, row_gap: px(5), border: UiRect::top(px(1)), padding: UiRect::top(px(8)), flex_shrink: 0.0 }
                        BorderColor::all(colors::COPPER_BORDER_WEAK)
                        Children [

                                TerrainSidebarAvailabilityText
                                ~{PanelStatusText("塑形工具")}
                                Text("等待当前场景作者数据") @feathrise_editor_ui::localization::label_scene("等待当前场景作者数据")
                                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                                TextColor(colors::TEXT_VALUE)
                                TextLayout { linebreak: LineBreak::AnyCharacter }
                                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                            --

                                TerrainSidebarPanel(TerrainSidebarPage::Surface)
                                Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                                Children [@surface_preset_feedback()]
                            --

                                TerrainSidebarMessageText
                                ~{PanelStatusText("塑形最近操作")}
                                Text("")
                                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                                TextColor(colors::TEXT_MUTED)
                                TextLayout { linebreak: LineBreak::AnyCharacter }
                                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }

                        ]

                ]

        ]
    }
}
