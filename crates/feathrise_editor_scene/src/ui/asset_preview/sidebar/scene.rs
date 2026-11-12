//! 工程目录浏览与所选文件属性分区；保留固定行池与唯一预览。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use feathrise_editor_documents_model::search::components::SearchAction;
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineBreak};
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

use super::components::*;
use super::types::SidebarField;
use crate::components::{
    SceneEditorAssetList, SceneEditorAssetListTitle,
    SceneEditorLeftPanelScroll, SceneEditorRightPanelScroll,
};
use crate::ui::scene::{FONT_PATH, section_title};
use crate::ui::sidebars::{
    EditorNavigatorTab, EditorRightSidebarTab, EditorSidebarHelp, EditorSidebarTab,
    SceneEditorSidebarPage, sidebar_help_body, sidebar_help_button,
};
use crate::ui::theme as colors;
use crate::ui::asset_thumbnails::ViewModeButton;
use crate::ui::project_browser::{
    ProjectBrowserContent, ProjectBrowserView, asset_folder_path, parent_directory_row, refresh_button, toolbar_button,
};
use crate::ui::tooltips::{EditorTooltipTarget, TooltipAction};

fn field(field: SidebarField, initial: &str) -> impl Scene {
    let feedback = matches!(field, SidebarField::DirectoryFeedback | SidebarField::CopyFeedback)
        .then(|| bsn! { ~{crate::ui::status_bar::PanelStatusText("资产目录")} feathrise_editor_ui::feedback::GlobalPanelStatus });
    (bsn! {
        ~{AssetSidebarText(field)}
        Text(initial) ~{FocusPolicy::Pass}
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextLayout { linebreak: LineBreak::AnyCharacter }
        TextColor(colors::TEXT_VALUE)
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }, feedback)
}

fn copy_button() -> impl Scene {
    bsn! {
        InteractionButton AssetSidebarCopyPath::default()
        Node { height: px(26), flex_shrink: 0.0, padding: UiRect::horizontal(px(6)),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            Text("复制路径") @feathrise_editor_ui::localization::label_scene("复制路径") ~{FocusPolicy::Pass} TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

/// 左侧只浏览目录；右侧属性独立显示所选文件，切换工具不会让列表消失。
pub(crate) fn assets_page(scenes: &feathrise_editor_documents_model::DocumentScenes) -> impl Scene + use<> {
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Navigator(EditorNavigatorTab::Assets))
        Node { display: Display::None, width: percent(100), min_width: px(0), min_height: px(0),
            flex_grow: 1.0, flex_direction: FlexDirection::Column, row_gap: px(4), overflow: Overflow::clip() }
        Children [
            @crate::ui::project_browser::filter_toolbar()--
            @assets_view(scenes)--
            @crate::ui::project_browser::files_panel()
        ]
    }
}

fn asset_tools() -> impl Scene {
    const ICON_WIDTH: f32 = 22.0;
    const HELP: &str = concat!(
        "内容搜索按文字查找文件内容，可按文件类型筛选。按 Ctrl+Shift+F 打开，按 Enter 搜索。\n",
        "缺失检查核对场景和配置引用的资产路径。再次点击可收起结果。\n",
        "收藏显示已收藏的文件和目录。右键文件或目录可以加入收藏。\n",
        "最近访问列出近期访问的文件和目录，点击条目可定位。\n",
        "刷新会重新读取当前目录。\n",
        "缩略图可在列表和缩略卡片之间切换。\n",
        "点击文件行或卡片主体查看属性。点击动画文件上的箭头展开片段。",
    );
    bsn! {
        // 窄栏也为六个操作及帮助预留完整命中区，路径使用下一行的整栏宽度。
        Node { width: percent(100), min_width: px(0), height: px(24), column_gap: px(2),
            flex_shrink: 0.0, padding: UiRect::right(px(20)), align_items: AlignItems::Center }
        Children [
            @toolbar_button("icons/editor/tool_search.png", ICON_WIDTH)
                ~{SearchAction::Toggle} ~{EditorTooltipTarget(TooltipAction::AssetSearch)}--
            @toolbar_button("icons/editor/tool_missing_assets.png", ICON_WIDTH)
                ~{SearchAction::MissingAssets} ~{EditorTooltipTarget(TooltipAction::AssetMissingCheck)}--
            @crate::ui::asset_shortcuts::toolbar_buttons()--
            @refresh_button(ProjectBrowserView::Assets, ICON_WIDTH)--
            @toolbar_button("icons/editor/tree_image.png", ICON_WIDTH)
                ViewModeButton ~{EditorTooltipTarget(TooltipAction::AssetThumbnails)}--
            @crate::ui::context_help::help_button("工程工具", HELP)
        ]
    }
}

/// 登记资产目录继续借用原行池与功能；离开目录只隐藏，不销毁任何选择或草稿。
fn assets_view(scenes: &feathrise_editor_documents_model::DocumentScenes) -> impl Scene + use<> {
    bsn! {
        ProjectBrowserContent(ProjectBrowserView::Assets)
        SceneEditorLeftPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_grow: 1.0,
            min_height: px(0), flex_direction: FlexDirection::Column, row_gap: px(4),
            padding: UiRect::top(px(3)), overflow: Overflow::scroll_y() }
        Children [
            @asset_tools()--
            @asset_folder_path()--
            @super::super::asset_search_panel(scenes)-- // 固定跨文件搜索表单随资产页迁移，保留只读定位和替换入口。
            @crate::ui::asset_shortcuts::panel()--

                SceneEditorAssetListTitle Text("工程 · 等待目录") @feathrise_editor_ui::localization::label_scene("工程 · 等待目录") ~{FocusPolicy::Pass}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextLayout { linebreak: LineBreak::AnyCharacter } TextColor(colors::TEXT_MUTED)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            --
            @field(SidebarField::DirectoryFeedback, "等待目录同步")--
            @parent_directory_row(ProjectBrowserView::Assets)--

                SceneEditorAssetList
                Node { display: Display::None, width: percent(100), min_width: px(0), height: px(0),
                    flex_direction: FlexDirection::Column, row_gap: px(2),
                    flex_shrink: 0.0, overflow: Overflow::clip() }

        ]
    }
}

/// 文件属性与唯一预览留在右侧“属性”正文；不复制目录行池、文档输入或预览相机。
pub(crate) fn asset_properties_page(
    scenes: &feathrise_editor_documents_model::DocumentScenes,
    weight_scenes: &feathrise_editor_weights_model::WeightPanelScenes,
    animation_scenes: &feathrise_editor_animation_model::assembly::AnimationScenes,
) -> impl Scene + use<> {
    let weight_entry = (weight_scenes.entry)();
    let bind_model = (animation_scenes.binding_properties_host)();
    let animation_properties = (animation_scenes.properties_host)();
    bsn! {
        SceneEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::Assets))
        SceneEditorRightPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_grow: 1.0,
            min_height: px(0), flex_direction: FlexDirection::Column, row_gap: px(4),
            padding: UiRect::top(px(3)), overflow: Overflow::scroll_y() }
        Children [
            @section_title("资产属性")--
            @field(SidebarField::Name, "尚未选择文件")--
            @field(SidebarField::Kind, "")--

                AssetSidebarPathRow
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0,
                    align_items: AlignItems::Center, column_gap: px(6) }
                Children [

                        ~{AssetSidebarText(SidebarField::Path)}
                        Text("") ~{FocusPolicy::Pass}
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextLayout { linebreak: LineBreak::AnyCharacter } TextColor(colors::TEXT_VALUE)
                        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
                    --
                    @copy_button()
                ]
            --
            @field(SidebarField::CopyFeedback, "")--
            @bind_model--
            @super::details::panel()--
            @super::super::asset_preview_panel(scenes, animation_scenes)--
            @super::super::asset_preview_sidebar_launch(scenes)--
            @super::super::rigging::scene::motion_properties_host()--
            @weight_entry--
            @animation_properties--
            @sidebar_help_button(EditorSidebarHelp::Assets)--
            @sidebar_help_body(EditorSidebarHelp::Assets,
                "单击文件只选择并预览，双击打开中央文件标签；右键可复制文件或目录路径。模型默认Scene0，可选择静态MeshN/PrimitiveN并复制精确网格和原材质引用；蒙皮网格仅支持完整场景。选择不放置实例、不改预设或场景。Cube KTX2请到环境页查看，同目录需有_diffuse与_specular派生图。")
        ]
    }
}
