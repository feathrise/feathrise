//! 工程与资产行右键菜单的开关、来源复验与系统剪贴板写入；不改变左键选择或场景数据。

use feathrise_editor_ui::UiInteraction;
use bevy::ecs::system::SystemParam; // 汇总只读输入资格，避免系统参数与其他作者域混杂。
use bevy::prelude::*; // 原生Bevy鼠标边沿、UI命中、消息和剪贴板。
use bevy::window::PrimaryWindow; // 只接受当前聚焦主窗口的鼠标位置。
use feathrise_editor_ui::ActiveNumericInputScrub; // 数字拖动在同一帧已经开始时也必须让位。

use super::components::{AssetMenuCopyButton, AssetMenuExternalButton, AssetMenuReferencesButton, AssetMenuFavoriteButton}; // 只由菜单本身的左键按钮触发。
use super::helpers::{copy_path_status, copy_project_path_status, logical_anchor, project_target_is_current, target_is_current}; // 身份校验不进行磁盘扫描。
use super::messages::{CopyAssetPath, CopyProjectPath}; // 确认意图后再访问系统剪贴板。
use super::resources::AssetContextMenuState; // 固定唯一临时菜单。
use super::types::{AssetMenuSource, AssetMenuTarget}; // 从右键行冻结真实路径。
use crate::authoring::TerrainExternalChangeState; // Blender冲突模态优先。
use crate::components::{AssetRowBinding, SceneEditorAssetRow, SceneEditorRoot}; // 只遍历可见窗口固定池和唯一编辑器根。
use crate::resources::{
    EditorAssetCatalog, SceneBrowserDragState, SceneBrowserState, SceneDeletionTask,
    SceneEditorNumericScrubMouseGuard, SceneEditorState,
}; // 复用既有鼠标与模态所有权。
use crate::ui::asset_preview::AssetPreviewNavigation; // 从预览开始的右键轨道不能变成列表菜单。
use crate::ui::sidebars::SceneEditorSidebarState; // 左侧资产页隐藏后不允许打开菜单。
use crate::{asset_catalog::project_files::EditorProjectFileCatalog, ui::project_browser::{ProjectBrowserContent, ProjectBrowserView, ProjectFileRow, ProjectFileViewport}};

/// 所有资格都只读；复制结果单独写入状态栏，避免ECS资源访问冲突。
#[derive(SystemParam)] // 主输入系统只管理一个菜单资源。
pub(super) struct MenuInputContext<'w, 's> {
    editor: Res<'w, SceneEditorState>,      // F12开关、场景身份与新建弹框。
    catalog: Res<'w, EditorAssetCatalog>, // 当前目录的一条O(1)索引。
    viewport: crate::ui::rows::AssetViewport<'w, 's>, // 滚轮已变化但表现尚未换绑时拒绝旧行。
    project_browser: Option<Res<'w, crate::ui::project_browser::ProjectBrowserState>>,
    project_catalog: Option<Res<'w, EditorProjectFileCatalog>>,
    project_viewport: Option<Res<'w, ProjectFileViewport>>,
    documents: Option<Res<'w, crate::ui::asset_preview::DocumentState>>,
    available: Res<'w, crate::ui::external_files::ExternalEditors>,
    shortcuts: Option<Res<'w, crate::ui::asset_shortcuts::AssetShortcuts>>,
    sidebar: Res<'w, SceneEditorSidebarState>, // 只有资产页允许弹出。
    layout: Option<Res<'w, crate::resources::SceneEditorLayoutState>>,
    numeric: Res<'w, SceneEditorNumericScrubMouseGuard>, // 既有拖动及松键帧不可抢占。
    active_numeric: Res<'w, ActiveNumericInputScrub>, // 同帧刚建立的数字拖动也不可抢占。
    tab_menu: Option<Res<'w, crate::ui::asset_preview::AssetTabMenuState>>, // 中央文件菜单遮罩拥有鼠标时不能弹出背景列表菜单。
    preview: Option<Res<'w, AssetPreviewNavigation>>, // 预览拖动可越过画布，仍保留输入权。
    scene_drag: Res<'w, SceneBrowserDragState>, // 场景拖动候选和活动状态都需要让位。
    browser: Res<'w, SceneBrowserState>,    // 文件夹模态高于普通菜单。
    deletion: Res<'w, SceneDeletionTask>,   // 回收确认不能被背景菜单打断。
    conflict: Res<'w, TerrainExternalChangeState>, // 地形冲突弹框同样阻止点透。
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>, // 失焦立即关闭。
    roots: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<SceneEditorRoot>>, // 坐标使用真实UI倍率与根位置。
}

/// 每帧运行以消费关闭边沿，但没有菜单与新右键时不遍历行或重建UI。
pub(super) fn control_menu(
    context: MenuInputContext,            // 本帧完整的输入与目录资格。
    mouse: Res<ButtonInput<MouseButton>>, // 右键按下打开；普通菜单项由按钮内释放的点击执行。
    keyboard: Res<ButtonInput<KeyCode>>,  // Esc只关闭菜单，F12可继续交给编辑器关闭流程。
    rows: Query<(&UiInteraction, &SceneEditorAssetRow, &Node, Option<&AssetRowBinding>)>, // 老最小夹具允许无旁路绑定；正式行均携带完整窗口身份。
    project_rows: Query<(&UiInteraction, &ProjectFileRow, &Node)>, // 工程行只用可见固定池和完整目录索引。
    project_scrolls: Query<(&ProjectBrowserContent, &ScrollPosition)>, // Input早于滚动窗口重绑时也拒绝旧行。
    copy_buttons: Query<(&feathrise_editor_ui::EditorButtonClick, Option<&AssetMenuCopyButton>, Option<&AssetMenuExternalButton>, Has<AssetMenuReferencesButton>, Has<AssetMenuFavoriteButton>),
        (Or<(With<AssetMenuCopyButton>, With<AssetMenuExternalButton>, With<AssetMenuReferencesButton>, With<AssetMenuFavoriteButton>)>, Changed<feathrise_editor_ui::EditorButtonClick>)>,
    menu_hits: Query<&UiInteraction,
        Or<(With<AssetMenuCopyButton>, With<AssetMenuExternalButton>, With<AssetMenuReferencesButton>, With<AssetMenuFavoriteButton>)>>,
    mut menu: ResMut<AssetContextMenuState>,                // 不写文件、不切换工具、不修改预设。
    mut copies: MessageWriter<CopyAssetPath>,               // 仅明确左击菜单项时产生外部写入意图。
    mut project_copies: Option<ResMut<Messages<CopyProjectPath>>>,
    mut external: MessageWriter<crate::ui::external_files::OpenExternalFile>,
    mut project_external: Option<ResMut<Messages<crate::ui::external_files::OpenExternalProjectFile>>>,
    mut search: Option<ResMut<crate::ui::asset_preview::AssetSearchState>>,
    mut favorites: Option<ResMut<Messages<crate::ui::asset_shortcuts::SetAssetFavorite>>>,
) {
    let had_menu = menu.target.is_some(); // 关闭帧也必须独占这一次鼠标操作。
    if menu.captured_this_frame != had_menu {
        menu.captured_this_frame = had_menu; // 稳定帧不触发Changed或UI重排。
    }
    if !had_menu && !mouse.just_pressed(MouseButton::Right) {
        return; // 普通左键、悬停与稳定帧完全沿用现有功能。
    }
    let window = context.windows.single().ok(); // 唯一主窗口缺失时失败关闭菜单。
    let project_scroll = project_scrolls.iter().find(|(view, _)| view.0 == ProjectBrowserView::ProjectFiles)
        .map(|(_, scroll)| scroll);
    let project_visible = context.project_browser.as_ref().is_some_and(|browser| !browser.assets_visible());
    let source_ready = if project_visible {
        context.project_catalog.as_ref().is_some_and(|catalog| catalog.active && !catalog.is_loading())
            && context.project_viewport.as_ref().zip(project_scroll)
                .is_some_and(|(viewport, scroll)| viewport.scroll == scroll.0)
    } else { context.viewport.is_current(&context.catalog) };
    let enabled = context.editor.open
        && !context.editor.close_requested
        && context.editor.pending_scene_transition.is_none()
        && context.sidebar.assets_browser_visible()
        && source_ready
        && context.layout.as_ref().is_none_or(|layout| layout.content_shown())
        && !context.editor.new_scene_dialog_open
        && context.browser.dialog.is_none()
        && context.deletion.confirmation.is_none()
        && !context.conflict.modal_open()
        && !context.scene_drag.blocks_other_mouse()
        && context.scene_drag.press.is_none()
        && context.active_numeric.entity.is_none()
        && !context.tab_menu.as_ref().is_some_and(|menu| menu.owns_mouse())
        && !context
            .preview
            .as_ref()
            .is_some_and(|preview| preview.dragging())
        && window.is_some_and(|window| window.focused); // 不从任何已有手势、失焦或模态中抢输入。
    if !enabled {
        if had_menu {
            menu.target = None;
        } // 高优先级状态变化同帧关闭已有菜单。
        return; // 不允许后台菜单写入剪贴板。
    }
    if keyboard.just_pressed(KeyCode::F12) {
        menu.target = None; // 按F12时先隐藏菜单，继续使用原有安全关闭门。
        menu.captured_this_frame = false; // 没有按住鼠标时不额外吞掉这次F12。
        return; // 不自行修改编辑器打开状态。
    }
    if had_menu {
        let current = menu
            .target
            .as_ref()
            .is_some_and(|target| match &target.source {
                AssetMenuSource::Assets => !project_visible && target_is_current(target, &context.catalog, &context.editor),
                AssetMenuSource::Project { .. } => project_visible && context.project_catalog.as_ref()
                    .zip(context.project_viewport.as_ref()).zip(project_scroll)
                    .is_some_and(|((catalog, viewport), scroll)| project_target_is_current(target, catalog, viewport, scroll, &context.editor)),
            }); // 点击前按右键时的真实来源复验，不能跨assets路由复用同名路径。
        if !current
            || keyboard.just_pressed(KeyCode::Escape)
            || mouse.just_pressed(MouseButton::Right)
            || mouse.just_pressed(MouseButton::Middle)
        {
            menu.target = None; // 失效、Esc或其他鼠标键只关闭，不复制路径。
            return; // 关闭帧所有权由captured_this_frame保留。
        }
        // 菜单按住期间保留冻结的目标；只有原按钮内松开才消费动作，移出松开可继续选择。
        if let Some((_, copy, action, references, favorite)) = copy_buttons
            .iter()
            .find(|(click, _, _, _, _)| click.is_clicked())
        {
            let target = menu.target.take().expect("活动菜单已冻结目标"); // 本次点击最多消费一次。
            if let Some(copy) = copy {
                match target.source {
                    AssetMenuSource::Assets => { copies.write(CopyAssetPath { path: target.path, kind: copy.0 }); }
                    AssetMenuSource::Project { root, .. } => {
                        if let Some(requests) = project_copies.as_mut() {
                            requests.write(CopyProjectPath { root, path: target.path, kind: copy.0 });
                        }
                    }
                }
            } else if let Some(action) = action {
                let file = match &target.source {
                    AssetMenuSource::Assets => context.catalog.entries.get(target.index),
                    AssetMenuSource::Project { .. } => context.project_catalog.as_ref().and_then(|catalog| catalog.entries.get(target.index)),
                }.is_some_and(|entry| !entry.is_directory);
                if !context.available.visible(action.0, file) { return; } // 缓存换代后不得重放已隐藏IDE的旧按下。
                let has_draft = context.documents.as_ref().is_some_and(|docs| match &target.source {
                    AssetMenuSource::Assets => docs.has_unsaved_draft(&target.path),
                    AssetMenuSource::Project { root, .. } => docs.has_unsaved_project_draft(root, &target.path),
                });
                let request = crate::ui::external_files::OpenExternalFile { path: target.path, action: action.0, has_draft };
                match target.source {
                    AssetMenuSource::Assets => { external.write(request); }
                    AssetMenuSource::Project { root, .. } => {
                        if let Some(requests) = project_external.as_mut() {
                            requests.write(crate::ui::external_files::OpenExternalProjectFile { root, request });
                        }
                    }
                } // 外部打开磁盘文件；不隐式保存或丢弃内置草稿。
            } else if favorite && matches!(target.source, AssetMenuSource::Assets) && let Some(favorites) = favorites.as_mut() {
                let is_directory = context.catalog.entries[target.index].is_directory;
                let favorite = !context.shortcuts.as_ref().is_some_and(|shortcuts| shortcuts.is_favorite(&target.path, is_directory));
                favorites.write(crate::ui::asset_shortcuts::SetAssetFavorite { path: target.path, is_directory, favorite });
            } else if references && matches!(target.source, AssetMenuSource::Assets)
                && context.catalog.entries.get(target.index).is_some_and(|entry| !entry.is_directory)
                && let Some(search) = search.as_deref_mut()
                && let Some(documents) = context.documents.as_ref()
            {
                search.request_references(target.path, documents.asset_root().to_path_buf()); // 冻结目标和根；鼠标释放、确认输入后才启动后台扫描。
            }
        } else if mouse.just_pressed(MouseButton::Left)
            && !menu_hits.iter().any(|interaction| *interaction == UiInteraction::Pressed)
        {
            menu.target = None; // 外部按下只关闭菜单，不能借后续松开执行菜单动作。
        }
        return; // 活动菜单不重新扫描行或重新读取源文件。
    }
    if context.numeric.blocked()
        || [MouseButton::Left, MouseButton::Middle]
            .into_iter()
            .any(|button| mouse.pressed(button) || mouse.just_released(button))
        || (!project_visible && context.catalog.requested_revision != context.catalog.applied_revision)
    {
        return; // 拖动和弦、松键或目录换代期间不建立新菜单。
    }
    let target = if project_visible {
        let Some((catalog, viewport)) = context.project_catalog.as_ref().zip(context.project_viewport.as_ref()) else { return; };
        let Some((_, row, _)) = project_rows.iter().find(|(interaction, _, node)|
            **interaction == UiInteraction::Hovered && node.display != Display::None) else { return; };
        let Some(index) = row.entry_index.filter(|index|
            row.slot < viewport.capacity && *index == viewport.first.saturating_add(row.slot)
                && row.revision == catalog.applied_revision && row.viewport_revision == viewport.revision) else { return; };
        let Some(entry) = catalog.entries.get(index) else { return; };
        let Some((root, context_id)) = catalog.project_root().zip(catalog.source_context()) else { return; };
        AssetMenuTarget { path: entry.asset_path.clone(), index, revision: catalog.applied_revision,
            viewport_revision: viewport.revision, scene_id: context.editor.selected_scene_id.clone(),
            source: AssetMenuSource::Project { root: root.to_path_buf(), context: context_id } }
    } else {
        let Some((_, row, _, binding)) = rows.iter().find(|(interaction, _, node, _)| {
            **interaction == UiInteraction::Hovered && node.display != Display::None
        }) else {
            return; // 右键空白、裁剪区或前方覆盖控件不产生菜单。
        };
        let Some(index) = row.entry_index.filter(|index| {
            row.slot < context.catalog.visible_count
                && *index == context.catalog.visible_start.saturating_add(row.slot)
                && binding.is_none_or(|binding| binding.index == Some(*index)
                    && binding.revision == context.catalog.applied_revision
                    && binding.viewport_revision == context.catalog.viewport_revision)
        }) else {
            return; // 固定行池尚未换绑完成时不能使用旧下标。
        };
        let Some(entry) = context.catalog.entries.get(index) else { return; };
        AssetMenuTarget { path: entry.asset_path.clone(), index, revision: context.catalog.applied_revision,
            viewport_revision: context.catalog.viewport_revision, scene_id: context.editor.selected_scene_id.clone(),
            source: AssetMenuSource::Assets }
    }; // 两种行池均冻结真实路径，不改变左键选择或触发预览。
    let Some(cursor) = window.and_then(Window::physical_cursor_position) else {
        return;
    }; // 坐标丢失不猜测菜单位置。
    let Ok((root, transform)) = context.roots.single() else {
        return;
    }; // 编辑器根尚未布局时等待用户下一次明确点击。
    let root_rect = Rect::from_center_size(transform.affine().translation, root.size()); // F12根不旋转，真实物理矩形可直接换算。
    let Some(anchor) = logical_anchor(cursor, root_rect, root.inverse_scale_factor()) else {
        return;
    }; // 避免零尺寸或非法倍率。
    menu.anchor = anchor; // 只在打开时冻结位置，不让菜单逐帧追随鼠标。
    menu.target = Some(target); // 打开菜单只改临时UI，不修改selected_asset或触发预览加载。
    menu.captured_this_frame = true; // 右键按住移入预览或中央场景也不能变成相机操作。
}

/// 平台剪贴板只在显式菜单动作时写一个短路径，失败不改任何作者数据。
pub(super) fn copy_requested_path(
    mut requests: MessageReader<CopyAssetPath>, // 空队列的稳定帧没有系统剪贴板调用。
    mut project_requests: MessageReader<CopyProjectPath>, // 工程根随同一次明确复制意图冻结。
    mut clipboard: ResMut<Clipboard>,           // Bevy唯一资源串行化原生剪贴板访问。
    mut editor: ResMut<SceneEditorState>,         // 状态栏只反映真实复制结果。
    documents: Option<Res<crate::ui::asset_preview::DocumentState>>,
) {
    let asset_request = requests.read().last();
    if let Some(request) = project_requests.read().last() {
        editor.status = copy_project_path_status(request, |path| clipboard.set_text(path));
    } else if let Some(request) = asset_request {
        if crate::ui::asset_preview::documents::is_project_document(&request.path) {
            editor.status = match documents.as_ref().ok_or("工程文档会话已关闭")
                .and_then(|documents| documents.project_path_to_copy(&request.path,
                    request.kind == super::types::AssetPathCopyKind::Absolute).map_err(|_| "工程文档路径无效")) {
                Ok(path) => super::helpers::copy_asset_text_status(request.kind.description(), &path, |path| clipboard.set_text(path)),
                Err(error) => format!("复制路径失败：{error}"),
            };
            return;
        }
        let fallback = feathrise_editor_core::workspace_asset_root();
        let root = documents.as_ref().map_or(fallback.as_path(), |documents| documents.asset_root());
        editor.status = copy_path_status(request, root, |path| clipboard.set_text(path)); // 两个菜单共用一次真实写入，不读取或隐式重试。
    } // 多次排队最多处理最后一个明确请求，避免同帧重复平台调用。
}
