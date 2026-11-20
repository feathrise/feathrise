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
use feathrise_editor_project_browser_model::file_operations::FileSources;
use feathrise_editor_ui::UiInteraction;
use std::path::Path;

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
    let (root, directory, selected) = match browser.view {
        ProjectBrowserView::ProjectFiles => {
            let Some(root) = files.project_root() else { fixed.directory.clear(); fixed.selected.clear(); return; };
            (root, root.join(&files.current_folder), files.selected_file.as_ref()
                .filter(|identity| identity.context == files.source_context().unwrap_or(0))
                .map(|identity| root.join(&identity.relative_path)))
        }
        ProjectBrowserView::Assets => (asset_root.as_path(), asset_root.join(&assets.current_folder), editor.selected_asset.as_ref()
            .map(|path| asset_root.join(path))),
    };
    let revision = match browser.view { ProjectBrowserView::Assets => assets.applied_revision,
        ProjectBrowserView::ProjectFiles => files.applied_revision };
    let identity = (browser.view, directory.clone(), files.source_context().unwrap_or(0), selected.clone(), revision);
    if observed.as_ref() != Some(&identity) {
        // 预览身份真正变化时才跟随；右键改选或清空后，旧预览不能在下一帧抢回整理来源。
        let selection_changed = observed.as_ref().is_none_or(|previous| previous.0 != identity.0
            || previous.1 != identity.1 || previous.2 != identity.2 || previous.3 != identity.3);
        // 同目录的筛选/刷新换代取消旧整理选择，避免快捷键命中已隐藏或删除的文件；不在主帧扫描整份快照。
        fixed.selected = selected.filter(|path| selection_changed && path.parent() == Some(directory.as_path())).and_then(|path| {
            // 资产预览可能显示 GLB 子资产。按下已从物理行冻结文件，反向定位也有实际目录行的回执。
            // 只以这两种来源确认文件身份，原样保留文件名中的 #。
            if observed.as_ref().is_some_and(|previous| previous.0 == identity.0 && previous.1 == identity.1
                && previous.2 == identity.2 && previous.4 == identity.4 && fixed.selected.contains(&path))
            {
                return Some(fixed.selected.clone());
            }
            let stamp = match browser.view {
                ProjectBrowserView::ProjectFiles => files.selected_stamp()?,
                ProjectBrowserView::Assets => assets.revealed_entry.filter(|(rev, _)| *rev == revision)
                    .and_then(|(_, index)| assets.entries.get(index))
                    .filter(|entry| !entry.is_directory && root.join(&entry.asset_path) == path)?
                    .metadata.stamp,
            };
            Some(FileSources::single(path, stamp))
        }).unwrap_or_default();
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
                    .flatten()
            })
        }),
        ProjectBrowserView::Assets if !assets.is_loading() && asset_viewport.is_current(&assets) => asset_rows.iter().find_map(|(row, interaction, visible, binding)| {
            (visible.get() && *interaction != UiInteraction::None).then(|| row.entry_index
                // 拖出前核对目录、视口与行池身份，不能将尚未换绑的旧行作为移动源。
                .filter(|index| binding.is_some_and(|binding| binding.revision == assets.applied_revision
                    && binding.viewport_revision == assets.viewport_revision && binding.index == Some(*index)
                    && row.slot < assets.visible_count && *index == assets.visible_start + row.slot))
                .and_then(|index| assets.entries.get(index))).flatten()
        }),
        _ => None,
    };
    fixed.hover_destination = entry.filter(|entry| entry.is_directory).map(|entry| root.join(&entry.asset_path));
    let right = mouse.just_pressed(MouseButton::Right);
    let left = mouse.just_pressed(MouseButton::Left);
    // 按下冻结物理路径和目录扫描时的身份，拖放阈值及释放仍由桌面统一处理。
    if left || right {
        fixed.selected = entry.map(|entry| FileSources::single(root.join(&entry.asset_path), entry.metadata.stamp)).unwrap_or_default();
    }
    if right {
        let open_directory = fixed.hover_destination.clone().unwrap_or_else(|| fixed.directory.clone());
        if let Some(mut contexts) = contexts
            && let Ok((root, transform)) = roots.single()
            && let Some(cursor) = windows.single().ok().and_then(Window::physical_cursor_position)
            && let Some(position) = crate::ui::asset_context_menu::helpers::logical_anchor(cursor,
                Rect::from_center_size(transform.affine().translation, root.size()), root.inverse_scale_factor())
        {
            contexts.write(crate::ui::file_manager::FileManagerContextRequest { position,
                directory: fixed.directory.clone(), sources: fixed.selected.clone(), open_directory,
                is_directory: Some(entry.is_none_or(|entry| entry.is_directory)) });
        }
    }
    if left && !fixed.selected.is_empty() && let Some(mut drags) = drags {
        drags.write(FileManagerExternalDrag { sources: fixed.selected.clone() });
    }
}

/// 窗口外的固定栏也刷新真实目录；最多读取最近32个轻量回执，不依赖某个窗口仍然打开。
pub(super) fn refresh_fixed_after_operation(
    mut files: ResMut<EditorProjectFileCatalog>,
    mut assets: ResMut<EditorAssetCatalog>,
    mut editor: ResMut<SceneEditorState>,
    manager: Option<Res<crate::ui::file_manager::FileManagerState>>,
    hosted: Option<Res<WorkbenchModel>>,
    mut last_receipt: Local<u64>,
) {
    let Some(manager) = manager else { return; };
    let Some(latest) = manager.receipts().next_back().map(|receipt| receipt.id)
        .filter(|id| *id > *last_receipt) else { return; };
    let file_path = files.project_root().map(|root| root.join(&files.current_folder));
    let asset_path = hosted.as_ref().map_or_else(|| Some(feathrise_editor_core::workspace_asset_root()),
        |host| host.asset_root.clone()).map(|root| root.join(&assets.current_folder));
    let previous = *last_receipt;
    *last_receipt = latest;
    let affected = |path: Option<&Path>| path.is_some_and(|path|
        manager.receipts().any(|receipt| receipt.id > previous && receipt.affects_directory(path)));
    if affected(file_path.as_deref()) {
        files.navigate(crate::asset_catalog::project_files::ProjectFileNavigation::Refresh);
    }
    if affected(asset_path.as_deref()) {
        crate::asset_catalog::navigation::navigate(crate::types::EditorAction::RefreshAssets, &mut assets, &mut editor);
    }
    editor.status = manager.notice.clone();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        resources::{SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard},
        types::EditorAssetEntry,
        ui::{file_manager::FileManagerCommand, sidebars::{EditorNavigatorTab, SceneEditorSidebarState}},
    };
    use feathrise_scene_content::SceneAssetKind;
    use feathrise_editor_project_browser_model::{FileEntryMetadata, file_operations::FileEntryStamp};
    use std::path::PathBuf;

    #[test]
    fn fixed_sidebar_cut_keeps_literal_hash_from_accepted_physical_row() {
        let root = PathBuf::from("C:/fixture/assets");
        let filename = "a.glb#备份.glb";
        let mut assets = EditorAssetCatalog::default();
        assets.entries = ["a.glb", filename].into_iter().map(|name| EditorAssetEntry {
            asset_path: name.into(), display_name: name.into(), is_directory: false, kind: SceneAssetKind::Scene,
            metadata: FileEntryMetadata { stamp: FileEntryStamp::Missing, ..default() },
        }).collect();
        assets.visible_count = assets.entries.len();
        let mut sidebar = SceneEditorSidebarState::default();
        sidebar.navigator = EditorNavigatorTab::Assets;
        let mut app = App::new();
        app.insert_resource(SceneEditorState { open: true, selected_asset: Some("其他.glb".into()), ..default() })
            .insert_resource(ProjectBrowserState { view: ProjectBrowserView::Assets, ..default() })
            .insert_resource(WorkbenchModel { asset_root: Some(root.clone()), visible: true, ..default() })
            .insert_resource(sidebar).insert_resource(assets)
            .init_resource::<FixedFileSelection>().init_resource::<EditorProjectFileCatalog>()
            .init_resource::<ProjectFileViewport>().init_resource::<SceneEditorLayoutState>()
            .init_resource::<SceneEditorNumericScrubMouseGuard>().init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>().add_message::<FileManagerCommand>()
            .add_systems(Update, (sync_fixed_selection, fixed_shortcuts).chain());
        app.world_mut().spawn((ProjectBrowserContent(ProjectBrowserView::Assets),
            RelativeCursorPosition { cursor_over: true, ..default() }, InheritedVisibility::VISIBLE, ScrollPosition::default()));
        app.world_mut().spawn((SceneEditorAssetRow { slot: 1, entry_index: Some(1) },
            AssetRowBinding { index: Some(1), ..default() }, UiInteraction::Hovered, InheritedVisibility::VISIBLE));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update(); // 正式物理行路径在按下时冻结。
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.release(MouseButton::Left);
        mouse.clear();
        app.world_mut().resource_mut::<SceneEditorState>().selected_asset = Some(filename.into());
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyX);
        app.update();
        let mut commands = app.world_mut().resource_mut::<Messages<FileManagerCommand>>();
        let cut = commands.drain().next().expect("固定栏接收真实 Ctrl+X 命令");
        assert_eq!(cut.action, crate::ui::file_manager::FileAction::Cut);
        assert_eq!(cut.sources.first(), Some(root.join(filename).as_path()));
        assert!(matches!(cut.sources.iter_with_stamp().next(), Some((_, FileEntryStamp::Missing))));
    }
}
