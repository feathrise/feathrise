//! 固定文件栏与桌面窗口的桥；只读取当前可见行和已采用快照，文件读写交给统一后台。

use super::{ProjectBrowserContent, ProjectBrowserState, ProjectBrowserView, ProjectFileRow, ProjectFileViewport};
use crate::{
    asset_catalog::{EditorAssetCatalog, project_files::EditorProjectFileCatalog},
    components::{AssetRowBinding, SceneEditorAssetRow},
    resources::SceneEditorState,
    ui::file_manager::{FileManagerExternalDrag, FixedFileSelection},
    workbench::{WorkbenchFloatingPanels, WorkbenchModel},
};
use bevy::{prelude::*, ui::RelativeCursorPosition};
use feathrise_editor_ui::UiInteraction;

/// 固定文件栏的键盘入口只在其实际悬停且没有文本焦点时消费，避免抢走中央源码的文本剪贴板。
pub(super) fn fixed_shortcuts(
    fixed: Option<Res<FixedFileSelection>>,
    manager: Option<Res<crate::ui::file_manager::FileManagerState>>,
    focus: Option<Res<bevy::input_focus::InputFocus>>,
    text: Query<(), With<bevy::text::EditableText>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut requests: MessageWriter<crate::ui::file_manager::FileManagerCommand>,
) {
    let Some(fixed) = fixed.filter(|fixed| fixed.hovered && !fixed.loading && !fixed.directory.as_os_str().is_empty()) else { return; };
    if manager.as_ref().is_some_and(|manager| manager.owns_keyboard())
        || focus.as_ref().and_then(|focus| focus.get()).is_some_and(|entity| text.contains(entity)) { return; }
    use crate::ui::file_manager::FileAction;
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let candidates = [(KeyCode::KeyC, control.then_some(FileAction::Copy)),
        (KeyCode::KeyX, control.then_some(FileAction::Cut)), (KeyCode::KeyV, control.then_some(FileAction::Paste)),
        (KeyCode::KeyN, control.then_some(if shift { FileAction::NewFolder } else { FileAction::NewWindow })),
        (KeyCode::Delete, Some(FileAction::Delete)), (KeyCode::F2, Some(FileAction::Rename))];
    for (key, action) in candidates {
        if let Some(action) = action.filter(|_| keys.just_pressed(key)) {
            requests.write(crate::ui::file_manager::FileManagerCommand {
                action, path: Some(fixed.directory.clone()), sources: fixed.selected.clone(),
            });
            keys.clear_just_pressed(key); // 原场景快捷键不得再消费同一文件整理边沿。
        }
    }
}

/// 固定栏保留资产预览身份；整理工具条只接收真实绝对路径，不能把 GLB # 子资产当作文件。
pub(super) fn sync_fixed_selection(
    editor: Res<SceneEditorState>,
    browser: Res<ProjectBrowserState>,
    gate: super::input::BrowserInputGate,
    files: Res<EditorProjectFileCatalog>,
    assets: Res<EditorAssetCatalog>,
    viewport: Res<ProjectFileViewport>,
    asset_viewport: crate::ui::rows::AssetViewport,
    hosted: Option<Res<WorkbenchModel>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pages: Query<(&ProjectBrowserContent, &RelativeCursorPosition, &InheritedVisibility, &ScrollPosition)>,
    project_rows: Query<(&ProjectFileRow, &UiInteraction, &InheritedVisibility)>,
    asset_rows: Query<(&SceneEditorAssetRow, &UiInteraction, &InheritedVisibility, Option<&AssetRowBinding>)>,
    floating: WorkbenchFloatingPanels,
    io: (Option<ResMut<FixedFileSelection>>, Option<ResMut<Messages<FileManagerExternalDrag>>>,
        Option<ResMut<Messages<crate::ui::file_manager::FileManagerContextRequest>>>,
        Local<Option<(ProjectBrowserView, std::path::PathBuf, u64, Option<std::path::PathBuf>, u64)>>),
    roots: Query<(&ComputedNode, &UiGlobalTransform), With<crate::components::SceneEditorRoot>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let (fixed, drags, contexts, mut observed) = io;
    let Some(mut fixed) = fixed else { return; }; // 不安装桌面模块的独立目录夹具仍可复用原浏览器。
    let active = gate.context_available(&editor); // 设置、管理对话框、失焦和宿主模态沿用原固定浏览器资格。
    let page = pages.iter().find(|(page, _, visible, _)| visible.get() && page.0 == browser.view);
    let hovered = active && floating.hovered().is_none()
        && page.is_some_and(|(_, cursor, _, _)| cursor.cursor_over);
    fixed.hovered = hovered;
    fixed.hover_destination = None;
    if !active { return; }
    let asset_root = hosted.as_ref().and_then(|model| model.asset_root.clone())
        .unwrap_or_else(feathrise_editor_core::workspace_asset_root);
    let (directory, selected) = match browser.view {
        ProjectBrowserView::ProjectFiles => {
            let Some(root) = files.project_root() else { fixed.directory.clear(); fixed.selected.clear(); return; };
            (root.join(&files.current_folder), files.selected_file.as_ref()
                .filter(|identity| identity.context == files.source_context().unwrap_or(0))
                .map(|identity| root.join(&identity.relative_path)))
        }
        ProjectBrowserView::Assets => (asset_root.join(&assets.current_folder), editor.selected_asset.as_ref()
            .map(|path| asset_root.join(path.split('#').next().unwrap_or(path)))),
    };
    let revision = match browser.view { ProjectBrowserView::Assets => assets.applied_revision,
        ProjectBrowserView::ProjectFiles => files.applied_revision };
    let identity = (browser.view, directory.clone(), files.source_context().unwrap_or(0), selected.clone(), revision);
    if observed.as_ref() != Some(&identity) {
        // 预览身份真正变化时才跟随；右键改选或清空后，旧预览不能在下一帧抢回整理来源。
        let selection_changed = observed.as_ref().is_none_or(|previous| previous.0 != identity.0
            || previous.1 != identity.1 || previous.2 != identity.2 || previous.3 != identity.3);
        // 同目录的筛选/刷新换代取消旧整理选择，避免快捷键命中已隐藏或删除的文件；不在主帧扫描整份快照。
        fixed.selected = if selection_changed { selected.filter(|path| path.parent() == Some(directory.as_path())).into_iter().collect() }
            else { Vec::new() };
        *observed = Some(identity);
    }
    fixed.directory = directory;
    fixed.asset_view = browser.view == ProjectBrowserView::Assets;
    fixed.loading = match browser.view { ProjectBrowserView::Assets => assets.is_loading(),
        ProjectBrowserView::ProjectFiles => files.is_loading() };
    if fixed.asset_view { fixed.asset_folder.clone_from(&assets.current_folder); }
    if !hovered { return; }
    // 悬停目标和按下来源共用相同行身份检查；提示的子目录就是最后真正提交的目标。
    let entry = match browser.view {
        ProjectBrowserView::ProjectFiles if !files.is_loading() => page.and_then(|(_, _, _, scroll)| {
            project_rows.iter().find_map(|(row, interaction, visible)| {
                (visible.get() && *interaction != UiInteraction::None
                    && row.revision == files.applied_revision && super::window::current(&viewport, row, scroll))
                    .then(|| row.entry_index.and_then(|index| files.entries.get(index)))
                    .flatten().and_then(|entry| files.project_root().map(|root| (root.join(&entry.asset_path), entry.is_directory)))
            })
        }),
        ProjectBrowserView::Assets if !assets.is_loading() && asset_viewport.is_current(&assets) => asset_rows.iter().find_map(|(row, interaction, visible, binding)| {
            (visible.get() && *interaction != UiInteraction::None).then(|| row.entry_index
                // 拖出前核对目录、视口与行池身份，不能将尚未换绑的旧行作为移动源。
                .filter(|index| binding.is_some_and(|binding| binding.revision == assets.applied_revision
                    && binding.viewport_revision == assets.viewport_revision && binding.index == Some(*index)
                    && row.slot < assets.visible_count && *index == assets.visible_start + row.slot))
                .and_then(|index| assets.entries.get(index))).flatten().map(|entry| (asset_root.join(&entry.asset_path), entry.is_directory))
        }),
        _ => None,
    };
    fixed.hover_destination = entry.as_ref().filter(|(_, directory)| *directory).map(|(path, _)| path.clone());
    if mouse.just_pressed(MouseButton::Right) {
        let sources = entry.as_ref().map(|(path, _)| vec![path.clone()]).unwrap_or_default();
        fixed.selected.clone_from(&sources);
        let open_directory = entry.as_ref().filter(|(_, directory)| *directory)
            .map_or_else(|| fixed.directory.clone(), |(path, _)| path.clone());
        if let Some(mut contexts) = contexts
            && let Ok((root, transform)) = roots.single()
            && let Some(cursor) = windows.single().ok().and_then(Window::physical_cursor_position)
            && let Some(position) = crate::ui::asset_context_menu::helpers::logical_anchor(cursor,
                Rect::from_center_size(transform.affine().translation, root.size()), root.inverse_scale_factor())
        {
            contexts.write(crate::ui::file_manager::FileManagerContextRequest { position,
                directory: fixed.directory.clone(), sources, open_directory,
                is_directory: entry.as_ref().map(|(_, directory)| *directory).or(Some(true)) });
        }
    }
    // 按下就冻结路径；超过拖动阈值和真正释放的判定由桌面统一处理，换绑行不能换掉来源。
    if mouse.just_pressed(MouseButton::Left) {
        fixed.selected = entry.as_ref().map(|(source, _)| source.clone()).into_iter().collect();
        if let Some((source, _)) = entry && let Some(mut drags) = drags {
            drags.write(FileManagerExternalDrag { sources: vec![source] });
        }
    }
}

/// 窗口外的固定栏也刷新真实目录；最多读取最近32个轻量回执，不依赖某个窗口仍然打开。
pub(super) fn refresh_fixed_after_operation(
    service: Option<Res<feathrise_editor_project_browser_model::file_operations::FileOperationService>>,
    mut files: ResMut<EditorProjectFileCatalog>,
    mut assets: ResMut<EditorAssetCatalog>,
    mut editor: ResMut<SceneEditorState>,
    manager: Option<Res<crate::ui::file_manager::FileManagerState>>,
    mut last_receipt: Local<u64>,
) {
    let Some(manager) = manager else { return; };
    let Some(snapshot) = service.and_then(|service| service.snapshot()) else { return; };
    let Some(latest) = snapshot.completed.iter().filter(|receipt| receipt.context == manager.service_context())
        .map(|receipt| receipt.id).max().filter(|id| *id > *last_receipt) else { return; };
    *last_receipt = latest;
    files.navigate(crate::asset_catalog::project_files::ProjectFileNavigation::Refresh);
    crate::asset_catalog::navigation::navigate(crate::types::EditorAction::RefreshAssets, &mut assets, &mut editor);
    editor.status = manager.notice.clone();
}
