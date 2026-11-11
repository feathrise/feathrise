//! 一体与独立工作台共用原资产目录按钮语义；不复制面板或重新实现分页。
use super::EditorAssetCatalog;
use crate::{resources::SceneEditorState, types::EditorAction};

pub(crate) fn is_navigation(action: EditorAction) -> bool {
    matches!(
        action,
        EditorAction::RefreshAssets
            | EditorAction::AssetFolderUp
    )
}

pub(crate) fn navigate(
    action: EditorAction,
    catalog: &mut EditorAssetCatalog,
    editor: &mut SceneEditorState,
) {
    match action {
        EditorAction::RefreshAssets => {
            catalog.worker.request_refresh();
            catalog.requested_revision = catalog.requested_revision.wrapping_add(1);
            editor.status = format!("正在刷新 assets/{}", catalog.current_folder);
        }
        EditorAction::AssetFolderUp => {
            catalog.current_folder = catalog
                .current_folder
                .rsplit_once('/')
                .map_or_else(String::new, |(parent, _)| parent.to_owned());
            catalog.filename_filter.clear();
            catalog.page = 0;
            catalog.requested_revision = catalog.requested_revision.wrapping_add(1);
        }
        EditorAction::AssetPreviousPage | EditorAction::AssetNextPage => {} // 旧枚举保留读取兼容，不再生成或执行分页。
        _ => {}
    }
}
