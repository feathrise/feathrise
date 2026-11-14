//! 桌面文件会话；浏览窗可关闭，文件任务及进程内剪贴板由独立服务持续持有。

use super::types::*;
use super::messages::OpenAnimationPicker;
use bevy::prelude::*;
use feathrise_editor_project_browser_model::file_operations::{ConflictPolicy, FileOperationSnapshot};
use std::path::PathBuf;

#[derive(Resource, Default)]
pub(crate) struct FixedFileSelection {
    pub(crate) directory: PathBuf,
    pub(crate) selected: Vec<PathBuf>,
    pub(crate) hovered: bool,
    pub(crate) hover_destination: Option<PathBuf>,
    pub(crate) asset_view: bool,
    pub(crate) asset_folder: String,
    pub(crate) loading: bool,
}

impl FixedFileSelection {
    /// 固定资产行显示真实整理选择；目录行可被右键选中，空白取消后不能仍显示旧预览高亮。
    pub(crate) fn asset_row_selected(&self, path: &str) -> Option<bool> {
        if !self.asset_view || self.directory.as_os_str().is_empty() { return None; }
        let relative = std::path::Path::new(path);
        if relative.parent() != Some(std::path::Path::new(&self.asset_folder)) { return None; }
        let name = relative.file_name()?;
        Some(self.selected.contains(&self.directory.join(name)))
    }
}

#[derive(Resource)]
pub(crate) struct FileManagerState {
    pub(crate) windows: Vec<FileWindow>,
    pub(crate) focused: Option<u64>,
    pub(crate) notice: String,
    pub(crate) bounds: Rect,
    pub(super) movement_bounds: Rect,
    pub(super) next_id: u64,
    pub(super) context: u64,
    pub(super) source_context: Option<u64>,
    pub(super) project: Option<PathBuf>,
    pub(super) allowed_roots: Vec<PathBuf>,
    pub(super) clipboard: Vec<PathBuf>,
    pub(super) cut: bool,
    pub(super) gesture: Option<WindowGesture>,
    pub(super) drag: Option<FileDrag>,
    pub(super) prompt: Option<NamePrompt>,
    pub(super) menu: Option<FileContextMenu>,
    pub(super) hovered: bool,
    pub(super) release_guard: bool,
    pub(super) conflict: ConflictPolicy,
    pub(super) snapshot: Option<FileOperationSnapshot>,
    pub(super) operation_revision: u64,
    pub(super) last_receipt: u64,
    pub(super) refresh_pending: bool,
    pub(super) suspended: bool,
}

impl Default for FileManagerState {
    fn default() -> Self {
        Self { windows: vec![], focused: None, notice: String::new(), bounds: Rect::from_corners(Vec2::new(8., 88.), Vec2::new(1200., 740.)), movement_bounds: Rect::from_corners(Vec2::new(8., 88.), Vec2::new(1200., 740.)), next_id: 1,
            context: 0, source_context: None, project: None, allowed_roots: vec![], clipboard: vec![], cut: false, gesture: None,
            drag: None, prompt: None, menu: None, hovered: false, release_guard: false, conflict: ConflictPolicy::KeepBoth,
            snapshot: None, operation_revision: 0, last_receipt: 0, refresh_pending: false, suspended: false }
    }
}

impl FileManagerState {
    pub(crate) fn service_context(&self) -> u64 { self.context }
    #[cfg(any(test, feature = "validation"))]
    pub(crate) fn context_menu_open(&self) -> bool { self.menu.is_some() }
    pub(crate) fn owns_mouse(&self) -> bool { self.hovered || self.gesture.is_some() || self.drag.as_ref().is_some_and(|drag| drag.dragging) || self.prompt.is_some() || self.menu.is_some() || self.release_guard }
    pub(crate) fn owns_keyboard(&self) -> bool { self.focused.is_some() || self.prompt.is_some() || self.menu.is_some() || self.release_guard }
    pub(super) fn active(&self, id: u64) -> Option<&FileWindow> { self.windows.iter().find(|window| window.id == id) }
    pub(super) fn active_mut(&mut self, id: u64) -> Option<&mut FileWindow> { self.windows.iter_mut().find(|window| window.id == id) }
    pub(super) fn raise(&mut self, id: u64) {
        if let Some(index) = self.windows.iter().position(|window| window.id == id) {
            let window = self.windows.remove(index);
            self.windows.push(window);
            self.focused = Some(id);
        }
    }
    pub(super) fn open(&mut self, path: PathBuf) {
        if self.windows.iter().filter(|window| !window.is_animation_picker()).count() >= 8 {
            self.notice = "最多同时打开 8 个文件窗口，请先关闭一个窗口".into();
            return;
        }
        self.open_window(path, None);
    }
    pub(super) fn open_animation_picker(&mut self, request: &OpenAnimationPicker) {
        if request.context != self.context || self.suspended || self.prompt.is_some() || self.menu.is_some() {
            return;
        }
        if let Some(window) = self.windows.iter().find(|window| window.is_animation_picker()) {
            let id = window.id;
            if window.animation_picker.as_ref().is_some_and(|picker| picker.target == request.target) {
                self.active_mut(id).unwrap().minimized = false;
                self.raise(id);
                return;
            }
            self.close(id);
        }
        let picker = AnimationPicker { target: request.target.clone(), clip: None };
        self.open_window(request.directory.clone(), Some(picker));
    }
    fn open_window(&mut self, path: PathBuf, animation_picker: Option<AnimationPicker>) {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let size = Vec2::new(690., 460.).min(self.movement_bounds.size());
        let offset = (self.windows.len() % 5) as f32 * 26.;
        let rect = clamp_rect(Rect::from_corners(self.bounds.min + Vec2::splat(offset), self.bounds.min + Vec2::splat(offset) + size), self.movement_bounds);
        self.windows.push(FileWindow { id, path: path.clone(), requested_path: path.clone(), rect, restored_rect: rect,
            minimized: false, maximized: false, entries: Default::default(), selection: Default::default(), history: vec![path], history_index: 0,
            first: 0, thumbnail_wheel: 0., loading: true, error: None, filter: String::new(), revision: 1,
            animation_picker, scanned_revision: 0, anchor: None, last_click: None });
        self.focused = Some(id);
    }
    pub(super) fn close(&mut self, id: u64) {
        if let Some(index) = self.windows.iter().position(|window| window.id == id) {
            let window = self.windows.remove(index);
            bevy::tasks::IoTaskPool::get().spawn(async move { drop(window); }).detach();
        }
        if self.focused == Some(id) {
            self.focused = self.windows.iter().rev().find(|window| !window.minimized).map(|window| window.id);
        }
        if self.gesture.as_ref().is_some_and(|gesture| gesture.window == id) {
            self.gesture = None;
        }
    }
    pub(crate) fn finish_animation_picker(&mut self, target: &ProjectMotionTarget) {
        if let Some(id) = self.animation_picker_id(|current| current == target) {
            self.close(id);
            self.release_guard = true;
        }
    }
    pub(crate) fn close_stale_animation_picker(&mut self, valid: impl Fn(&ProjectMotionTarget) -> bool) {
        if let Some(id) = self.animation_picker_id(|current| !valid(current)) {
            self.close(id);
        }
    }
    fn animation_picker_id(&self, matches: impl Fn(&ProjectMotionTarget) -> bool) -> Option<u64> {
        self.windows.iter().find_map(|window| {
            window.animation_picker.as_ref().filter(|picker| matches(&picker.target)).map(|_| window.id)
        })
    }
    pub(super) fn allow_directory(&mut self, path: PathBuf) {
        // 只接收扫描已成功的目录。访问父级后收拢已覆盖子根，避免搬走子目录后留下过期授权。
        if self.allowed_roots.iter().any(|root| path.starts_with(root)) { return; }
        self.allowed_roots.retain(|root| !root.starts_with(&path));
        if self.allowed_roots.len() < 32 {
            self.allowed_roots.push(path);
        }
    }
}

pub(super) fn clamp_rect(rect: Rect, bounds: Rect) -> Rect {
    let size = rect.size().max(Vec2::new(360., 240.)).min(bounds.size().max(Vec2::splat(1.)));
    let minimum = rect.min.max(bounds.min).min((bounds.max - size).max(bounds.min));
    Rect::from_corners(minimum, minimum + size)
}
