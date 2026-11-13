//! F12左侧目录/场景导航，以及顶部模块栏对应的唯一工具页子树。

use feathrise_editor_ui::metrics as ui_metrics;
use crate::ui::theme as colors;
use bevy::prelude::*; // BSN Scene、UI布局、滚动位置与显示切换。
use bevy::text::FontSourceTemplate;
use bevy::ui::RelativeCursorPosition; // 每页独立滚动只响应鼠标所在页面。 // 左栏保持水墨铜木配色。

use crate::components::*; // 页面摘要、固定行池、滚动与状态marker。
use crate::constants::EDITOR_CONTENT_WIDTH; // 初始侧栏宽度由原布局会话控制。
use crate::types::EditorPane; // 布局标记不改变既有结构化作者动作。
use crate::ui::scene::FONT_PATH;

use super::super::building_sidebar::building_tools_panel; // 单栋建筑与院落共用顶部工具页和反馈区。
use super::super::gameplay_sidebar::gameplay_tools_panel; // 三类玩法工具独占正文，导航固定不随列表滚走。
use super::super::scene_sidebar::scene_page; // 场景页复用原虚拟树与显式作者动作。
use super::super::terrain_sidebar::terrain_tools_panel; // 顶部雕刻/地表/积水工具页保留唯一正文滚动宿主。
use super::super::terrain_objects; // 网格创建在工具页，列表在左侧导航，所选属性在右侧检查器。
use super::super::vegetation_scene::vegetation_authoring_panel; // 植被页复用独立职责面板。
use super::super::water_scene::water_authoring_panel; // 水域页复用独立职责面板。
use super::{
    // 左栏页签、层级标题与一次创建页面marker。
    EditorLeftSidebarTab,
    EditorNavigatorTab,
    EditorRightSidebarTab,
    EditorSidebarTab,
    SceneEditorSidebarPage,
    sidebar_context_title,
    sidebar_tab_button,
};


/// 网格工具只准备新增规格；左侧树选择对象，右侧属性页编辑正式对象。
fn meshes_page() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Left(EditorLeftSidebarTab::Meshes))
        SceneEditorRightPanelScroll
        RelativeCursorPosition::default()
        ScrollPosition::default()
        Node {
            display: Display::None, // 一次挂载保留表单与滚动，切页只退出布局。
            width: percent(100),
            min_width: px(0),
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::top(px(3)),
            overflow: Overflow::scroll_y(),
        }
        Children [
            @sidebar_context_title("当前模块 · 网格")--
            @terrain_objects::panel() // 新增网格的唯一规格表单。
        ]
    }
}

/// 地形页导航常驻，只有当前工具正文参与原有侧栏滚动。
fn terrain_page() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Left(EditorLeftSidebarTab::Terrain))
        Node {
            display: Display::None, // 非活动页保留工具实体与正文滚动状态，但退出布局。
            width: percent(100),
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::top(px(3)),
        }
        Children [
            @sidebar_context_title("当前模块 · 塑形工具")-- // 网格创建在网格页，属性在右侧；此页只负责有能力的作者工具。
            @terrain_tools_panel()
        ]
    }
}

/// 植被页只承载植被作者职责，内部上下文控件按选择动态显隐。
fn vegetation_page() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Left(EditorLeftSidebarTab::Vegetation))
        SceneEditorRightPanelScroll
        RelativeCursorPosition::default()
        ScrollPosition::default()
        Node {
            display: Display::None, // 非活动页保留实体和ScrollPosition但退出布局。
            width: percent(100),
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::top(px(3)),
            overflow: Overflow::scroll_y(),
        }
        Children [
            @sidebar_context_title("当前模块 · 植被工具")-- // 植被作者控件属于当前页的二级工具。
            @vegetation_authoring_panel() // 继续复用现有植被作者面板与滚动状态。
        ]
    }
}

/// 水域页固定导航，正文继续使用唯一独立滚动宿主。
fn water_page() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Left(EditorLeftSidebarTab::Water))
        Node {
            display: Display::None, // 非活动页保留实体与正文滚动宿主，但退出布局。
            width: percent(100),
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::top(px(3)),
        }
        Children [
            @sidebar_context_title("当前模块 · 水域工具")-- // 水域对象与属性明确归入当前模块。
            @water_authoring_panel() // 继续复用现有统一水对象作者面板。
        ]
    }
}

/// 玩法页的固定导航外壳；唯一滚动区由工具面板提供。
fn gameplay_page() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Left(EditorLeftSidebarTab::Gameplay))
        Node {
            display: Display::None, // 非活动页保留控件实体，正文滚动状态由内部唯一宿主管理。
            width: percent(100),
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            padding: UiRect::top(px(3)),
        }
        Children [
            @sidebar_context_title("当前模块 · 交互工具")-- // 摆放、传送和边界均是交互页内工具。
            @crate::ui::subscenes::creation_button()--
            @gameplay_tools_panel()
        ]
    }
}

/// 建筑页：全局预设目录与当前专用摆放参数共享一个可滚动作者上下文。
fn building_page() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Left(EditorLeftSidebarTab::Building))
        SceneEditorRightPanelScroll
        RelativeCursorPosition::default()
        ScrollPosition::default()
        Node {
            display: Display::None, // 非活动页保留列表实体与滚动位置。
            width: percent(100), // 页面完整使用固定左栏正文宽度。
            flex_grow: 1.0, // 列表可使用导航之外的剩余高度。
            min_height: px(0), // 低窗口允许外层滚动收缩。
            flex_direction: FlexDirection::Column, // 管理卡和说明纵向排列。
            row_gap: px(3), // 与其余作者页保持统一紧凑间距。
            padding: UiRect::top(px(3)), // 正文与一级页签保留分隔。
            overflow: Overflow::scroll_y(), // 任意数量预设连续显示且不分页。
        }
        Children [
            @sidebar_context_title("当前模块 · 建筑预设与摆放")-- // 明确本页不属于普通资产点放。
            @building_tools_panel() // 工具页保留原目录、名称和参数实体，只收起非当前分区。
        ]
    }
}

/// 场景树占满左侧导航；与右侧属性各自滚动，不再竞争同一栏的高度。
pub(crate) fn scene_tree_page() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree))
        Node { display: Display::None, width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_direction: FlexDirection::Column, overflow: Overflow::clip() }
        Children [
            SceneEditorSceneContext crate::ui::asset_preview::gltf_tree::EditorSceneTreePanel
            Node { width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_direction: FlexDirection::Column }
            Children [
                crate::ui::scene_inspector::InspectorTree
                SceneEditorLeftPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
                Node { width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_basis: px(0), flex_direction: FlexDirection::Column, row_gap: px(3), overflow: Overflow::scroll_y() }
                Children [
                    @crate::ui::gltf_browser::scene_display_panel()--
                    @crate::ui::terrain_objects::scene_list_panel()--
                    @crate::ui::gltf_browser::gltf_browser_panel()--
                    crate::viewport::content_selection::LayerCandidateSlot
                        Node { display: Display::None, width: percent(100), flex_shrink: 0.0, flex_direction: FlexDirection::Column }
                ]
            ]
        --
            SceneEditorSceneContext SceneEditorSceneContextHint
            Node { display: Display::None, width: percent(100), min_width: px(0), padding: UiRect::all(px(4)) }
            Children [Text("当前正在编辑预设，切回场景标签查看场景树。") @feathrise_editor_ui::localization::label_scene("当前正在编辑预设，切回场景标签查看场景树。")
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) TextLayout { linebreak: LineBreak::AnyCharacter }]
        --
            @crate::ui::asset_preview::gltf_tree::panel()
        ]
    }
}

/// 全编辑器共用一行创建与工具入口；页内工具和表单移入右侧固定工作区。
pub(crate) fn tool_module_bar() -> impl Scene {
    bsn! {
        Node { width: percent(100), height: px(32), flex_shrink: 0.0, align_items: AlignItems::Center, column_gap: px(6), padding: UiRect::horizontal(px(8)), border: UiRect::bottom(px(1)) }
        BackgroundColor(colors::PANEL_BG_STRONG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text("工具") @feathrise_editor_ui::localization::label_scene("工具") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) } TextColor(colors::TEXT_MUTED)--
            Node { width: px(7.0 * super::TOOL_MODULE_TAB_WIDTH + 6.0 * super::TOOL_MODULE_TAB_GAP), min_width: px(0), max_width: percent(85), height: px(26), column_gap: px(super::TOOL_MODULE_TAB_GAP) }
                Children [
                    @sidebar_tab_button("网格", EditorSidebarTab::Left(EditorLeftSidebarTab::Meshes))--
                    @sidebar_tab_button("塑形", EditorSidebarTab::Left(EditorLeftSidebarTab::Terrain))--
                    @sidebar_tab_button("植被", EditorSidebarTab::Left(EditorLeftSidebarTab::Vegetation))--
                    @sidebar_tab_button("水域", EditorSidebarTab::Left(EditorLeftSidebarTab::Water))--
                    @sidebar_tab_button("建筑", EditorSidebarTab::Left(EditorLeftSidebarTab::Building))--
                    @sidebar_tab_button("交互", EditorSidebarTab::Left(EditorLeftSidebarTab::Gameplay))--
                    // 环境入口归入顶部工具条，沿用原页面身份与唯一参数树，保留缓冲和滚动位置。
                    @sidebar_tab_button("环境", EditorSidebarTab::Left(EditorLeftSidebarTab::Environment))
                ]--
            // 插件共用这一行剩余空间；入口较多时横滚，不能撑出编辑器边界。
            crate::game_authoring::DomainModuleHost
                crate::game_authoring::scene_interaction::DomainHorizontalScroll
                RelativeCursorPosition::default() ScrollPosition::default()
                Node { min_width:px(0),height:px(26),flex_grow:1.0,flex_basis:px(0),column_gap:px(super::TOOL_MODULE_TAB_GAP),overflow:Overflow::scroll_x() }
        ]
    }
}

/// 右侧工具容器只挂载原表单一次；切到属性/资产后保留输入实体和各页滚动。
pub(super) fn tools_page() -> impl Scene {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::Tools))
        Node { display: Display::None, width: percent(100), min_width: px(0), min_height: px(0), flex_grow: 1.0, flex_direction: FlexDirection::Column, overflow: Overflow::clip() }
        Children [
            @meshes_page()-- @terrain_page()-- @vegetation_page()-- @water_page()--
            crate::game_authoring::DomainToolsHost Node { width:percent(100),min_width:px(0),min_height:px(0),flex_direction:FlexDirection::Column }-- @building_page()-- @gameplay_page()
        ]
    }
}

/// 左栏并列场景目录、场景树和资产；各自保留滚动，右侧属性与导航页互不抢位。
pub(crate) fn left_sidebar(scenes: &feathrise_editor_documents_model::DocumentScenes) -> impl Scene + use<> {
    bsn! {
        SceneEditorPane(EditorPane::Content)
        Node {
            width: px(EDITOR_CONTENT_WIDTH), // 初始宽度随后由分隔条与窗口适配原位更新。
            height: percent(100),
            flex_shrink: 0.0, // 中央视口在窄窗口承担剩余空间，不侵占侧栏。
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(6)),
            border: UiRect::right(px(1)),
        }
        BackgroundColor(colors::PANEL_BG)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [

                Node {
                    width: percent(100), // 一级导航分区占满左栏正文宽度。
                    flex_shrink: 0.0, // 深墨分类区固定在滚动页面之外。
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    padding: UiRect::all(px(2)),
                    margin: UiRect::bottom(px(3)), // 导航和当前页面正文之间留出分隔。
                    border: UiRect::bottom(px(1)), // 一级导航以细分隔线组织，不再嵌套铜框。
                }
                BackgroundColor(colors::PANEL_BG_STRONG) // 一级分类与普通页面工具使用不同墨色层级。
                BorderColor::all(colors::COPPER_BORDER_WEAK)
                Children [

                        Node {
                            width: percent(100),
                            height: px(24),
                            flex_shrink: 0.0, // 页签行不被正文滚动内容压缩。
                            column_gap: px(2), // 两个导航页之间保留细缝。
                        }
                        Children [
                            @sidebar_tab_button("场景", EditorSidebarTab::Navigator(EditorNavigatorTab::Scenes))--
                            @sidebar_tab_button("场景树", EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree))--
                            @sidebar_tab_button("文件", EditorSidebarTab::Navigator(EditorNavigatorTab::Assets))
                        ]

                ]
            --
            @scene_page()--
            @scene_tree_page()--
            @crate::ui::asset_preview::assets_page(scenes)
        ]
    }
}
