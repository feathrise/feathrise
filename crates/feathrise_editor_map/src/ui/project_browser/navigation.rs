//! 单一工程目录的来源路由：登记资产根仍由原资产目录承载，根外源码沿工程文件域处理。

pub(super) mod types;

use std::path::{Component, Path, PathBuf};

use bevy::prelude::*;
use feathrise_editor_ui::EditorButtonClick;

use super::{input::BrowserInputGate, ProjectBrowserState, ProjectBrowserView};
use crate::{
    asset_catalog::{request_asset_folder, EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    components::MapEditorActionButton,
    resources::{MapEditorLayoutState, MapEditorState},
    types::EditorAction,
    ui::sidebars::MapEditorSidebarState,
    workbench::WorkbenchModel,
};
use types::{NavigationState, PendingDirectory, Roots};

fn roots(model: Option<&WorkbenchModel>) -> Option<Roots> {
    if let Some(model) = model {
        return Some(Roots { context: model.context, project: model.project_root.clone()?, assets: model.asset_root.clone()? });
    }
    let paths = feathrise_editor_core::editor_project_paths()?;
    Some(Roots { context: 0, project: paths.project_root().to_path_buf(), assets: paths.asset_root().to_path_buf() })
}

/// 仅比较已核验的显式路径组件，不访问磁盘；Windows的assets大小写不能改变导航来源。
fn relative_under(root: &Path, path: &Path) -> Option<PathBuf> {
    let mut remaining = path.components();
    for expected in root.components() {
        let actual = remaining.next()?;
        #[cfg(windows)]
        let equal = expected.as_os_str().to_string_lossy().eq_ignore_ascii_case(&actual.as_os_str().to_string_lossy());
        #[cfg(not(windows))]
        let equal = expected == actual;
        if !equal { return None; }
    }
    Some(remaining.as_path().to_path_buf())
}

fn relative_text(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.components() {
        let Component::Normal(part) = part else { return None; };
        parts.push(part.to_str()?);
    }
    Some(parts.join("/"))
}

pub(super) fn asset_prefix(project_root: &Path, asset_root: &Path) -> Option<String> {
    if !project_root.is_absolute() || !asset_root.is_absolute() { return None; }
    let relative = relative_under(project_root, asset_root)?;
    if relative.as_os_str().is_empty() { return None; }
    relative_text(&relative)
}

/// Task阶段在两个实际采用系统之后执行；内部路由只有真实目录成功才切换原资产正文。
pub(super) fn follow_directory(
    model: Option<Res<WorkbenchModel>>,
    files: Res<EditorProjectFileCatalog>,
    mut assets: ResMut<EditorAssetCatalog>,
    mut state: ResMut<ProjectBrowserState>,
    mut editor: ResMut<MapEditorState>,
    mut sidebar: ResMut<MapEditorSidebarState>,
    layout: Res<MapEditorLayoutState>,
    mut navigation: Local<NavigationState>,
) {
    let source = roots(model.as_deref());
    if navigation.source != source {
        state.asset_prefix = source.as_ref().and_then(|source| asset_prefix(&source.project, &source.assets));
        state.view = ProjectBrowserView::ProjectFiles;
        navigation.source = source.clone();
        navigation.pending = None;
        navigation.inspected = None;
        navigation.observed_pending = None;
        navigation.observed_completed = None;
    }
    let pending = assets.pending_reveal_revision();
    let completed = assets.revealed_revision();
    let owned = navigation.pending.as_ref().map(|pending| pending.asset_revision);
    let external_pending = pending.is_some() && pending != navigation.observed_pending && pending != owned;
    let external_completed = completed.is_some() && completed != navigation.observed_completed && completed != owned;
    navigation.observed_pending = pending;
    navigation.observed_completed = completed;
    if external_pending || external_completed {
        navigation.pending = None;
        // 定位先获得优先权，只有完整成功后才采用目标目录；失败不能把工程根换成旧资产目录。
        if external_completed { state.show_assets(); }
    }
    let available = editor.open && !editor.close_requested && editor.pending_map_transition.is_none()
        && sidebar.assets_browser_visible() && layout.content_shown();
    if !available {
        if owned.is_some() && assets.pending_reveal_revision() == owned { assets.cancel_pending_reveal(); }
        navigation.pending = None;
        navigation.inspected = None;
        return;
    }
    let Some(source) = source else { return; };
    if let Some(route) = navigation.pending.as_ref() {
        let current = source == route.roots && state.view == ProjectBrowserView::ProjectFiles
            && files.source_context() == Some(source.context) && files.project_root() == Some(source.project.as_path())
            && files.applied_revision == route.project_revision && files.requested_revision == route.project_revision
            && files.current_folder == route.project_folder;
        if !current || assets.requested_revision != route.asset_revision {
            if assets.pending_reveal_revision() == Some(route.asset_revision) { assets.cancel_pending_reveal(); }
            navigation.pending = None;
            return;
        }
        if assets.applied_revision == route.asset_revision {
            if assets.current_folder == route.asset_folder
                && assets.feedback.as_ref().is_some_and(|feedback| feedback.revision == route.asset_revision && feedback.result.is_ok()) {
                state.show_assets(); // 只切已有正文，不改selected_asset、模型、缩略图或任何作者文档。
            }
            navigation.pending = None; // 失败也只结束一次，显式刷新或新导航才重试。
        }
        return;
    }
    if state.view != ProjectBrowserView::ProjectFiles || files.is_loading()
        || files.source_context() != Some(source.context) || files.project_root() != Some(source.project.as_path())
        || !files.feedback.as_ref().is_some_and(|feedback| feedback.revision == files.applied_revision && feedback.result.is_ok()) {
        return;
    }
    let inspected = (source.clone(), files.applied_revision);
    if navigation.inspected.as_ref() == Some(&inspected) { return; }
    navigation.inspected = Some(inspected);
    if asset_prefix(&source.project, &source.assets).is_none() { return; }
    let folder = source.project.join(&files.current_folder);
    let Some(relative) = relative_under(&source.assets, &folder).and_then(|path| relative_text(&path)) else { return; };
    if let Err(error) = request_asset_folder(&mut assets, &mut editor, &mut sidebar, &relative) {
        editor.status = error;
        return;
    }
    navigation.pending = Some(PendingDirectory {
        roots: source, project_revision: files.applied_revision, project_folder: files.current_folder.clone(),
        asset_folder: relative, asset_revision: assets.requested_revision,
    });
    navigation.observed_pending = assets.pending_reveal_revision();
}

/// 原资产根的“上级”跨回工程父目录；必须先于原资产动作消费，并使这次释放脉冲失效。
pub(super) fn asset_root_up(
    gate: BrowserInputGate,
    model: Option<Res<WorkbenchModel>>,
    editor: Res<MapEditorState>,
    mut state: ResMut<ProjectBrowserState>,
    mut files: ResMut<EditorProjectFileCatalog>,
    mut assets: ResMut<EditorAssetCatalog>,
    mut buttons: Query<(&MapEditorActionButton, &mut EditorButtonClick), Changed<EditorButtonClick>>,
) {
    if state.view != ProjectBrowserView::Assets || !assets.current_folder.is_empty() || !gate.available(&editor) { return; }
    let Some(source) = roots(model.as_deref()) else { return; };
    if files.source_context() != Some(source.context) || files.project_root() != Some(source.project.as_path()) { return; }
    let Some(prefix) = asset_prefix(&source.project, &source.assets) else { return; };
    let parent = prefix.rsplit_once('/').map_or("", |(parent, _)| parent);
    for (button, mut click) in &mut buttons {
        if button.0 != EditorAction::AssetFolderUp || !click.is_clicked() { continue; }
        if files.request_folder(parent).is_ok() {
            assets.cancel_pending_reveal();
            state.view = ProjectBrowserView::ProjectFiles;
            click.invalidate(); // 同一按钮释放不能再被Workbench或原一体动作导航一次。
        }
    }
}

#[cfg(test)]
mod tests;
