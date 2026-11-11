//! 模式按钮、当前页身份与唯一后台检查槽；迟到任务不改变UI或资产。

use super::{
    components::ViewModeButton, constants::*, io::inspect_page, resources::ThumbnailState, types::*,
};
use crate::resources::{
    EditorAssetCatalog, SceneBrowserState, SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard,
    SceneEditorState,
};
use crate::ui::{
    project_browser::ProjectBrowserState,
    sidebars::SceneEditorSidebarState,
};
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, futures::check_ready};
use feathrise_scene_content::SceneAssetKind;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::AssetBrowserViewMode;

pub(super) fn control_view_mode(
    buttons: Query<&feathrise_editor_ui::EditorButtonClick, (With<ViewModeButton>, Changed<feathrise_editor_ui::EditorButtonClick>)>,
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    layout: Option<Res<SceneEditorLayoutState>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    browser: Option<Res<SceneBrowserState>>,
    project_browser: Option<Res<ProjectBrowserState>>,
    mut settings: ResMut<EditorSettings>,
) {
    // 每帧运行并消费Changed；模态、数值拖动和菜单关闭帧的点击不会在解锁后重放。
    if !editor.open
        || editor.new_scene_dialog_open
        || !sidebar.assets_browser_visible()
        || project_browser.as_ref().is_some_and(|browser| !browser.assets_visible())
        || layout.as_ref().is_some_and(|layout| !layout.content_shown())
        || guard.blocked()
        || browser
            .as_ref()
            .is_some_and(|browser| browser.dialog.is_some())
    {
        return;
    }
    if buttons.iter().any(|click| click.is_clicked()) {
        // 同一入口从当前偏好反转；沿用原保存字段以及列表、缩略图共用的目录和选择。
        settings.asset_browser_view_mode = match settings.asset_browser_view_mode {
            AssetBrowserViewMode::List => AssetBrowserViewMode::Thumbnails,
            AssetBrowserViewMode::Thumbnails => AssetBrowserViewMode::List,
        };
    }
}

pub(super) fn sync_demand(
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    layout: Res<SceneEditorLayoutState>,
    settings: Res<EditorSettings>,
    catalog: Res<EditorAssetCatalog>,
    viewport: crate::ui::rows::AssetViewport,
    browser: Option<Res<crate::ui::project_browser::ProjectBrowserState>>,
    time: Res<Time<Real>>,
    mut state: ResMut<ThumbnailState>,
) {
    let enabled = editor.open
        && !editor.close_requested
        && layout.content_shown()
        && sidebar.assets_browser_visible()
        && browser.as_ref().is_none_or(|browser| browser.assets_visible())
        && settings.asset_browser_view_mode == AssetBrowserViewMode::Thumbnails
        && catalog.requested_revision == catalog.applied_revision && viewport.is_current(&catalog);
    let start = catalog.visible_start;
    let count = catalog.visible_count;
    let identity = enabled.then(|| WindowKey {
        folder: catalog.current_folder.clone(),
        start, count,
        revision: catalog.applied_revision,
    });
    if identity != state.identity {
        state.epoch = state.epoch.wrapping_add(1); // 关闭、换页和目录换代都撤销旧检查及GPU任务。
        state.inspect_after = 0.0;
        state.paths.clear();
        if let Some(key) = &identity {
            let refresh = state
                .last_catalog
                .as_ref()
                .is_some_and(|old| old.folder == key.folder && old.revision != key.revision);
            state.paths = catalog
                .entries
                .iter()
                .skip(start)
                .take(count)
                .filter(|entry| {
                    !entry.is_directory
                        && matches!(entry.kind, SceneAssetKind::Scene | SceneAssetKind::Texture)
                })
                .map(|entry| entry.asset_path.clone())
                .collect();
            if refresh {
                let paths = state.paths.clone();
                for path in paths {
                    if let Some(entry) = state.cache.get_mut(&path)
                        && entry.source.is_ok()
                        && matches!(entry.result, ThumbnailResult::Failed(_))
                    {
                        entry.result = ThumbnailResult::Queued;
                        state.reload_paths.insert(path); // 显式刷新可重试已知源的加载失败/超时。
                    }
                }
            }
            state.used = state.used.wrapping_add(1);
            let used = state.used;
            for path in state.paths.clone() {
                if let Some(entry) = state.cache.get_mut(&path) {
                    entry.used = used;
                }
            }
            state.last_catalog = Some(key.clone());
        }
        state.identity = identity;
        state.changed();
    }
    if let Some(result) = state.inspection.as_mut().and_then(check_ready) {
        state.inspection = None; // 旧任务真正结束才释放单飞槽，不假定取消句柄能抢占IO。
        if state.identity.is_some() && result.epoch == state.epoch {
            adopt_inspection(&mut state, result);
        }
    }
    let now = time.elapsed_secs_f64();
    if state.identity.is_none()
        || state.inspection.is_some()
        || now < state.inspect_after
        || state.paths.is_empty()
    {
        return;
    }
    let inputs = catalog
        .entries
        .iter()
        .skip(start)
        .take(count)
        .filter(|entry| state.paths.contains(&entry.asset_path))
        .map(|entry| InspectionInput {
            path: entry.asset_path.clone(),
            kind: entry.kind,
            previous: state
                .cache
                .get(&entry.asset_path)
                .and_then(|entry| entry.source.as_ref().ok())
                .cloned(),
        })
        .collect();
    let root = feathrise_editor_core::workspace_asset_root(); // 缩略图检查与AssetServer加载必须读取同一工程。
    let epoch = state.epoch;
    state.inspection =
        Some(IoTaskPool::get().spawn(async move { inspect_page(root, epoch, inputs) }));
    state.inspect_after = now + RECHECK_SECONDS;
}

pub(super) fn adopt_inspection(state: &mut ThumbnailState, result: InspectionResult) {
    if result.epoch != state.epoch || state.identity.is_none() {
        return;
    }
    for (path, source) in result.sources {
        if !state.paths.contains(&path)
            || state
                .cache
                .get(&path)
                .is_some_and(|entry| entry.source == source)
        {
            continue;
        }
        if !state.make_room(&path) { continue; } // 当前超大视口也不能突破64张快照预算，其他卡片保留类型占位。
        if source.is_ok() && state.cache.contains_key(&path) {
            state.reload_paths.insert(path.clone());
        }
        let outcome = match &source {
            Ok(_) => ThumbnailResult::Queued,
            Err(error) => ThumbnailResult::Failed(error.clone()),
        };
        state.used = state.used.wrapping_add(1);
        let used = state.used;
        state.cache.insert(
            path,
            CacheEntry {
                source,
                result: outcome,
                used,
            },
        );
        state.changed();
    }
}
