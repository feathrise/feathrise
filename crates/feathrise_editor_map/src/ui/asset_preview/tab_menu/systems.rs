//! 右击文件标签打开菜单；显式批量关闭仅释放标签身份，草稿与文件任务继续归原会话所有。
use super::super::{
    AssetPreviewWorkspace, DocumentState,
    components::AssetPreviewTabRoot,
    documents::{DocumentSource, DocumentTabRoot, SaveAllDocuments},
};
use super::{
    components::{
        CloseAllTabsButton, CloseUnmodifiedTabsButton, CopyFilePathButton, ExternalFileButton,
        SaveAllDraftsButton,
    },
    messages::{CloseAllAssetTabs, CloseUnmodifiedAssetTabs},
    resources::AssetTabMenuState,
};
use crate::ui::{
    asset_context_menu::{AssetContextMenuState, helpers::logical_anchor},
    vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
};
use crate::{components::MapEditorRoot, resources::*};
use bevy::{
    ecs::system::SystemParam, input_focus::InputFocus, prelude::*, text::EditableText,
    ui::RelativeCursorPosition, window::PrimaryWindow,
};

#[derive(SystemParam)]
pub(super) struct MenuContext<'w, 's> {
    editor: Res<'w, MapEditorState>,
    guard: Res<'w, MapEditorNumericScrubMouseGuard>,
    text: Option<Res<'w, EditorTextInputGuard>>,
    settings: Option<Res<'w, MapEditorSettingsState>>,
    browser: Option<Res<'w, MapBrowserState>>,
    deletion: Option<Res<'w, MapDeletionTask>>,
    conflict: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
    map_drag: Option<Res<'w, MapBrowserDragState>>,
    numeric: Option<Res<'w, feathrise_editor_ui::ActiveNumericInputScrub>>,
    preview: Option<Res<'w, super::super::AssetPreviewNavigation>>,
    minimap: Option<Res<'w, super::super::DocumentMinimapState>>,
    asset_menu: Option<Res<'w, AssetContextMenuState>>,
    layout: Option<Res<'w, MapEditorLayoutState>>,
    focus: Option<Res<'w, InputFocus>>,
    inputs: Query<'w, 's, &'static EditableText>,
    sources: Query<'w, 's, &'static EditableText, With<DocumentSource>>, // 唯一正文在菜单抢焦点后也必须完成粘贴和编辑采用。
    documents: Res<'w, DocumentState>,
    available: Res<'w, crate::ui::external_files::ExternalEditors>,
    media: Res<'w, AssetPreviewWorkspace>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    roots: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<MapEditorRoot>>,
}

pub(super) fn control_menu(
    context: MenuContext,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    tabs: Query<&RelativeCursorPosition, Or<(With<DocumentTabRoot>, With<AssetPreviewTabRoot>)>>,
    file_targets: Query<(
        &RelativeCursorPosition,
        &crate::ui::external_files::ExternalFileTab,
    )>,
    buttons: Query<
        (
            &feathrise_editor_ui::EditorButtonClick,
            Has<CloseUnmodifiedTabsButton>,
            Has<SaveAllDraftsButton>,
            Option<&ExternalFileButton>,
            Option<&CopyFilePathButton>,
        ),
        (
            Or<(
                With<CloseAllTabsButton>,
                With<CloseUnmodifiedTabsButton>,
                With<SaveAllDraftsButton>,
                With<ExternalFileButton>,
                With<CopyFilePathButton>,
            )>,
            Changed<feathrise_editor_ui::EditorButtonClick>,
        ),
    >,
    menu_hits: Query<
        &Interaction,
        Or<(
            With<CloseAllTabsButton>,
            With<CloseUnmodifiedTabsButton>,
            With<SaveAllDraftsButton>,
            With<ExternalFileButton>,
            With<CopyFilePathButton>,
        )>,
    >,
    mut menu: ResMut<AssetTabMenuState>,
    mut close: MessageWriter<CloseAllAssetTabs>,
    mut close_unmodified: MessageWriter<CloseUnmodifiedAssetTabs>,
    mut save_all: MessageWriter<SaveAllDocuments>,
    mut external: MessageWriter<crate::ui::external_files::OpenExternalFile>,
    mut project_external: MessageWriter<crate::ui::external_files::OpenExternalProjectFile>,
    mut copies: MessageWriter<crate::ui::asset_context_menu::CopyAssetPath>,
) {
    let had_menu = menu.open;
    if menu.captured != had_menu {
        menu.captured = had_menu;
    }
    if !had_menu && !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let enabled = context.editor.open
        && !context.editor.close_requested
        && (!had_menu || menu.map_id == context.editor.selected_map_id)
        && (!had_menu
            || menu
                .path
                .as_ref()
                .is_none_or(|path| file_targets.iter().any(|(_, target)| &target.0 == path)))
        && !context.editor.new_map_dialog_open
        && context.editor.pending_map_transition.is_none()
        && context.windows.single().is_ok_and(|window| window.focused)
        && (context.documents.expanded() || context.media.has_tabs())
        && !context
            .text
            .as_ref()
            .is_some_and(|guard| guard.block_shortcuts)
        && !context
            .settings
            .as_ref()
            .is_some_and(|state| state.open || state.owns_mouse_this_frame)
        && !context
            .browser
            .as_ref()
            .is_some_and(|state| state.dialog.is_some())
        && !context
            .deletion
            .as_ref()
            .is_some_and(|state| state.confirmation.is_some())
        && !context
            .conflict
            .as_ref()
            .is_some_and(|state| state.modal_open())
        && !context
            .map_drag
            .as_ref()
            .is_some_and(|state| state.blocks_other_mouse() || state.press.is_some())
        && !context
            .numeric
            .as_ref()
            .is_some_and(|state| state.entity.is_some())
        && !context
            .preview
            .as_ref()
            .is_some_and(|state| state.dragging())
        && !context
            .minimap
            .as_ref()
            .is_some_and(|state| state.owns_mouse())
        && !context
            .asset_menu
            .as_ref()
            .is_some_and(|state| state.owns_mouse())
        && !context
            .layout
            .as_ref()
            .is_some_and(|state| state.drag.is_some())
        && !context
            .focus
            .as_ref()
            .and_then(|focus| focus.get())
            .and_then(|entity| context.inputs.get(entity).ok())
            .is_some_and(|text| {
                text.is_composing()
                    || text.pending_paste.is_some()
                    || !text.pending_edits.is_empty()
            })
        && !context.sources.iter().any(|text| {
            text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()
        });
    if !enabled {
        menu.open = false;
        return;
    } // 隐藏、失焦、模态和未确认输入立即取消，不积压批量关闭。
    if keyboard.just_pressed(KeyCode::F12) {
        menu.open = false;
        menu.captured = false;
        return; // 仍由正式F12关闭门处理。
    }
    if had_menu {
        if keyboard.just_pressed(KeyCode::Escape)
            || mouse.just_pressed(MouseButton::Right)
            || mouse.just_pressed(MouseButton::Middle)
        {
            menu.open = false;
        } else if let Some((_, unmodified, save, action, copy)) = buttons
            .iter()
            .find(|(click, _, _, _, _)| click.is_clicked())
        {
            // 按下只显示反馈；冻结的文件身份要保留到按钮内松开，不能提前关菜单。
            menu.open = false;
            if let Some(copy) = copy {
                if let Some(path) = menu.path.clone() {
                    copies
                        .write(crate::ui::asset_context_menu::CopyAssetPath { path, kind: copy.0 });
                } // 复制后台标签的文件身份，不能拿当前活动文档或隐式保存草稿。
            } else if let Some(action) = action {
                if let Some(path) = menu.path.clone()
                    && context.available.visible(action.0, true)
                {
                    let has_draft = context.documents.has_unsaved_draft(&path);
                    if let Some((root, relative)) = context.documents.project_source_path(&path) {
                        project_external.write(crate::ui::external_files::OpenExternalProjectFile {
                            root: root.to_path_buf(), request: crate::ui::external_files::OpenExternalFile {
                                path: relative.to_owned(), action: action.0, has_draft,
                            },
                        });
                        return;
                    }
                    external.write(crate::ui::external_files::OpenExternalFile {
                        path,
                        action: action.0,
                        has_draft,
                    });
                }
            } else if save {
                save_all.write(SaveAllDocuments); // 明确文件动作；不关标签、不切作者页或丢草稿。
            } else if unmodified {
                close_unmodified.write(CloseUnmodifiedAssetTabs);
            } else {
                close.write(CloseAllAssetTabs);
            }
        } else if mouse.just_pressed(MouseButton::Left)
            && !menu_hits
                .iter()
                .any(|interaction| *interaction == Interaction::Pressed)
        {
            menu.open = false; // 外部按下只收起，后续释放不得触发原菜单项。
        }
        return; // 外部点击只收起，关闭帧继续独占鼠标。
    }
    if context.guard.blocked()
        || [MouseButton::Left, MouseButton::Middle]
            .into_iter()
            .any(|button| mouse.pressed(button) || mouse.just_released(button))
        || !tabs.iter().any(RelativeCursorPosition::cursor_over)
    {
        return;
    }
    let (Ok(window), Ok((root, transform))) = (context.windows.single(), context.roots.single())
    else {
        return;
    };
    let Some(cursor) = window.physical_cursor_position() else {
        return;
    };
    let root_rect = Rect::from_center_size(transform.affine().translation, root.size());
    let Some(anchor) = logical_anchor(cursor, root_rect, root.inverse_scale_factor()) else {
        return;
    };
    menu.anchor = anchor;
    menu.open = true;
    menu.captured = true;
    menu.map_id = context.editor.selected_map_id.clone();
    menu.path = file_targets
        .iter()
        .find(|(cursor, _)| cursor.cursor_over())
        .map(|(_, target)| target.0.clone());
}

/// 最多8个文档、32个媒体身份；既不复制全文/模型，也不取消在途保存或清空原生编辑状态。
pub(super) fn close_all_tabs(
    mut requests: MessageReader<CloseAllAssetTabs>,
    mut unmodified_requests: MessageReader<CloseUnmodifiedAssetTabs>,
    mut documents: ResMut<DocumentState>,
    animation: Option<Res<super::super::AnimationEditorState>>,
    mut media: ResMut<AssetPreviewWorkspace>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut editor: ResMut<MapEditorState>,
    mut sidebar: ResMut<crate::ui::sidebars::MapEditorSidebarState>,
    mut layout: Option<ResMut<MapEditorLayoutState>>,
    mut preview_focus: Option<ResMut<super::super::AssetPreviewFocus>>,
    mut focus: Option<ResMut<InputFocus>>,
    sources: Query<(), With<DocumentSource>>,
    authors: (
        Option<Res<crate::ui::surface_preset_editor::SurfacePresetEditorState>>,
        Option<Res<crate::ui::water_preset_editor::WaterPresetEditorState>>,
        Option<Res<crate::ui::building_presets::BuildingPresetEditorState>>,
        Option<Res<feathrise_editor_core::WorkspaceDomainState>>,
    ),
) {
    let all = requests.read().last().is_some();
    let unmodified = unmodified_requests.read().last().is_some();
    if !all && !unmodified {
        return;
    }
    let previous_document_count = documents.tab_count();
    let had_media = media.has_tabs();
    let document = if all {
        documents.close_all_tabs()
    } else {
        documents.close_unmodified_tabs()
    };
    let preview = if all {
        media.close_all_tabs()
    } else {
        media.close_unmodified_tabs(|path| {
            animation
                .as_ref()
                .is_some_and(|state| state.has_unsaved_draft(documents.asset_root(), path))
        })
    };
    if !all && previous_document_count == documents.tab_count() && !had_media {
        editor.status = "没有未修改的文件标签；未保存文档继续保留".into();
        return;
    }
    // 当前草稿仍开着时保留焦点；当前已保存文档关闭后优先切到剩余草稿。
    let next_document = documents.expanded();
    if workspace.active_tab == PresetWorkspaceTab::AssetPreview && media.has_tabs() {
        workspace.touch_ui();
        editor.status = "已关闭未修改文件标签；未保存动画继续保留".into();
        return;
    }
    if workspace.active_tab == PresetWorkspaceTab::Document && next_document {
        workspace.touch_ui();
        editor.status = "已关闭未修改文件标签；未保存文档继续保留".into();
        return;
    }
    let active = match workspace.active_tab {
        PresetWorkspaceTab::Document => document,
        PresetWorkspaceTab::AssetPreview if next_document => {
            workspace.active_tab = PresetWorkspaceTab::Document;
            workspace.touch_ui();
            editor.status = "已关闭未修改文件标签；已切回未保存文档".into();
            return;
        }
        PresetWorkspaceTab::AssetPreview => preview,
        _ => None, // 作者页及未保存预设保持原来的独立关闭确认。
    };
    if let Some((path, return_tab)) = active {
        let (surface, water, building, domains) = authors;
        let exists = match return_tab {
            PresetWorkspaceTab::Map => true,
            PresetWorkspaceTab::Document | PresetWorkspaceTab::AssetPreview => false,
            PresetWorkspaceTab::Vegetation(id) => workspace.has_tab(id),
            PresetWorkspaceTab::Surface(id) => surface.is_some_and(|state| state.has_tab(id)),
            PresetWorkspaceTab::Water(id) => water.is_some_and(|state| state.has_tab(id)),
            PresetWorkspaceTab::Building(id) => building.is_some_and(|state| state.has_tab(id)),
            PresetWorkspaceTab::Domain(_, _) => {
                domains.is_some_and(|state| state.is_open(return_tab))
            }
        };
        workspace.active_tab = if exists {
            return_tab
        } else {
            PresetWorkspaceTab::Map
        };
        if !super::super::documents::is_project_document(&path) {
            editor.selected_asset = Some(path);
        } // 工程文件关闭只释放标签；保持资产选择与场景拖放原身份。
        sidebar.reveal_asset_properties();
        if let Some(preview_focus) = preview_focus.as_mut() {
            preview_focus.browse(workspace.active_tab);
        }
        if let Some(layout) = layout.as_mut() {
            if let Some((content, _)) = layout.focus_restore.take() {
                layout.content_visible = content;
            }
            layout.inspector_visible = true;
            if !layout.inspector_fits {
                layout.content_visible = false;
            }
            layout.drag = None;
            layout.owns_mouse_this_frame = true;
        }
    }
    if let Some(focus) = focus.as_mut()
        && focus.get().is_some_and(|entity| sources.contains(entity))
    {
        focus.clear();
    }
    workspace.touch_ui();
    editor.status = if all {
        "已关闭全部文件标签；未保存文档草稿仍保留"
    } else {
        "已关闭未修改文件标签；未保存文档继续保留"
    }
    .into();
}
