//! 工程文件打开桥：消费有工程身份的双击意图，复用中央文本会话。

use bevy::prelude::*;
use crate::{
    asset_catalog::{document_format, project_files::{EditorProjectFileCatalog, ProjectFileIdentity}},
    resources::SceneEditorState,
    ui::vegetation_preset_editor::VegetationPresetEditorState,
    workbench::WorkbenchModel,
};
use super::{resources::DocumentState, source};

#[derive(Message, Clone)]
pub(crate) struct OpenProjectDocument { pub(crate) identity: ProjectFileIdentity }

pub(super) fn install(app: &mut App) {
    app.init_resource::<EditorProjectFileCatalog>();
    app.add_message::<OpenProjectDocument>()
        .add_message::<crate::ui::file_manager::OpenManagedFile>()
        .add_systems(Update, open_managed
            .after(crate::ui::file_manager::FileManagerInput)
            .before(super::session::sync_selection).in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(Update, open.after(crate::ui::project_browser::ProjectBrowserTaskSet)
            .after(crate::ui::project_browser::ProjectBrowserRoutingSet)
            .before(super::session::sync_selection).in_set(crate::SceneEditorSystemSet::Tasks));
}

/// 桌面窗口已经冻结绝对文件路径；打开文档不依赖固定栏是否正显示该目录。
fn open_managed(
    mut requests: MessageReader<crate::ui::file_manager::OpenManagedFile>,
    model: Option<Res<WorkbenchModel>>,
    mut documents: ResMut<DocumentState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut editor: ResMut<SceneEditorState>,
    manager: Option<ResMut<crate::ui::file_manager::FileManagerState>>,
    mut media: Option<ResMut<super::super::AssetPreviewWorkspace>>,
    preview: Option<Res<super::super::AssetPreviewState>>,
    navigation: Option<Res<super::super::AssetPreviewNavigation>>,
    mut sidebar: Option<ResMut<crate::ui::sidebars::SceneEditorSidebarState>>,
    inputs: Query<&bevy::text::EditableText>,
) {
    let Some(path) = requests.read().last().map(|request| request.path.clone()) else { return; };
    let Some(mut manager) = manager else { return; }; // 文档库的独立夹具不必安装桌面窗口。
    if !editor.open || editor.close_requested || model.as_ref().is_some_and(|model| model.modal || model.busy) { return; }
    if inputs.iter().any(|text| text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()) {
        manager.notice = "请先确认文字输入或等待粘贴完成，再打开文件".into(); return;
    }
    let bound = feathrise_editor_core::editor_project_paths();
    let root = model.as_ref().and_then(|model| model.project_root.as_deref())
        .or_else(|| bound.map(|paths| paths.project_root()));
    let assets = model.as_ref().and_then(|model| model.asset_root.as_deref())
        .or_else(|| bound.map(|paths| paths.asset_root()));
    // 地址栏可输入普通盘符路径；工程根保持权威身份，只对包含关系统一 Windows 扩展前缀。
    let display_path = crate::ui::external_files::platform_path(&path);
    let display_root = root.map(crate::ui::external_files::platform_path);
    let Some((root, relative)) = root.zip(display_root.as_deref()).and_then(|(root, display_root)|
        display_path.strip_prefix(display_root).ok().map(|relative| (root, relative))) else {
        manager.notice = "此文件位于工程外，可在文件窗口整理；内置文档与媒体预览仅打开当前工程文件".into(); return;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    let result = documents.bind_project_source(root, assets).and_then(|_| {
        source::project_key(documents.asset_root(), root, &relative)
    }).and_then(|key| {
        if document_format(&relative).is_some() {
            documents.expand(key, &mut workspace)
        } else if !source::is_project(&key) && super::super::workspace::media_path(&key) {
            let (Some(media), Some(preview), Some(navigation)) = (media.as_deref_mut(), preview.as_deref(), navigation.as_deref()) else {
                return Err("当前工作台没有安装媒体预览".into());
            };
            editor.selected_asset = Some(key.clone());
            if let Some(sidebar) = sidebar.as_deref_mut() { sidebar.reveal_asset_properties(); }
            super::super::workspace::expand(media, &mut workspace, &key, preview, navigation)
        } else { Err("此文件类型没有内置预览，可继续在文件窗口复制、移动或重命名".into()) }
    });
    if let Err(error) = result { manager.notice = error; }
}

fn current(identity: &ProjectFileIdentity, catalog: &EditorProjectFileCatalog) -> bool {
    catalog.active && catalog.selected_file.as_ref() == Some(identity)
        && catalog.project_root() == Some(identity.project_root.as_path())
}

fn open(
    mut requests: MessageReader<OpenProjectDocument>,
    catalog: Option<Res<EditorProjectFileCatalog>>,
    model: Option<Res<WorkbenchModel>>,
    mut documents: ResMut<DocumentState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut editor: ResMut<SceneEditorState>,
    sources: Query<&bevy::text::EditableText, With<super::components::DocumentSource>>,
) {
    let Some(identity) = requests.read().last().map(|request| request.identity.clone()) else { return; };
    let Some(catalog) = catalog else { return; };
    if !editor.open || editor.close_requested || editor.new_scene_dialog_open
            || model.as_ref().is_some_and(|model| model.modal || model.busy)
            || !current(&identity, &catalog) { return; }
    if sources.iter().any(|text| text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()) {
        editor.status = "请先确认中文输入或等待粘贴完成，再打开工程文件".into();
        return;
    }
    let bound_paths = feathrise_editor_core::editor_project_paths();
    let (expected, assets) = if let Some(model) = &model {
        if model.context != identity.context { return; }
        (model.project_root.as_deref(), model.asset_root.as_deref())
    } else {
        (bound_paths.map(|paths| paths.project_root()), bound_paths.map(|paths| paths.asset_root()))
    };
    if expected != Some(identity.project_root.as_path()) { return; }
    let result = documents.bind_project_source(&identity.project_root, assets).and_then(|_| {
        source::project_key(documents.asset_root(), &identity.project_root, &identity.relative_path)
    });
    let key = match result { Ok(key) => key, Err(error) => { editor.status = error; return; } };
    if document_format(&identity.relative_path).is_some() {
        if let Err(error) = documents.expand(key, &mut workspace) { editor.status = error; }
    } else {
        editor.status = "此文件不支持内置文本编辑，可使用外部IDE打开".into();
    }
}
