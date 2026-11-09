//! 目录完成反馈与所选文件只读同步；显式复制核验按钮绑定，不启动资源任务。

use feathrise_editor_ui::UiInteraction;
use bevy::clipboard::Clipboard;
use bevy::prelude::*;

use super::{components::*, types::*};
use crate::resources::{EditorAssetCatalog, MapEditorNumericScrubMouseGuard, MapEditorState};
use crate::ui::asset_context_menu::helpers::copy_asset_text_status;
use crate::ui::asset_preview::types::{AssetPreviewFileKind, preview_file_kind};
use crate::ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState};
use crate::ui::theme as colors;

/// 失败只有同修订完成证据才有效；刷新期不把保留的旧条目冒充新目录就绪。
pub(super) fn directory_feedback(catalog: &EditorAssetCatalog) -> (&str, bool) {
    if catalog.requested_revision != catalog.applied_revision {
        return ("正在同步目录 · 暂时保留已显示条目", false);
    }
    if let Some(feedback) = &catalog.feedback {
        match &feedback.result {
            Err(error) if feedback.revision == catalog.applied_revision => return (error, true),
            Ok(()) => {
                return (
                    if catalog.entries.is_empty() {
                        "当前目录没有资源"
                    } else {
                        ""
                    },
                    false,
                );
            }
            Err(_) => return ("目录请求已取消 · 保留已显示条目", false),
        }
    }
    ("等待目录同步", false)
}

pub(super) fn kind_label(path: &str) -> String {
    let kind = match preview_file_kind(path) {
        AssetPreviewFileKind::Image => "图片 / 纹理",
        AssetPreviewFileKind::Model => "模型",
        AssetPreviewFileKind::Audio => "音频 / 试听",
        AssetPreviewFileKind::Document => crate::asset_catalog::document_format(path)
            .map_or("文本文档", |format| format.label()),
        AssetPreviewFileKind::Data => "二进制数据",
        AssetPreviewFileKind::Unsupported => "其他文件 · 只读说明",
    };
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or_default();
    if extension.is_empty() {
        kind.to_owned()
    } else {
        format!("{kind} · {}", extension.to_ascii_uppercase())
    }
}

pub(super) fn sync_sidebar(
    editor: Res<MapEditorState>,
    catalog: Res<EditorAssetCatalog>,
    sidebar: Res<MapEditorSidebarState>,
    mut previous: Local<SidebarObservation>,
    mut buttons: Query<(
        &mut AssetSidebarCopyPath,
        &UiInteraction,
        &mut BackgroundColor,
        &mut BorderColor,
        Option<&mut feathrise_editor_ui::EditorButtonClick>,
    )>,
    mut texts: Query<
        (&AssetSidebarText, &mut Text, &mut TextColor, &mut Node),
        Without<AssetSidebarPathRow>,
    >,
    mut path_rows: Query<&mut Node, (With<AssetSidebarPathRow>, Without<AssetSidebarText>)>,
) {
    if !editor.open || (!sidebar.assets_browser_visible() && sidebar.right != EditorRightSidebarTab::Assets) {
        return; // 隐藏页不构造显示字符串，再次显示时从当前输入一次同步。
    }
    let Ok((mut button, interaction, mut background, mut border, click)) = buttons.single_mut() else {
        return;
    };
    let selected = editor.selected_asset.as_deref();
    if button.path.as_deref() != selected {
        if let Some(mut click) = click { click.invalidate(); } // 属性切换文件时取消旧复制按钮的按住意图。
        button.path = editor.selected_asset.clone();
        button.feedback.clear(); // 旧文件复制回执不能串入新文件。
    }
    let color = match interaction {
        UiInteraction::Pressed => colors::SELECT_BG_PRESSED,
        UiInteraction::Hovered => colors::SELECT_BG_HOVER,
        UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL,
    };
    background.set_if_neq(BackgroundColor(color));
    border.set_if_neq(BorderColor::all(if *interaction == UiInteraction::None {
        colors::COPPER_BORDER_WEAK
    } else {
        colors::COPPER_BORDER
    }));
    let directory_changed = !previous.initialized
        || previous.folder != catalog.current_folder
        || previous.requested != catalog.requested_revision
        || previous.applied != catalog.applied_revision
        || previous.count != catalog.entries.len()
        || previous.feedback != catalog.feedback;
    let selection_changed = !previous.initialized || previous.selected.as_deref() != selected;
    let copy_changed = !previous.initialized || previous.copied != button.feedback;
    if !directory_changed && !selection_changed && !copy_changed {
        return; // 其他工具的底栏、预览旋转或文档逐字编辑不能重写静态文件信息。
    }
    let (directory, failed) = directory_feedback(&catalog);
    let name = selected
        .map(|path| path.rsplit('/').next().unwrap_or(path))
        .unwrap_or("尚未选择文件");
    let kind = if selection_changed {
        selected.map(kind_label).unwrap_or_default()
    } else {
        String::new()
    };
    let path = if selection_changed {
        selected
            .map(|path| format!("assets/{path}"))
            .unwrap_or_default()
    } else {
        String::new()
    };
    for (field, mut text, mut text_color, mut node) in &mut texts {
        let wanted = match field.0 {
            SidebarField::DirectoryFeedback if directory_changed => directory,
            SidebarField::Name if selection_changed => name,
            SidebarField::Kind if selection_changed => &kind,
            SidebarField::Path if selection_changed => &path,
            SidebarField::CopyFeedback if copy_changed => &button.feedback,
            _ => continue,
        };
        if text.as_str() != wanted {
            **text = wanted.to_owned();
        }
        let color = match field.0 {
            SidebarField::DirectoryFeedback if failed => colors::TEXT_ACCENT,
            SidebarField::DirectoryFeedback | SidebarField::Kind => colors::TEXT_MUTED,
            SidebarField::Name => colors::TEXT_PRIMARY,
            SidebarField::CopyFeedback => colors::TEXT_ACCENT,
            SidebarField::Path => colors::TEXT_VALUE,
        };
        text_color.set_if_neq(TextColor(color));
        let hidden =
            matches!(field.0, SidebarField::Kind | SidebarField::CopyFeedback) && wanted.is_empty();
        let display = if hidden { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
    }
    if selection_changed {
        for mut node in &mut path_rows {
            let display = if selected.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
        previous.selected = editor.selected_asset.clone();
    }
    if directory_changed {
        previous.folder = catalog.current_folder.clone();
        previous.requested = catalog.requested_revision;
        previous.applied = catalog.applied_revision;
        previous.count = catalog.entries.len();
        previous.feedback = catalog.feedback.clone(); // 仅一次受检完成诊断，不复制完整目录或文档。
    }
    if copy_changed {
        previous.copied = button.feedback.clone();
    }
    previous.initialized = true;
}

/// 与右键菜单共用真实剪贴板结果；旧按下被消费而不复制当前新绑定的文件。
pub(super) fn copy_selected_path(
    mut buttons: Query<(&feathrise_editor_ui::EditorButtonClick, &mut AssetSidebarCopyPath), Changed<feathrise_editor_ui::EditorButtonClick>>,
    mut editor: ResMut<MapEditorState>,
    sidebar: Res<MapEditorSidebarState>,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    mut clipboard: Option<ResMut<Clipboard>>,
    windows: Query<&Window>,
    modals: (
        Option<Res<crate::resources::EditorTextInputGuard>>,
        Option<Res<crate::resources::MapEditorSettingsState>>,
        Option<Res<crate::resources::MapBrowserState>>,
        Option<Res<crate::resources::MapDeletionTask>>,
        Option<Res<crate::authoring::TerrainExternalChangeState>>,
    ),
) {
    let (text, settings, browser, deletion, conflict) = modals;
    if !editor.open
        || editor.close_requested
        || editor.pending_map_transition.is_some()
        || editor.new_map_dialog_open
        || sidebar.right != EditorRightSidebarTab::Assets
        || guard.blocked()
        || windows.iter().any(|window| !window.focused)
        || text.is_some_and(|guard| guard.block_shortcuts)
        || settings.is_some_and(|state| state.open)
        || browser.is_some_and(|state| state.dialog.is_some())
        || deletion.is_some_and(|state| state.confirmation.is_some())
        || conflict.is_some_and(|state| state.modal_open())
    {
        return;
    }
    for (interaction, mut button) in &mut buttons {
        if !interaction.is_clicked() {
            continue;
        }
        let Some(path) = button.path.as_deref().filter(|path| !path.is_empty()) else {
            continue;
        };
        if editor.selected_asset.as_deref() != Some(path) {
            continue;
        }
        let status = if let Some(clipboard) = clipboard.as_mut() {
            copy_asset_text_status("路径", path, |value| clipboard.set_text(value))
        } else {
            "复制失败：系统剪贴板尚未就绪，请稍后重试".to_owned()
        };
        button.feedback = status.clone();
        editor.status = status; // 不改变预览来源、原生输入、地图脏位或撤销历史。
    }
}
