//! 工程文件按实际可见窗口复用固定行；文件名筛选、排序与完整目录释放均不在UI帧执行。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::{ProjectBrowserState, ProjectFileViewport}, types::*, window};
use crate::{asset_catalog::project_files::EditorProjectFileCatalog, ui::theme as colors};
use bevy::{prelude::*, ui::FocusPolicy};
use feathrise_editor_ui::{EditorButtonClick, localization::EditorLocale};

/// 完整作者和轻量工作台共用工程路径回显；目录来源相同，不因借用资产行池退回旧根标签。
pub(super) fn asset_folder(
    catalog: Res<crate::asset_catalog::EditorAssetCatalog>,
    browser: Res<ProjectBrowserState>,
    locale: Option<Res<EditorLocale>>,
    mut labels: Query<(Ref<crate::components::SceneEditorAssetFolderText>, &mut Text)>,
) {
    if !browser.assets_visible() { return; }
    if !catalog.is_changed() && !browser.is_changed()
        && !locale.as_ref().is_some_and(|locale| locale.is_changed())
        && labels.iter().all(|(label, _)| !label.is_added()) { return; }
    let value = browser.asset_folder_label(&catalog.current_folder, locale.as_deref().copied().unwrap_or_default());
    for (_, mut text) in &mut labels {
        if text.0 != value { text.0 = value.clone(); }
    }
}

pub(super) fn mount_rows(
    mut commands: Commands,
    window: Res<ProjectFileViewport>,
    mut lists: Query<(Entity, &mut ProjectFileList)>,
    asset_server: Res<AssetServer>,
) {
    let needed = window.capacity.max(window::MIN_POOL);
    if lists.iter().all(|(_, list)| list.spawned_rows >= needed) { return; }
    let font = feathrise_editor_core::editor_font(&asset_server, crate::ui::scene::FONT_PATH);
    for (list, mut pool) in &mut lists {
        if pool.spawned_rows >= needed { continue; }
        let start = pool.spawned_rows;
        pool.spawned_rows = needed; // 只随真实可视高度扩池，目录数量和滚动不能创建更多实体。
        commands.entity(list).with_children(|parent| {
            for slot in start..needed {
                parent.spawn((InteractionButton, ProjectFileRow { slot, ..default() },
                    Node { display: Display::None, width: percent(100), height: px(feathrise_editor_ui::metrics::ROW_HEIGHT),
                        position_type: PositionType::Absolute, left: px(0), top: px(0),
                        min_width: px(0), flex_shrink: 0.0, align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(5)), column_gap: px(5), overflow: Overflow::clip_x(),
                        border_radius: BorderRadius::all(px(2)), ..default() },
                    BackgroundColor(colors::PANEL_BG), BorderColor::all(colors::COPPER_BORDER_WEAK)))
                    .with_children(|row| {
                        row.spawn((ProjectFileRowIcon, ImageNode::default(), FocusPolicy::Pass,
                            Node { width: px(16), height: px(16), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
                        row.spawn((ProjectFileRowText, Text::new(""), FocusPolicy::Pass,
                            TextFont { font: font.clone().into(), font_size: feathrise_editor_ui::metrics::FONT_CONTROL.into(), ..default() },
                            TextColor(colors::TEXT_PRIMARY), TextLayout::no_wrap(),
                            Node { min_width: px(0), flex_grow: 1.0, flex_basis: px(0), ..default() }, Pickable::IGNORE));
                    });
            }
        });
    }
}

pub(super) fn views(
    state: Res<ProjectBrowserState>,
    mut contents: Query<(&ProjectBrowserContent, &mut Node)>,
) {
    for (content, mut node) in &mut contents {
        let display = if content.0 == state.view { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
}

pub(super) fn files(
    catalog: Res<EditorProjectFileCatalog>,
    state: Res<ProjectBrowserState>,
    locale: Option<Res<EditorLocale>>,
    window: Res<ProjectFileViewport>,
    asset_server: Res<AssetServer>,
    mut rows: Query<(&mut ProjectFileRow, &mut Node, &Children, &UiInteraction, &mut BackgroundColor, &mut BorderColor, Option<&mut EditorButtonClick>)>,
    mut labels: Query<(&mut Text, &mut TextColor), With<ProjectFileRowText>>,
    mut icons: Query<&mut ImageNode, With<ProjectFileRowIcon>>,
    mut fields: Query<(&mut Text, &mut TextColor, Option<&ProjectFileFolderText>, Option<&ProjectFileSummaryText>, Option<&ProjectFileStatusText>, Option<&ProjectFileSelectionText>),
        (Without<ProjectFileRowText>, Or<(With<ProjectFileFolderText>, With<ProjectFileSummaryText>, With<ProjectFileStatusText>, With<ProjectFileSelectionText>)>)>,
) {
    if state.view != ProjectBrowserView::ProjectFiles { return; }
    for (mut row, mut node, children, interaction, mut background, mut border, click) in &mut rows {
        let index = window.first.saturating_add(row.slot);
        let entry = (row.slot < window.capacity).then(|| catalog.entries.get(index)).flatten();
        let wanted = entry.map(|_| index);
        let changed = row.entry_index != wanted || row.revision != catalog.applied_revision || row.viewport_revision != window.revision;
        if changed {
            if let Some(mut click) = click { click.invalidate(); }
            row.entry_index = wanted;
            row.revision = catalog.applied_revision;
            row.viewport_revision = window.revision;
        }
        let display = if entry.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        let Some(entry) = entry else { continue; };
        let top = px(index as f32 * window::ROW_PITCH);
        if node.top != top { node.top = top; }
        let selected = !entry.is_directory && catalog.selected_file.as_ref().is_some_and(|selected| selected.relative_path == entry.asset_path);
        background.set_if_neq(BackgroundColor(match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            _ => colors::PANEL_BG,
        }));
        border.set_if_neq(BorderColor::all(if selected || *interaction != UiInteraction::None { colors::COPPER_BORDER } else { colors::COPPER_BORDER_WEAK }));
        for child in children {
            if let Ok((mut text, mut color)) = labels.get_mut(*child) {
                if text.0 != entry.display_name { text.0 = entry.display_name.clone(); }
                color.set_if_neq(TextColor(if selected { colors::TEXT_ACCENT } else { colors::TEXT_PRIMARY }));
            }
            if changed && let Ok(mut image) = icons.get_mut(*child) {
                image.image = asset_server.load(if entry.is_directory { "icons/editor/tree_folder.png" } else { "icons/editor/tree_file.png" });
                image.color = colors::TEXT_PRIMARY;
            }
        }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    // 只翻译目录标头；真实相对路径和文件名保持目录中的原字节。
    let folder = format!("{}{}", locale.text("label.project"), catalog.current_folder);
    let summary = locale.format("project.files.count", &[("count", catalog.entries.len().to_string())]);
    let error = catalog.feedback.as_ref().and_then(|feedback| feedback.result.as_ref().err());
    let status = if catalog.project_root().is_none() { locale.text("project.files.requires_project") }
        else if catalog.is_loading() { locale.text("project.files.loading") }
        else if let Some(error) = error { error.as_str() }
        else if catalog.entries.is_empty() { locale.text("project.files.empty") } else { "" };
    let selected = catalog.selected_file.as_ref().map(|selected| selected.relative_path.as_str());
    for (mut text, mut color, directory, count, feedback, selection) in &mut fields {
        let value = if directory.is_some() { folder.as_str() } else if count.is_some() { summary.as_str() }
            else if feedback.is_some() { status }
            else if selection.is_some() { selected.unwrap_or_else(|| locale.text("label.double-click-a-text-file-to-open")) } else { continue; };
        if text.0 != value { text.0 = value.to_owned(); }
        if feedback.is_some() { color.set_if_neq(TextColor(if error.is_some() { colors::TEXT_ACCENT } else { colors::TEXT_MUTED })); }
    }
}

pub(super) fn action_feedback(
    catalog: Res<EditorProjectFileCatalog>,
    mut buttons: Query<(&ProjectFileActionButton, &UiInteraction, &Children, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<&mut TextColor>,
) {
    for (button, interaction, children, mut background, mut border) in &mut buttons {
        let enabled = match button.0 {
            ProjectFileAction::Up => !catalog.current_folder.is_empty(),
            ProjectFileAction::Refresh => catalog.project_root().is_some(),
        };
        background.set_if_neq(BackgroundColor(if !enabled { colors::PANEL_BG } else { match interaction {
            UiInteraction::Pressed => colors::SELECT_BG_PRESSED,
            UiInteraction::Hovered => colors::SELECT_BG_HOVER,
            UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL,
        } }));
        border.set_if_neq(BorderColor::all(if enabled && *interaction != UiInteraction::None { colors::COPPER_BORDER } else { colors::COPPER_BORDER_WEAK }));
        for child in children { if let Ok(mut color) = labels.get_mut(*child) { color.set_if_neq(TextColor(if enabled { colors::TEXT_PRIMARY } else { colors::TEXT_MUTED })); } }
    }
}
