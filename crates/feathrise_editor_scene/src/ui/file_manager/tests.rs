//! 文件窗状态的边界回归；真实按钮、后台文件和 GPU 由显式 validation 场景验证。

use super::{browser::{BrowserSessions, resources::{BrowserScan, BrowserSnapshot}}, resources::{FileManagerState, clamp_rect}, types::{FileDrag, FileEntry}};
use bevy::prelude::*;
use std::path::PathBuf;

#[test]
fn external_file_press_keeps_original_browser_click_until_drag_threshold() {
    let mut state = FileManagerState::default();
    state.drag = Some(FileDrag { sources: vec![PathBuf::from("C:/fixture/a.txt")], origin: Vec2::ZERO, position: Vec2::ZERO, copy: false, destination: None, dragging: false });
    assert!(!state.owns_mouse(), "固定文件行普通点击继续交给原预览和目录导航");
    state.drag.as_mut().unwrap().dragging = true;
    assert!(state.owns_mouse(), "越过阈值后直到释放都归文件拖放");
    state.drag = None;
    state.release_guard = true;
    assert!(state.owns_mouse(), "拖放松手帧不能对场景重复放置");
}

#[test]
fn directory_history_and_selection_are_independent_per_window() {
    let mut state = FileManagerState::default();
    state.open(PathBuf::from("C:/fixture/source"));
    state.open(PathBuf::from("C:/fixture/target"));
    state.windows[0].selection.insert(PathBuf::from("C:/fixture/source/a.txt"));
    state.windows[0].navigate(PathBuf::from("C:/fixture/source/nested"), true);
    assert_eq!(state.windows[0].history_index, 1);
    assert_eq!(state.windows[1].history_index, 0);
    assert!(state.windows[1].selection.is_empty());
    let second = state.windows[1].id;
    state.raise(state.windows[0].id);
    assert_eq!(state.windows[0].id, second, "前后顺序改变不能重置另一个窗口路径");
    assert_eq!(state.windows[0].path, PathBuf::from("C:/fixture/target"));
}

#[test]
fn resized_desktop_keeps_full_normal_window_inside_current_bounds() {
    let bounds = Rect::from_corners(Vec2::new(4., 88.), Vec2::new(796., 544.));
    let previous = Rect::from_corners(Vec2::new(1000., 700.), Vec2::new(1900., 1300.));
    let restored = clamp_rect(previous, bounds);
    assert_eq!(restored, bounds);
    let tiny = Rect::from_corners(Vec2::new(0., 44.), Vec2::new(250., 180.));
    let restored = clamp_rect(previous, tiny);
    assert_eq!(restored, tiny, "主窗口缩小后标题栏仍留在可操作区域");
}

#[test]
fn late_directory_result_cannot_replace_new_navigation_or_revive_closed_window() {
    use bevy::tasks::IoTaskPool;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).init_resource::<FileManagerState>().init_resource::<BrowserSessions>().add_systems(Update, super::scan::poll);
    {
        let mut state = app.world_mut().resource_mut::<FileManagerState>();
        state.open(PathBuf::from("C:/fixture/old"));
        let id = state.windows[0].id;
        let path = state.windows[0].path.clone();
        let context = state.context;
        state.windows[0].navigate(PathBuf::from("C:/fixture/new"), true);
        drop(state);
        app.world_mut().resource_mut::<BrowserSessions>().tasks.push((id, IoTaskPool::get().spawn(async move {
            BrowserScan { window: id, revision: 1, context, requested: path,
                result: Ok(BrowserSnapshot { root: PathBuf::from("C:/fixture"), assets: false, asset_folder: None,
                    files: vec![FileEntry { path: PathBuf::from("C:/fixture/old/stale.txt"), name: "stale.txt".into(), directory: false, bytes: 1 }].into(),
                    entries: Default::default(), animations: Default::default(), selection: Default::default(), reveal_index: None }) }
        })));
    }
    drain_scans(&mut app);
    assert!(app.world().resource::<FileManagerState>().windows[0].entries.is_empty());
    assert_eq!(app.world().resource::<FileManagerState>().windows[0].requested_path, PathBuf::from("C:/fixture/new"));
    {
        let mut state = app.world_mut().resource_mut::<FileManagerState>();
        let id = state.windows[0].id;
        let context = state.context;
        state.windows.clear();
        drop(state);
        app.world_mut().resource_mut::<BrowserSessions>().tasks.push((id, IoTaskPool::get().spawn(async move {
            BrowserScan { window: id, revision: 2, context, requested: PathBuf::from("C:/fixture/new"),
                result: Ok(BrowserSnapshot { root: PathBuf::from("C:/fixture"), assets: false, asset_folder: None,
                    files: Default::default(), entries: Default::default(), animations: Default::default(), selection: Default::default(), reveal_index: None }) }
        })));
    }
    drain_scans(&mut app);
    assert!(app.world().resource::<FileManagerState>().windows.is_empty(), "关闭浏览窗后迟到扫描不能重新创建窗口");
}

#[test]
fn failed_directory_does_not_enter_file_operation_roots() {
    use bevy::tasks::IoTaskPool;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).init_resource::<FileManagerState>().init_resource::<BrowserSessions>().add_systems(Update, super::scan::poll);
    {
        let mut state = app.world_mut().resource_mut::<FileManagerState>();
        state.allow_directory(PathBuf::from("C:/fixture"));
        state.open(PathBuf::from("D:/missing-directory"));
        let id = state.windows[0].id;
        let context = state.context;
        drop(state);
        app.world_mut().resource_mut::<BrowserSessions>().tasks.push((id, IoTaskPool::get().spawn(async move {
            BrowserScan { window: id, revision: 1, context,
                requested: PathBuf::from("D:/missing-directory"), result: Err("目录不存在".into()) }
        })));
    }
    drain_scans(&mut app);
    let state = app.world().resource::<FileManagerState>();
    assert_eq!(state.allowed_roots, vec![PathBuf::from("C:/fixture")]);
    assert_eq!(state.windows[0].error.as_deref(), Some("目录不存在"));
}

#[test]
fn empty_floating_window_selection_cannot_borrow_fixed_sidebar_file() {
    use super::{messages::{FileManagerCommand, OpenFileWindow}, resources::FixedFileSelection, types::FileAction};
    let mut app = App::new();
    app.init_resource::<FileManagerState>().init_resource::<bevy::input_focus::InputFocus>()
        .insert_resource(FixedFileSelection { directory: PathBuf::from("C:/fixture/left"), selected: vec![PathBuf::from("C:/fixture/left/keep.txt")], ..default() })
        .add_message::<OpenFileWindow>().add_message::<FileManagerCommand>().add_systems(Update, super::operations::control);
    for action in [FileAction::Copy, FileAction::Cut, FileAction::Delete, FileAction::Rename] {
        app.world_mut().resource_mut::<Messages<FileManagerCommand>>().write(FileManagerCommand { action,
            path: Some(PathBuf::from("C:/fixture/right")), sources: vec![] });
        app.update();
        let state = app.world().resource::<FileManagerState>();
        assert!(state.clipboard.is_empty(), "右窗空选择不能剪切或复制左侧路径");
        assert!(state.prompt.is_none(), "右窗空选择不能为左侧文件生成删除或重命名确认");
    }
}

#[test]
fn ime_commit_guard_blocks_file_keyboard_actions_only_for_guarded_frame() {
    use super::{messages::{ExtraRequest, FileManagerCommand, FileManagerContextRequest, FileManagerExternalDrag, OpenManagedFile}, resources::FixedFileSelection, types::{FileAction, NamePrompt}};
    let mut app = App::new();
    app.init_resource::<FileManagerState>().init_resource::<FixedFileSelection>()
        .init_resource::<bevy::input_focus::InputFocus>().init_resource::<crate::resources::EditorTextInputGuard>()
        .init_resource::<ButtonInput<KeyCode>>().init_resource::<ButtonInput<MouseButton>>().init_resource::<Time<Real>>()
        .insert_resource(crate::resources::SceneEditorState { open: true, ..default() })
        .add_message::<FileManagerCommand>().add_message::<FileManagerExternalDrag>().add_message::<OpenManagedFile>()
        .add_message::<FileManagerContextRequest>().add_message::<ExtraRequest>()
        .add_message::<bevy::input::mouse::MouseWheel>().add_systems(Update, super::input::control);
    app.world_mut().spawn((Window { focused: true, ..default() }, bevy::window::PrimaryWindow));
    app.world_mut().resource_mut::<FileManagerState>().prompt = Some(NamePrompt { action: FileAction::NewFolder,
        directory: PathBuf::from("C:/fixture"), sources: vec![], value: "中文目录".into(), initialized: true });
    for (key, action) in [(KeyCode::Enter, FileAction::Confirm), (KeyCode::Escape, FileAction::Cancel)] {
        app.world_mut().resource_mut::<crate::resources::EditorTextInputGuard>().block_shortcuts = true;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        app.update();
        assert!(app.world_mut().resource_mut::<Messages<FileManagerCommand>>().drain().next().is_none(), "IME提交末帧不得确认或取消名称对话框");
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().resource_mut::<crate::resources::EditorTextInputGuard>().block_shortcuts = false;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        app.update();
        let actions: Vec<_> = app.world_mut().resource_mut::<Messages<FileManagerCommand>>().drain().map(|request| request.action).collect();
        assert_eq!(actions, vec![action], "普通非IME键盘仍通过原命令入口");
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    }
    {
        let mut state = app.world_mut().resource_mut::<FileManagerState>();
        state.prompt = None;
        state.open(PathBuf::from("C:/fixture"));
        state.windows[0].loading = false; // 单独检验 IME 门；加载中的动作由目录身份门另行拒绝。
    }
    for blocked in [true, false] {
        app.world_mut().resource_mut::<crate::resources::EditorTextInputGuard>().block_shortcuts = blocked;
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft); keys.press(KeyCode::KeyN);
        drop(keys);
        app.update();
        let actions: Vec<_> = app.world_mut().resource_mut::<Messages<FileManagerCommand>>().drain().map(|request| request.action).collect();
        assert_eq!(actions, if blocked { vec![] } else { vec![FileAction::NewWindow] });
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    }
}

#[test]
fn context_target_preserves_selected_group_and_blank_uses_current_directory() {
    let mut state = FileManagerState::default();
    let directory = PathBuf::from("C:/fixture/window");
    state.open(directory.clone());
    let window = &mut state.windows[0];
    let folder = directory.join("子目录");
    let file = directory.join("notes.txt");
    window.entries = vec![FileEntry { path: folder.clone(), name: "子目录".into(), directory: true, bytes: 0 },
        FileEntry { path: file.clone(), name: "notes.txt".into(), directory: false, bytes: 0 }].into();
    window.selection.extend([folder.clone(), file.clone()]);
    let multiple = super::context_menu::target_for_window(window, Some(0), Vec2::new(800., 600.));
    assert_eq!(multiple.sources.len(), 2, "右键多选组内的目录不能把整理范围缩成一项");
    assert_eq!(multiple.open_directory, directory);
    window.selection.clear();
    let single = super::context_menu::target_for_window(window, Some(0), Vec2::ZERO);
    assert_eq!(single.open_directory, folder, "单目录的新窗口入口冻结所点目录");
    assert_eq!(single.directory, directory, "粘贴和新建目标仍为当前窗口目录");
    let blank = super::context_menu::target_for_window(window, None, Vec2::ZERO);
    assert!(blank.sources.is_empty());
    assert!(window.selection.is_empty());
    assert_eq!(blank.open_directory, directory);
    assert_eq!(single.sources, vec![folder], "后续空白点击不能改写已经冻结的菜单来源");
    assert_eq!(multiple.sources.len(), 2);
}

#[test]
fn closing_context_menu_consumes_press_and_same_frame_file_shortcut() {
    use super::{messages::*, resources::FixedFileSelection};
    let mut app = App::new();
    app.init_resource::<FileManagerState>().init_resource::<FixedFileSelection>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>().init_resource::<ButtonInput<MouseButton>>().init_resource::<Time<Real>>()
        .insert_resource(crate::resources::SceneEditorState { open: true, ..default() })
        .add_message::<FileManagerCommand>().add_message::<FileManagerExternalDrag>().add_message::<OpenManagedFile>()
        .add_message::<FileManagerContextRequest>().add_message::<ExtraRequest>()
        .add_message::<bevy::input::mouse::MouseWheel>().add_systems(Update, super::input::control);
    app.world_mut().spawn((Window { focused: true, ..default() }, bevy::window::PrimaryWindow));
    {
        let mut state = app.world_mut().resource_mut::<FileManagerState>();
        state.open(PathBuf::from("C:/fixture"));
        super::context_menu::open(&mut state, FileManagerContextRequest { position: Vec2::new(500., 400.),
            directory: PathBuf::from("C:/fixture"), sources: vec![], open_directory: PathBuf::from("C:/fixture"), is_directory: Some(true) }, Vec2::new(1280., 800.));
    }
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft); keys.press(KeyCode::KeyN);
    }
    app.update();
    let state = app.world().resource::<FileManagerState>();
    assert!(!state.context_menu_open());
    assert!(state.owns_mouse(), "关闭菜单的按压和后续释放仍需公共鼠标门保护");
    assert!(state.owns_keyboard(), "同帧Esc或快捷键也不能传给底层作者界面");
    assert_eq!(state.windows.len(), 1);
    assert!(app.world().resource::<Messages<FileManagerCommand>>().is_empty(), "关闭菜单同帧不得继续派发背景窗口快捷键");
}

#[test]
fn foreground_popup_blocks_file_rows_and_drop_but_window_gesture_finishes() {
    use super::{components::{FileList, FileRow, FileTitleBar, FileWindowRoot}, messages::*, resources::FixedFileSelection};
    use crate::{ui::status_bar::StatusPopup, workbench::WorkbenchFloatingPanel};
    use bevy::{input::{mouse::{MouseScrollUnit, MouseWheel}, touch::TouchPhase}, ui::RelativeCursorPosition, window::PrimaryWindow};
    use feathrise_editor_ui::{EditorButtonClick, UiInteraction};

    let mut app = App::new();
    app.init_resource::<FileManagerState>()
        .insert_resource(FixedFileSelection { directory: PathBuf::from("C:/fixture/fixed"), hovered: true, ..default() })
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>().init_resource::<ButtonInput<MouseButton>>().init_resource::<Time<Real>>()
        .insert_resource(crate::resources::SceneEditorState { open: true, ..default() })
        .add_message::<FileManagerCommand>().add_message::<FileManagerExternalDrag>().add_message::<OpenManagedFile>()
        .add_message::<FileManagerContextRequest>().add_message::<ExtraRequest>().add_message::<MouseWheel>()
        .add_systems(PreUpdate, super::input::track_pointer).add_systems(Update, super::input::control);
    let point = Vec2::new(200., 300.);
    let mut native_window = Window { focused: true, ..default() };
    native_window.set_physical_cursor_position(Some(point.as_dvec2()));
    let native_window = app.world_mut().spawn((native_window, PrimaryWindow)).id();
    let directory = PathBuf::from("C:/fixture/floating");
    let first_file = directory.join("0.txt");
    let (id, revision, old_rect) = {
        let mut state = app.world_mut().resource_mut::<FileManagerState>();
        state.open(directory.clone());
        let window = &mut state.windows[0];
        window.loading = false;
        window.entries = (0..32).map(|i| {
            let name = format!("{i}.txt");
            FileEntry { path: directory.join(&name), name, directory: false, bytes: 0 }
        }).collect::<Vec<_>>().into();
        (window.id, window.revision, window.rect)
    };
    // 覆层和下层行都具有真实布局可产生的几何悬停，只能让前景浮层接收此次输入。
    let hovered = RelativeCursorPosition { cursor_over: true, ..default() };
    let size = ComputedNode { size: Vec2::splat(300.), ..default() };
    app.world_mut().spawn((FileWindowRoot { id }, WorkbenchFloatingPanel, GlobalZIndex(423), size, InheritedVisibility::VISIBLE, hovered));
    let popup = app.world_mut().spawn((StatusPopup, GlobalZIndex(425), size, InheritedVisibility::VISIBLE, hovered)).id();
    app.world_mut().spawn((FileList { window: id }, hovered));
    let row = app.world_mut().spawn((FileRow { window: id, slot: 0, index: Some(0), path: Some(first_file.clone()), revision },
        hovered, UiInteraction::None, EditorButtonClick::default())).id();

    for button in [MouseButton::Right, MouseButton::Left] {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.reset_all();
        mouse.press(button);
        drop(mouse);
        app.world_mut().write_message(MouseWheel { unit: MouseScrollUnit::Line, x: 0., y: -1., window: native_window, phase: TouchPhase::Moved });
        app.update();
        let state = app.world().resource::<FileManagerState>();
        assert!(state.windows[0].selection.is_empty());
        assert_eq!(state.windows[0].first, 0);
        assert!(state.menu.is_none() && state.drag.is_none() && !state.owns_mouse());
    }
    // 拖到详情时取消上一帧的有效落点，也不能回退到固定栏的旧悬停快照。
    app.world_mut().resource_mut::<FileManagerState>().drag = Some(FileDrag {
        sources: vec![first_file], origin: point - Vec2::X * 20., position: point, copy: false,
        destination: Some(directory), dragging: true,
    });
    {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.clear();
        mouse.release(MouseButton::Left);
    }
    app.update();
    assert!(app.world().resource::<Messages<FileManagerCommand>>().is_empty());
    assert!(app.world().resource::<FileManagerState>().owns_mouse(), "取消放下的释放帧仍受保护");

    app.world_mut().get_mut::<RelativeCursorPosition>(popup).unwrap().cursor_over = false;
    app.world_mut().get_mut::<RelativeCursorPosition>(row).unwrap().cursor_over = false;
    app.world_mut().spawn((FileTitleBar { window: id }, hovered));
    let title_point = old_rect.min + Vec2::new(36., 12.);
    app.world_mut().get_mut::<Window>(native_window).unwrap().set_physical_cursor_position(Some(title_point.as_dvec2()));
    {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.clear();
        mouse.press(MouseButton::Left);
    }
    app.update();
    assert!(app.world().resource::<FileManagerState>().gesture.is_some());

    app.world_mut().get_mut::<RelativeCursorPosition>(popup).unwrap().cursor_over = true;
    let delta = Vec2::new(40., 20.);
    app.world_mut().get_mut::<Window>(native_window).unwrap().set_physical_cursor_position(Some((title_point + delta).as_dvec2()));
    {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.clear();
        mouse.release(MouseButton::Left);
    }
    app.update();
    let state = app.world().resource::<FileManagerState>();
    assert_eq!(state.windows[0].rect.min, old_rect.min + delta);
    assert!(state.gesture.is_none() && state.owns_mouse(), "已捕获的手势在覆层上仍采用最终位置并保护释放帧");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.update();
    assert!(!app.world().resource::<FileManagerState>().owns_mouse(), "静默帧归还鼠标所有权");
}

#[test]
fn new_file_wrong_extension_keeps_name_prompt_for_correction() {
    use super::{messages::{FileManagerCommand, OpenFileWindow}, resources::FixedFileSelection, types::{FileAction, NamePrompt}};
    let mut app = App::new();
    app.init_resource::<FileManagerState>().init_resource::<FixedFileSelection>().init_resource::<bevy::input_focus::InputFocus>()
        .add_message::<FileManagerCommand>().add_message::<OpenFileWindow>().add_systems(Update, super::operations::control);
    app.world_mut().resource_mut::<FileManagerState>().prompt = Some(NamePrompt { action: FileAction::NewGlb,
        directory: PathBuf::from("C:/fixture"), sources: vec![], value: "我的场景.txt".into(), initialized: true });
    app.world_mut().resource_mut::<Messages<FileManagerCommand>>().write(FileManagerCommand { action: FileAction::Confirm, path: None, sources: vec![] });
    app.update();
    let state = app.world().resource::<FileManagerState>();
    assert_eq!(state.prompt.as_ref().unwrap().value, "我的场景.txt");
    assert!(state.notice.contains(".glb"));
    assert!(super::operations::creation_extension_matches(FileAction::NewGlb, "我的场景.GLB"));
    assert!(super::operations::creation_extension_matches(FileAction::NewMarkdown, "说明.md"));
    assert!(super::operations::creation_extension_matches(FileAction::NewText, "笔记.txt"));
    assert!(!super::operations::creation_extension_matches(FileAction::NewText, "笔记.md"));
}

fn drain_scans(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !app.world().resource::<BrowserSessions>().tasks.is_empty() && std::time::Instant::now() < deadline {
        app.update();
        std::thread::yield_now();
    }
    assert!(app.world().resource::<BrowserSessions>().tasks.is_empty(), "测试扫描任务未按预算完成");
}
