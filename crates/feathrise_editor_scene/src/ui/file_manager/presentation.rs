//! 桌面布局和有限可见行投影；目录快照 O(1) 共享，不在绘制帧扫描磁盘。

use super::{components::*, resources::*, types::*};
use crate::{components::SceneEditorRoot, resources::SceneEditorLayoutState, ui::theme as colors};
use bevy::{prelude::*, input_focus::{InputFocus, FocusCause}, text::EditableText};
use feathrise_editor_project_browser_model::file_operations::ConflictPolicy;
use feathrise_editor_ui::{EditorButtonClick, UiInteraction, localization::EditorLocale};

pub(super) fn sync(world: &mut World) {
    world.resource_scope(|world, mut state: Mut<FileManagerState>| {
        let english = world.get_resource::<EditorLocale>().is_some_and(|locale| *locale == EditorLocale::English);
        let mut roots = world.query_filtered::<&ComputedNode, With<SceneEditorRoot>>();
        let size = roots.single(world).map(|node| node.size() * node.inverse_scale_factor).unwrap_or(Vec2::new(1280., 800.));
        let (left, right) = world.get_resource::<SceneEditorLayoutState>().map_or((0., 0.), |layout| (
            if layout.content_shown() { layout.content_width } else { 0. },
            if layout.inspector_shown() { layout.inspector_width } else { 0. }));
        let mut bounds = Rect::from_corners(Vec2::new(left + 8., 88.), Vec2::new(size.x - right - 8., size.y - 56.));
        if bounds.width() < 360. { bounds.min.x = 4.; bounds.max.x = (size.x - 4.).max(5.); }
        if bounds.height() < 240. { bounds.min.y = 44.; bounds.max.y = (size.y - 56.).max(45.); }
        state.bounds = bounds;
        state.movement_bounds = Rect::from_corners(Vec2::new(4., bounds.min.y), Vec2::new((size.x - 4.).max(5.), bounds.max.y));
        let movement_bounds = state.movement_bounds;
        let mut capacities = Vec::with_capacity(state.windows.len());
        for window in &mut state.windows {
            window.rect = if window.maximized { bounds } else { clamp_rect(window.rect, movement_bounds) };
            let capacity = world.get_resource::<super::browser::BrowserSessions>().map_or_else(|| window.visible_capacity(), |browser| browser.capacity(window));
            window.first = window.first.min(window.entries.len().saturating_sub(capacity));
            capacities.push((window.id, capacity));
        }
        let prompt_visible = state.prompt.is_some() && !state.suspended;
        let delete_visible = prompt_visible && state.prompt.as_ref().is_some_and(|prompt| prompt.action == FileAction::Delete);
        let focus_entity = world.get_resource::<InputFocus>().and_then(|focus| focus.get());
        let mut search_inputs = world.query::<(Entity, &FileSearchInput)>();
        let focused_search = search_inputs.iter(world).find_map(|(entity, input)| (focus_entity == Some(entity)).then_some(input.window));
        let mut drag_nodes = world.query_filtered::<&ComputedNode, With<DragLabel>>();
        let drag_size = drag_nodes.single(world).map(|node| node.size() * node.inverse_scale_factor).unwrap_or(Vec2::ZERO);
        let mut dialogs = world.query_filtered::<&ComputedNode, With<NameDialog>>();
        let dialog_height = dialogs.single(world).map(|node| node.size().y * node.inverse_scale_factor).unwrap_or(170.).max(170.);
        let task_visible = !state.windows.is_empty() || !state.notice.is_empty() || state.snapshot.as_ref().is_some_and(|snapshot| snapshot.active.is_some() || !snapshot.queued.is_empty());
        let mut nodes = world.query_filtered::<(Entity, &mut Node, Option<&FileWindowRoot>, Option<&DesktopTaskbar>, Option<&NameDialog>, Option<&FileNameInput>, Option<&DragLabel>, Option<&NameDialogBackdrop>, Option<&NameDialogBody>, Option<&FileSearchHint>),
            Or<(With<FileWindowRoot>, With<DesktopTaskbar>, With<NameDialog>, With<FileNameInput>, With<DragLabel>, With<NameDialogBackdrop>, With<NameDialogBody>, With<FileSearchHint>)>>();
        let mut z_updates = Vec::new();
        for (entity, mut node, root, bar, dialog, name, drag, backdrop, body, search_hint) in nodes.iter_mut(world) {
            if let Some(root) = root {
                if let Some((rank, window)) = state.windows.iter().enumerate().find(|(_, window)| window.id == root.id) {
                    node.display = if window.minimized { Display::None } else { Display::Flex };
                    node.left = px(window.rect.min.x); node.top = px(window.rect.min.y);
                    node.width = px(window.rect.width()); node.height = px(window.rect.height());
                    z_updates.push((entity, rank as i32));
                }
            }
            if bar.is_some() { node.display = if task_visible { Display::Flex } else { Display::None }; }
            if dialog.is_some() {
                node.display = if prompt_visible { Display::Flex } else { Display::None };
                let width = (size.x - 24.).clamp(200., 440.);
                node.width = px(width); node.max_height = px((size.y - 80.).max(100.));
                node.left = px(((size.x - width) * 0.5).max(0.)); node.top = px(((size.y - dialog_height) * 0.5).max(40.));
            }
            if name.is_some() { node.display = if prompt_visible && state.prompt.as_ref().is_some_and(|prompt| prompt.action != FileAction::Delete) { Display::Flex } else { Display::None }; }
            if backdrop.is_some() { node.display = if prompt_visible { Display::Flex } else { Display::None }; }
            if body.is_some() { node.display = if delete_visible { Display::Flex } else { Display::None }; node.max_height = px((size.y - 236.).clamp(48., 240.)); }
            if let Some(hint) = search_hint {
                node.display = if focused_search != Some(hint.window) && state.active(hint.window).is_some_and(|window| window.filter.is_empty()) { Display::Flex } else { Display::None };
            }
            if drag.is_some() {
                node.display = if state.drag.as_ref().is_some_and(|drag| drag.dragging) { Display::Flex } else { Display::None };
                let maximum = Vec2::new((size.x - 24.).clamp(24., 520.), (size.y - 112.).max(24.));
                node.max_width = px(maximum.x); node.max_height = px(maximum.y);
                // 提示层不参加拾取，手势可命中它下方的真实目标文件夹。
                if let Some(drag) = &state.drag {
                    let measured = if drag_size.min_element() > 0. { drag_size.min(maximum) } else { Vec2::new(maximum.x, 96.).min(maximum) };
                    node.left = px((drag.position.x + 16.).clamp(12., (size.x - measured.x - 12.).max(12.)));
                    node.top = px((drag.position.y + 18.).clamp(44., (size.y - measured.y - 56.).max(44.)));
                }
            }
        }
        let reset_prompt_scroll = state.prompt.as_ref().is_some_and(|prompt| !prompt.initialized);
        let mut body_scroll = world.query_filtered::<(&ComputedNode, &mut ScrollPosition), With<NameDialogBody>>();
        for (node, mut scroll) in body_scroll.iter_mut(world) {
            let maximum = ((node.content_size().y - node.size().y) * node.inverse_scale_factor).max(0.);
            scroll.y = if reset_prompt_scroll { 0. } else { scroll.y.clamp(0., maximum) };
        }
        for (entity, z) in z_updates { world.entity_mut(entity).insert(ZIndex(z)); }
        let mut row_changes = Vec::new();
        let mut rows = world.query::<(Entity, &mut FileRow, &mut Node, &mut BackgroundColor, &mut EditorButtonClick, &UiInteraction)>();
        for (entity, mut row, mut node, mut background, mut click, interaction) in rows.iter_mut(world) {
            let Some(window) = state.active(row.window) else { continue; };
            let index = window.first + row.slot;
            let capacity = capacities.iter().find(|(id, _)| *id == window.id).map_or_else(|| window.visible_capacity(), |(_, capacity)| *capacity);
            let entry = (row.slot < capacity).then(|| window.entries.get(index)).flatten();
            let path = entry.map(|entry| entry.path.clone());
            if row.path != path || row.revision != window.revision { click.invalidate(); row.path = path; row.revision = window.revision; }
            row.index = entry.map(|_| index);
            node.display = if entry.is_some() { Display::Flex } else { Display::None };
            let selected = entry.is_some_and(|entry| window.selection.contains(&entry.path));
            background.0 = if selected { colors::SELECT_BG } else if *interaction != UiInteraction::None { colors::SELECT_BG_HOVER } else { colors::PANEL_BG };
            if let Some(entry) = entry {
                let cut = state.cut && state.clipboard.binary_search(&entry.path).is_ok();
                let suffix = if entry.directory { if english { "    Folder".into() } else { "    文件夹".into() } }
                    else { format!("    {}", bytes_label(entry.bytes)) };
                row_changes.push((entity, format!("{}{suffix}", entry.name), cut, entry.directory));
            }
        }
        let folder_image: Handle<Image> = world.resource::<AssetServer>().load("icons/editor/tree_folder.png");
        let file_image: Handle<Image> = world.resource::<AssetServer>().load("icons/editor/tree_file.png");
        let mut icons = world.query_filtered::<(&ChildOf, &mut ImageNode), With<FileRowIcon>>();
        for (parent, mut image) in icons.iter_mut(world) {
            if let Some((_, _, cut, directory)) = row_changes.iter().find(|(row, ..)| *row == parent.parent()) {
                let wanted = if *directory { &folder_image } else { &file_image };
                if &image.image != wanted { image.image = wanted.clone(); }
                image.color = if *cut { colors::TEXT_MUTED } else { colors::TEXT_PRIMARY };
            }
        }
        let mut texts = world.query_filtered::<(&mut Text, &mut TextColor, Option<&WindowTitle>, Option<&WindowStatus>, Option<&ButtonLabel>, Option<&NameDialogTitle>, Option<&OperationStatus>, Option<&DragLabel>, Option<&RowLabel>, Option<&ChildOf>, Option<&TaskTitle>, Option<&NameDialogBodyText>, Option<&FileSearchHint>),
            Or<(With<WindowTitle>, With<WindowStatus>, With<ButtonLabel>, With<NameDialogTitle>, With<OperationStatus>, With<DragLabel>, With<RowLabel>, With<TaskTitle>, With<NameDialogBodyText>, With<FileSearchHint>)>>();
        for (mut text, mut color, title, status, button, dialog_title, operation, drag_label, row_label, parent, task_title, dialog_body, search_hint) in texts.iter_mut(world) {
            let wanted = if let Some(title) = title {
                state.active(title.0).map(|window| format!("{} — {}", window.path.file_name().unwrap_or_default().to_string_lossy(), if english { "Files" } else { "文件" }))
            } else if let Some(task) = task_title {
                state.active(task.0).map(|window| format!("{}{}", window.path.file_name().unwrap_or_default().to_string_lossy(), if window.minimized { if english { " (min)" } else { "（最小化）" } } else { "" }))
            } else if let Some(status) = status {
                state.active(status.0).map(|window| {
                    if let Some(error) = &window.error { error.clone() }
                    else if window.loading { if english { "Reading folder…".into() } else { "正在读取目录…".into() } }
                    else if english { format!("{} items  ·  {} selected  ·  Drag to move, Ctrl to copy", window.entries.len(), window.selection.len()) }
                    else { format!("{} 项 · 已选 {} 项 · 拖动移动，Ctrl 拖动复制", window.entries.len(), window.selection.len()) }
                })
            } else if let Some(button) = button {
                Some(if button.0 == FileAction::ConflictPolicy {
                    match (state.conflict, english) {
                        (ConflictPolicy::KeepBoth, false) => "同名：保留两者", (ConflictPolicy::KeepBoth, true) => "Conflict: keep both",
                        (ConflictPolicy::Skip, false) => "同名：跳过", (ConflictPolicy::Skip, true) => "Conflict: skip",
                        (ConflictPolicy::Fail, false) => "同名：停止", (ConflictPolicy::Fail, true) => "Conflict: stop",
                    }.into()
                } else { button.0.label(english).into() })
            } else if dialog_title.is_some() {
                state.prompt.as_ref().map(|prompt| {
                    if prompt.action == FileAction::Delete { if english { format!("Recycle {} items?", prompt.sources.len()) } else { format!("将 {} 项放入回收站？", prompt.sources.len()) } }
                    else { prompt.action.label(english).into() }
                })
            } else if dialog_body.is_some() {
                state.prompt.as_ref().filter(|prompt| prompt.action == FileAction::Delete).map(|prompt| delete_prompt_body(prompt, english))
            } else if search_hint.is_some() {
                Some(if english { "Filter by filename".into() } else { "按文件名筛选".into() })
            } else if operation.is_some() {
                Some(state.snapshot.as_ref().and_then(|snapshot| snapshot.active.as_ref()).map_or_else(|| state.notice.clone(),
                    |active| format!("{} · {} 项 · {}", active.label, active.completed, bytes_label(active.bytes))))
            } else if drag_label.is_some() {
                state.drag.as_ref().map(|drag| drag.destination.as_ref().map_or_else(
                    || if english { "Cannot drop here".into() } else { "不能放到这里".into() },
                    |destination| format!("{} {} {} → {}", match (drag.copy, english) { (true, true) => "Copy", (false, true) => "Move", (true, false) => "复制", (false, false) => "移动" }, drag.sources.len(), if english { "items" } else { "项" }, crate::ui::external_files::platform_path(destination).display())))
            } else if row_label.is_some() {
                parent.and_then(|parent| row_changes.iter().find(|(row, ..)| *row == parent.parent())).map(|(_, text, cut, _)| {
                    color.0 = if *cut { colors::TEXT_MUTED } else { colors::TEXT_PRIMARY }; text.clone()
                })
            } else { None };
            if let Some(wanted) = wanted { if text.as_str() != wanted { text.0 = wanted; } }
        }
        let mut task_buttons = world.query::<(&FileTaskButton, &mut BackgroundColor, &mut BorderColor)>();
        for (task, mut background, mut border) in task_buttons.iter_mut(world) {
            let focused = state.focused == Some(task.window);
            background.0 = if focused { colors::SELECT_BG } else { colors::BUTTON_DEV_SMALL_NORMAL };
            border.set_if_neq(BorderColor::all(if focused { colors::COPPER_BORDER } else { colors::COPPER_BORDER_WEAK }));
        }
        let task_active = state.snapshot.as_ref().is_some_and(|snapshot| snapshot.active.is_some());
        let mut buttons = world.query::<(&FileManagerButton, &mut Node)>();
        for (button, mut node) in buttons.iter_mut(world) {
            if button.action == FileAction::CancelTask { node.display = if task_active { Display::Flex } else { Display::None }; }
        }
        let focus_entity = world.get_resource::<InputFocus>().and_then(|focus| focus.get());
        let mut next_focus = None;
        let mut inputs = world.query::<(Entity, &mut EditableText, Option<&PathInput>, Option<&FileSearchInput>, Option<&FileNameInput>)>();
        for (entity, mut input, path, search, name) in inputs.iter_mut(world) {
            if input.is_composing() || !input.pending_edits.is_empty() || input.pending_paste.is_some() { continue; }
            let wanted = if let Some(path) = path {
                if focus_entity == Some(entity) { None } else { state.active(path.window).map(|window| crate::ui::external_files::platform_path(&window.path).to_string_lossy().into_owned()) }
            } else if let Some(search) = search {
                if focus_entity == Some(entity) { None } else { state.active(search.window).map(|window| window.filter.clone()) }
            } else if name.is_some() {
                state.prompt.as_mut().filter(|prompt| prompt_visible && !prompt.initialized).map(|prompt| { prompt.initialized = true; if prompt.action != FileAction::Delete { next_focus = Some(entity); } prompt.value.clone() })
            } else { None };
            if let Some(wanted) = wanted { if input.value().to_string() != wanted { crate::ui::text_input::replace_single_line_value(&mut input, &wanted, 4096); } }
        }
        if let Some(entity) = next_focus { world.resource_mut::<InputFocus>().set(entity, FocusCause::Navigated); }
        super::context_menu::sync(world, &mut state, size, english);
    });
}

fn delete_prompt_body(prompt: &NamePrompt, english: bool) -> String {
    let mut lines: Vec<_> = prompt.sources.iter().take(5).map(|source| crate::ui::external_files::platform_path(source).display().to_string()).collect();
    if prompt.sources.len() > 5 { lines.push(if english { format!("… and {} more items", prompt.sources.len() - 5) } else { format!("……另有 {} 项", prompt.sources.len() - 5) }); }
    lines.push(if english { "Existing path references will need to be updated.".into() } else { "已有路径引用不会自动更新，请按需调整。".into() });
    lines.join("\n")
}

fn bytes_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 { format!("{:.1} GB", bytes as f64 / (1024. * 1024. * 1024.)) }
    else if bytes >= 1024 * 1024 { format!("{:.1} MB", bytes as f64 / (1024. * 1024.)) }
    else if bytes >= 1024 { format!("{:.1} KB", bytes as f64 / 1024.) }
    else { format!("{bytes} B") }
}
