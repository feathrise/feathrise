//! 工程目录的文件动作；双击只发布文档意图，不把源码当作场景资产。

use super::{components::*, resources::{ProjectBrowserState, ProjectFileViewport}, types::*, window};
use crate::{
    asset_catalog::{EditorAssetCatalog, project_files::{EditorProjectFileCatalog, ProjectFileIdentity, ProjectFileNavigation}},
    resources::*, ui::sidebars::MapEditorSidebarState,
};
use bevy::{ecs::system::SystemParam, prelude::*, text::EditableText};
use feathrise_editor_ui::EditorButtonClick;

#[derive(SystemParam)]
pub(super) struct BrowserInputGate<'w, 's> {
    sidebar: Res<'w, MapEditorSidebarState>,
    layout: Res<'w, MapEditorLayoutState>,
    mouse: Res<'w, MapEditorNumericScrubMouseGuard>,
    hosted: Option<Res<'w, crate::workbench::WorkbenchModel>>,
    windows: Query<'w, 's, &'static Window>,
    settings: Option<Res<'w, MapEditorSettingsState>>,
    maps: Option<Res<'w, MapBrowserState>>,
    deletion: Option<Res<'w, MapDeletionTask>>,
    conflict: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
}

impl BrowserInputGate<'_, '_> {
    pub(super) fn available(&self, editor: &MapEditorState) -> bool {
        editor.open && !editor.close_requested && !editor.new_map_dialog_open
            && editor.pending_map_transition.is_none() && !self.mouse.blocked()
            && self.sidebar.assets_browser_visible() && self.layout.content_shown()
            && !self.windows.iter().any(|window| !window.focused)
            && !self.hosted.as_ref().is_some_and(|model| !model.visible || model.modal || model.busy)
            && !self.settings.as_ref().is_some_and(|settings| settings.open)
            && !self.maps.as_ref().is_some_and(|maps| maps.dialog.is_some())
            && !self.deletion.as_ref().is_some_and(|deletion| deletion.confirmation.is_some())
            && !self.conflict.as_ref().is_some_and(|conflict| conflict.modal_open())
    }
}

pub(super) fn update_active(
    editor: Res<MapEditorState>,
    state: Res<ProjectBrowserState>,
    sidebar: Res<MapEditorSidebarState>,
    layout: Res<MapEditorLayoutState>,
    mut files: ResMut<EditorProjectFileCatalog>,
) {
    let active = editor.open && !editor.close_requested && sidebar.assets_browser_visible()
        && layout.content_shown() && state.view == ProjectBrowserView::ProjectFiles;
    if files.active != active { files.active = active; }
}

pub(super) fn files(
    gate: BrowserInputGate,
    state: Res<ProjectBrowserState>,
    editor: Res<MapEditorState>,
    mut catalog: ResMut<EditorProjectFileCatalog>,
    mut assets: ResMut<EditorAssetCatalog>,
    viewport: Res<ProjectFileViewport>,
    scrolls: Query<(&ProjectBrowserContent, &ScrollPosition)>,
    fields: Query<Ref<EditableText>, With<ProjectFileFilter>>,
    buttons: Query<(&EditorButtonClick, &ProjectFileActionButton), Changed<EditorButtonClick>>,
    rows: Query<(&EditorButtonClick, &ProjectFileRow), Changed<EditorButtonClick>>,
    time: Option<Res<Time<Real>>>,
    documents: Option<Res<crate::ui::asset_preview::DocumentState>>,
    mut last_click: Local<Option<(ProjectFileIdentity, u64, u64, f64)>>,
    mut open: Option<ResMut<Messages<crate::ui::asset_preview::OpenProjectDocument>>>,
) {
    if state.view != ProjectBrowserView::ProjectFiles || !gate.available(&editor) {
        *last_click = None;
        return;
    }
    for input in &fields {
        if input.is_changed() && !input.is_composing() && input.pending_edits.is_empty() && input.pending_paste.is_none() {
            let value = input.value().to_string();
            if value != catalog.filter {
                assets.cancel_pending_reveal(); // 新目录输入优先于尚未完成的旧引用定位。
                catalog.set_filter(&value); // 只冻结短字符串，完整筛选留在后台目录槽。
            }
        }
    }
    for (click, button) in &buttons {
        if !click.is_clicked() { continue; }
        let navigation = match button.0 {
            ProjectFileAction::Up => ProjectFileNavigation::Up,
            ProjectFileAction::Refresh => ProjectFileNavigation::Refresh,
        };
        assets.cancel_pending_reveal();
        catalog.navigate(navigation);
        *last_click = None;
    }
    if catalog.is_loading() { return; } // 保留旧目录仅供阅读；新请求完成前不接受旧行的迟到释放。
    let Some((_, scroll)) = scrolls.iter().find(|(source, _)| source.0 == ProjectBrowserView::ProjectFiles) else { return; };
    for (click, row) in &rows {
        if !click.is_clicked() || row.revision != catalog.applied_revision || !window::current(&viewport, row, scroll) { continue; }
        let Some(index) = row.entry_index else { continue; };
        assets.cancel_pending_reveal(); // 主动行选择拥有最新导航意图，不能被后台旧定位抢回。
        let Some(identity) = catalog.select_entry(index) else { *last_click = None; continue; };
        let now = time.as_ref().map_or(0., |time| time.elapsed_secs_f64());
        let double = time.is_some() && last_click.as_ref().is_some_and(|(old, revision, window, seconds)|
            old == &identity && *revision == catalog.applied_revision && *window == viewport.revision
                && (0.0..=0.4).contains(&(now - seconds)));
        *last_click = if double { None } else { Some((identity.clone(), catalog.applied_revision, viewport.revision, now)) };
        if double && documents.is_some() && crate::asset_catalog::document_format(&identity.relative_path).is_some()
            && let Some(open) = open.as_mut() {
            open.write(crate::ui::asset_preview::OpenProjectDocument { identity });
        }
    }
}
