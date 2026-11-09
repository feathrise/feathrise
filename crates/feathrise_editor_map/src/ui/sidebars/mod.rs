//! F12地图编辑器左右侧栏的页签化静态布局、会话状态与增量交互系统。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use feathrise_editor_ui::localization::LocalizedText;
use crate::ui::theme as colors;
use bevy::prelude::*; // 页签与折叠说明复用统一BSN按钮外壳。
use bevy::text::FontSourceTemplate; // 侧栏静态中文标签使用项目正式字体。 // 页签沿用水墨、铜边和米白文字配色。

use super::scene::FONT_PATH; // 与其余编辑器控件共享同一字体资产路径。

mod asset_reveal; // 定位成功后一次性把高亮文件滚入当前资产页可见区域。
mod left; // 左侧双导航、顶部模块条与复用的工具表单。
mod right; // 全高工具、属性、资产与环境页，以及原唯一预设预览。
mod state; // 页签、折叠帮助和上下文容器的轻量marker与会话状态。
mod systems; // 页签输入、工具跟随、显隐和水墨铜木交互样式。

pub(crate) use asset_reveal::sync_asset_reveal_scroll;
pub(crate) use left::left_sidebar; // 主三栏Scene只依赖稳定左栏构造入口。
pub(crate) use left::tool_module_bar; // 顶部模块栏与左右导航共用同一套会话页签。
#[cfg(test)]
pub(crate) use left::scene_tree_page; // 回归通过真实导航树验证选择和独立滚动。
pub(crate) use right::right_sidebar; // 主三栏Scene只依赖稳定右栏构造入口。
#[cfg(test)]
pub(crate) use right::scene_page; // 检查器回归使用真实树、属性和唯一预览宿主。
pub use state::*; // 插件初始化资源，邻接面板挂载上下文与帮助marker。
pub(crate) use systems::*; // Input与Presentation阶段注册侧栏增量系统。 // 使用最终布局处理窗口与全屏不同滚动范围。

/// 布局夹具复用真实工作台；文档正文由根应用注入，这里只替换未参与测试的六个文档工厂。
#[cfg(test)]
pub(crate) fn test_document_scenes() -> feathrise_editor_documents_model::DocumentScenes {
    fn empty() -> Box<dyn Scene> { Box::new(bsn! { Node::default() }) }
    feathrise_editor_documents_model::DocumentScenes {
        panel: empty, center_host: empty, sidebar_launch: empty,
        open_selected_button: empty, search_panel: empty, replace_tab: empty,
    }
}

#[cfg(test)]
pub(crate) fn test_right_sidebar() -> impl Scene {
    right_sidebar(&test_document_scenes())
}

// 顶部内置与插件模块共用占宽和间距；侧栏内的等分页签仍由各自容器分配宽度。
pub const TOOL_MODULE_TAB_WIDTH: f32 = 58.0;
pub const TOOL_MODULE_TAB_GAP: f32 = 2.0;

/// 原生页签与插件入口只共用外观，不混用点击动作，避免双重分派覆盖插件工作台。
pub fn sidebar_tab_node() -> Node {
    Node {
        height: px(24), // 传统编辑器导航统一24px，节省高度并保留完整鼠标命中区。
        flex_grow: 1.0, // 同一栏的全部页签严格等宽分配。
        flex_basis: px(0), // 中文标签长度不参与列宽计算。
        min_width: px(0), // 1280宽窗口仍允许三列页签完整留在固定侧栏内。
        border: UiRect::ZERO, // 选中仅用蓝灰填色表达；按参考图去掉页签下方的白色横杠。
        border_radius: BorderRadius::all(px(3)), // 延续项目紧凑小按钮圆角。
        align_items: AlignItems::Center, // 页签文字垂直居中。
        justify_content: JustifyContent::Center, // 页签文字水平居中。
        padding: UiRect::horizontal(px(2)), // 窄标签只保留必要内边距。
        ..default()
    }
}

pub fn sidebar_tab_background(interaction: UiInteraction, selected: bool) -> Color {
    match (interaction, selected) {
        (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
        (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
        (UiInteraction::None, true) => colors::SELECT_BG,
        (UiInteraction::None, false) => colors::PANEL_BG_STRONG,
    }
}

pub fn sidebar_tab_text_color(selected: bool) -> Color {
    if selected { colors::TEXT_TAB_ACTIVE } else { colors::TEXT_TAB_INACTIVE }
}

/// 构造占满可用宽度的等分页签按钮；活动样式由Presentation统一回显。
pub fn sidebar_tab_button(label: &str, tab: EditorSidebarTab) -> impl Scene {
    // 顶部模块与窄侧栏各保留原按钮占宽，使用上下文短名，避免英文跨进相邻的命中区域。
    let copy = match tab {
        EditorSidebarTab::Navigator(EditorNavigatorTab::Maps) => LocalizedText::new("sidebar.maps"),
        EditorSidebarTab::Navigator(EditorNavigatorTab::Scene) => LocalizedText::new("sidebar.scene"),
        EditorSidebarTab::Left(EditorLeftSidebarTab::Meshes) => LocalizedText::new("sidebar.mesh"),
        EditorSidebarTab::Left(EditorLeftSidebarTab::Gameplay) => LocalizedText::new("sidebar.interact"),
        EditorSidebarTab::Left(EditorLeftSidebarTab::Environment) => LocalizedText::new("sidebar.world"),
        _ => LocalizedText::label(label),
    };
    bsn! {
        InteractionButton
        MapEditorSidebarTabButton(tab)
        ~{sidebar_tab_node()}
        BackgroundColor(colors::PANEL_BG_STRONG) // 一级分类默认使用比页内按钮更深的墨色。
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            MapEditorSidebarTabText
            Text(label) ~{copy}
            TextLayout::no_wrap()
            Node { max_width: percent(100), min_width: px(0), overflow: Overflow::clip_x() }
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 中文页签不回退系统字体。
                font_size: px(ui_metrics::FONT_CONTROL), // 与参考图的可读字号和紧凑行高统一。
            }
            TextColor(colors::TEXT_TAB_INACTIVE) // 非活动一级页签默认退为次要文字色。
        ]
    }
}

/// 构造左栏页面顶部的当前模块上下文标题与细铜分隔线。
pub fn sidebar_context_title(label: &str) -> impl Scene {
    bsn! {
        Node {
            width: percent(100), // 上下文标题完整横跨当前左栏页面。
            height: px(21), // 标题明显低于一级页签，维持二级信息密度。
            flex_shrink: 0.0, // 低高度窗口由页面滚动访问，不压扁标题。
            align_items: AlignItems::Center, // 标题文字在紧凑栏内垂直居中。
            padding: UiRect::horizontal(px(4)), // 与页内按钮文字保留统一呼吸空间。
            margin: UiRect::bottom(px(3)), // 铜线之后留出一级导航与工具的视觉间隔。
            border: UiRect::bottom(px(1)), // 细铜线明确结束一级导航上下文。
        }
        BackgroundColor(colors::PANEL_BG_SECTION) // 轻墨底承接上方深墨导航分区。
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 分隔线弱于活动一级页签底线。
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) // 标签由各页面提供，避免运行时拼接和额外状态。
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 页面上下文支持完整中文标签。
                font_size: px(ui_metrics::FONT_CONTROL), // 介于一级页签与普通提示文字之间。
            }
            TextColor(colors::TEXT_ACCENT) // 铜金强调当前模块身份。
        ]
    }
}

/// 构造角色页内的二级分区标题。
pub fn sidebar_subsection_title(label: &str) -> impl Scene {
    bsn! {
        Node {
            width: percent(100), // 二级标题与其下方全宽入口对齐。
            height: px(18), // 低于入口按钮，避免被误认成第三组页签。
            flex_shrink: 0.0, // 滚动内容增长时保持标题可读。
            align_items: AlignItems::Center, // 标题在固定高度内垂直居中。
            padding: UiRect::left(px(5)), // 左侧铜线后保留文字间距。
            margin: UiRect::top(px(2)), // 相邻内容之间建立紧凑分组距离。
            border: UiRect::left(px(2)), // 细铜竖线标记页内二级分区。
        }
        BorderColor::all(colors::COPPER_BORDER_WEAK) // 二级分区弱于一级活动铜边。
        Children [
            Text(label) @feathrise_editor_ui::localization::label_scene(label) // 角色页传入“全局预设”或“当前地图”。
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), // 二级标题沿用正式中文字体。
                font_size: px(ui_metrics::FONT_HINT), // 小字号表明它不是可点击页签。
            }
            TextColor(colors::TEXT_HINT) // 提示色与可点击入口形成明确差异。
        ]
    }
}

/// 构造分组右上角问号；入口绝对定位，不再单独占一行说明开关。
pub(super) fn sidebar_help_button(help: EditorSidebarHelp) -> impl Scene {
    bsn! {
        InteractionButton
        MapEditorSidebarHelpButton(help)
        Node {
            position_type: PositionType::Absolute, right: px(0), top: px(0),
            width: px(18), height: px(18),
            flex_shrink: 0.0, // 低高度窗口由页内滚动访问。
            border: UiRect::ZERO,
            border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        BackgroundColor(colors::PANEL_BG_SECTION)
        BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            MapEditorSidebarHelpButtonText
            Text("?")
            TextFont {
                font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
                font_size: px(ui_metrics::FONT_HINT),
            }
            TextColor(colors::TEXT_HINT)
        ]
    }
}

/// 构造原位隐藏正文；打开只移挂此实体，浮层统一提供边框与背景。
pub(super) fn sidebar_help_body(help: EditorSidebarHelp, copy: &str) -> impl Scene {
    bsn! {
        MapEditorSidebarHelpBody(help)
        Text(copy) @feathrise_editor_ui::localization::label_scene(copy)
        TextFont {
            font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)),
            font_size: px(ui_metrics::FONT_HINT),
        }
        TextColor(colors::TEXT_HINT)
        Node {
            display: Display::None, // 默认折叠且不保留任何高度。
            width: percent(100),
            flex_shrink: 0.0, min_width: px(0),
        }
    }
}
