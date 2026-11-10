//! 草稿切换、完整输入捕获和后台任务采用；旧任务结束前不释放并发槽。

use super::{components::*, constants::*, io, resources::DocumentState, types::*};
use crate::asset_catalog::{DocumentFormat, document_format};
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use crate::{
    resources::{SceneEditorLayoutState, SceneEditorState},
    ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, IoTaskPool, futures::check_ready},
    text::EditableText,
};
use std::sync::Arc;

impl Document {
    pub(super) fn dirty(&self) -> bool {
        self.content_revision() != self.saved_revision
    }
    pub(super) fn source_key(&self) -> SourceKey {
        SourceKey {
            path: self.path.clone(),
            generation: self.generation,
        }
    }
    pub(super) fn source_text(&self) -> &str {
        &self.source
    }
}

impl DocumentState {
    /// 路径复制只借用现有根身份，不能规范化文件系统而把当前工作树替换成链接目标。
    pub(crate) fn asset_root(&self) -> &std::path::Path { &self.root }

    #[cfg(test)]
    pub(in super::super) fn set_asset_root_for_tests(&mut self, root: std::path::PathBuf) { self.root = root; }

    /// 外部打开只提示磁盘版本；不把未保存草稿隐式提交到外部工具。
    pub(crate) fn has_unsaved_draft(&self, path: &str) -> bool {
        self.documents.iter().any(|doc| doc.path == path && doc.dirty())
    }

    /// 仅文件首次读取确已失败时压缩空画布；保存/解析诊断不得隐藏已有正文与草稿。
    pub(in super::super) fn load_failed(&self) -> bool {
        self.current().is_none() && self.pending_load.is_none() && self.io.is_none() && !self.error.is_empty()
    }

    pub(super) fn current(&self) -> Option<&Document> {
        self.documents
            .iter()
            .find(|doc| self.active.as_deref() == Some(doc.path.as_str()))
    }
    pub(super) fn current_mut(&mut self) -> Option<&mut Document> {
        let path = self.active.as_deref()?;
        self.documents.iter_mut().find(|doc| path == doc.path)
    }
    pub(crate) fn owns_shortcuts(&self) -> bool {
        self.visible && self.active.is_some()
    }
    pub(crate) fn owns_wheel(&self) -> bool {
        self.visible && self.wheel_owned
    }
    pub(crate) fn in_center(&self) -> bool {
        self.central_active
    }
    pub(crate) fn expanded(&self) -> bool {
        self.expanded.is_some() || !self.parked.is_empty()
    }
    pub(in super::super) fn panel_in_center(&self) -> bool {
        self.expanded.is_some()
    }

    pub(in super::super) fn tab_count(&self) -> usize {
        self.expanded.iter().count() + self.parked.len()
    }

    pub(in super::super) fn active_tab_id(&self) -> Option<u64> {
        self.expanded.as_ref().map(|tab| tab.id)
    }

    pub(super) fn tabs(&self) -> impl Iterator<Item = &ExpandedDocument> {
        self.expanded.iter().chain(self.parked.iter())
    }

    /// 批量关闭只释放中央身份；所有原文、脏草稿、原生状态和在途IO仍留在本进程会话。
    pub(in super::super) fn close_all_tabs(&mut self) -> Option<(String, PresetWorkspaceTab)> {
        self.parked.clear();
        self.central_active = false;
        self.expanded.take().map(|tab| (tab.path, tab.return_tab))
    }

    /// 仅关闭无草稿的文件身份；尚未载入的只读标签也可关闭，不取消文件任务或历史。
    pub(in super::super) fn close_unmodified_tabs(&mut self) -> Option<(String, PresetWorkspaceTab)> {
        self.parked.retain(|tab| {
            self.documents.iter().any(|doc| doc.path == tab.path && doc.dirty())
                || self.saving_batch.iter().any(|key| key.path == tab.path)
        });
        if self.expanded.as_ref().is_none_or(|tab| {
            self.documents.iter().any(|doc| doc.path == tab.path && doc.dirty())
                || self.saving_batch.iter().any(|key| key.path == tab.path)
        }) {
            return None;
        }
        let removed = self.expanded.take().map(|tab| (tab.path, tab.return_tab));
        self.expanded = self.parked.pop(); // 活动已保存文件关闭后，优先继续最近使用的未保存文档。
        if self.expanded.is_none() {
            self.central_active = false;
        }
        removed
    }

    /// 放大固定路径并复用已有标签；满槽拒绝新文件，不替换当前标签或丢弃草稿。
    pub(in super::super) fn expand(
        &mut self,
        path: String,
        workspace: &mut VegetationPresetEditorState,
    ) -> Result<(), String> {
        if !self.tabs().any(|tab| tab.path == path) && self.tab_count() >= MAX_DOCUMENTS {
            return Err("8个文档标签已打开，请先关闭一个标签再打开新文件；草稿仍会保留".into());
        }
        self.reserve_slot(&path)?;
        let return_tab = if workspace.active_tab == PresetWorkspaceTab::Document {
            self.expanded
                .as_ref()
                .map_or(PresetWorkspaceTab::Scene, |doc| doc.return_tab)
        } else {
            workspace.active_tab
        };
        if self.expanded.as_ref().is_none_or(|tab| tab.path != path) {
            let entry = self.parked.iter().position(|tab| tab.path == path)
                .map(|index| self.parked.remove(index));
            if let Some(previous) = self.expanded.take() {
                self.parked.push(previous);
            }
            self.expanded = Some(entry.unwrap_or_else(|| {
                self.next_tab = self.next_tab.wrapping_add(1).max(1);
                ExpandedDocument { id: self.next_tab, path, return_tab }
            }));
        }
        self.expanded.as_mut().unwrap().return_tab = return_tab;
        workspace.active_tab = PresetWorkspaceTab::Document;
        self.center_activation = self.center_activation.wrapping_add(1);
        workspace.touch_ui();
        Ok(())
    }

    /// 尚未读完的后台标签也预留会话槽；快速双击多个文件不能绕过8份草稿预算。
    fn reserve_slot(&mut self, path: &str) -> Result<(), String> {
        loop {
            let unloaded_tabs = self.tabs().filter(|tab| {
                !self.documents.iter().any(|doc| doc.path == tab.path)
            }).count();
            let new_path = !self.documents.iter().any(|doc| doc.path == path)
                && !self.tabs().any(|tab| tab.path == path);
            if self.documents.len() + unloaded_tabs + usize::from(new_path) <= MAX_DOCUMENTS {
                return Ok(());
            }
            if let Some(index) = self.documents.iter().position(|doc| {
                !doc.dirty()
                    && self.saving.as_deref() != Some(&doc.path)
                    && !self.saving_batch.iter().any(|key| key.path == doc.path && key.generation == doc.generation)
                    && !self.tabs().any(|tab| tab.path == doc.path)
                    && doc.path != path
            }) {
                self.documents.remove(index);
            } else {
                return Err("8份文档会话已占满，请先保存或丢弃草稿，并关闭一个文档标签后再打开新文件".into());
            }
        }
    }

    pub(super) fn request_load(&mut self, path: String) {
        if let Err(error) = self.reserve_slot(&path) {
            self.error = error;
            return;
        }
        self.load_ticket = self.load_ticket.wrapping_add(1);
        let expected_document = self.documents.iter().find(|doc| doc.path == path)
            .map(|doc| (doc.source_key(), doc.revision));
        self.pending_load = Some(LoadRequest {
            path,
            ticket: self.load_ticket,
            expected_document,
        });
        self.error.clear();
    }

    pub(super) fn request_save(&mut self) {
        if self.io.is_some() || self.pending_load.is_some() {
            if let Some(doc) = self.current_mut() {
                doc.status = "文件任务进行中，请完成后再保存".into();
            }
            return;
        }
        let Some(doc) = self.current_mut() else {
            return;
        };
        if !doc.dirty() {
            doc.status = "文档没有未保存改动".into();
            return;
        }
        doc.confirm_reload = false;
        doc.history.break_group(); // 保存点不能藏在连续输入分组内；保存之后的输入另成一步。
        let path = doc.path.clone();
        if super::json::before_save(self, &path) {
            self.start_save(&path);
        }
    }

    /// JSON仅在同修订校验通过后抵达这里；切文件仍结清用户明确提交的原文件。
    pub(super) fn start_save(&mut self, path: &str) {
        let Some(doc) = self.documents.iter_mut().find(|doc| doc.path == path) else { return; };
        doc.status = "正在后台保存完整文档…".into();
        let path = doc.path.clone();
        let generation = doc.generation;
        let revision = doc.content_revision(); // 保存回执结清内容身份，撤销仍使用单调增长的派生修订。
        let disk = doc.disk.clone();
        let pages = doc.pages.clone(); // 保存只共享有界源分块的Arc，不复制全文。
        let roots = self.source_roots();
        self.saving = Some(path.clone());
        self.io = Some(IoTaskPool::get().spawn(async move {
            IoResult::Saved {
                result: io::save_source(&roots, &path, &disk, &pages),
                path,
                generation,
                revision,
            }
        }));
    }
}

/// 本帧官方输入先应用，然后捕获完整业务值，最后才允许保存或切换资产。
pub(super) fn capture_edits(
    mut state: ResMut<DocumentState>,
    capture: Option<Res<super::history::HistoryCapture>>,
    sources: Query<(&DocumentSource, &EditableText), Changed<EditableText>>,
) {
    if sources.is_empty() {
        return;
    } // 稳定帧不复制全文，也不标记草稿资源Changed。
    if state.active.as_deref().is_some_and(|path| state.is_loading(path)) {
        return; // 即使其他入口切回Edit，旧绑定的本帧残余输入也不能写进待替换正文。
    }
    for (binding, text) in &sources {
        let Some(doc) = state.current().filter(|doc| doc.mode == ViewMode::Edit) else {
            return;
        };
        if !text.is_composing()
            && binding.key.as_ref() == Some(&doc.source_key())
            && text.value() != doc.source_text()
        {
            let confirmed = text.value().to_string(); // 只复制实际变化的已确认全文；光标与IME候选不复制缓冲。
            let after = super::history::Selection::read(text);
            let context = capture.as_ref().map_or(
                super::history::EditContext { after, ..default() },
                |capture| capture.context(&doc.source_key(), after),
            );
            state.current_mut().unwrap().edit_with(&confirmed, context);
        }
    }
}

pub(super) fn visible(
    editor: &SceneEditorState,
    sidebar: &SceneEditorSidebarState,
    layout: Option<&SceneEditorLayoutState>,
    central: bool,
) -> bool {
    editor.open
        && !editor.close_requested
        && (central
            || (sidebar.right == EditorRightSidebarTab::Assets
                && layout.is_none_or(SceneEditorLayoutState::inspector_shown)))
        && !editor.new_scene_dialog_open
}

pub(super) fn sync_selection(
    preview: Res<super::super::types::AssetPreviewState>,
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    workspace: Option<Res<VegetationPresetEditorState>>,
    layout: Option<Res<SceneEditorLayoutState>>,
    replacement: Option<Res<super::search::AssetReplaceState>>,
    mut state: ResMut<DocumentState>,
) {
    if state.expanded.is_none()
        && workspace.as_ref().is_some_and(|workspace| workspace.active_tab == PresetWorkspaceTab::Document)
        && !state.parked.is_empty()
    {
        state.expanded = state.parked.pop(); // 从其他作者页返回文档宿主时恢复最近使用的剩余标签。
    }
    let central = state.expanded.is_some()
        && workspace.is_some_and(|workspace| workspace.active_tab == PresetWorkspaceTab::Document);
    if state.central_active != central {
        state.central_active = central;
    }
    let shown = visible(&editor, &sidebar, layout.as_deref(), central)
        && (central || state.expanded.is_none())
        && replacement.as_ref().is_none_or(|state| !state.preview_visible()); // 差异预览也停放唯一源码，不能让隐藏正文接收输入。
    if state.visible != shown {
        state.visible = shown;
    }
    let wanted = if state.expanded.is_some() {
        state.expanded.as_ref().map(|doc| doc.path.as_str())
    } else if matches!(
        preview.content,
        super::super::types::AssetPreviewContent::Document
    ) {
        preview.selected_path.as_deref()
    } else {
        None
    };
    if state.active.as_deref() == wanted {
        return;
    }
    let wanted = wanted.map(str::to_owned);
    state.active = wanted.clone();
    state.error.clear();
    state.pending_load = None;
    state.load_ticket = state.load_ticket.wrapping_add(1); // 已运行旧IO继续占槽，结果因票据失效不采用。
    if let Some(path) = wanted.filter(|path| !state.documents.iter().any(|doc| doc.path == *path)) {
        state.request_load(path);
    }
}

/// IO和解析各一个槽；结果只在路径、文档代次及内容修订仍一致时原子采用。
pub(super) fn drive_tasks(
    mut state: ResMut<DocumentState>,
    content: Res<feathrise_editor_documents_model::DocumentContentOperations>,
    mut batch: Option<ResMut<super::batch_save::DocumentBatchSaveState>>,
    sources: Query<&EditableText, With<DocumentSource>>,
) {
    if state.io.is_some()
        && let Some(result) = state.io.as_mut().and_then(check_ready)
    {
        state.io = None;
        state.loading = None; // 成功、失败及旧票据均结清读取身份，失败后原草稿可以重新编辑。
        state.saving = None;
        state.saving_batch.clear();
        match result {
            IoResult::Checked(reply) => state.external.ready = Some(reply),
            IoResult::SavedBatch(reply) => super::batch_save::adopt(&mut state, batch.as_deref_mut(), reply),
            IoResult::Loaded(request, result) => {
                if request.ticket == state.load_ticket
                    && state.active.as_deref() == Some(&request.path)
                {
                    let current = state.documents.iter().find(|doc| doc.path == request.path)
                        .map(|doc| (doc.source_key(), doc.revision));
                    match result {
                        Ok(_) if current != request.expected_document => {
                            // 输入解绑之外仍复核完整旧会话身份，不能覆盖确认后发生的撤销或业务改字。
                            let error = "重新载入期间文档已变化，已取消载入；当前草稿和历史已保留";
                            if let Some(doc) = state.current_mut() { doc.status = error.into(); }
                            state.error = error.into();
                        }
                        Ok((disk, pages)) => {
                            let format =
                                document_format(&request.path).expect("IO只采用白名单文本文档");
                            let document = Document {
                                path: request.path.clone(),
                                format,
                                generation: request.ticket,
                                disk,
                                pages: pages.pages,
                                source: pages.text,
                                revision: 0,
                                saved_revision: 0,
                                history: default(),
                                mode: ViewMode::Preview,
                                preview_scroll: Vec2::ZERO,
                                preview: Arc::new(Vec::new()),
                                preview_revision: None,
                                preview_clipped: false,
                                confirm_reload: false,
                                external: default(),
                                status: if format == DocumentFormat::Markdown {
                                    "Markdown排版预览 · UTF-8 · 保存写回完整文件"
                                } else {
                                    "原文预览 · UTF-8 · 保存写回完整文件"
                                }
                                .into(),
                            };
                            if let Some(old) = state
                                .documents
                                .iter_mut()
                                .find(|doc| doc.path == request.path)
                            {
                                *old = document;
                            } else {
                                state.documents.push(document);
                            }
                            state.error.clear();
                        }
                        Err(error) => {
                            if let Some(doc) = state.current_mut() {
                                doc.status = error.clone();
                            }
                            state.error = error;
                        }
                    }
                }
            }
            IoResult::Saved {
                path,
                generation,
                revision,
                result,
            } => {
                // 保存是用户显式发出的文件动作，即使已切资产也要结清原文档，不污染新文档。
                if let Some(doc) = state
                    .documents
                    .iter_mut()
                    .find(|doc| doc.path == path && doc.generation == generation)
                {
                    match result {
                        Ok(disk) => {
                            doc.disk = disk;
                            doc.saved_revision = revision;
                            doc.external = default();
                            doc.status = if doc.dirty() {
                                "已保存提交时的完整文档；之后的新增改动仍为草稿"
                            } else {
                                "已保存完整文档"
                            }
                            .into();
                        }
                        Err(error) => doc.status = error,
                    }
                }
            }
        }
    }
    // 先采用已完成核对，再启动新加载；刷新代次不能使本帧刚启动的加载票据失效。
    if state.external.ready.is_some() || (state.external.available && state.external.requested && state.io.is_none()) {
        let busy = sources.iter().any(|text| text.is_composing()
            || text.pending_paste.is_some() || !text.pending_edits.is_empty());
        super::external::drive(&mut state, busy);
    }
    if state.io.is_none() {
        if let Some(request) = state.pending_load.take() {
            let roots = state.source_roots();
            state.loading = Some(request.clone());
            state.io = Some(IoTaskPool::get().spawn(async move {
                let result = io::load_source(&roots, &request.path);
                IoResult::Loaded(request, result)
            }));
        }
    }
    if state.parser.is_some()
        && let Some(result) = state.parser.as_mut().and_then(check_ready)
    {
        state.parser = None;
        if let Some(doc) = state.documents.iter_mut().find(|doc| {
            doc.path == result.path
                && doc.generation == result.generation
                && doc.revision == result.revision
        }) {
            doc.preview = result.pages;
            doc.preview_clipped = result.clipped;
            doc.preview_revision = Some(result.revision);
        }
    }
    if state.parser.is_none() && state.visible {
        if let Some(doc) = state.current().filter(|doc| {
            doc.mode == ViewMode::Preview && doc.preview_revision != Some(doc.revision)
        }) {
            let path = doc.path.clone();
            let generation = doc.generation;
            let revision = doc.revision;
            let pages = doc.pages.clone();
            let format = doc.format;
            let parse = content.parse_preview; // 后台仅捕获静态操作，不捕获World或另一份会话。
            state.parser = Some(AsyncComputeTaskPool::get().spawn(async move {
                let parsed = parse(format, &pages);
                PreviewResult {
                    path,
                    generation,
                    revision,
                    pages: parsed.pages,
                    clipped: parsed.clipped,
                }
            }));
        }
    }
}
