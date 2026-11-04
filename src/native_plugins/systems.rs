//! 主线程处理真实控件输入并非阻塞采用有界响应；磁盘及插件调用始终在串行后台线程。
use super::resources::Event;
use super::{components::*, resources::*};
use bevy::{input_focus::InputFocus, prelude::*, text::EditableText};
use feathrise_editor_core::{AssetPreviewFocus, EditorTextInputGuard, WorkspaceDomainState};
use feathrise_editor_map::game_authoring::{
    self,
    map_api::{
        resources::{EditorAssetCatalog, MapEditorState},
        types::EditorTool,
        ui::{
            sidebars::{EditorLeftSidebarTab, EditorRightSidebarTab, MapEditorSidebarState},
            vegetation_preset_editor::*,
        },
    },
};
use feathrise_editor_ui::{
    EditorButtonClick, NumericInputCommitOutcome, NumericInputCommitted, NumericValue,
};
use feathrise_plugin_api::Request;

pub(super) fn input(
    mut state: ResMut<NativeTools>,
    mut sidebar: ResMut<MapEditorSidebarState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    buttons: Query<(&ToolButton, &EditorButtonClick)>,
    mut fields: Query<(Entity, &mut ToolText, &mut EditableText)>,
    numbers: Query<&ToolNumber>,
    text_targets: Query<(), With<feathrise_editor_core::EditorTextInput>>,
    mut commits: MessageReader<NumericInputCommitted>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    guard: Res<EditorTextInputGuard>,
    mut editor: ResMut<MapEditorState>,
    mut preview_focus: ResMut<AssetPreviewFocus>,
    numeric: Res<feathrise_editor_ui::ActiveNumericInput>,
    scrub: Res<feathrise_editor_ui::ActiveNumericInputScrub>,
    mut catalog: ResMut<EditorAssetCatalog>,
) {
    let clicked = buttons
        .iter()
        .find(|(button, click)| {
            click.is_clicked()
                && (matches!(
                    button.action,
                    UiAction::OpenTool
                        | UiAction::Tab(_)
                        | UiAction::CloseLibrary
                        | UiAction::Close(_)
                ) || state
                    .tools
                    .get(&button.index)
                    .is_some_and(|t| tool_visible(t, &sidebar, &workspace)))
        })
        .map(|(button, _)| button.clone());
    if let Some(ToolButton {
        index,
        action: UiAction::Locate { document, field },
    }) = &clicked
    {
        // 定位只导航当前行的可见输入，不提交字段、不触碰历史，也不借用另一文档的同名控件。
        let valid = state.tools.get(index).is_some_and(|tool| {
            *document != 0
                && tool.snapshot.active == *document
                && tool.snapshot.tabs.iter().any(|tab| tab.id == *document)
                && tool.snapshot.fields.iter().any(|entry| {
                    entry.id == *field
                        && matches!(entry.kind, feathrise_plugin_api::FieldKind::Asset { .. })
                })
        });
        let mut rows = fields.iter().filter(|(_, text, _)| {
            text.index == *index && text.document == *document && text.field == *field
        });
        let row = rows.next().map(|(entity, _, text)| {
            // Parley可将正文分成输入法预编辑前后两段；to_string拼接完整正文，不能只复制SplitString包装。
            (entity, text.value().to_string())
        });
        let unique = rows.next().is_none();
        drop(rows);
        if valid && unique && let Some((entity, path)) = row {
            // 按钮可能暂时取得InputFocus；还给本行才能避免下一帧失焦被误当作字段提交。
            focus.set(entity, bevy::input_focus::FocusCause::Pressed);
            if let Err(error) = game_authoring::map_api::asset_catalog::request_asset_reveal(
                &mut catalog, &mut editor, &mut sidebar, path.trim(),
            ) {
                editor.status = error;
            }
        } else {
            editor.status = "资源字段已换代，请在当前文档重新定位".into();
        }
        state.input_pending = fields.iter().any(|(_, field, text)| {
            text.value() != field.baseline.as_str() || text.is_composing()
        }) || numeric.entity.is_some_and(|entity| numbers.contains(entity))
            || scrub.entity.is_some_and(|entity| numbers.contains(entity));
        return; // 本次导航不能落入后面的文本失焦/按钮提交分支。
    }
    let composing = fields.iter().any(|(_, _, input)| input.is_composing());
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let save = ctrl && keyboard.just_pressed(KeyCode::KeyS);
    let enter = keyboard.just_pressed(KeyCode::Enter) && !guard.block_shortcuts && !composing;
    // 字段先提交、按钮后入队，同一插件串行执行，保存绝不会抢在刚输入的中文之前。
    let mut blocked = false;
    for (entity, mut field, mut input) in &mut fields {
        let visible = state
            .tools
            .get(&field.index)
            .is_some_and(|t| tool_visible(t, &sidebar, &workspace));
        let focused = focus.get() == Some(entity);
        if focused && keyboard.just_pressed(KeyCode::Escape) && !composing {
            field.baseline = input.value().to_string();
            focus.clear();
            state.ui_revision += 1;
            continue;
        }
        if focused
            && enter
            && field.multiline
            && (keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight))
        {
            input.queue_edit(bevy::text::TextEdit::Insert("\n".into()));
            blocked = true;
            continue;
        }
        if input.value() != field.baseline.as_str()
            && (!focused || !visible || clicked.is_some() || enter || save)
            && !composing
        {
            let request = if field.document == 0 {
                Request::Search {
                    value: input.value().to_string(),
                    page: 0,
                }
            } else {
                Request::Edit {
                    document: field.document,
                    field: field.field.clone(),
                    value: input.value().to_string(),
                }
            };
            if let Some(tool) = state.tools.get_mut(&field.index) {
                match tool.send(request, false) {
                    Ok(_) => {
                        field.baseline = input.value().to_string();
                    }
                    Err(error) => {
                        tool.snapshot.status = error;
                        blocked = true;
                    }
                }
            }
        }
        if focused && (!visible || enter || save || clicked.is_some()) && !blocked && !composing {
            focus.clear();
        }
        blocked |= input.value() != field.baseline.as_str() || input.is_composing();
    }
    blocked |= numeric.entity.is_some_and(|e| numbers.contains(e))
        || scrub.entity.is_some_and(|e| numbers.contains(e));
    state.input_pending = blocked;
    for event in commits.read() {
        if event.outcome != NumericInputCommitOutcome::Accepted || (!event.changed && !event.edited)
        {
            continue;
        }
        let Ok(field) = numbers.get(event.entity) else {
            continue;
        };
        let value = match event.value {
            NumericValue::U32(value) => value.to_string(),
            NumericValue::I32(value) => value.to_string(),
            NumericValue::F32(value) if value.is_finite() => value.to_string(),
            _ => continue,
        };
        if let Some(tool) = state.tools.get_mut(&field.index) {
            if let Err(error) = tool.send(
                Request::Edit {
                    document: field.document,
                    field: field.field.clone(),
                    value,
                },
                false,
            ) {
                tool.snapshot.status = error;
            }
        }
    }
    if blocked || composing {
        return;
    }
    let action = clicked.or_else(|| {
        if guard.block_shortcuts {
            return None;
        }
        let (&index, tool) = state
            .tools
            .iter()
            .find(|(_, t)| tool_visible(t, &sidebar, &workspace))?;
        if tool.snapshot.map_domain.is_some() {
            // 地图领域由原地图会话唯一保存/撤销；通用预设快捷键不能再发另一套独立保存。
            if save { editor.save_requested = true; }
            return None;
        }
        if tool.snapshot.active == 0 {
            return None;
        }
        let request = if save {
            Request::Save {
                document: tool.snapshot.active,
            }
        } else if ctrl
            && (keyboard.just_pressed(KeyCode::KeyZ) || keyboard.just_pressed(KeyCode::KeyY))
            // 原生按钮也可以持有InputFocus；只有文本焦点才把撤销交给文字编辑。
            && !focus.get().is_some_and(|entity|text_targets.contains(entity))
        {
            Request::Action {
                document: tool.snapshot.active,
                action: if keyboard.just_pressed(KeyCode::KeyY) {
                    "redo"
                } else {
                    "undo"
                }
                .into(),
                value: String::new(),
            }
        } else {
            return None;
        };
        Some(ToolButton {
            index,
            action: UiAction::Request(request),
        })
    });
    let Some(button) = action else {
        return;
    };
    let Some(tool) = state.tools.get_mut(&button.index) else {
        return;
    };
    if matches!(button.action, UiAction::OpenTool) {
        sidebar.left = EditorLeftSidebarTab::Domain(tool.domain);
        sidebar.right = EditorRightSidebarTab::Tools;
        editor.tool = EditorTool::Domain(tool.domain);
        sidebar.observed_tool = editor.tool; // 只切右栏，普通工具跟随不能改写中央文档。
        let first = !tool.library_open;
        tool.library_open = true;
        if tool.snapshot.workbench.is_some() {
            let id = if first { 0 } else { tool.snapshot.active };
            workspace.active_tab =
                feathrise_editor_core::PresetWorkspaceTab::Domain(tool.domain, id.into());
            workspace.touch_ui();
            if id != tool.snapshot.active {
                let _ = tool.send(Request::Activate { document: id }, true);
            }
        }
        preview_focus.show_workspace();
        state.ui_revision += 1;
        return;
    }
    let request = match button.action {
        UiAction::OpenTool | UiAction::Locate { .. } => unreachable!(),
        UiAction::Tab(document) => {
            sidebar.left = EditorLeftSidebarTab::Domain(tool.domain);
            sidebar.right = EditorRightSidebarTab::Tools;
            editor.tool = EditorTool::Domain(tool.domain);
            sidebar.observed_tool = editor.tool;
            workspace.active_tab =
                feathrise_editor_core::PresetWorkspaceTab::Domain(tool.domain, document.into());
            workspace.touch_ui();
            Request::Activate { document }
        }
        UiAction::CloseLibrary => {
            tool.library_open = false;
            if workspace.active_tab
                == feathrise_editor_core::PresetWorkspaceTab::Domain(tool.domain, 0)
            {
                workspace.active_tab = feathrise_editor_core::PresetWorkspaceTab::Map;
            }
            workspace.touch_ui();
            state.ui_revision += 1;
            return;
        }
        UiAction::Choices(field) => {
            tool.choices = if tool.choices.as_ref() == Some(&field) {
                None
            } else {
                Some(field)
            };
            state.ui_revision += 1;
            return;
        }
        UiAction::Close(document) => {
            let dirty = tool
                .snapshot
                .tabs
                .iter()
                .any(|t| t.id == document && t.dirty);
            if dirty && tool.close_confirmation != Some(document) {
                tool.close_confirmation = Some(document);
                state.ui_revision += 1;
                return;
            }
            tool.close_confirmation = None;
            Request::Close {
                document,
                discard: dirty,
            }
        }
        UiAction::Adopt {
            document,
            field,
            extensions,
        } => {
            let Some(path) = editor.selected_asset.clone() else {
                tool.snapshot.status = "请先在资源浏览器中选择文件".into();
                state.ui_revision += 1;
                return;
            };
            let extension = std::path::Path::new(&path)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if !extensions.iter().any(|e| e.eq_ignore_ascii_case(extension)) {
                tool.snapshot.status = "所选文件类型与字段不匹配".into();
                state.ui_revision += 1;
                return;
            }
            Request::Edit {
                document,
                field,
                value: path,
            }
        }
        UiAction::Request(request) => request,
    };
    let reveal = matches!(
        request,
        Request::Open | Request::Activate { .. } | Request::Edit { .. }
    ) || matches!(&request, Request::Action { action, .. } if ["preview", "new", "edit", "duplicate"].contains(&action.as_str()));
    tool.choices = None;
    let activate = matches!(&request, Request::Activate { .. } | Request::Close { .. })
        || matches!(&request,Request::Action {action,..} if ["new","edit","duplicate"].contains(&action.as_str()));
    match tool.send(request, activate) {
        Ok(_) => {
            if reveal {
                preview_focus.show_workspace();
                sidebar.left = EditorLeftSidebarTab::Domain(tool.domain);
                sidebar.right = EditorRightSidebarTab::Tools;
            }
        }
        Err(error) => {
            tool.snapshot.status = error;
        }
    }
    state.ui_revision += 1;
}

pub(super) fn poll(
    mut state: ResMut<NativeTools>,
    mut editor: ResMut<MapEditorState>,
    mut domains: ResMut<WorkspaceDomainState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut clipboard: ResMut<bevy::clipboard::Clipboard>,
    mut sidebar: ResMut<MapEditorSidebarState>,
    mut preview_focus: ResMut<AssetPreviewFocus>,
) {
    let mut handoffs = Vec::new();
    // 每帧最多采用4份512KiB快照；稳定帧没有目录扫描、磁盘IO或JSON解析。
    for _ in 0..4 {
        let Ok(event) = state.receiver.try_recv() else {
            break;
        };
        match event {
            Event::ManagementWarning(error) => {
                state.management_errors.push(error);
                state.diagnostics.advance();
                state.ui_revision += 1;
            }
            Event::Discovered { count, states, origins, warnings } => {
                state.discovery_pending = false;
                state.loading = count;
                state.load_states = states;
                state.load_origins = origins;
                if !warnings.is_empty() { state.diagnostics.advance(); }
                state.errors.extend(warnings);
                state.ui_revision += 1;
            }
            Event::Failed { id, error } => {
                state.loading = state.loading.saturating_sub(1);
                state.load_states.insert(id.clone(), format!("加载失败：{error}"));
                state.errors.push(format!("{id}：{error}"));
                state.diagnostics.advance();
                state.ui_revision += 1;
            }
            Event::Loaded {
                index,
                domain,
                snapshot,
                requests,
            } => {
                state.loading = state.loading.saturating_sub(1);
                state.load_states.insert(domain.into(), "已加载".into());
                state.tools.insert(
                    index,
                    Tool {
                        domain,
                        snapshot,
                        requests,
                        queue: Default::default(),
                        inflight: None,
                        completed: 0,
                        sequence: 0,
                        fault: None,
                        library_open: false,
                        close_confirmation: None,
                        choices: None,
                        latest_activation: 0,
                        map_pending: false,
                    },
                );
                state.ui_revision += 1;
            }
            Event::Updated {
                index,
                sequence,
                activate,
                request,
                snapshot,
            } => {
                let Some(tool) = state.tools.get_mut(&index) else {
                    continue;
                };
                if tool.inflight != Some(sequence) {
                    continue;
                }
                tool.inflight = None;
                tool.completed = sequence;
                match snapshot {
                    Ok(mut snapshot) => {
                        if snapshot.revision <= tool.snapshot.revision {
                            tool.fault = Some("插件返回了过期的工作台版本".into());
                            tool.queue.clear();
                        } else {
                            if let Some(handoff) = snapshot.handoff.take() {
                                if snapshot.error.is_none()
                                    && matches!(workspace.active_tab,feathrise_editor_core::PresetWorkspaceTab::Domain(domain,document)
                                        if domain == tool.domain && document == u64::from(snapshot.active)) {
                                    handoffs.push((index,handoff));
                                }
                            }
                            if let Some(text) = snapshot.clipboard.take() {
                                if let Err(error) = clipboard.set_text(&text) {
                                    snapshot.status = format!("剪贴板写入失败：{error}");
                                }
                            }
                            publish_feedback(&mut editor, tool, &request, &snapshot);
                            if snapshot.error.is_some() {
                                // 刚提交的字段若被拒绝，不能继续执行同次点击排在后面的保存/切页。
                                // 被取消的请求视为结束，但保存适配仍必须核对saved_sequence，不能假报成功。
                                tool.cancel_queued();
                            }
                            // 只有仍停留在发起请求的插件文档时才导航；迟到保存不能抢回中央或侧栏。
                            if activate
                                && sequence == tool.latest_activation
                                && snapshot.workbench.is_some()
                                && matches!(workspace.active_tab,feathrise_editor_core::PresetWorkspaceTab::Domain(domain,_) if domain==tool.domain)
                            {
                                workspace.active_tab =
                                    feathrise_editor_core::PresetWorkspaceTab::Domain(
                                        tool.domain,
                                        snapshot.active.into(),
                                    );
                                workspace.touch_ui();
                            }
                            tool.map_pending = snapshot.map_domain.as_ref().is_some_and(|next|
                                tool.snapshot.map_domain.as_ref().is_some_and(|before| next.edit_sequence != before.edit_sequence));
                            tool.snapshot = snapshot;
                        }
                    }
                    Err(error) => {
                        tool.fault = Some(error);
                        tool.queue.clear();
                    }
                }
                let open: Vec<_> = tool.snapshot.tabs.iter().map(|tab| tab.id).collect();
                // 已关闭文档的保存票据不随会话时长累积；只保留当前有界打开集合。
                state.save_tickets.retain(|id, _| {
                    (*id >> 32) != index as u64 + 1 || open.contains(&(*id as u32))
                });
                state.ui_revision += 1;
            }
        }
    }
    for (source, handoff) in handoffs {
        let result = (|| -> Result<&'static str,String> {
            if !editor.open || editor.selected_map_id.is_none() || editor.map_transition_in_progress {
                return Err("请先打开一张可编辑地图，再将此预设用于地图".into());
            }
            let tool = state.tools.values_mut().find(|tool|tool.domain == handoff.target && tool.snapshot.map_domain.is_some())
                .ok_or("当前工程尚未安装目标地图作者插件")?;
            if tool.snapshot.active == 0 || tool.busy() { return Err("地图作者仍在加载或处理操作，请完成后重试".into()); }
            for edit in handoff.edits {
                tool.send(Request::Edit { document:tool.snapshot.active, field:edit.field, value:edit.value }, false)?;
            }
            tool.library_open = true;
            Ok(tool.domain)
        })();
        match result {
            Ok(domain) => {
                workspace.active_tab = feathrise_editor_core::PresetWorkspaceTab::Map;
                workspace.touch_ui();
                sidebar.left = EditorLeftSidebarTab::Domain(domain);
                sidebar.right = EditorRightSidebarTab::Tools;
                editor.tool = EditorTool::Domain(domain);
                sidebar.observed_tool = editor.tool;
                preview_focus.show_workspace();
                editor.status = "已选择已保存预设；在当前地图真实承托面上放置点位。".into();
            }
            Err(error) => {
                editor.status = error.clone();
                if let Some(tool) = state.tools.get_mut(&source) { tool.snapshot.status = error; }
            }
        }
        state.ui_revision = state.ui_revision.wrapping_add(1);
    }
    for tool in state.tools.values_mut() {
        if tool.inflight.is_none() && tool.fault.is_none() && !tool.map_pending {
            if let Some(job) = tool.queue.pop_front() {
                let sequence = job.sequence;
                match tool.requests.try_send(job) {
                    Ok(()) => tool.inflight = Some(sequence),
                    Err(async_channel::TrySendError::Full(job)) => tool.queue.push_front(job),
                    Err(_) => {
                        tool.fault = Some("插件后台会话意外退出，草稿未确认保存".into());
                        tool.queue.clear();
                    }
                }
            }
        }
    }
    // 文档工作台登记原中央标签身份；其他作者域的登记保持不变。
    let mut open = domains.open_tabs.clone();
    open.retain(|tab| !matches!(tab,feathrise_editor_core::PresetWorkspaceTab::Domain(domain,_) if state.tools.values().any(|t|t.domain==*domain)));
    for tool in state
        .tools
        .values()
        .filter(|t| t.snapshot.workbench.is_some())
    {
        if tool.library_open {
            open.push(feathrise_editor_core::PresetWorkspaceTab::Domain(
                tool.domain,
                0,
            ));
        }
        open.extend(tool.snapshot.tabs.iter().map(|tab| {
            feathrise_editor_core::PresetWorkspaceTab::Domain(tool.domain, tab.id.into())
        }));
    }
    if open != domains.open_tabs {
        domains.open_tabs = open;
        workspace.touch_ui();
    }
    domains.input_blocked = state.input_pending;
}

/// 正文反馈实体只供底栏详情收集，不能据此判断保存结果。
/// 插件错误与保存回执显式进入公共反馈槽，用户无需展开详情才知道失败。
fn publish_feedback(
    editor: &mut MapEditorState,
    tool: &Tool,
    request: &Request,
    snapshot: &feathrise_plugin_api::Snapshot,
) {
    use feathrise_editor_core::{EditorStatusKind, EditorStatusOperation, EditorStatusTarget};
    let document = match request {
        Request::Activate { document }
        | Request::Edit { document, .. }
        | Request::Close { document, .. }
        | Request::Save { document }
        | Request::Action { document, .. } => *document,
        _ => 0,
    };
    let saving = matches!(request, Request::Save { .. })
        || matches!(request, Request::Action { action, .. } if action == "save");
    let (kind, status) = if let Some(error) = &snapshot.error {
        (EditorStatusKind::Error, error.clone())
    } else if saving {
        let before = tool.snapshot.tabs.iter().find(|tab| tab.id == document);
        let saved = snapshot.tabs.iter().find(|tab| tab.id == document);
        if before.zip(saved).is_some_and(|(before, saved)| {
            saved.saved_sequence > before.saved_sequence && !saved.dirty
        }) {
            (EditorStatusKind::Success, snapshot.status.clone())
        } else {
            (
                EditorStatusKind::Error,
                format!("{}未返回实际保存回执，草稿保留", snapshot.tool),
            )
        }
    } else {
        return;
    };
    let title = snapshot
        .tabs
        .iter()
        .chain(&tool.snapshot.tabs)
        .find(|tab| tab.id == document)
        .map_or(snapshot.tool.as_str(), |tab| tab.title.as_str());
    // 使用请求文档身份；保存全部可保存后台文档，不能误记到当前活动标签。
    let target = EditorStatusTarget::new(&format!("{}:{document}", tool.domain), title);
    editor.status = status;
    editor.record_operation_result(EditorStatusOperation::NativePlugin, &target, kind);
}

pub(super) fn preview(
    mut commands: Commands,
    state: Res<NativeTools>,
    workspace: Res<VegetationPresetEditorState>,
    focus: Res<AssetPreviewFocus>,
    mut external: ResMut<game_authoring::ExternalAssetPreviewRequest>,
    mut hosts: Query<(Entity, &ToolPreviewHost, &mut Node, Option<&ImageNode>, Option<&super::runtime_preview::RuntimeCanvas>)>,
    sidebar: Res<MapEditorSidebarState>,
    mut runtime: super::runtime_preview::Context,
    local: Res<super::local_prefab::State>,
    assets: Res<AssetServer>,
    mut captions: Query<(&super::runtime_preview::RuntimeCaption, &mut Text, &mut Node), Without<ToolPreviewHost>>,
) {
    let selected = state.tools.iter().find(|(_, t)| {
        sidebar.tool_page_visible(EditorLeftSidebarTab::Domain(t.domain))
            && t.snapshot.active > 0
            && (t.snapshot.workbench.is_none()
                || workspace.active_tab
                    == feathrise_editor_core::PresetWorkspaceTab::Domain(
                        t.domain,
                        t.snapshot.active.into(),
                    ))
    });
    let selected = selected.filter(|_| !focus.browsing(workspace.active_tab));
    let request = selected.and_then(|(index, tool)| tool.snapshot.preview.as_ref()
        .and_then(|preview| preview.runtime.as_ref()).map(|preview| (*index, tool.snapshot.active, preview)));
    // SDK中性外观在编辑器本地装配；只有领域执行请求才能进入工程构建/进程通道。
    let live = request.filter(|(_, _, preview)| !super::local_prefab::is_local(preview));
    let local_request = request.filter(|(_, _, preview)| super::local_prefab::is_local(preview));
    let image = runtime.update(live);
    let status = if local_request.is_some() { local.caption() } else { runtime.caption() };
    for (caption, mut text, mut node) in &mut captions {
        let current = request.is_some_and(|(index, _, _)| index == caption.0);
        node.display = if current { Display::Flex } else { Display::None };
        if current && text.0 != status { text.0.clone_from(&status); }
    }
    let mut host = None;
    for (entity, marker, mut node, old_image, canvas) in &mut hosts {
        let current = selected.is_some_and(|(index, _)| *index == marker.0);
        let runtime_current = current && live.is_some();
        let local_current = current && local_request.is_some();
        let display = if runtime_current || local_current || (current
            && selected
                .and_then(|(_, t)| t.snapshot.preview.as_ref())
                .and_then(|p| p.model.as_ref())
                .is_some())
        {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if runtime_current || local_current {
            // 真实工程帧直接交给右栏常驻画布；表单快照重建不能重建进程或丢失画面。
            if canvas.is_none() {
                let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
                commands.entity(entity)
                    .insert((super::runtime_preview::RuntimeCanvas, bevy::ui::RelativeCursorPosition::default()))
                    .with_children(|parent| {
                        parent.spawn((super::runtime_preview::RuntimeCaption(marker.0),
                            super::ui::caption(&font, &status, 11.),
                            Node { max_width: percent(100), padding: UiRect::all(px(5)), ..default() }));
                    });
            }
            if let Some(image) = image.as_ref().filter(|_| runtime_current) {
                if old_image.is_none_or(|old| old.image != *image) {
                    commands.entity(entity).insert(ImageNode::new(image.clone()));
                }
            } else if old_image.is_some() {
                commands.entity(entity).remove::<ImageNode>(); // 换草稿即撤旧图，不拿上一页画面冒充本次结果。
            }
        } else if canvas.is_some() && old_image.is_some() {
            commands.entity(entity).remove::<ImageNode>();
        }
        if current {
            host = Some(entity);
        }
    }
    let selected = selected.map(|(_, tool)| tool);
    let Some(tool) = selected else {
        if external.host.is_some_and(|id| hosts.contains(id))
            || external
                .identity
                .is_some_and(|id| state.tools.values().any(|t| t.domain == id.domain))
        {
            *external = Default::default();
        }
        return;
    };
    let identity =
        tool.snapshot
            .preview
            .as_ref()
            .map(|p| game_authoring::ExternalPreviewIdentity {
                domain: tool.domain,
                tab: tool.snapshot.active.into(),
                revision: p.revision,
                ticket: p.revision,
            });
    if live.is_some() {
        // 同时关闭原GLB回退，避免把施法者静态模型当作技能运行成功。原资产画布退回普通宿主。
        *external = game_authoring::ExternalAssetPreviewRequest {
            active: true, identity, title: Some(format!("{}真实运行预览", tool.snapshot.tool)),
            ..default()
        };
        return;
    }
    if let Some((index, document, preview)) = local_request {
        *external = game_authoring::ExternalAssetPreviewRequest {
            active: true, identity,
            model_path: super::local_prefab::model(&local),
            root: local.root(index, document, preview.revision), constructed: true,
            title: Some(format!("{} · 编辑器3D", tool.snapshot.tool)), host,
        };
        return;
    }
    *external = game_authoring::ExternalAssetPreviewRequest {
        active: true,
        identity,
        model_path: tool.snapshot.preview.as_ref().and_then(|p| p.model.clone()),
        root: None,
        constructed: false,
        title: Some(format!("{}预览", tool.snapshot.tool)),
        host,
    };
}

/// 文档工作台可在浏览右侧候选资产时继续编辑；旧通用工具仍按工具页显隐。
fn tool_visible(
    tool: &Tool,
    sidebar: &MapEditorSidebarState,
    workspace: &VegetationPresetEditorState,
) -> bool {
    if tool.snapshot.workbench.is_some() {
        matches!(workspace.active_tab,feathrise_editor_core::PresetWorkspaceTab::Domain(domain,_) if domain==tool.domain)
    } else {
        sidebar.tool_page_visible(EditorLeftSidebarTab::Domain(tool.domain))
    }
}
