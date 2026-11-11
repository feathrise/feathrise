//! 把原资产浏览器接到显式工程根；目录算法、固定行、文件选择和属性回显都使用原实现。
use super::WorkbenchModel;
use crate::{
    asset_catalog::{EditorAssetCatalog, navigation},
    components::SceneEditorActionButton,
    resources::{SceneEditorLayoutState, SceneEditorState},
    ui::sidebars::SceneEditorSidebarState,
};
use bevy::prelude::*;
use feathrise_editor_ui::EditorButtonClick;

/// 浏览来源只读取启动时已核验的显式路径；无工程的工作台不能回退浏览编辑器源码。
pub(crate) fn bind_project_files_source(
    model: Option<Res<WorkbenchModel>>,
    mut files: ResMut<crate::asset_catalog::project_files::EditorProjectFileCatalog>,
) {
    let (context, root, asset_root) = if let Some(model) = model {
        (model.context, model.project_root.clone(), model.asset_root.clone())
    } else {
        let paths = feathrise_editor_core::editor_project_paths();
        (0, paths.as_ref().map(|paths| paths.project_root().to_path_buf()),
            paths.as_ref().map(|paths| paths.asset_root().to_path_buf()))
    };
    files.bind_project_source(context, root);
    files.set_asset_root(asset_root);
}

pub(super) fn bind_source(
    model: Res<WorkbenchModel>,
    mut catalog: ResMut<EditorAssetCatalog>,
    mut editor: ResMut<SceneEditorState>,
) {
    if catalog.bind_project_source(model.context, model.asset_root.clone()) {
        editor.selected_asset = None; // 同名文件也属于不同工程，不带走旧属性或选择。
    }
}

pub(super) fn navigate(
    model: Res<WorkbenchModel>,
    sidebar: Res<SceneEditorSidebarState>,
    layout: Res<SceneEditorLayoutState>,
    mut catalog: ResMut<EditorAssetCatalog>,
    mut editor: ResMut<SceneEditorState>,
    buttons: Query<(&SceneEditorActionButton, &EditorButtonClick), Changed<EditorButtonClick>>,
) {
    if !model.visible
        || model.modal
        || model.busy
        || model.asset_root.is_none()
        || !sidebar.assets_browser_visible()
        || !layout.content_shown()
    {
        return; // 系统仍每帧运行以推进Changed边界，模态期间的释放不会延后重放。
    }
    for (button, click) in &buttons {
        if click.is_clicked() && navigation::is_navigation(button.0) {
            navigation::navigate(button.0, &mut catalog, &mut editor);
        }
    }
}
