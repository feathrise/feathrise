//! 使用Bevy Scene Notation声明地图编辑器静态外壳、顶栏、视口和底栏。

use feathrise_editor_ui::metrics as ui_metrics;
use crate::ui::theme as colors;
use bevy::prelude::*; // BSN Scene、UI组件和布局值。
use bevy::text::FontSourceTemplate; // BSN通过AssetServer路径取得项目中文字体。
use bevy::ui::RelativeCursorPosition; // 中央视口提供归一化光标坐标。 // 全屏编辑器复用项目水墨、铜边和米白文字色。

use crate::components::*; // BSN节点使用稳定业务marker而非名称匹配。
use crate::constants::EDITOR_SPLITTER_WIDTH;
use crate::map_browser::{map_browser_drag_preview, map_folder_dialog}; // 虚拟文件夹模态和地图拖拽浮层由独立功能域提供。
use crate::types::{EditorAction, EditorLayoutAction, EditorPane}; // 业务动作与纯布局操作分别分派。

use super::courtyard::selection_region; // 院落框选在同一离屏画面上绘制 UI 轮廓。
use super::delete_map_dialog::delete_map_dialog; // 可恢复删除使用独立模态确认层。
use super::new_map_dialog::new_map_dialog; // 新地图尺寸与名称使用独立模态层。
use super::sidebars::{left_sidebar, right_sidebar}; // 八个侧栏页面由独立模块一次创建。
use super::terrain_conflict_dialog::terrain_conflict_dialog; // 外部GLB与脏草稿冲突时阻止继续作者输入。
use super::vegetation_preset_editor::{
    PresetWorkspaceMapPage, vegetation_workspace_editor_page, vegetation_workspace_tab_bar,
}; // 中央永久地图页与多个植被预设编辑标签共享一块大工作区。
use super::viewport_toolbar::viewport_toolbar; // 视图和笔刷参数同排定高，原地图视口仍使用剩余正文空间。

pub(crate) use feathrise_editor_core::FONT_PATH; // 原生作者控件复用同一中文字体。

const EDITOR_BUTTON_WIDTH_SCALE: f32 = 0.9; // 兼容旧弹框与工具条的业务宽度基准。
const EDITOR_BUTTON_HEIGHT: f32 = 26.0; // 保持工具密度，但不再依赖过小文字腾空间。

/// 场景树、资产和作者层统一使用独立轮廓图标，图形不截获原行的点击或右键。
pub(crate) fn tree_icon(path: &'static str) -> impl Scene {
    bsn! { template_value(MapEditorTreeGlyph(path)) ImageNode::default()
        Node { width: px(16), height: px(16), flex_shrink: 0.0 } Pickable::IGNORE }
}

/// 构造保留显式业务宽度的统一铜边小按钮。
pub(crate) fn editor_button(label: &str, action: EditorAction, width: f32) -> impl Scene {
    bsn! {
        Button
        MapEditorActionButton(action)
        Node {
            width: px(width * EDITOR_BUTTON_WIDTH_SCALE), // 旧工具条与模态继续使用既有视觉基准。
            height: px(EDITOR_BUTTON_HEIGHT),
            border: px(1),
            border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(5)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label)
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                font_size: px(ui_metrics::FONT_CONTROL),
            }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

/// 构造显式等宽行中的弹性按钮，中文长度不再决定相邻列宽。
pub(crate) fn flex_editor_button(label: &str, action: EditorAction) -> impl Scene {
    bsn! {
        Button
        MapEditorActionButton(action)
        Node {
            height: px(EDITOR_BUTTON_HEIGHT),
            flex_grow: 1.0, // 同一行平均分配可用宽度。
            flex_basis: px(0), // 标签长度不参与等宽计算。
            min_width: px(0), // 三列和四列允许安全收缩。
            border: px(1),
            border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::horizontal(px(3)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text(label) feathrise_editor_ui::localization::label_scene(label)
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                font_size: px(ui_metrics::FONT_CONTROL),
            }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

/// 构造弹框或页面内部仍有信息价值的分组标题。
pub(crate) fn section_title(label: &str) -> impl Scene {
    bsn! {
        Text(label) feathrise_editor_ui::localization::label_scene(label)
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(ui_metrics::FONT_CONTROL),
        }
        TextColor(colors::TEXT_ACCENT)
        Node {
            margin: UiRect::bottom(px(3)),
        }
    }
}

/// 检查器二级分组使用弱分隔线，不再给每段属性叠加卡片边框。
pub(crate) fn inspector_section_title(label: &str) -> impl Scene {
    bsn! {
        Text(label) feathrise_editor_ui::localization::label_scene(label)
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(ui_metrics::FONT_CONTROL),
        }
        TextColor(colors::TEXT_ACCENT)
        TextLayout { linebreak: bevy::text::LineBreak::AnyCharacter }
        Node {
            width: percent(100),
            min_width: px(0),
            flex_shrink: 0.0,
            margin: UiRect::top(px(5)),
            padding: UiRect::bottom(px(3)),
            border: UiRect::bottom(px(1)),
        }
        BorderColor::all(colors::COPPER_BORDER_WEAK)
    }
}

/// 启动入口沿用原关闭F12事务，图标与文字分开着色，不改当前场景或草稿责任。
fn launch_button() -> impl Scene {
    bsn! {
        Button MapEditorActionButton(EditorAction::Close) feathrise_editor_core::ProjectLaunchButton
        Node {
            width: px(64), height: px(EDITOR_BUTTON_HEIGHT), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            column_gap: px(4), padding: UiRect::horizontal(px(5)),
            border: px(1), border_radius: BorderRadius::all(px(3)),
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            Text("▶") Pickable::IGNORE
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_GREEN)
        ), (
            Text("启动") feathrise_editor_ui::localization::label_scene("启动") Pickable::IGNORE feathrise_editor_core::ProjectLaunchLabel
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

/// 标题、保存/历史、筛选菜单与布局控制同用28px顶行；五种模式沿用视口左侧的唯一图标栏。
fn header_slot(item: crate::workbench::WorkbenchHeaderItem, content: impl Scene) -> impl Scene {
    bsn! { template_value(item) Node { flex_shrink: 0.0 } Children [(content)] }
}

pub(super) fn header() -> impl Scene {
    use crate::workbench::WorkbenchHeaderItem as Item;
    bsn! {
        super::tooltips::TooltipHeader
        crate::workbench::WorkbenchHeader
        Node {
            width: percent(100),
            height: px(28), // 地图上下文移到底栏后，26px命令按钮在一行内保持完整命中区。
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: px(2),
            padding: UiRect::horizontal(px(6)),
            border: UiRect::bottom(px(1)),
        }
        BackgroundColor(colors::PANEL_BG_STRONG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            (Text("编辑器") feathrise_editor_ui::localization::label_scene("编辑器") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                TextColor(colors::TEXT_PRIMARY) Node { flex_shrink: 0.0, margin: UiRect::right(px(4)) }),
            // 快捷键仍由原悬停提示说明；短标签为1280px/150%保留模式和右端操作的完整命中区。
            (header_slot(Item::Save, editor_button("保存", EditorAction::Save, 46.0))),
            (header_slot(Item::Undo, editor_button("撤销", EditorAction::Undo, 46.0))),
            (header_slot(Item::Redo, editor_button("重做", EditorAction::Redo, 46.0))),
            (header_slot(Item::Selection, crate::viewport::modes::global_mode_toolbar())),
            (template_value(Item::Spacer) Node { flex_grow: 1.0, min_width: px(0) }), // 窄窗优先收缩空隙，保留右端设置和启动。
            (header_slot(Item::Navigation, layout_button("导航", EditorLayoutAction::Content, 42.0))),
            (header_slot(Item::Inspector, layout_button("工作区", EditorLayoutAction::Inspector, 52.0))),
            (header_slot(Item::Focus, layout_button("专注编辑", EditorLayoutAction::Focus, 60.0))),
            (header_slot(Item::Settings, super::editor_settings::settings_entry())),
            (launch_button()),
        ]
    }
}

fn layout_button(label: &str, action: EditorLayoutAction, width: f32) -> impl Scene {
    bsn! {
        Button
        MapEditorLayoutButton(action)
        Node {
            width: px(width), height: px(26), flex_shrink: 0.0,
            border: UiRect::bottom(px(1)), border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
        }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [(
            MapEditorLayoutButtonText(action)
            Text(label) feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

fn pane_splitter(pane: EditorPane) -> impl Scene {
    bsn! {
        Button
        MapEditorPaneSplitter(pane)
        Node { width: px(EDITOR_SPLITTER_WIDTH), height: percent(100), flex_shrink: 0.0 }
        BackgroundColor(colors::PANEL_BG_STRONG)
    }
}

/// 中央浏览器式工作区；地图视口为永久标签，预设编辑页按需出现。
pub(super) fn viewport_panel(scenes: &feathrise_editor_documents_model::DocumentScenes) -> impl Scene + use<> {
    bsn! {
        Node {
            height: percent(100),
            flex_grow: 1.0,
            min_width: px(360), // 1280×720下仍保证中央视口最小可用宽度。
            min_height: px(0), // 正文可在工作台可用高度内收缩，不把公共标签挤出窗口。
            flex_direction: FlexDirection::Column,
        }
        BackgroundColor(colors::MODEL_PREVIEW_BG)
        Children [
            (vegetation_workspace_tab_bar()), // 地图、文件和预设共用顶部常驻28px行，位于视图工具栏上方。
            (
                MapEditorEmptyWorkspace
                Node {
                    display: Display::None,
                    width: percent(100),
                    flex_grow: 1.0,
                    min_height: px(0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                }
                Children [(
                    Text("尚未进入地图\n从地图目录进入地图，或从顶部工具打开预设库") feathrise_editor_ui::localization::label_scene("尚未进入地图\n从地图目录进入地图，或从顶部工具打开预设库")
                    TextFont {
                        font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                        font_size: px(14),
                    }
                    TextColor(colors::TEXT_MUTED)
                )]
            ),
            (
                PresetWorkspaceMapPage
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    min_height: px(0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(0),
                }
                Children [
                    (viewport_toolbar()),
                    (
                        MapEditorViewport
                        EditorViewportPointerCapture::default()
                        RelativeCursorPosition::default()
                        Interaction::default()
                        Node {
                            width: percent(100),
                            flex_grow: 1.0,
                            min_height: px(0), // 视口只占标签和视图工具栏之后的剩余空间。
                            overflow: Overflow::clip(),
                        }
                        BackgroundColor(colors::MODEL_PREVIEW_BG)
                        BorderColor::all(colors::COPPER_BORDER)
                        Children [(selection_region())] // 纯视觉选框不改变正式视口相机与 RTT。
                    ),
                ]
            ),
            (vegetation_workspace_editor_page()),
            (super::asset_preview::document_center_host(scenes)), // 放大文档与地图/预设正文互斥，共用已有原生输入子树。
            (super::asset_preview::asset_preview_center_host()), // 图片/模型放大复用原预览子树，无地图也可查看。
            (
                crate::workbench::WorkbenchCentralPageHost
                // 工程开始与表单覆盖原中央正文，标签与两侧栏始终使用原工作台。
                Node { display: Display::None, position_type: PositionType::Absolute,
                    left: px(0), right: px(0), top: px(28), bottom: px(0),
                    min_width: px(0), min_height: px(0), flex_direction: FlexDirection::Column }
                BackgroundColor(colors::MODEL_PREVIEW_BG)
            ),
        ]
    }
}

/// 主体组合可调侧栏、固定分隔条与当前文档区，不引入任意停靠树。
fn body(scenes: &feathrise_editor_documents_model::DocumentScenes) -> impl Scene + use<> {
    // 三个大面板只在启动时分段装箱，避免完整工作台的BSN模板在Windows小栈上形成巨型临时值。
    let panes: Vec<Box<dyn Scene>> = vec![
        Box::new(left_sidebar(scenes)),
        Box::new(pane_splitter(EditorPane::Content)),
        Box::new(viewport_panel(scenes)),
        Box::new(pane_splitter(EditorPane::Inspector)),
        Box::new(right_sidebar(scenes)),
    ];
    bsn! {
        Node {
            width: percent(100),
            flex_grow: 1.0,
            min_height: px(240),
            flex_direction: FlexDirection::Row,
        }
        Children [{panes}]
    }
}

/// 返回由BSN声明的完整编辑器Scene。
pub(crate) fn map_editor_scene(scenes: &feathrise_editor_documents_model::DocumentScenes) -> impl Scene + use<> {
    bsn! {
        MapEditorRoot
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
        }
        GlobalZIndex(420)
        BackgroundColor(colors::OVERLAY_BG)
        Children [
            (header()),
            (super::sidebars::tool_module_bar()),
            (body(scenes)),
            (super::status_bar::footer()),
            (new_map_dialog()),
            (delete_map_dialog()),
            (terrain_conflict_dialog()),
            (map_folder_dialog()),
            (map_browser_drag_preview()),
            (super::editor_settings::settings_panel()),
            (super::map_loading::map_loading_input_blocker()),
            (super::map_switch_dialog::map_switch_dialog()),
            (super::tooltips::tooltip_popup()),
            (super::status_bar::popup()),
            (super::context_help::popup()),
        ]
    }
}

/// Bevy 原 spawn() 也是 Startup 独占构造；只在这里读取稳定工厂，不改变生成时机和依赖加载。
pub(crate) fn spawn_map_editor_scene(world: &mut World) -> bevy::ecs::error::Result {
    let scenes = *world.resource::<feathrise_editor_documents_model::DocumentScenes>();
    world.spawn_scene(map_editor_scene(&scenes))?;
    Ok(())
}
