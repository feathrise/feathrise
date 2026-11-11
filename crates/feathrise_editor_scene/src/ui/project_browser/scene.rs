//! 单一工程目录的文件正文；进入资产子目录时由内部来源路由复用原资产行池及预览。

use feathrise_editor_ui::InteractionButton;
use super::{components::*, types::*};
use crate::{
    components::{SceneEditorActionButton, SceneEditorAssetFolderText, SceneEditorCustomActionStyle, SceneEditorLeftPanelScroll},
    types::EditorAction,
    ui::{
        scene::{FONT_PATH, tree_icon},
        theme as colors,
        tooltips::{EditorTooltipTarget, TooltipAction},
    },
};
use bevy::{prelude::*, text::FontSourceTemplate, ui::{FocusPolicy, RelativeCursorPosition}};
use feathrise_editor_ui::metrics as ui_metrics;

fn directory_action(view: ProjectBrowserView, action: ProjectFileAction) -> impl Scene {
    let editor_action = match action {
        ProjectFileAction::Up => EditorAction::AssetFolderUp,
        ProjectFileAction::Refresh => EditorAction::RefreshAssets,
    };
    let asset = (view == ProjectBrowserView::Assets).then(|| bsn! {
        ~{SceneEditorActionButton(editor_action)} SceneEditorCustomActionStyle
    });
    let project = (view == ProjectBrowserView::ProjectFiles)
        .then(|| bsn! { ~{ProjectFileActionButton(action)} });
    bsn! {
        ~{DirectoryActionButton { view, action }} @asset @project
        ~{EditorTooltipTarget(TooltipAction::Editor(editor_action))}
    }
}

pub(in crate::ui) fn toolbar_button(icon: &'static str, width: f32) -> impl Scene {
    bsn! {
        InteractionButton
        Node { width: px(width), height: px(24), flex_shrink: 0.0,
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            border: px(1), border_radius: BorderRadius::all(px(3)) }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [@tree_icon(icon)]
    }
}

pub(in crate::ui) fn refresh_button(view: ProjectBrowserView, width: f32) -> impl Scene {
    bsn! { @toolbar_button("icons/editor/tool_refresh.png", width) @directory_action(view, ProjectFileAction::Refresh) }
}

fn folder_text(path: &'static str) -> impl Scene {
    bsn! {
        Text(path) ~{FocusPolicy::Pass}
        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
        TextColor(colors::TEXT_VALUE) TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
    }
}

pub(in crate::ui) fn asset_folder_path() -> impl Scene {
    bsn! {
        SceneEditorAssetFolderText @folder_text("assets/")
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
    }
}

fn directory_toolbar() -> impl Scene {
    bsn! {
        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0, column_gap: px(4),
            align_items: AlignItems::Start }
        Children [
            ProjectFileFolderText @folder_text("工程/")
                Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), align_self: AlignSelf::Center }--
            @refresh_button(ProjectBrowserView::ProjectFiles, 24.0)
        ]
    }
}

pub(crate) fn parent_directory_row(view: ProjectBrowserView) -> impl Scene {
    bsn! {
        // 父目录是独立导航行，不进入可复用文件槽、筛选计数、缩略图或文件右键菜单。
        InteractionButton @directory_action(view, ProjectFileAction::Up)
        Node { display: Display::None, width: percent(100), height: px(ui_metrics::ROW_HEIGHT),
            flex_shrink: 0.0, align_items: AlignItems::Center, padding: UiRect::horizontal(px(5)),
            column_gap: px(5), border_radius: BorderRadius::all(px(2)) }
        BackgroundColor(colors::PANEL_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
        Children [
            @tree_icon("icons/editor/tree_folder.png")--
            Text("..") ~{FocusPolicy::Pass} Pickable::IGNORE TextLayout::no_wrap()
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

pub(crate) fn files_panel() -> impl Scene {
    bsn! {
        ProjectBrowserContent(ProjectBrowserView::ProjectFiles)
        SceneEditorLeftPanelScroll RelativeCursorPosition::default() ScrollPosition::default()
        Node { display: Display::Flex, width: percent(100), min_width: px(0), min_height: px(0),
            flex_grow: 1.0, flex_direction: FlexDirection::Column, row_gap: px(4),
            padding: UiRect::top(px(3)), overflow: Overflow::scroll_y() }
        Children [
            @directory_toolbar()--
            ProjectFileSummaryText Text("") ~{FocusPolicy::Pass}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }--
            ProjectFileStatusText Text("尚未打开工程") ~{FocusPolicy::Pass}
                ~{crate::ui::status_bar::PanelStatusText("工程目录")} feathrise_editor_ui::feedback::GlobalPanelStatus
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_MUTED) TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }--
            @parent_directory_row(ProjectBrowserView::ProjectFiles)--
            ProjectFileList::default() Node { width: percent(100), min_width: px(0), height: px(0), flex_shrink: 0.0,
                position_type: PositionType::Relative, overflow: Overflow::clip() }--
            ProjectFileSelectionText Text("双击文本文件打开；右键显示文件操作") ~{FocusPolicy::Pass}
                TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_HINT) }
                TextColor(colors::TEXT_VALUE) TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
        ]
    }
}
