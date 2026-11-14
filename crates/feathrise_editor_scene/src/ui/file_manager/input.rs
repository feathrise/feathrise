//! 文件窗拥有独立焦点与捕获；拖出窗口仍完成同一手势，不把释放事件传给场景。

use super::{components::*, messages::*, resources::*, types::*};
use crate::{components::SceneEditorRoot, resources::{SceneEditorState, SceneEditorSettingsState}, workbench::WorkbenchFloatingPanels};
use bevy::{ecs::system::SystemParam, input::{mouse::{MouseWheel, MouseScrollUnit}}, input_focus::{InputFocus, FocusCause}, prelude::*, text::EditableText, ui::RelativeCursorPosition, window::PrimaryWindow};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction};

#[derive(SystemParam)]
pub(super) struct InputGate<'w, 's> {
    editor: Res<'w, SceneEditorState>,
    settings: Option<Res<'w, SceneEditorSettingsState>>,
    host: Option<Res<'w, crate::workbench::WorkbenchModel>>,
    overlays: Option<Res<'w, feathrise_editor_core::EditorHostOverlay>>,
    ime: Option<Res<'w, crate::resources::EditorTextInputGuard>>,
    modals: (Option<Res<'w, crate::resources::SceneBrowserState>>, Option<Res<'w, crate::resources::SceneDeletionTask>>, Option<Res<'w, crate::authoring::TerrainExternalChangeState>>),
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}
impl InputGate<'_, '_> {
    fn available(&self) -> bool {
        self.editor.open && !self.editor.close_requested && !self.editor.new_scene_dialog_open
            && self.editor.scene_switch_confirmation.is_none() && self.editor.pending_scene_transition.is_none()
            && !self.settings.as_ref().is_some_and(|settings| settings.open)
            && !self.host.as_ref().is_some_and(|host| host.modal || host.busy || !host.visible)
            && !self.overlays.as_ref().is_some_and(|overlay| overlay.active)
            && !self.modals.0.as_ref().is_some_and(|browser| browser.dialog.is_some())
            && !self.modals.1.as_ref().is_some_and(|deletion| deletion.confirmation.is_some())
            && !self.modals.2.as_ref().is_some_and(|conflict| conflict.modal_open())
            && self.windows.single().is_ok_and(|window| window.focused)
    }
}

pub(super) fn track_pointer(
    mut state: ResMut<FileManagerState>,
    floating: WorkbenchFloatingPanels,
    file_panels: Query<(), Or<(With<FileWindowRoot>, With<DesktopTaskbar>)>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    state.hovered = floating.hovered().is_some_and(|entity| file_panels.contains(entity));
    if !mouse.pressed(MouseButton::Left) && !mouse.pressed(MouseButton::Right) && !mouse.pressed(MouseButton::Middle)
        && mouse.get_just_released().next().is_none() { state.release_guard = false; }
    if state.hovered && mouse.get_just_pressed().next().is_some() { state.release_guard = true; }
}

#[derive(SystemParam)]
pub(super) struct Controls<'w, 's> {
    reference: crate::ui::asset_preview::rigging::RigReferenceInput<'w>,
    floating: WorkbenchFloatingPanels<'w, 's>,
    window_roots: Query<'w, 's, &'static FileWindowRoot>,
    taskbars: Query<'w, 's, (), With<DesktopTaskbar>>,
    buttons: Query<'w, 's, (&'static FileManagerButton, &'static EditorButtonClick, Option<&'static FileContextEntry>), Changed<EditorButtonClick>>,
    rows: Query<'w, 's, (&'static FileRow, &'static RelativeCursorPosition, &'static UiInteraction, &'static EditorButtonClick)>,
    handles: Query<'w, 's, (Option<&'static FileTitleBar>, Option<&'static FileResizeHandle>, &'static RelativeCursorPosition), Or<(With<FileTitleBar>, With<FileResizeHandle>)>>,
    tasks: Query<'w, 's, (&'static FileTaskButton, &'static EditorButtonClick), Changed<EditorButtonClick>>,
    roots: Query<'w, 's, &'static ComputedNode, With<SceneEditorRoot>>,
    inputs: Query<'w, 's, (Entity, Option<&'static PathInput>, Option<&'static FileSearchInput>, &'static EditableText)>,
    lists: Query<'w, 's, (&'static FileList, &'static RelativeCursorPosition)>,
    menu_entries: Query<'w, 's, (&'static FileContextEntry, &'static RelativeCursorPosition, &'static EditorButtonClick, &'static InheritedVisibility, &'static ComputedNode, &'static UiInteraction)>,
    menu_panels: Query<'w, 's, (&'static FileContextPanel, &'static ComputedNode, &'static mut ScrollPosition), Without<NameDialogBody>>,
    dialog_body: Query<'w, 's, (&'static ComputedNode, &'static mut ScrollPosition), (With<NameDialogBody>, Without<FileContextPanel>)>,
}

pub(super) fn control(
    mut state: ResMut<FileManagerState>,
    fixed: Res<FixedFileSelection>,
    gate: InputGate,
    mut controls: Controls,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut focus: ResMut<InputFocus>,
    mut external: MessageReader<FileManagerExternalDrag>,
    mut contexts: MessageReader<FileManagerContextRequest>,
    mut extras: MessageWriter<ExtraRequest>,
    mut requests: MessageWriter<FileManagerCommand>,
    mut open_files: MessageWriter<OpenManagedFile>,
    mut wheels: MessageReader<MouseWheel>,
    mut title_click: Local<Option<(u64, f64)>>,
    browser: Option<Res<super::browser::BrowserSessions>>,
) {
    let (wheel_lines, wheel_pixels) = wheels.read().fold((0., 0.), |(lines, pixels), wheel| match wheel.unit {
        MouseScrollUnit::Line => (lines - wheel.y, pixels),
        MouseScrollUnit::Pixel => (lines, pixels - wheel.y),
    });
    let wheel_delta = wheel_lines * 3. + wheel_pixels / 26.;
    if !gate.available() {
        state.suspended = true;
        state.gesture = None; state.drag = None; state.focused = None;
        state.menu = None;
        external.clear(); contexts.clear(); return;
    }
    if state.suspended {
        if let Some(prompt) = &mut state.prompt { prompt.initialized = false; }
        state.suspended = false;
    }
    let inverse = controls.roots.single().map_or(1., |node| node.inverse_scale_factor);
    let point = gate.windows.single().ok().and_then(Window::physical_cursor_position).map(|point| point * inverse);
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    // Commit 末帧正文可能已经采用，is_composing 已恢复 false；仍须保护用来选字的 Enter/Esc。
    let ime_blocked = gate.ime.as_ref().is_some_and(|guard| guard.block_shortcuts);
    // 以这一帧实际命中的浮层为准。状态详情、日志等覆盖层内的指针也可能落在文件窗矩形中。
    let foreground = controls.floating.hovered();
    let top = foreground.and_then(|entity| controls.window_roots.get(entity).ok())
        .and_then(|root| state.active(root.id).filter(|window| !window.minimized).map(|window| window.id));
    let over_taskbar = foreground.is_some_and(|entity| controls.taskbars.contains(entity));
    let service_open = |id| browser.as_ref().is_some_and(|browser| browser.service_open(id));
    if state.prompt.is_some() {
        contexts.clear(); external.clear();
        if wheel_delta != 0. {
            for (node, mut scroll) in &mut controls.dialog_body {
                let maximum = ((node.content_size().y - node.size().y) * node.inverse_scale_factor).max(0.);
                scroll.y = (scroll.y + wheel_delta * 26.).clamp(0., maximum);
            }
        }
        for (button, click, _) in &controls.buttons {
            if click.is_clicked() && matches!(button.action, FileAction::Confirm | FileAction::Cancel) {
                requests.write(FileManagerCommand { action: button.action, path: None, sources: vec![] });
            }
        }
        let composing = controls.inputs.iter().any(|(entity, _, _, input)| focus.get() == Some(entity) && input.is_composing());
        if !ime_blocked && keys.just_pressed(KeyCode::Escape) { requests.write(FileManagerCommand { action: FileAction::Cancel, path: None, sources: vec![] }); }
        if !ime_blocked && keys.just_pressed(KeyCode::Enter) && !composing { requests.write(FileManagerCommand { action: FileAction::Confirm, path: None, sources: vec![] }); }
        return;
    }
    let mut opened_menu = false;
    for target in contexts.read() {
        if foreground.is_some() || target.directory.as_os_str().is_empty() { continue; }
        super::context_menu::open(&mut state, target.clone());
        state.focused = None;
        focus.clear();
        opened_menu = true;
    }
    if state.menu.is_some() {
        external.clear();
        state.release_guard |= mouse.get_just_pressed().next().is_some();
        if opened_menu { return; }
        if !ime_blocked && keys.just_pressed(KeyCode::Escape) {
            state.menu = None; state.release_guard = true; return;
        }
        let hovered = controls.menu_entries.iter().find_map(|(entry, cursor, _, visible, node, interaction)|
            (visible.get() && !node.is_empty() && *interaction != UiInteraction::None && cursor.cursor_over()
                && (!entry.submenu || state.menu.as_ref().unwrap().submenu)).then_some(*entry));
        if let Some(entry) = hovered {
            if entry.action == FileContextAction::File(FileAction::NewMenu) && super::context_menu::enabled(entry.action, &state) {
                state.menu.as_mut().unwrap().submenu = true;
            } else if !entry.submenu { state.menu.as_mut().unwrap().submenu = false; }
        }
        let clicked = controls.menu_entries.iter().find_map(|(entry, _, click, visible, node, _)|
            (visible.get() && !node.is_empty() && click.is_clicked() && (!entry.submenu || state.menu.as_ref().unwrap().submenu)
                && super::context_menu::enabled(entry.action, &state)).then_some(entry.action));
        if let Some(action) = clicked {
            if action == FileContextAction::File(FileAction::NewMenu) {
                state.menu.as_mut().unwrap().submenu = true;
            } else {
                let target = state.menu.take().unwrap().target;
                match action {
                    FileContextAction::File(action) => {
                        let path = if action == FileAction::NewWindow { target.open_directory } else { target.directory };
                        requests.write(FileManagerCommand { action, path: Some(path), sources: target.sources });
                    }
                    FileContextAction::Extra(action) => { extras.write(ExtraRequest { action, target }); }
                }
            }
            state.release_guard = true;
            return;
        }
        if let Some(menu) = &state.menu {
            let inside = point.is_some_and(|point| menu.main_rect.contains(point) || menu.submenu && menu.submenu_rect.contains(point));
            if !inside && mouse.get_just_pressed().next().is_some() {
                state.menu = None; state.release_guard = true; return;
            }
        }
        if wheel_delta != 0. {
            let submenu = state.menu.as_ref().is_some_and(|menu| menu.submenu && point.is_some_and(|point| menu.submenu_rect.contains(point)));
            for (panel, node, mut scroll) in &mut controls.menu_panels {
                if panel.submenu != submenu { continue; }
                let maximum = ((node.content_size().y - node.size().y) * node.inverse_scale_factor).max(0.);
                scroll.y = (scroll.y + wheel_delta * 26.).clamp(0., maximum);
            }
        }
        return;
    }
    for drag in external.read() {
        if foreground.is_none() && let Some(origin) = point {
            state.drag = Some(FileDrag { sources: drag.sources.clone(), origin, position: origin, copy: control, destination: None, dragging: false });
        }
    }
    if mouse.get_just_pressed().next().is_some() {
        if let Some(id) = top { state.raise(id); }
        else if !over_taskbar && state.gesture.is_none() {
            state.focused = None;
            if controls.inputs.iter().any(|(entity, path, search, _)| focus.get() == Some(entity) && (path.is_some() || search.is_some())) { focus.clear(); }
        }
    }
    if mouse.just_pressed(MouseButton::Right) {
        if let (Some(id), Some(position)) = (top, point) {
            let over_list = controls.lists.iter().any(|(list, cursor)| list.window == id && cursor.cursor_over());
            if over_list && !service_open(id) {
                if let Some(window) = state.active_mut(id).filter(|window| !window.loading && !window.is_animation_picker()) {
                    let index = controls.rows.iter().find_map(|(row, cursor, _, _)| {
                        if row.window != id || !cursor.cursor_over() || row.revision != window.revision { return None; }
                        row.index.filter(|index| window.entries.get(*index).is_some_and(|entry| row.path.as_ref() == Some(&entry.path)))
                    });
                    let target = super::context_menu::target_for_window(window, index, position);
                    super::context_menu::open(&mut state, target);
                    focus.clear();
                    return;
                }
            }
        }
    }
    for (task, click) in &controls.tasks {
        if !over_taskbar || !click.is_clicked() { continue; }
        if let Some(window) = state.active_mut(task.window) { window.minimized = false; }
        state.raise(task.window);
        state.release_guard = true;
    }
    for (button, click, context) in &controls.buttons {
        if !click.is_clicked() || context.is_some() { continue; }
        if let Some(id) = button.window {
            if top != Some(id) { continue; }
            handle_action(&mut state, id, button.action, &mut requests);
            if matches!(button.action, FileAction::Minimize | FileAction::Close | FileAction::CancelPicker) { focus.clear(); }
        } else if over_taskbar && !matches!(button.action, FileAction::Confirm | FileAction::Cancel) {
            requests.write(FileManagerCommand { action: button.action, path: Some(fixed.directory.clone()), sources: fixed.selected.clone() });
        }
        state.release_guard = true;
    }
    if let Some(id) = top {
        if mouse.just_pressed(MouseButton::Left) && state.gesture.is_none() {
            let handle = controls.handles.iter().filter_map(|(_, handle, cursor)| handle.filter(|handle| handle.window == id && cursor.cursor_over()))
                .max_by_key(|handle| handle.edges.count_ones());
            if let (Some(handle), Some(origin), Some(window)) = (handle, point, state.active(id)) {
                if !window.maximized { state.gesture = Some(WindowGesture { window: id, origin, rect: window.rect, edges: handle.edges }); }
            } else if let (Some(origin), Some(window)) = (point, state.active(id)) {
                let title = controls.handles.iter().any(|(title, _, cursor)| title.is_some_and(|title| title.window == id) && cursor.cursor_over());
                if title && origin.x < window.rect.max.x - 112. {
                    if title_click.as_ref().is_some_and(|(old, seconds)| *old == id && time.elapsed_secs_f64() - seconds <= 0.4) {
                        handle_action(&mut state, id, FileAction::Maximize, &mut requests); *title_click = None;
                    } else {
                        *title_click = Some((id, time.elapsed_secs_f64()));
                        if !window.maximized { state.gesture = Some(WindowGesture { window: id, origin, rect: window.rect, edges: 0 }); }
                    }
                }
            }
        }
        if (wheel_lines != 0. || wheel_pixels != 0.) && !service_open(id) {
            if let Some(window) = state.active_mut(id) {
                if let Some(grid) = browser.as_ref().and_then(|browser| browser.thumbnail_grid(window)) {
                    grid.scroll(window, wheel_lines, wheel_pixels);
                } else {
                    window.first = window.clamp_list_first((window.first as f32 + wheel_delta).max(0.) as usize);
                }
                window.last_click = None;
            }
        }
    }
    if let (Some(gesture), Some(point)) = (&state.gesture, point) {
        let id = gesture.window;
        let mut rect = gesture.rect;
        let delta = point - gesture.origin;
        if gesture.edges == 0 { rect.min += delta; rect.max += delta; }
        else {
            if gesture.edges & 1 != 0 { rect.min.x = (rect.min.x + delta.x).min(rect.max.x - 360.); }
            if gesture.edges & 2 != 0 { rect.max.x = (rect.max.x + delta.x).max(rect.min.x + 360.); }
            if gesture.edges & 4 != 0 { rect.min.y = (rect.min.y + delta.y).min(rect.max.y - 240.); }
            if gesture.edges & 8 != 0 { rect.max.y = (rect.max.y + delta.y).max(rect.min.y + 240.); }
        }
        let rect = clamp_rect(rect, state.movement_bounds);
        if let Some(window) = state.active_mut(id) { window.rect = rect; window.restored_rect = rect; }
    }
    if mouse.just_released(MouseButton::Left) {
        // 已捕获的标题或缩放手势跨出窗口也接收最终位置，释放仍由公共鼠标门保护。
        state.release_guard |= state.gesture.take().is_some();
    }
    for (row, cursor, _, click) in &controls.rows {
        if top != Some(row.window) || service_open(row.window) || !cursor.cursor_over() { continue; }
        let Some(window) = state.active_mut(row.window).filter(|window| !window.loading && window.revision == row.revision) else { continue; };
        let Some(index) = row.index else { continue; };
        let Some(entry) = window.entries.get(index).filter(|entry| row.path.as_ref() == Some(&entry.path)).cloned() else { continue; };
        // 选择窗只维护本地单选，拖动、双击文件和修饰键都不能接管原模型属性或整理文件。
        if window.is_animation_picker() {
            if mouse.just_pressed(MouseButton::Left) || click.is_clicked() {
                window.select_picker_entry(index);
                focus.clear();
            }
            if click.is_clicked() {
                let double = window.last_click.as_ref().is_some_and(|(old, seconds)|
                    old == &entry.path && time.elapsed_secs_f64() - seconds <= 0.4);
                window.last_click = if double { None } else { Some((entry.path.clone(), time.elapsed_secs_f64())) };
                if double && entry.directory { window.navigate(entry.path, true); }
            }
            continue;
        }
        if mouse.just_pressed(MouseButton::Left) {
            if shift {
                let anchor = window.anchor.unwrap_or(index);
                let range = anchor.min(index)..=anchor.max(index).min(anchor.min(index) + 4095);
                if !control { window.selection.clear(); }
                for index in range { if let Some(entry) = window.entries.get(index) { window.selection.insert(entry.path.clone()); } }
            } else if control {
                if !window.selection.remove(&entry.path) && window.selection.len() < 4096 { window.selection.insert(entry.path.clone()); }
                window.anchor = Some(index);
            } else if !window.selection.contains(&entry.path) { window.selection.clear(); window.selection.insert(entry.path.clone()); window.anchor = Some(index); }
            let sources: Vec<_> = window.selection.iter().cloned().collect();
            if mouse.just_pressed(MouseButton::Left) && !sources.is_empty() {
                if let Some(origin) = point { state.drag = Some(FileDrag { sources, origin, position: origin, copy: control, destination: None, dragging: false }); }
            }
            focus.clear();
        }
        if click.is_clicked() && !state.drag.as_ref().is_some_and(|drag| drag.dragging) {
            let window = state.active_mut(row.window).unwrap();
            if !control && !shift {
                window.selection.clear(); window.selection.insert(entry.path.clone()); window.anchor = Some(index);
            }
            let double = window.last_click.as_ref().is_some_and(|(old, seconds)| old == &entry.path && time.elapsed_secs_f64() - seconds <= 0.4);
            window.last_click = if double { None } else { Some((entry.path.clone(), time.elapsed_secs_f64())) };
            if double {
                if entry.directory { window.navigate(entry.path, true); }
                else if !browser.as_ref().and_then(|browser| browser.asset_path(row.window, index))
                    .is_some_and(|path| controls.reference.select(path)) {
                    open_files.write(OpenManagedFile { path: entry.path });
                }
            }
        }
    }
    let destination = top.and_then(|id| {
        if service_open(id) { return None; }
        let window = state.active(id).filter(|window| !window.loading && !window.is_animation_picker())?;
        controls.rows.iter().find_map(|(row, cursor, _, _)| {
            if row.window != id || !cursor.cursor_over() || row.revision != window.revision { return None; }
            window.entries.get(row.index?)
                .filter(|entry| entry.directory && row.path.as_ref() == Some(&entry.path))
                .map(|entry| entry.path.clone())
        }).or_else(|| Some(window.path.clone()))
    }).or_else(|| (foreground.is_none() && fixed.hovered).then(|| fixed.hover_destination.clone().unwrap_or_else(|| fixed.directory.clone())));
    if let Some(drag) = &mut state.drag {
        // 目标随前景与指针更新。覆层和窗口外释放都不能复用上一帧的目录。
        drag.destination = point.and(destination)
            .filter(|destination| !drag.sources.iter().any(|source| destination == source || destination.starts_with(source)));
        if let Some(point) = point {
            drag.position = point;
            drag.copy = control;
            drag.dragging |= point.distance(drag.origin) >= 5.;
        }
    }
    if mouse.just_released(MouseButton::Left) {
        if let Some(drag) = state.drag.take().filter(|drag| drag.dragging) {
            state.release_guard = true;
            if let Some(destination) = drag.destination {
                requests.write(FileManagerCommand { action: if control { FileAction::CopyHere } else { FileAction::MoveHere }, path: Some(destination), sources: drag.sources });
            } else { state.notice = "请拖到一个文件窗口或左侧文件目录；未移动任何文件".into(); }
        }
    }
    if !ime_blocked && keys.just_pressed(KeyCode::Escape) {
        if let Some(id) = state.focused.filter(|id| state.active(*id).is_some_and(|window| window.is_animation_picker()))
            && !service_open(id) && state.gesture.is_none()
        {
            state.close(id);
            state.release_guard = true;
        }
        state.drag = None; state.gesture = None; focus.clear();
    }
    let editing = controls.inputs.iter().any(|(entity, _, _, _)| focus.get() == Some(entity));
    for (entity, path, search, input) in &controls.inputs {
        if input.is_composing() || !input.pending_edits.is_empty() || input.pending_paste.is_some() { continue; }
        if let Some(path) = path {
            if !ime_blocked && focus.get() == Some(entity) && keys.just_pressed(KeyCode::Enter) {
                let target = std::path::PathBuf::from(input.value().to_string());
                if target.is_absolute() {
                    if let Some(window) = state.active_mut(path.window) { window.navigate(target, true); }
                    focus.clear();
                } else { state.notice = "请输入完整的本地目录地址".into(); }
            }
        }
        if let Some(search) = search {
            if let Some(window) = state.active_mut(search.window) {
                let value = input.value().to_string();
                if focus.get() == Some(entity) && value != window.filter { window.filter = value; window.refresh(); }
            }
        }
    }
    if let Some(id) = state.focused {
        if service_open(id) { return; }
        if !ime_blocked && control && keys.just_pressed(KeyCode::KeyL) {
            if let Some((entity, ..)) = controls.inputs.iter().find(|(_, path, _, _)| path.is_some_and(|path| path.window == id)) { focus.set(entity, FocusCause::Navigated); }
        }
        if editing || ime_blocked { return; }
        let action = if control && shift && keys.just_pressed(KeyCode::KeyN) { Some(FileAction::NewFolder) }
            else if control && keys.just_pressed(KeyCode::KeyN) { Some(FileAction::NewWindow) }
            else if control && keys.just_pressed(KeyCode::KeyC) { Some(FileAction::Copy) }
            else if control && keys.just_pressed(KeyCode::KeyX) { Some(FileAction::Cut) }
            else if control && keys.just_pressed(KeyCode::KeyV) { Some(FileAction::Paste) }
            else if keys.just_pressed(KeyCode::Delete) { Some(FileAction::Delete) }
            else if keys.just_pressed(KeyCode::F2) { Some(FileAction::Rename) }
            else if keys.just_pressed(KeyCode::F5) { Some(FileAction::Refresh) }
            else if alt && keys.just_pressed(KeyCode::ArrowLeft) { Some(FileAction::Back) }
            else if alt && keys.just_pressed(KeyCode::ArrowRight) { Some(FileAction::Forward) }
            else if alt && keys.just_pressed(KeyCode::ArrowUp) { Some(FileAction::Up) }
            else { None };
        if let Some(action) = action { handle_action(&mut state, id, action, &mut requests); }
        if control && keys.just_pressed(KeyCode::KeyA) {
            if let Some(window) = state.active_mut(id).filter(|window| !window.loading && !window.is_animation_picker()) {
                window.selection = window.entries.iter().take(4096).map(|entry| entry.path.clone()).collect();
                if window.entries.len() > 4096 { state.notice = "本次已选择前 4096 项；请按文件夹分批整理".into(); }
            }
        }
    }
}

fn handle_action(state: &mut FileManagerState, id: u64, action: FileAction, requests: &mut MessageWriter<FileManagerCommand>) {
    let bounds = state.bounds;
    if matches!(action, FileAction::Close | FileAction::CancelPicker) {
        state.close(id);
        return;
    }
    let Some(window) = state.active_mut(id) else { return; };
    match action {
        FileAction::Minimize => { window.minimized = true; state.focused = None; }
        FileAction::Maximize => {
            window.maximized = !window.maximized;
            if window.maximized { window.restored_rect = window.rect; window.rect = bounds; }
            else { window.rect = window.restored_rect; }
        }
        FileAction::Back => { if window.history_index > 0 { window.history_index -= 1; window.navigate(window.history[window.history_index].clone(), false); } }
        FileAction::Forward => { if window.history_index + 1 < window.history.len() { window.history_index += 1; window.navigate(window.history[window.history_index].clone(), false); } }
        FileAction::Up => { if let Some(parent) = window.path.parent() { let parent = parent.to_path_buf(); window.navigate(parent, true); } }
        FileAction::Refresh => window.refresh(),
        _ if !window.loading && !window.is_animation_picker() => {
            requests.write(FileManagerCommand { action, path: Some(window.path.clone()), sources: window.selection.iter().cloned().collect() });
        }
        _ if window.loading => { state.notice = "请等待当前目录加载完成".into(); }
        _ => {}
    }
}
