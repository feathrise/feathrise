//! 固定栏与活动窗口的右键扩展桥；只路由已冻结路径，收藏、引用和外部打开沿用原服务。

use crate::{
    asset_catalog::project_files::EditorProjectFileCatalog,
    resources::SceneEditorState,
    ui::{
        asset_preview::{AssetSearchState, DocumentState},
        asset_shortcuts::{AssetShortcuts, SetAssetFavorite},
        external_files::{ExternalEditors, OpenExternalFile, OpenExternalProjectFile, platform_path},
        file_manager::{ExtraAction, ExtraRequest, FileManagerState},
    },
    workbench::WorkbenchModel,
};
use bevy::prelude::*;
use std::path::{Component, Path, PathBuf};

/// 路径只做词法归属判断，真实文件和联接检查仍由原后台服务执行；不得在 UI 帧 canonicalize。
fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let root = platform_path(root);
    let path = platform_path(path);
    let relative = path.strip_prefix(root).ok()?;
    Some(if relative.as_os_str().is_empty() { ".".into() }
        else { relative.to_str()?.replace('\\', "/") })
}

pub(super) fn dispatch(
    mut requests: MessageReader<ExtraRequest>,
    mut manager: Option<ResMut<FileManagerState>>,
    mut editor: ResMut<SceneEditorState>,
    hosted: Option<Res<WorkbenchModel>>,
    documents: Option<Res<DocumentState>>,
    files: Res<EditorProjectFileCatalog>,
    mut clipboard: Option<ResMut<Clipboard>>,
    shortcuts: Option<Res<AssetShortcuts>>,
    mut search: Option<ResMut<AssetSearchState>>,
    available: Res<ExternalEditors>,
    mut external: MessageWriter<OpenExternalFile>,
    mut project_external: MessageWriter<OpenExternalProjectFile>,
    mut favorites: MessageWriter<SetAssetFavorite>,
) {
    let Some(request) = requests.read().last() else { return; };
    let Some(manager) = manager.as_deref_mut() else { return; };
    if !editor.open || editor.close_requested
        || hosted.as_ref().is_some_and(|model| !model.visible || model.modal || model.busy) { return; }
    // 宿主的显式空根表示当前无工程；不能悄悄回退到编辑器仓库。
    let (asset_root, project_root): (Option<PathBuf>, Option<PathBuf>) = if let Some(model) = hosted.as_ref() {
        (model.asset_root.clone(), model.project_root.clone())
    } else {
        let paths = feathrise_editor_core::editor_project_paths();
        (documents.as_ref().map(|documents| documents.asset_root().to_path_buf())
            .or_else(|| paths.map(|paths| paths.asset_root().to_path_buf())),
            files.project_root().map(Path::to_path_buf)
                .or_else(|| paths.map(|paths| paths.project_root().to_path_buf())))
    };
    let target = &request.target;
    let path = target.sources.first().unwrap_or(&target.directory);
    let valid = target.sources.len() <= 1 && path.is_absolute()
        && !path.components().any(|part| matches!(part, Component::CurDir | Component::ParentDir));
    let asset = asset_root.as_ref().and_then(|root| relative_to(root, path));
    let project = project_root.as_ref().and_then(|root| relative_to(root, path));
    let status = if !valid {
        "请选择一个有效文件或文件夹后再操作".into()
    } else {
        match request.action {
            ExtraAction::CopyAbsolute | ExtraAction::CopyRelative => {
                let value = if request.action == ExtraAction::CopyAbsolute {
                    platform_path(path).into_os_string().into_string().map_err(|_| "路径包含无法复制的字符")
                } else {
                    // 与原固定栏一致：相对路径以工程为根，资产文件保留 assets/ 等真实目录前缀。
                    project.clone().ok_or("当前文件在工程之外，无法生成工程相对路径")
                };
                match (value, clipboard.as_deref_mut()) {
                    (Ok(value), Some(clipboard)) => match clipboard.set_text(&value) {
                        Ok(()) => format!("已复制{}：{value}", if request.action == ExtraAction::CopyAbsolute { "绝对路径" } else { "相对路径" }),
                        Err(error) => format!("复制路径失败：{error}"),
                    },
                    (Err(error), _) => error.into(),
                    (_, None) => "系统文本剪贴板暂不可用".into(),
                }
            }
            ExtraAction::Favorite => {
                if let Some(relative) = asset.filter(|relative| relative != ".") {
                    let is_directory = target.is_directory == Some(true);
                    let favorite = !shortcuts.as_ref().is_some_and(|shortcuts| shortcuts.is_favorite(&relative, is_directory));
                    favorites.write(SetAssetFavorite { path: relative, is_directory, favorite });
                    if favorite { "已请求收藏此资产" } else { "已请求取消收藏" }.into()
                } else { "请在当前工程的资产目录中选择要收藏的文件或子目录".into() }
            }
            ExtraAction::References => {
                if let Some(relative) = asset.filter(|relative| relative != ".")
                    && target.is_directory == Some(false)
                    && let Some(search) = search.as_deref_mut()
                    && let Some(root) = asset_root
                {
                    search.request_references(relative, root);
                    "等待输入确认后查看直接引用…".into()
                } else { "请选择当前工程资产目录中的文件以查看引用".into() }
            }
            ExtraAction::External(action) => {
                if !available.visible(action, target.is_directory == Some(false)) {
                    "外部编辑器入口已不可用，请重新打开菜单".into()
                } else if let Some(relative) = asset {
                    let has_draft = documents.as_ref().is_some_and(|documents| documents.has_unsaved_draft(&relative));
                    external.write(OpenExternalFile { path: relative, action, has_draft });
                    "正在后台核验并打开文件…".into()
                } else if let Some(relative) = project && let Some(root) = project_root {
                    let has_draft = documents.as_ref().is_some_and(|documents| documents.has_unsaved_project_draft(&root, &relative));
                    project_external.write(OpenExternalProjectFile { root,
                        request: OpenExternalFile { path: relative, action, has_draft } });
                    "正在后台核验并打开工程文件…".into()
                } else { "当前文件在工程之外，可继续在文件窗口内整理".into() }
            }
        }
    };
    editor.status = status;
    manager.notice.clone_from(&editor.status);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_keep_asset_and_project_boundaries() {
        let root = std::env::temp_dir().join("file-menu-root");
        assert_eq!(relative_to(&root, &root), Some(".".into()));
        assert_eq!(relative_to(&root, &root.join("中文/模型.glb")), Some("中文/模型.glb".into()));
        assert!(relative_to(&root, &root.with_file_name("file-menu-root-other").join("file.md")).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn extended_windows_paths_share_the_authoritative_root() {
        assert_eq!(relative_to(Path::new(r"D:\工程\assets"), Path::new(r"\\?\D:\工程\assets\动画.glb")), Some("动画.glb".into()));
    }

    #[test]
    fn menu_services_keep_asset_and_project_scope() {
        use crate::ui::{external_files::ExternalFileAction, file_manager::FileManagerContextRequest};
        let project = std::env::temp_dir().join("file-menu-bridge-project");
        let assets = project.join("assets");
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<SceneEditorState>().init_resource::<FileManagerState>()
            .init_resource::<EditorProjectFileCatalog>().init_resource::<ExternalEditors>()
            .insert_resource(WorkbenchModel { project_root: Some(project.clone()), asset_root: Some(assets.clone()), ..default() })
            .add_message::<ExtraRequest>().add_message::<SetAssetFavorite>()
            .add_message::<OpenExternalFile>().add_message::<OpenExternalProjectFile>()
            .add_systems(Update, dispatch);
        app.world_mut().resource_mut::<SceneEditorState>().open = true;
        let target = |path: PathBuf| FileManagerContextRequest { position: Vec2::ZERO,
            directory: path.parent().unwrap().to_path_buf(), sources: vec![path],
            open_directory: assets.clone(), is_directory: Some(false) };
        app.world_mut().write_message(ExtraRequest { action: ExtraAction::Favorite, target: target(assets.join("模型.glb")) });
        app.update();
        let favorites: Vec<_> = app.world_mut().resource_mut::<Messages<SetAssetFavorite>>().drain().collect();
        assert_eq!(favorites.len(), 1);
        assert_eq!(favorites[0].path, "模型.glb");
        app.world_mut().write_message(ExtraRequest { action: ExtraAction::External(ExternalFileAction::Folder), target: target(project.join("src/main.rs")) });
        app.update();
        assert!(app.world_mut().resource_mut::<Messages<OpenExternalFile>>().drain().next().is_none());
        let requests: Vec<_> = app.world_mut().resource_mut::<Messages<OpenExternalProjectFile>>().drain().collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].root, project);
        assert_eq!(requests[0].request.path, "src/main.rs");
        // 宿主撤下根身份后，冻结的旧菜单也不能把资产送入原工程的收藏服务。
        app.world_mut().resource_mut::<WorkbenchModel>().asset_root = None;
        app.world_mut().write_message(ExtraRequest { action: ExtraAction::Favorite, target: target(assets.join("模型.glb")) });
        app.update();
        assert!(app.world_mut().resource_mut::<Messages<SetAssetFavorite>>().drain().next().is_none());
    }
}
