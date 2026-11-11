//! 搜索快捷键、显式后台请求与滚轮所有权；候选和未完成输入不能执行文件动作。

use super::super::{resources::DocumentState, source};
use super::{components::*, resources::AssetSearchState, types::*, worker};
use crate::{
    resources::*,
    ui::{
        sidebars::{EditorNavigatorTab, SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{
    ecs::system::SystemParam,
    input::{
        keyboard::KeyCode,
        mouse::{MouseScrollUnit, MouseWheel},
    },
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::EditableText,
    ui::RelativeCursorPosition,
};

#[derive(SystemParam)]
pub(in super::super) struct SearchGate<'w> {
    guard: Res<'w, EditorTextInputGuard>,
    mouse: Res<'w, SceneEditorNumericScrubMouseGuard>,
    settings: Option<Res<'w, SceneEditorSettingsState>>,
    browser: Option<Res<'w, SceneBrowserState>>,
    deletion: Option<Res<'w, SceneDeletionTask>>,
    conflict: Option<Res<'w, crate::authoring::TerrainExternalChangeState>>,
    menu: Option<Res<'w, crate::ui::asset_preview::AssetTabMenuState>>,
    available: Res<'w, crate::ui::external_files::ExternalEditors>,
}

impl SearchGate<'_> {
    pub(in super::super) fn blocked(&self, editor: &SceneEditorState) -> bool {
        !editor.open
            || editor.close_requested
            || editor.new_scene_dialog_open
            || editor.pending_scene_transition.is_some()
            || self.guard.block_shortcuts
            || self.mouse.blocked()
            || self.settings.as_ref().is_some_and(|state| state.open)
            || self
                .browser
                .as_ref()
                .is_some_and(|state| state.dialog.is_some())
            || self
                .deletion
                .as_ref()
                .is_some_and(|state| state.confirmation.is_some())
            || self
                .conflict
                .as_ref()
                .is_some_and(|state| state.modal_open())
            || self.menu.as_ref().is_some_and(|state| state.owns_mouse())
    }
}

pub(in super::super) fn busy(text: &EditableText) -> bool {
    text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()
}

pub(super) fn input(
    mut editor: ResMut<SceneEditorState>,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    mut layout: Option<ResMut<SceneEditorLayoutState>>,
    catalog: Res<EditorAssetCatalog>,
    mut documents: ResMut<DocumentState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut state: ResMut<AssetSearchState>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    gate: SearchGate,
    fields: Query<(Entity, &SearchField, &EditableText)>,
    inputs: Query<&EditableText>,
    buttons: Query<(&feathrise_editor_ui::EditorButtonClick, &SearchAction), Changed<feathrise_editor_ui::EditorButtonClick>>,
    mut outer_scrolls: Query<
        &mut ScrollPosition,
        With<crate::components::SceneEditorLeftPanelScroll>,
    >,
    mut external: MessageWriter<crate::ui::external_files::OpenExternalFile>,
) {
    if state.reference_requested && state.reference_root.as_ref().is_some_and(|root| *root != documents.root) {
        state.invalidate("路径查询范围已变化，请重新查询");
    }
    let shown = state.open
        && sidebar.assets_browser_visible()
        && layout
            .as_ref()
            .is_none_or(|layout| layout.content_shown());
    if gate.blocked(&editor) {
        if (state.reference_requested && (!editor.open || editor.close_requested || editor.pending_scene_transition.is_some()))
            || state.pending.is_some()
            || state.jump.is_some()
            || state
                .running
                .as_ref()
                .is_some_and(|request| request.ticket == state.ticket)
        {
            let hint = if state.missing_assets { "缺失检查已取消，点击重新检查" }
                else if state.reference_target.is_some() { "引用查询已取消，点击重新查找" }
                else { "搜索已取消，按Enter重新搜索" };
            state.invalidate(hint);
        }
        if (!editor.open || editor.close_requested)
            && focus.get().is_some_and(|entity| fields.contains(entity))
        {
            focus.clear();
        }
        return;
    }
    if !shown && focus.get().is_some_and(|entity| fields.contains(entity)) {
        focus.clear();
    }
    if shown && !state.path_query() {
        for (_, field, input) in &fields {
            let value = input.value().to_string();
            let old = match field {
                SearchField::Query => &state.query,
                SearchField::Filter => &state.filter,
            };
            if old != &value {
                match field {
                    SearchField::Query => state.query = value,
                    SearchField::Filter => state.filter = value,
                }
                state.invalidate("条件已变化，按Enter搜索 · 文件类型留空为全部受支持文本");
            }
        }
    }
    if inputs.iter().any(busy) {
        return;
    }
    if state.reference_requested && state.path_query() {
        // 右键菜单先冻结短路径，输入资格恢复后才共享至多8份已确认草稿；沿用唯一搜索槽。
        state.reference_requested = false;
        state.hint = if state.missing_assets { "正在检查资产路径…可取消" } else { "正在查看直接引用…可取消" }.into();
        state.pending = Some(SearchRequest {
            ticket: state.ticket, root: documents.root.clone(), folder: String::new(),
            query: state.reference_target.clone().unwrap_or_default(), references: true, missing_assets: state.missing_assets,
            filters: Vec::new(), case_sensitive: true, whole_word: false,
            // 工程源码与资产共用会话容量，但资产查询只冻结其自身受检根内的草稿。
            drafts: documents.documents.iter().filter(|doc| !source::is_project(&doc.path)).map(|doc| Draft {
                stamp: DraftStamp { key: doc.source_key(), revision: doc.revision },
                text: doc.source.clone(), dirty: doc.dirty(),
            }).collect(),
            cancel: AssetSearchState::cancellation(),
        });
        for mut scroll in &mut outer_scrolls { scroll.y = 0.0; }
        focus.clear();
    }
    let mut actions: Vec<_> = buttons
        .iter()
        .filter_map(|(interaction, action)| {
            (interaction.is_clicked()).then_some(*action)
        })
        .collect();
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if ctrl && shift && keys.just_pressed(KeyCode::KeyF) {
        if state.path_query() { state.text_search(); }
        if !state.open {
            actions.push(SearchAction::Toggle);
        }
        sidebar.navigator = EditorNavigatorTab::Assets; // 搜索只打开左侧资产导航，不覆盖对象/文件属性意图。
        if let Some(layout) = layout.as_mut() {
            if let Some((_, inspector)) = layout.focus_restore.take() {
                layout.inspector_visible = inspector;
            }
            layout.content_visible = true;
        }
        for mut scroll in &mut outer_scrolls {
            scroll.y = 0.0;
        }
        if let Some((entity, _, _)) = fields
            .iter()
            .find(|(_, kind, _)| **kind == SearchField::Query)
        {
            focus.set(entity, FocusCause::Navigated);
        }
        keys.clear_just_pressed(KeyCode::KeyF); // 与当前文件Ctrl+F共享实体键盘，先消费组合边沿。
    }
    if shown && focus.get().is_some_and(|entity| fields.contains(entity)) {
        if keys.just_pressed(KeyCode::Enter) {
            actions.push(SearchAction::Submit);
            keys.clear_just_pressed(KeyCode::Enter);
        }
        if keys.just_pressed(KeyCode::Escape) {
            actions.push(SearchAction::Toggle);
            keys.clear_just_pressed(KeyCode::Escape);
        }
    }
    for action in actions {
        if !matches!(action, SearchAction::Toggle | SearchAction::MissingAssets) && !state.open {
            continue;
        }
        match action {
            SearchAction::MissingAssets if !state.open || !state.missing_assets => {
                state.request_missing_assets(documents.root.clone());
                for mut scroll in &mut outer_scrolls { scroll.y = 0.0; }
                focus.clear(); // 入口可从折叠状态开始，不能让隐藏搜索输入吞掉后续键盘。
            }
            SearchAction::TextSearch => {
                state.text_search();
                if let Some((entity, _, _)) = fields.iter().find(|(_, kind, _)| **kind == SearchField::Query) {
                    focus.set(entity, FocusCause::Navigated);
                }
            }
            SearchAction::Toggle | SearchAction::MissingAssets => {
                // 两个标题共用结果区。再次点已展开的缺失检查收起，重新执行留在正文。
                if action == SearchAction::Toggle && state.missing_assets {
                    state.text_search();
                } else {
                    state.open = !state.open;
                }
                if state.open && !state.path_query() {
                    if let Some((entity, _, _)) = fields
                        .iter()
                        .find(|(_, kind, _)| **kind == SearchField::Query)
                    {
                        focus.set(entity, FocusCause::Navigated);
                    }
                } else if !state.open {
                    let hint = if state.missing_assets { "缺失检查已收起，展开后可重新检查" }
                        else if state.reference_target.is_some() { "引用查询已收起，展开后可重新查找" }
                        else { "搜索已收起，按Enter重新搜索" };
                    state.invalidate(hint);
                    if focus.get().is_some_and(|entity| fields.contains(entity)) {
                        focus.clear();
                    }
                }
            }
            SearchAction::Cancel => state.invalidate("查询已取消，可重新检查或搜索"),
            SearchAction::AllAssets | SearchAction::CurrentFolder if !state.path_query() => {
                let current_folder = action == SearchAction::CurrentFolder;
                if state.current_folder != current_folder {
                    state.current_folder = current_folder;
                    state.invalidate("范围已变化，按Enter搜索");
                }
            }
            SearchAction::CaseSensitive | SearchAction::WholeWord if !state.path_query() => {
                if action == SearchAction::CaseSensitive {
                    state.case_sensitive = !state.case_sensitive;
                } else {
                    state.whole_word = !state.whole_word;
                }
                state.invalidate("匹配选项已变化，按Enter搜索");
            }
            SearchAction::Submit => {
                if state.missing_assets {
                    state.request_missing_assets(documents.root.clone());
                    continue;
                }
                if let Some(path) = state.reference_target.clone() {
                    state.request_references(path, documents.root.clone());
                    continue;
                }
                if state.query.is_empty() {
                    state.hint = "请输入需要查找的文字".into();
                    continue;
                }
                let filters = match worker::filters(&state.filter) {
                    Ok(filters) => filters,
                    Err(error) => {
                        state.hint = error;
                        continue;
                    }
                };
                state.invalidate("正在搜索…可继续编辑或取消");
                state.pending = Some(SearchRequest {
                    ticket: state.ticket,
                    root: documents.root.clone(),
                    folder: if state.current_folder {
                        catalog.current_folder.clone()
                    } else {
                        String::new()
                    },
                    query: state.query.clone(),
                    references: false,
                    missing_assets: false,
                    filters,
                    case_sensitive: state.case_sensitive,
                    whole_word: state.whole_word,
                    drafts: documents
                        .documents
                        .iter()
                        .filter(|doc| !source::is_project(&doc.path))
                        .map(|doc| Draft {
                            stamp: DraftStamp {
                                key: doc.source_key(),
                                revision: doc.revision,
                            },
                            text: doc.source.clone(),
                            dirty: doc.dirty(),
                        })
                        .collect(),
                    cancel: AssetSearchState::cancellation(),
                });
            }
            SearchAction::ChooseEditor(ide) if shown && gate.available.installed(ide) => {
                state.preferred_editor = Some(ide); // 只选打开工具；不取消查询、不重新扫描、不触碰草稿。
            }
            SearchAction::External { ticket, index, editor: ide, in_project }
                if ticket == state.ticket && gate.available.installed(ide) =>
            {
                let Some(result) = state.current() else { continue; };
                let Some(ResultRow::Hit(file, hit)) = result.rows.get(index) else { continue; };
                let file = &result.files[*file];
                let hit = &file.hits[*hit];
                let position = Some((hit.line, if result.references { hit.column } else { 1 })); // 引用精确列；普通文字搜索沿用逻辑行首定位。
                use crate::ui::external_files::{ExternalEditor, ExternalFileAction, OpenExternalFile};
                let source = matches!(file.identity, ContentIdentity::ProjectSource(_));
                let action = match &file.identity {
                    ContentIdentity::ProjectSource(digest) => ExternalFileAction::ProjectSource {
                        editor: ide, line: hit.line, column: hit.column, digest: *digest, in_project,
                    },
                    _ => match (ide, in_project) {
                        (ExternalEditor::VsCode, false) => ExternalFileAction::VsCode(position),
                        (ExternalEditor::VsCode, true) => ExternalFileAction::VsCodeProject(position),
                        (ExternalEditor::Zed, false) => ExternalFileAction::Zed(position),
                        (ExternalEditor::Zed, true) => ExternalFileAction::ZedProject(position),
                        (ExternalEditor::RustRover, false) => ExternalFileAction::RustRover(position),
                        (ExternalEditor::RustRover, true) => ExternalFileAction::RustRoverProject(position),
                    },
                };
                external.write(OpenExternalFile {
                    path: file.path.clone(), action,
                    has_draft: !source && (file.dirty || documents.has_unsaved_draft(&file.path)),
                });
            }
            SearchAction::Row { ticket, index } if ticket == state.ticket => {
                let Some(result) = state.current() else {
                    continue;
                };
                let Some(ResultRow::Hit(file, hit)) = result.rows.get(index) else {
                    continue;
                };
                let file = &result.files[*file];
                if let ContentIdentity::ProjectSource(digest) = &file.identity {
                    use crate::ui::external_files::{ExternalFileAction, OpenExternalFile};
                    if let Some(ide) = state.selected_editor(&gate.available) {
                        let hit = &file.hits[*hit];
                        external.write(OpenExternalFile { path: file.path.clone(), has_draft: false,
                            action: ExternalFileAction::ProjectSource { editor: ide, line: hit.line, column: hit.column, digest: *digest, in_project: true } });
                        state.hint = format!("已请求用 {} 打开工程并定位源码 · 源码保持只读", ide.name());
                    } else {
                        state.hint = "源码引用已找到；未检测到VS Code、Zed或RustRover，安装后重新打开编辑器即可定位".into();
                    }
                    continue; // 项目源码不导入assets草稿，不改变中央当前文件或选中资产。
                }
                let jump = PendingJump {
                    path: file.path.clone(),
                    identity: file.identity.clone(),
                    range: file.hits[*hit].range.clone(),
                };
                match documents.expand(jump.path.clone(), &mut workspace) {
                    Ok(()) => {
                        editor.selected_asset = Some(jump.path.clone());
                        state.jump = Some(jump);
                        state.hint = "正在中央打开匹配位置…".into();
                        focus.clear();
                    }
                    Err(error) => state.hint = error,
                }
            }
            _ => (),
        }
    }
}

pub(super) fn scroll(
    mut wheels: MessageReader<MouseWheel>,
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    layout: Option<Res<SceneEditorLayoutState>>,
    gate: SearchGate,
    mut state: ResMut<AssetSearchState>,
    mut views: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<SearchResults>,
    >,
) {
    if state.wheel_owned {
        state.wheel_owned = false;
    }
    let delta: f32 = wheels
        .read()
        .map(|event| {
            -event.y
                * if event.unit == MouseScrollUnit::Line {
                    super::constants::ROW_HEIGHT * 3.0
                } else {
                    1.0
                }
        })
        .sum();
    if !state.open
        || !editor.open
        || !sidebar.assets_browser_visible()
        || layout.as_ref().is_some_and(|layout| !layout.content_shown())
        || gate.blocked(&editor)
    {
        return;
    }
    let count = state.current().map_or(0, |result| result.rows.len());
    for (cursor, node, mut scroll) in &mut views {
        if state.reset_scroll {
            scroll.y = 0.0;
        }
        if delta != 0.0 && cursor.cursor_over() && count > 0 {
            let height = node.size().y * node.inverse_scale_factor();
            scroll.y = (scroll.y + delta).clamp(
                0.0,
                (count as f32 * super::constants::ROW_HEIGHT - height).max(0.0),
            );
            state.wheel_owned = true;
        }
    }
    if state.reset_scroll {
        state.reset_scroll = false;
    }
}
