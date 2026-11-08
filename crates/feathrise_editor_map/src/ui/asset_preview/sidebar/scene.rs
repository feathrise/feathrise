//! 目录导航、十二槽浏览、所选文件与唯一预览按任务分区；保留原操作身份。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*;
use bevy::text::{FontSourceTemplate, LineBreak};
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

use super::components::*;
use super::types::SidebarField;
use crate::components::{
    MapEditorAssetFolderText, MapEditorAssetList, MapEditorAssetListTitle,
    MapEditorLeftPanelScroll, MapEditorRightPanelScroll,
};
use crate::types::EditorAction;
use crate::ui::scene::{FONT_PATH, flex_editor_button, section_title};
use crate::ui::sidebars::{
    EditorNavigatorTab, EditorRightSidebarTab, EditorSidebarHelp, EditorSidebarTab,
    MapEditorSidebarPage, sidebar_help_body, sidebar_help_button,
};
use crate::ui::theme as colors;
use crate::ui::project_browser::{ProjectBrowserContent, ProjectBrowserView};

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
        Children [(
            Text("复制路径") @feathrise_editor_ui::localization::label_scene("复制路径") ~{FocusPolicy::Pass} TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        )]
    }
}

/// 左侧只浏览目录；右侧属性独立显示所选文件，切换工具不会让列表消失。
pub(crate) fn assets_page(scenes: &feathrise_editor_documents_model::DocumentScenes) -> impl Scene + use<> {
    bsn! {
        MapEditorSidebarPage(EditorSidebarTab::Navigator(EditorNavigatorTab::Assets))
        Node { display: Display::None, width: percent(100), min_width: px(0), min_height: px(0),
            flex_grow: 1.0, flex_direction: FlexDirection::Column, row_gap: px(4), overflow: Overflow::clip() }
        Children [
            (@assets_view(scenes)),
            (@crate::ui::project_browser::files_panel()),
        ]
    }
}

/// 登记资产目录继续借用原行池与功能；离开目录只隐藏，不销毁任何选择或草稿。
fn assets_view(scenes: &feathrise_editor_documents_model::DocumentScenes) -> impl Scene + use<> {
    bsn! {
        ProjectBrowserContent(ProjectBrowserView::Assets)
        MapEditorLeftPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_grow: 1.0,
            min_height: px(0), flex_direction: FlexDirection::Column, row_gap: px(4),
            padding: UiRect::top(px(3)), overflow: Overflow::scroll_y() }
        Children [
            (@section_title("目录导航")),
            (
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column, row_gap: px(4) }
                Children [
                    (
                        MapEditorAssetFolderText Text("assets/") ~{FocusPolicy::Pass}
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextLayout { linebreak: LineBreak::AnyCharacter } TextColor(colors::TEXT_VALUE)
                        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                    ),
                    (
                        // 窄栏容不下两组时整组换行，翻译后的标签不挤占另一组。
                        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0,
                            flex_wrap: FlexWrap::Wrap, row_gap: px(4), column_gap: px(4) }
                        Children [
                            (
                                Node { flex_grow: 1.0, flex_basis: px(114), min_width: px(114), column_gap: px(4) }
                                Children [
                                    (@flex_editor_button("上级", EditorAction::AssetFolderUp)),
                                    (@flex_editor_button("刷新", EditorAction::RefreshAssets)),
                                ]
                            ),
                            (@crate::ui::asset_thumbnails::view_mode_controls()),
                        ]
                    ),
                ]
            ),
            (@super::super::asset_search_panel(scenes)), // 固定跨文件搜索表单随资产页迁移，保留只读定位和替换入口。
            (@crate::ui::asset_shortcuts::panel()),
            (@section_title("目录内容")),
            (
                MapEditorAssetListTitle Text("工程 · 等待目录") @feathrise_editor_ui::localization::label_scene("工程 · 等待目录") ~{FocusPolicy::Pass}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextLayout { linebreak: LineBreak::AnyCharacter } TextColor(colors::TEXT_MUTED)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
            ),
            (@field(SidebarField::DirectoryFeedback, "等待目录同步")),
            (
                MapEditorAssetList
                Node { display: Display::None, width: percent(100), min_width: px(0), height: px(0),
                    flex_direction: FlexDirection::Column, row_gap: px(2),
                    flex_shrink: 0.0, overflow: Overflow::clip() }
            ),
        ]
    }
}

/// 文件属性与唯一预览留在右侧“属性”正文；不复制目录行池、文档输入或预览相机。
pub(crate) fn asset_properties_page(scenes: &feathrise_editor_documents_model::DocumentScenes) -> impl Scene + use<> {
    bsn! {
        MapEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::Assets))
        MapEditorRightPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
        Node { display: Display::None, width: percent(100), min_width: px(0), flex_grow: 1.0,
            min_height: px(0), flex_direction: FlexDirection::Column, row_gap: px(4),
            padding: UiRect::top(px(3)), overflow: Overflow::scroll_y() }
        Children [
            (@section_title("资产属性")),
            (@field(SidebarField::Name, "尚未选择文件")),
            (@field(SidebarField::Kind, "")),
            (
                AssetSidebarPathRow
                Node { display: Display::None, width: percent(100), min_width: px(0), flex_shrink: 0.0,
                    align_items: AlignItems::Center, column_gap: px(6) }
                Children [
                    (
                        ~{AssetSidebarText(SidebarField::Path)}
                        Text("") ~{FocusPolicy::Pass}
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextLayout { linebreak: LineBreak::AnyCharacter } TextColor(colors::TEXT_VALUE)
                        Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0) }
                    ),
                    (@copy_button()),
                ]
            ),
            (@field(SidebarField::CopyFeedback, "")),
            (@super::super::asset_preview_panel(scenes)),
            (@super::super::asset_preview_sidebar_launch(scenes)),
            (@sidebar_help_button(EditorSidebarHelp::Assets)),
            (@sidebar_help_body(EditorSidebarHelp::Assets,
                "单击文件只选择并预览，双击打开中央文件标签；右键可复制文件或目录路径。模型默认Scene0，可选择静态MeshN/PrimitiveN并复制精确网格和原材质引用；蒙皮网格仅支持完整场景。选择不放置实例、不改预设或地图。Cube KTX2请到环境页查看，同目录需有_diffuse与_specular派生图。")),
        ]
    }
}
