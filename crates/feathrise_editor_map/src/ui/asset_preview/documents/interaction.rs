//! 文档显式预览/编辑/保存及局部滚轮；输入法和未完成粘贴先于文件动作。

use super::{components::*, resources::DocumentState, session, types::ViewMode};
use crate::resources::{
    EditorTextInputGuard, MapEditorLayoutState, MapEditorNumericScrubMouseGuard,
    MapEditorSettingsState, MapEditorState,
};
use crate::ui::sidebars::MapEditorSidebarState;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::EditableText,
    ui::RelativeCursorPosition,
};

pub(super) fn actions(
    mut editor: ResMut<MapEditorState>,
    mut sidebar: ResMut<MapEditorSidebarState>,
    settings: Option<Res<MapEditorSettingsState>>,
    replacement: Option<Res<super::search::AssetReplaceState>>,
    guard: Res<EditorTextInputGuard>,
    mouse: Res<MapEditorNumericScrubMouseGuard>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<DocumentState>,
    mut focus: ResMut<InputFocus>,
    buttons: Query<
        (&feathrise_editor_ui::EditorButtonClick, &DocumentAction),
        Changed<feathrise_editor_ui::EditorButtonClick>,
    >,
    source: Query<(Entity, &EditableText), With<DocumentSource>>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut layout: Option<ResMut<MapEditorLayoutState>>,
    mut preview_focus: Option<ResMut<super::super::AssetPreviewFocus>>,
    return_tabs: (
        Option<Res<crate::ui::surface_preset_editor::SurfacePresetEditorState>>,
        Option<Res<crate::ui::water_preset_editor::WaterPresetEditorState>>,
        Option<Res<crate::ui::building_presets::BuildingPresetEditorState>>,
        Option<Res<feathrise_editor_core::WorkspaceDomainState>>,
        Option<Res<super::super::AssetPreviewWorkspace>>,
    ),
) {
    if !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || settings.is_some_and(|settings| settings.open)
        || mouse.blocked()
    {
        return;
    }
    let central = state.expanded.is_some() && workspace.active_tab == PresetWorkspaceTab::Document;
    let document_visible = session::visible(&editor, &sidebar, layout.as_deref(), central)
        && replacement
            .as_ref()
            .is_none_or(|state| !state.preview_visible())
        && (central || state.expanded.is_none())
        && (central || state.active.as_deref() == editor.selected_asset.as_deref());
    let save_key = (keyboard.pressed(KeyCode::ControlLeft)
        || keyboard.pressed(KeyCode::ControlRight))
        && keyboard.just_pressed(KeyCode::KeyS)
        && !keyboard.pressed(KeyCode::ShiftLeft)
        && !keyboard.pressed(KeyCode::ShiftRight)
        && state.active.is_some()
        && document_visible;
    let mut actions: Vec<_> = buttons
        .iter()
        .filter(|(interaction, _)| interaction.is_clicked())
        .map(|(_, action)| *action)
        .collect(); // 工具条和最多8个文件标签，不扫描文件系统或全文。
    if save_key {
        actions.push(DocumentAction::Save);
    }
    if actions.is_empty() {
        return;
    }
    if guard.block_shortcuts
        || source.iter().any(|(_, text)| {
            text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()
        })
    {
        if let Some(doc) = state.current_mut() {
            doc.status = "请先确认中文输入或等待粘贴完成，再执行文档操作".into();
        }
        return;
    }
    for mut action in actions {
        if let DocumentAction::CloseTab(id) = action {
            if state.expanded.as_ref().is_none_or(|tab| tab.id != id) {
                state.parked.retain(|tab| tab.id != id);
                continue; // 关闭后台标签不改变活动文件、选择或焦点。
            }
            if central && !state.parked.is_empty() {
                state.expanded = state.parked.pop();
                workspace.touch_ui();
                continue; // 关闭活动标签优先回到最近使用的文档，其他文件仍保留。
            }
            action = DocumentAction::RestoreSidebar;
        }
        match action {
            DocumentAction::ToggleExpand if document_visible && !central => {
                if let Some(path) = state.active.clone() {
                    if let Err(error) = state.expand(path, &mut workspace) {
                        editor.status = error;
                    }
                }
            }
            DocumentAction::OpenSelected
                if session::visible(&editor, &sidebar, layout.as_deref(), false) =>
            {
                if let Some(path) = editor
                    .selected_asset
                    .clone()
                    .filter(|path| crate::asset_catalog::document_format(path).is_some())
                {
                    if let Err(error) = state.expand(path, &mut workspace) {
                        editor.status = error;
                    }
                }
            }
            DocumentAction::ActivateTab(id) => {
                let path = state
                    .tabs()
                    .find(|tab| tab.id == id)
                    .map(|tab| tab.path.clone());
                if let Some(path) = path {
                    if let Err(error) = state.expand(path, &mut workspace) {
                        editor.status = error;
                    }
                }
            }
            DocumentAction::RestoreSidebar | DocumentAction::ToggleExpand
                if central || action == DocumentAction::RestoreSidebar =>
            {
                if let Some(expanded) = state.expanded.take() {
                    if workspace.active_tab == PresetWorkspaceTab::Document {
                        if !super::source::is_project(&expanded.path) {
                            editor.selected_asset = Some(expanded.path);
                        } // 工程源码标签返回侧栏不能武装资产预览或场景摆放。
                        let (surface, water, building, domains, media) = &return_tabs;
                        let still_open = match expanded.return_tab {
                            PresetWorkspaceTab::Map => true,
                            PresetWorkspaceTab::Document => false,
                            PresetWorkspaceTab::AssetPreview => {
                                media.as_ref().is_some_and(|media| media.has_tabs())
                            }
                            PresetWorkspaceTab::Vegetation(id) => workspace.has_tab(id),
                            PresetWorkspaceTab::Surface(id) => {
                                surface.as_ref().is_none_or(|state| state.has_tab(id))
                            }
                            PresetWorkspaceTab::Water(id) => {
                                water.as_ref().is_none_or(|state| state.has_tab(id))
                            }
                            PresetWorkspaceTab::Building(id) => {
                                building.as_ref().is_none_or(|state| state.has_tab(id))
                            }
                            PresetWorkspaceTab::Domain(_, _) => domains
                                .as_ref()
                                .is_some_and(|state| state.is_open(expanded.return_tab)),
                        };
                        workspace.active_tab = if still_open {
                            expanded.return_tab
                        } else {
                            PresetWorkspaceTab::Map
                        }; // 期间被关掉的预设不能还原为空白失效页。
                        workspace.touch_ui();
                        sidebar.reveal_asset_properties();
                        if let Some(focus) = preview_focus.as_mut() {
                            focus.browse(workspace.active_tab); // 返回角色/技能预设时仍显示归还的文档，不被预设预览替换。
                        }
                        if let Some(layout) = layout.as_mut() {
                            if let Some((content, _)) = layout.focus_restore.take() {
                                layout.content_visible = content;
                            }
                            layout.inspector_visible = true; // 专注模式下也能使用“返回侧栏”。
                            if !layout.inspector_fits {
                                layout.content_visible = false;
                            }
                            layout.drag = None;
                            layout.owns_mouse_this_frame = true;
                        }
                    }
                }
            }
            _ => (),
        }
        if matches!(
            action,
            DocumentAction::ToggleExpand
                | DocumentAction::OpenSelected
                | DocumentAction::ActivateTab(_)
                | DocumentAction::RestoreSidebar
        ) {
            if (document_visible || matches!(action, DocumentAction::ActivateTab(_)))
                && state
                    .current()
                    .is_some_and(|doc| doc.mode == ViewMode::Edit)
                && let Ok((entity, _)) = source.single()
            {
                focus.set(entity, FocusCause::Navigated); // 原实体移位不重绑整篇输入，继续保留光标、撤销与EditableText.viewport。
            }
            continue;
        }
        if !document_visible {
            continue;
        }
        if action == DocumentAction::Save {
            state.request_save();
            continue;
        }
        if action == DocumentAction::KeepDraft {
            if let Some(doc) = state.current_mut().filter(|doc| doc.external.warning) {
                doc.external.warning = false;
                doc.confirm_reload = false;
                doc.status = "已保留内置内容；磁盘版本仍有变化，保存会继续检查冲突".into();
            }
            continue;
        }
        if action == DocumentAction::Reload {
            if state.io.is_some() {
                continue;
            }
            if let Some(doc) = state.current_mut() {
                if doc.dirty() && !doc.confirm_reload {
                    doc.confirm_reload = true;
                    doc.status = "重新载入会丢弃本文件全部草稿；再点“确认丢弃”继续".into();
                    continue;
                }
                doc.mode = ViewMode::Preview; // 确认丢弃后退出输入，读取期间的新按键不能再写进待替换草稿。
            }
            if let Some(path) = state.active.clone() {
                state.request_load(path);
            }
            continue;
        }
        if action == DocumentAction::Edit
            && state.active.as_deref().is_some_and(|path| state.is_loading(path))
        {
            if let Some(doc) = state.current_mut() {
                doc.mode = ViewMode::Preview;
                doc.status = "正在重新载入，完成后可编辑；读取失败会保留原草稿".into();
            }
            continue;
        }
        let Some(doc) = state.current_mut() else {
            continue;
        };
        doc.confirm_reload = false;
        match action {
            DocumentAction::Preview => doc.mode = ViewMode::Preview,
            DocumentAction::Edit => doc.mode = ViewMode::Edit,
            _ => (),
        }
        if let Ok((entity, _)) = source.single() {
            if doc.mode == ViewMode::Edit {
                focus.set(entity, FocusCause::Navigated);
            } else if focus.get() == Some(entity) {
                focus.clear();
            }
        }
    }
}

/// 预览正文滚轮先于资产外栏；源码文字仍由既有EditableText.viewport入口接管。
pub(super) fn scroll(
    mut wheels: MessageReader<MouseWheel>,
    mut state: ResMut<DocumentState>,
    mut areas: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<DocumentPreviewScroll>,
    >,
) {
    let delta: f32 = wheels
        .read()
        .map(|wheel| {
            -wheel.y
                * match wheel.unit {
                    MouseScrollUnit::Line => 27.0,
                    MouseScrollUnit::Pixel => 1.0,
                }
        })
        .sum();
    if state.wheel_owned {
        state.wheel_owned = false;
    }
    if !state.visible
        || state
            .current()
            .is_none_or(|doc| doc.mode != ViewMode::Preview)
        || delta == 0.0
    {
        return;
    }
    for (cursor, node, mut scroll) in &mut areas {
        if !cursor.cursor_over() {
            continue;
        }
        let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        let next = (scroll.y + delta).clamp(0.0, max);
        state.wheel_owned = next != scroll.y;
        scroll.y = next;
    }
}
