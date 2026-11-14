//! 文件命令只冻结明确目标；后台回执独立于窗口，删除确认与未保存作者保护在提交前完成。

use super::{messages::*, resources::*, types::*};
use crate::{asset_catalog::project_files::EditorProjectFileCatalog, ui::asset_preview::{DocumentState, AnimationEditorState, weights::WeightEditor}};
use bevy::{prelude::*, input_focus::InputFocus, text::EditableText};
use feathrise_editor_project_browser_model::file_operations::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

// 独立作者与游戏内 F12 来源没有宿主工程代次；绑定更换时分配独立高位票据，退役票据不能复用。
static LOCAL_CONTEXT: AtomicU64 = AtomicU64::new(1 << 63);

pub(super) fn control(
    mut state: ResMut<FileManagerState>,
    fixed: Res<FixedFileSelection>,
    project: Option<Res<EditorProjectFileCatalog>>,
    service: Option<Res<FileOperationService>>,
    mut open: MessageReader<OpenFileWindow>,
    mut pickers: MessageReader<OpenAnimationPicker>,
    mut requests: MessageReader<FileManagerCommand>,
    inputs: Query<&EditableText, With<super::components::FileNameInput>>,
    documents: Option<Res<DocumentState>>,
    animation: Option<Res<AnimationEditorState>>,
    weights: Option<Res<WeightEditor>>,
    scene_authors: (Option<Res<crate::resources::SceneEditorState>>, Option<Res<crate::authoring::EditorAuthoringSession>>, Option<Res<crate::ui::scene_documents::SceneDocuments>>),
    mut focus: ResMut<InputFocus>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
) {
    let english = locale.as_deref().is_some_and(|locale| *locale == feathrise_editor_ui::localization::EditorLocale::English);
    if let Some(project) = &project {
        let source_context = project.source_context().unwrap_or(0);
        let root = project.project_root().map(PathBuf::from);
        if state.project != root || state.source_context != Some(source_context) {
            let context = match (root.is_some(), source_context) {
                (false, _) => 0,
                (true, 0) => LOCAL_CONTEXT.fetch_add(1, Ordering::Relaxed),
                (true, context) => context,
            };
            if let Some(service) = &service { service.cancel_context(state.context); }
            state.context = context;
            state.source_context = Some(source_context);
            state.project = root.clone();
            state.allowed_roots.clear();
            if let Some(root) = root { state.allowed_roots.push(root); }
            let windows = std::mem::take(&mut state.windows);
            bevy::tasks::IoTaskPool::get().spawn(async move { drop(windows); }).detach();
            state.focused = None;
            state.clipboard.clear();
            state.gesture = None;
            state.drag = None;
            state.prompt = None;
            state.menu = None;
        }
    }
    for request in open.read() { state.open(request.path.clone()); }
    for request in pickers.read() { state.open_animation_picker(request); }
    if let Some(prompt) = &mut state.prompt {
        if prompt.initialized && prompt.action != FileAction::Delete {
            if let Ok(input) = inputs.single() {
                if !input.is_composing() && input.pending_edits.is_empty() && input.pending_paste.is_none() {
                    prompt.value = input.value().to_string();
                }
            }
        }
    }
    let destructive_blocker = documents.as_ref().and_then(|documents| documents.exit_blocker())
        .or_else(|| animation.as_ref().and_then(|animation| animation.exit_blocker()))
        .or_else(|| weights.as_ref().filter(|weights| weights.dirty()).map(|_| "存在未保存蒙皮权重"))
        .or_else(|| scene_authors.0.as_ref().filter(|editor| editor.dirty).map(|_| "存在未保存场景"))
        .or_else(|| scene_authors.1.as_ref().filter(|session| session.saving || session.has_scene_data_changes()).map(|_| "场景作者数据尚未保存或保存仍在进行"))
        .or_else(|| scene_authors.2.as_ref().and_then(|scenes| scenes.exit_blocker(scene_authors.0.as_ref().and_then(|editor| editor.selected_scene_id.as_ref()), false)));
    for request in requests.read() {
        let directory = request.path.clone().filter(|path| !path.as_os_str().is_empty()).unwrap_or_else(|| fixed.directory.clone());
        // 来源由发出命令的具体窗口冻结；空选择不能借用左侧另一窗口的选中项。
        let sources = request.sources.clone();
        match request.action {
            FileAction::NewWindow => {
                if !directory.as_os_str().is_empty() { state.open(directory); }
            }
            FileAction::Copy | FileAction::Cut => {
                if sources.is_empty() { state.notice = "请先选择文件或文件夹".into(); continue; }
                state.cut = request.action == FileAction::Cut;
                state.clipboard = sources;
                state.clipboard.sort();
                state.clipboard.dedup();
                state.notice = if state.cut { format!("已剪切 {} 项；粘贴后才移动源文件", state.clipboard.len()) }
                    else { format!("已复制 {} 项；复制的是磁盘版本，未保存草稿不包含在内", state.clipboard.len()) };
            }
            FileAction::Paste => {
                if state.clipboard.is_empty() { state.notice = "文件剪贴板为空".into(); continue; }
                if state.cut && destructive_blocker.is_some() {
                    state.notice = format!("请先保存并关闭文件草稿，再移动文件：{}", destructive_blocker.unwrap()); continue;
                }
                let operation = if state.cut { FileOperation::Move { sources: state.clipboard.clone(), destination: directory, conflict: state.conflict } }
                    else { FileOperation::Copy { sources: state.clipboard.clone(), destination: directory, conflict: state.conflict } };
                submit(&mut state, service.as_deref(), operation);
            }
            FileAction::CopyHere | FileAction::MoveHere => {
                if request.action == FileAction::MoveHere && destructive_blocker.is_some() {
                    state.notice = "请先保存并关闭文件草稿，再拖动源文件".into(); continue;
                }
                let operation = if request.action == FileAction::MoveHere { FileOperation::Move { sources, destination: directory, conflict: state.conflict } }
                    else { FileOperation::Copy { sources, destination: directory, conflict: state.conflict } };
                submit(&mut state, service.as_deref(), operation);
            }
            FileAction::Delete | FileAction::Rename | FileAction::NewFolder | FileAction::NewText | FileAction::NewMarkdown | FileAction::NewGlb => {
                if !request.action.creates_entry() && sources.is_empty() { state.notice = "请先选择文件或文件夹".into(); continue; }
                if !request.action.creates_entry() && destructive_blocker.is_some() {
                    state.notice = format!("请先保存并关闭文件草稿，再整理源文件：{}", destructive_blocker.unwrap()); continue;
                }
                if request.action == FileAction::Rename && sources.len() != 1 { state.notice = "重命名时请选择一项".into(); continue; }
                let value = if request.action == FileAction::Rename { sources[0].file_name().unwrap_or_default().to_string_lossy().into_owned() }
                    else { new_entry_name(request.action, english).into() };
                state.prompt = Some(NamePrompt { action: request.action, directory, sources, value, initialized: false });
                state.menu = None;
                state.release_guard = true;
            }
            FileAction::Confirm => {
                if state.prompt.as_ref().is_some_and(|prompt| prompt.action != FileAction::Delete)
                    && inputs.single().is_ok_and(|input| input.is_composing() || !input.pending_edits.is_empty() || input.pending_paste.is_some()) {
                    state.notice = "请先完成输入法候选或等待粘贴完成，再确认文件名".into(); continue;
                }
                if let Some(prompt) = &state.prompt {
                    if !creation_extension_matches(prompt.action, &prompt.value) {
                        state.notice = if english { format!("Keep the .{} extension for this file type", prompt.action.extension().unwrap()) }
                            else { format!("此类型文件名必须保留 .{} 扩展名", prompt.action.extension().unwrap()) };
                        continue;
                    }
                }
                let Some(prompt) = state.prompt.take() else { continue; };
                if !prompt.action.creates_entry() && destructive_blocker.is_some() {
                    state.notice = "仍有未保存草稿，请先保存并关闭，再整理源文件".into(); continue;
                }
                let operation = match prompt.action {
                    FileAction::NewFolder => FileOperation::CreateFolder { parent: prompt.directory, name: prompt.value },
                    FileAction::NewText | FileAction::NewMarkdown | FileAction::NewGlb => FileOperation::CreateFile {
                        parent: prompt.directory, name: prompt.value, kind: match prompt.action {
                            FileAction::NewText => NewFileKind::Text,
                            FileAction::NewMarkdown => NewFileKind::Markdown,
                            _ => NewFileKind::Glb,
                        },
                    },
                    FileAction::Rename => FileOperation::Rename { source: prompt.sources[0].clone(), name: prompt.value },
                    FileAction::Delete => FileOperation::Delete { sources: prompt.sources },
                    _ => continue,
                };
                focus.clear();
                state.release_guard = true;
                submit(&mut state, service.as_deref(), operation);
            }
            FileAction::Cancel => { state.prompt = None; focus.clear(); state.release_guard = true; }
            FileAction::ConflictPolicy => {
                state.conflict = match state.conflict { ConflictPolicy::KeepBoth => ConflictPolicy::Skip, ConflictPolicy::Skip => ConflictPolicy::Fail, ConflictPolicy::Fail => ConflictPolicy::KeepBoth };
            }
            FileAction::CancelTask => {
                if let (Some(service), Some(active)) = (service.as_ref(), state.snapshot.as_ref().and_then(|snapshot| snapshot.active.as_ref())) {
                    service.cancel(active.id);
                    state.notice = "已请求取消；已经完成的条目将保留并报告".into();
                }
            }
            _ => {}
        }
    }
}

fn new_entry_name(action: FileAction, english: bool) -> &'static str {
    match (action, english) {
        (FileAction::NewFolder, false) => "新建文件夹", (FileAction::NewFolder, true) => "New folder",
        (FileAction::NewText, false) => "新建文本文档.txt", (FileAction::NewText, true) => "New text document.txt",
        (FileAction::NewMarkdown, false) => "新建文档.md", (FileAction::NewMarkdown, true) => "New document.md",
        (FileAction::NewGlb, false) => "新建场景.glb", (FileAction::NewGlb, true) => "New scene.glb",
        _ => "",
    }
}

pub(super) fn creation_extension_matches(action: FileAction, value: &str) -> bool {
    // 类型由显式“新建”动作决定；改错后缀时保留输入，不伪装成另一类型，也不静默改名。
    action.extension().is_none_or(|extension| std::path::Path::new(value).extension()
        .and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case(extension)))
}

fn submit(state: &mut FileManagerState, service: Option<&FileOperationService>, operation: FileOperation) {
    let Some(service) = service else { state.notice = "文件操作服务尚未就绪".into(); return; };
    match service.enqueue(FileOperationRequest { context: state.context, allowed_roots: state.allowed_roots.clone(), operation }) {
        Ok(id) => state.notice = format!("文件任务 #{id} 已排队；可继续浏览或关闭文件窗口"),
        Err(error) => state.notice = error,
    }
}

pub(super) fn poll(mut state: ResMut<FileManagerState>, service: Option<Res<FileOperationService>>) {
    let Some(snapshot) = service.as_ref().and_then(|service| service.snapshot()) else { return; };
    if snapshot.revision == state.operation_revision { return; }
    state.operation_revision = snapshot.revision;
    for receipt in &snapshot.completed {
        if receipt.id <= state.last_receipt { continue; }
        state.last_receipt = receipt.id;
        if receipt.context != state.context { continue; }
        state.refresh_pending |= !receipt.completed.is_empty(); // 同帧后续失败回执不能抹掉前一任务已经落盘的刷新。
        if receipt.kind == FileOperationKind::Move && !receipt.completed.is_empty() {
            // 只清除已真正搬移的剪切条目；失败或取消的剩余源仍可再次粘贴。
            let moved: std::collections::HashSet<_> = receipt.completed.iter().filter(|change| !change.source_retained)
                .filter_map(|change| change.source.as_deref()).map(crate::ui::external_files::platform_path).collect();
            // canonicalize 回执在 Windows 带扩展前缀；只做词法转换，不在主帧重新访问磁盘。
            state.clipboard.retain(|source| !moved.contains(&crate::ui::external_files::platform_path(source)));
        }
        state.notice = format!("任务 #{}：{} 项完成，{} 项跳过{}{}", receipt.id, receipt.completed.len(), receipt.skipped.len(),
            if receipt.cancelled { "，已取消" } else { "" }, receipt.error.as_ref().map_or(String::new(), |error| format!("；{error}")));
        if matches!(receipt.kind, FileOperationKind::Move | FileOperationKind::Rename | FileOperationKind::Delete) && !receipt.completed.is_empty() {
            state.notice.push_str("。文件路径已改变，已有场景或源码引用请按需更新");
        }
    }
    if state.refresh_pending {
        for window in &mut state.windows { window.refresh(); }
        state.refresh_pending = false;
    }
    state.snapshot = Some(snapshot);
}
