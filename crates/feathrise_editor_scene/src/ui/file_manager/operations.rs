//! 文件命令只冻结明确目标；后台回执独立于窗口，删除确认与未保存作者保护在提交前完成。

use super::{file_identity::{FileWriteReservations, ProtectedAuthors}, messages::*, resources::*, types::*};
use crate::asset_catalog::project_files::EditorProjectFileCatalog;
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
    mut authors: ProtectedAuthors,
    mut reservations: ResMut<FileWriteReservations>,
    mut focus: ResMut<InputFocus>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
) {
    let english = locale.as_deref().is_some_and(|locale| *locale == feathrise_editor_ui::localization::EditorLocale::English);
    if let Some(project) = &project {
        if state.asset_root.as_deref() != project.asset_root() { state.asset_root = project.asset_root().map(PathBuf::from); }
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
            replace_clipboard(&mut state, FileSources::default(), false);
            state.gesture = None;
            state.drag = None;
            state.prompt = None;
            state.reference_reviews.clear();
            state.menu = None;
            state.notice.clear(); // 文件回执只属于打开它的工程会话。
        }
    }
    for request in open.read() { state.open(request.path.clone()); }
    for request in pickers.read() { state.open_animation_picker(request); }
    if let Some(prompt) = &mut state.prompt {
        if prompt.initialized && prompt.has_input() {
            if let Ok(input) = inputs.single() {
                if !input.is_composing() && input.pending_edits.is_empty() && input.pending_paste.is_none() {
                    prompt.value = input.value().to_string();
                }
            }
        }
    }
    publish_clipboard(&mut state, service.as_deref());
    paste(&mut state, service.as_deref(), &authors, &mut reservations);
    for request in requests.read() {
        if state.prompt.as_ref().is_some_and(|p| p.action == FileAction::ReviewReferences) {
            let Some(review) = state.reference_reviews.front().cloned() else { state.prompt = None; continue; };
            match request.action {
                FileAction::PreviousReferences => state.reference_page = state.reference_page.saturating_sub(1),
                FileAction::NextReferences if (state.reference_page + 1) * super::presentation::REFERENCE_PAGE < review.documents.len() => state.reference_page += 1,
                FileAction::Confirm | FileAction::MoveWithoutRepair => {
                    let update = request.action == FileAction::Confirm;
                    if update && review.unrepresentable || !update && matches!(review.operation, FileOperation::Relink { .. }) { continue; }
                    if update && let Some(reason) = authors.reference_blocker(&review.documents_selection) {
                        state.prompt.as_mut().unwrap().value.clone_from(&reason);
                        state.notice = reason;
                        continue;
                    }
                    let choice = if update { ReferenceUpdate::Confirm(review.clone()) } else { ReferenceUpdate::FilesOnly(review.clone()) };
                    if !enqueue(&mut state, service.as_deref(), &authors, &mut reservations, review.operation.clone(), Some(choice)) { continue; }
                    if update { authors.hold_reference_sessions(&review.documents_selection, &mut reservations); }
                    state.reference_reviews.pop_front();
                    state.prompt = None;
                    state.release_guard = true;
                }
                FileAction::Cancel => {
                    state.reference_reviews.pop_front();
                    state.prompt = None;
                    state.release_guard = true;
                    state.notice = "已取消文件整理".into();
                }
                _ => {}
            }
            continue;
        }
        let directory = request.path.clone().filter(|path| !path.as_os_str().is_empty()).unwrap_or_else(|| fixed.directory.clone());
        // 来源由发出命令的具体窗口冻结；空选择不能借用左侧另一窗口的选中项。
        let sources = request.sources.clone();
        if matches!(request.action, FileAction::Copy | FileAction::Cut | FileAction::Paste)
            && let Some((destination, _)) = &state.pending_paste
        {
            state.notice = format!("粘贴到「{}」的请求正在等待剪贴板同步。请在该请求提交后重试。", destination.display());
            continue;
        }
        match request.action {
            FileAction::SelectRelinkTarget => { if let Some(missing) = &request.path { state.open_relink_picker(missing); } }
            FileAction::Relink => {
                let Some(missing) = state.asset_root.as_ref().and_then(|root| request.path.as_ref()?.strip_prefix(root).ok()?.to_str()) else { continue; };
                let Some((target, stamp)) = sources.iter_with_stamp().next() else { continue; };
                let operation = FileOperation::Relink { missing: missing.replace('\\', "/"), target: target.to_owned(), stamp };
                submit(&mut state, service.as_deref(), &authors, &mut reservations, operation);
            }
            FileAction::NewWindow => {
                if !directory.as_os_str().is_empty() { state.open(directory); }
            }
            FileAction::Copy | FileAction::Cut => {
                if sources.is_empty() { state.notice = "请先选择文件或文件夹".into(); continue; }
                replace_clipboard(&mut state, sources, request.action == FileAction::Cut);
                publish_clipboard(&mut state, service.as_deref());
                state.notice = if state.cut { format!("已剪切 {} 项；粘贴后才移动源文件", state.clipboard.len()) }
                    else { format!("已复制 {} 项；复制的是磁盘版本，未保存草稿不包含在内", state.clipboard.len()) };
            }
            FileAction::Paste => {
                if state.clipboard.is_empty() { state.notice = "文件剪贴板为空".into(); continue; }
                state.pending_paste = Some((directory, state.conflict));
                paste(&mut state, service.as_deref(), &authors, &mut reservations);
                if let Some((destination, _)) = &state.pending_paste {
                    state.notice = format!("正在同步文件剪贴板，完成后会粘贴到「{}」", destination.display());
                }
            }
            FileAction::CopyHere | FileAction::MoveHere => {
                let operation = if request.action == FileAction::MoveHere { FileOperation::Move { sources, destination: directory, conflict: state.conflict } }
                    else { FileOperation::Copy { sources, destination: directory, conflict: state.conflict } };
                submit(&mut state, service.as_deref(), &authors, &mut reservations, operation);
            }
            FileAction::Delete | FileAction::Rename | FileAction::NewFolder | FileAction::NewText | FileAction::NewMarkdown | FileAction::NewGlb => {
                if !request.action.creates_entry() && sources.is_empty() { state.notice = "请先选择文件或文件夹".into(); continue; }
                if !request.action.creates_entry() && let Some(reason) = authors.blocker(&sources) {
                    state.notice = reason; continue;
                }
                if request.action == FileAction::Rename && sources.len() != 1 { state.notice = "重命名时请选择一项".into(); continue; }
                let value = if request.action == FileAction::Rename { sources.first().and_then(std::path::Path::file_name).unwrap_or_default().to_string_lossy().into_owned() }
                    else { new_entry_name(request.action, english).into() };
                state.prompt = Some(NamePrompt { action: request.action, directory, sources, value, initialized: false });
                state.menu = None;
                state.release_guard = true;
            }
            FileAction::Confirm => {
                if state.prompt.as_ref().is_some_and(NamePrompt::has_input)
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
                let operation = match prompt.action {
                    FileAction::NewFolder => FileOperation::CreateFolder { parent: prompt.directory, name: prompt.value },
                    FileAction::NewText | FileAction::NewMarkdown | FileAction::NewGlb => FileOperation::CreateFile {
                        parent: prompt.directory, name: prompt.value, kind: match prompt.action {
                            FileAction::NewText => NewFileKind::Text,
                            FileAction::NewMarkdown => NewFileKind::Markdown,
                            _ => NewFileKind::Glb,
                        },
                    },
                    FileAction::Rename => {
                        // 确认沿用打开弹窗时冻结的行，等待输入期间同名文件可能已被替换。
                        let (source, stamp) = prompt.sources.iter_with_stamp().next().expect("重命名已冻结单个源文件");
                        FileOperation::Rename { source: source.to_owned(), stamp, name: prompt.value }
                    }
                    FileAction::Delete => FileOperation::Delete { sources: prompt.sources },
                    _ => continue,
                };
                focus.clear();
                state.release_guard = true;
                submit(&mut state, service.as_deref(), &authors, &mut reservations, operation);
            }
            FileAction::Cancel => { state.prompt = None; focus.clear(); state.release_guard = true; }
            FileAction::ConflictPolicy => {
                state.conflict = match state.conflict { ConflictPolicy::KeepBoth => ConflictPolicy::Skip, ConflictPolicy::Skip => ConflictPolicy::Fail, ConflictPolicy::Fail => ConflictPolicy::KeepBoth };
            }
            _ => {}
        }
    }
    if state.prompt.is_none() && !state.reference_reviews.is_empty() {
        state.reference_page = 0;
        state.prompt = Some(NamePrompt { action: FileAction::ReviewReferences, directory: default(), sources: default(), value: default(), initialized: false });
        state.menu = None;
        state.release_guard = true;
    }
    if reservations.is_changed() && let Some(documents) = authors.external.documents.as_mut() {
        documents.file_writes = reservations.clone();
    }
}

fn replace_clipboard(state: &mut FileManagerState, sources: FileSources, cut: bool) {
    state.clipboard_generation = state.clipboard_generation.wrapping_add(1);
    state.clipboard_through = state.last_receipt;
    state.clipboard = sources;
    state.cut = cut;
    state.publish_clipboard = true;
    state.pending_paste = None;
}

fn publish_clipboard(state: &mut FileManagerState, service: Option<&FileOperationService>) {
    if !state.publish_clipboard { return; }
    let clipboard = (!state.clipboard.is_empty()).then(|| FileClipboard {
        context: state.context,
        generation: state.clipboard_generation,
        sources: state.clipboard.clone(),
        through: state.clipboard_through,
    });
    if service.is_some_and(|service| service.publish_clipboard(clipboard)) { state.publish_clipboard = false; }
}

fn adopt_clipboard(state: &mut FileManagerState, clipboard: &FileClipboard) -> bool {
    if (clipboard.context, clipboard.generation) != (state.context, state.clipboard_generation) { return false; }
    state.clipboard = clipboard.sources.clone();
    true
}

fn paste(
    state: &mut FileManagerState,
    service: Option<&FileOperationService>,
    authors: &ProtectedAuthors,
    reservations: &mut FileWriteReservations,
) {
    if state.pending_paste.is_none() { return; }
    let Some(service) = service else {
        state.pending_paste = None;
        state.notice = "文件操作服务尚未就绪".into();
        return;
    };
    // 提交前直接核对后台快照，包含本帧尚未显示的独立拖动回执。等待期间仅保留本次粘贴目的地。
    if !service.snapshot().and_then(|snapshot| snapshot.clipboard).is_some_and(|clipboard| adopt_clipboard(state, &clipboard)) {
        return;
    }
    let (destination, conflict) = state.pending_paste.take().expect("此处有待提交的粘贴");
    if state.clipboard.is_empty() { state.notice = "文件剪贴板为空".into(); return; }
    let sources = state.clipboard.clone();
    let operation = if state.cut { FileOperation::Move { sources, destination, conflict } }
        else { FileOperation::Copy { sources, destination, conflict } };
    submit(state, Some(service), authors, reservations, operation);
}

pub(super) fn cancel(
    mut requests: MessageReader<CancelFileTask>,
    mut state: ResMut<FileManagerState>,
    service: Option<Res<FileOperationService>>,
) {
    for request in requests.read() {
        let Some(service) = service.as_ref() else { continue; };
        // 排队任务可能已接替前一个任务，只接受按钮显示过且仍在途的同一工程、同一任务。
        if state.cancellable_task() != Some(*request) { continue; }
        if service.cancel(request.id) {
            if let Some(active) = state.snapshot.as_mut().and_then(|snapshot| snapshot.active.as_mut()) {
                active.cancel_requested = true;
            }
            state.notice = "已请求取消；已经完成的条目将保留并报告".into();
        } else {
            state.notice = "暂时无法取消文件任务，可重试".into();
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

fn submit(
    state: &mut FileManagerState,
    service: Option<&FileOperationService>,
    authors: &ProtectedAuthors,
    reservations: &mut FileWriteReservations,
    operation: FileOperation,
) {
    let references = match &operation {
        FileOperation::Rename { .. } | FileOperation::Move { .. } | FileOperation::Relink { .. } => state.asset_root.clone().map(ReferenceUpdate::Inspect),
        _ => None,
    };
    enqueue(state, service, authors, reservations, operation, references);
}

fn enqueue(
    state: &mut FileManagerState, service: Option<&FileOperationService>, authors: &ProtectedAuthors,
    reservations: &mut FileWriteReservations, operation: FileOperation, references: Option<ReferenceUpdate>,
) -> bool {
    let Some(service) = service else { state.notice = "文件操作服务尚未就绪".into(); return false; };
    let sources = match &operation {
        FileOperation::Move { sources, .. } | FileOperation::Delete { sources } => Some(sources.clone()),
        FileOperation::Rename { source, .. } => Some(vec![source.clone()].into()),
        _ => None,
    };
    // 确认与入队之间再查受影响草稿。在同一输入回合占用源身份，避免排队期间出现新的写入。
    if let Some(reason) = sources.as_ref().and_then(|sources| authors.blocker(sources)) {
        state.notice = reason;
        return false;
    }
    let documents = match &references { Some(ReferenceUpdate::Confirm(plan)) => Some(plan.documents_selection.clone()), _ => None };
    match service.enqueue(FileOperationRequest { context: state.context, allowed_roots: state.allowed_roots.clone(), operation, references }) {
        Ok(id) => {
            if let Some(sources) = sources { reservations.add(state.context, id, sources); }
            if let Some(documents) = documents { reservations.add(state.context, id, documents); }
            state.notice = format!("文件任务 #{id} 已排队；可继续浏览或关闭文件窗口");
            true
        }
        Err(error) => { state.notice = error; false }
    }
}

pub(super) fn poll(mut state: ResMut<FileManagerState>, service: Option<Res<FileOperationService>>,
    mut search: Option<ResMut<crate::ui::asset_preview::AssetSearchState>>) {
    let Some(snapshot) = service.as_ref().and_then(|service| service.snapshot()) else { return; };
    if snapshot.revision == state.operation_revision { return; }
    state.operation_revision = snapshot.revision;
    for receipt in &snapshot.completed {
        if receipt.id <= state.last_receipt { continue; }
        state.last_receipt = receipt.id;
        if receipt.context != state.context { continue; }
        if let Some(preview) = &receipt.reference_preview {
            state.reference_reviews.push_back(preview.clone());
            state.notice = format!("{} 份场景或预设引用了所选资产，请确认引用更新", preview.documents.len());
            continue;
        }
        for window in &mut state.windows {
            // 不抢占在途导航或定位；这次结果采用后再刷新受影响的目录。
            window.refresh_after_scan |= receipt.affects_directory(&window.requested_path);
        }
        if receipt.kind == FileOperationKind::Relink && receipt.repaired_documents != 0 {
            state.notice = format!("任务 #{}：重新关联完成，已更新 {} 份引用文件", receipt.id, receipt.repaired_documents);
            if let Some(search) = search.as_deref_mut() && search.is_open() && search.is_missing_assets()
                && let Some(root) = state.asset_root.clone() { search.request_missing_assets(root); }
            continue;
        }
        state.notice = format!("任务 #{}：{} 项完成，{} 项跳过{}{}", receipt.id, receipt.completed.len(), receipt.skipped.len(),
            if receipt.cancelled { "，已取消" } else { "" }, receipt.error.as_ref().map_or(String::new(), |error| format!("；{error}")));
        if receipt.repaired_documents != 0 {
            state.notice.push_str(&format!("；已更新 {} 份引用文件", receipt.repaired_documents));
        }
    }
    if let Some(clipboard) = snapshot.clipboard.as_ref() { adopt_clipboard(&mut state, clipboard); }
    state.snapshot = Some(snapshot);
}
