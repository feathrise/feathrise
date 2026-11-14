//! 桌面文件窗口真交互验收；只有显式validation和宿主核验过的自建工程才会安装驱动。

mod evidence;
mod assets;

use super::file_manager::{
    FileAction, FileContextLabel, FileContextPanel, FileList, FileManagerButton, FileManagerState, FileNameInput, FileResizeHandle, FileRow,
    FileTaskButton, FileTitleBar,
};
use super::sidebars::{EditorNavigatorTab, EditorSidebarTab, SceneEditorSidebarTabButton};
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState};
use bevy::{
    app::AppExit,
    input::{ButtonState, keyboard::{Key, KeyboardInput}, mouse::MouseButtonInput},
    input_focus::InputFocus,
    math::DVec2,
    prelude::*,
    text::{EditableText, TextLayoutInfo},
    window::{Ime, PrimaryWindow, WindowEvent},
};
use feathrise_editor_project_browser_model::file_operations::{FileOperationKind, FileOperationService};
use feathrise_editor_ui::UiInteraction;
use serde_json::{Map, Value, json};
use std::{fs, path::{Path, PathBuf}, time::{Duration, Instant}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    OpenSource, FixedSelection, FixedBlankContext, FixedBlankEscape, FixedBlankCopy, FixedBlankVerify,
    FixedEntry, FixedRootReady, EnterAssets, EnterSource, SourceReady,
    OpenSecond, SecondReady, TargetUp, TargetEnter, TargetReady,
    CopySelect, CopyCommand, CopyTarget, CopyPaste, CopyWait,
    CutSelect, CutCommand, CutTarget, CutPaste, CutWait,
    NestedSource, NestedDrag, NestedWait,
    NewFolderCommand, NameNewFolder, NewFolderWait,
    NewTextCommand, NameNewText, NewTextWait, NewMarkdownCommand, NameNewMarkdown, NewMarkdownWait,
    NewGlbCommand, NameNewGlb, NewGlbWait, VerifyNewFiles,
    RenameSelect, RenameCommand, NameRename, RenameWait,
    DeleteSelect, DeleteCommand, DeleteConfirm, DeleteWait,
    DragWindow, ResizeWindow, CaptureDesktop, Maximize, VerifyMaximize, RestoreSize,
    Minimize, VerifyMinimize, RestoreTask, VerifyRestore, CaptureRestored, CloseWindow, VerifyClose, Assets, Complete,
}

#[derive(Resource)]
struct Run {
    work: PathBuf,
    project: PathBuf,
    source: PathBuf,
    destination: PathBuf,
    source_window: Option<u64>,
    target_window: Option<u64>,
    started: Instant,
    stage_started: Instant,
    stage: Stage,
    click: Option<(Entity, bool)>,
    right: Option<(DVec2, u8)>,
    menu_step: u8,
    pointer: Option<DVec2>,
    drag: Option<(Entity, DVec2, DVec2, u8)>,
    typed: u8,
    receipt: u64,
    rect_before: Option<Rect>,
    restored: Option<Rect>,
    blocked_seen: bool,
    capture: Option<evidence::CaptureTask>,
    new_files: Option<evidence::DiskTask>,
    steps: Map<String, Value>,
    screenshots: Map<String, Value>,
}

pub fn install(app: &mut App, work: PathBuf, project: PathBuf) -> Result<(), String> {
    let owner: Value = serde_json::from_slice(&fs::read(work.join(".file-manager-owner.json"))
        .map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    if work.canonicalize().ok().as_ref() != Some(&work)
        || project != work.join("project") || project.canonicalize().ok().as_ref() != Some(&project)
        || owner["purpose"] != "owned-file-manager-validation"
        || owner["work"] != json!(work) || owner["project"] != json!(project)
    {
        return Err("文件桌面驱动拒绝未核验归属的工程".into());
    }
    // 归属仍使用 canonical 身份核验；浏览行遵循展示路径，统一移除 Windows 扩展前缀再比较。
    let project = crate::ui::external_files::platform_path(&project);
    let now = Instant::now();
    app.insert_resource(Run {
        source: project.join("assets/整理来源"), destination: project.join("assets/整理目标"), work, project,
        source_window: None, target_window: None, started: now, stage_started: now, stage: Stage::OpenSource,
        click: None, right: None, menu_step: 0, pointer: None, drag: None, typed: 0, receipt: 0, rect_before: None, restored: None,
        blocked_seen: false, capture: None, new_files: None, steps: Map::new(), screenshots: Map::new(),
    }).add_systems(PreUpdate, hold_pointer.before(bevy::ui::UiSystems::Focus))
        .add_systems(Update, fixture_focus.before(crate::SceneEditorSystemSet::Input))
        .add_systems(Last, drive);
    Ok(())
}

fn hold_pointer(world: &mut World) {
    let destination = world.get_resource::<Run>().and_then(|run| {
        if let Some((_, origin, destination, frame)) = run.drag {
            let frame = frame.saturating_sub(1);
            Some(if frame < 2 { origin } else { origin.lerp(destination, ((frame - 1) as f64 / 4.).min(1.)) })
        } else if let Some((position, _)) = run.right {
            Some(position)
        } else {
            run.click.and_then(|(entity, _)| world.get::<UiGlobalTransform>(entity).map(|transform| transform.translation.as_dvec2())).or(run.pointer)
        }
    });
    let Some(destination) = destination else { return; };
    let Some(window) = world.query_filtered::<Entity, With<PrimaryWindow>>().iter(world).next() else { return; };
    // 与原绑骨实窗专项一致，防止Winit重报实体光标覆盖逐帧内部指针；不改变生产捕获状态。
    move_cursor(world, window, destination);
}

fn fixture_focus(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    // 内部窗口事件只通过生产焦点门；该设置不声称取得操作系统前台或实体键鼠证据。
    if let Ok(mut window) = windows.single_mut() { window.focused = true; }
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        if run.stage == Stage::Complete { return; }
        run.blocked_seen |= world.get_resource::<SceneEditorNumericScrubMouseGuard>().is_some_and(|guard| guard.blocked());
        let outcome = if run.started.elapsed() > Duration::from_secs(240)
            || run.stage_started.elapsed() > Duration::from_secs(if run.stage == Stage::Assets { 120 } else { 25 }) {
            Err(format!("文件桌面等待 {:?} 超时", run.stage))
        } else { advance(world, &mut run) };
        let error = match outcome {
            Ok(Some(Stage::Complete)) => None,
            Ok(Some(next)) => { run.stage = next; run.stage_started = Instant::now(); run.typed = 0; return; }
            Ok(None) => return,
            Err(error) => Some(error),
        };
        let report = json!({"schema":1,"passed":error.is_none(),"error":error,"pid":std::process::id(),
            "stage":format!("{:?}",run.stage),"elapsed_seconds":run.started.elapsed().as_secs_f32(),
            "source":"真实Feathrise编辑器+Bevy WindowEvent+原文件业务服务", "os_input_verified":false,
            "fixed_sidebar_entry_verified":run.steps.contains_key("fixed_sidebar_entry"),"steps":run.steps,"screenshots":run.screenshots,
            "context_menu_verified":run.steps.contains_key("fixed_sidebar_context_menu"),
            "menu_width_verified":(["files-context-menu.png", "files-new-submenu.png"].iter()
                .all(|name| run.screenshots.get(*name).is_some_and(|shot| shot["measured_width"].is_object()))),
            "new_file_types_verified":run.steps.contains_key("new_files_reread"),
            "mouse_guard_observed":run.blocked_seen,"windows":windows_proof(world)});
        let written = evidence::report(&run.work.join("window.json"), &report);
        if let Err(error) = &written { error!("写入文件桌面专项报告失败：{error}"); }
        run.stage = Stage::Complete;
        world.write_message(if error.is_none() && written.is_ok() { AppExit::Success } else { AppExit::error() });
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<Option<Stage>, String> {
    let Some(window) = world.query_filtered::<Entity, With<PrimaryWindow>>().iter(world).next() else { return Ok(None); };
    let source = run.source_window.unwrap_or(0);
    let target = run.target_window.unwrap_or(0);
    let next = match run.stage {
        Stage::OpenSource => {
            if !world.get_resource::<SceneEditorState>().is_some_and(|editor| editor.open && editor.pending_scene_transition.is_none())
                || !world.contains_resource::<FileManagerState>() { return Ok(None); }
            let tab = visible(world, |tab: &SceneEditorSidebarTabButton| tab.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets));
            let Some(tab) = tab else { return Ok(None); };
            let label = descendant_text(world, tab);
            if label.trim() != "文件" { return Ok(None); }
            if !click(world, &mut run.click, window, Some(tab)) { return Ok(None); }
            run.steps.insert("files_sidebar_label".into(), json!(label));
            Stage::FixedSelection
        }
        Stage::FixedSelection => {
            let index = world.get_resource::<crate::asset_catalog::project_files::EditorProjectFileCatalog>()
                .filter(|catalog| !catalog.is_loading())
                .and_then(|catalog| catalog.entries.iter().position(|entry| entry.asset_path == "Cargo.toml"));
            let Some(index) = index else { return Ok(None); };
            let row = visible(world, |row: &super::project_browser::ProjectFileRow| row.entry_index == Some(index));
            if !click(world, &mut run.click, window, row) { return Ok(None); }
            Stage::FixedBlankContext
        }
        Stage::FixedBlankContext => {
            let Some(position) = fixed_blank_position(world) else { return Ok(None); };
            run.pointer = Some(position);
            if !right_click(world, &mut run.right, window, position) { return Ok(None); }
            if !world.resource::<FileManagerState>().context_menu_open() { return Err("固定栏目录空白右键没有打开文件菜单".into()); }
            Stage::FixedBlankEscape
        }
        Stage::FixedBlankEscape => {
            key(world, window, KeyCode::Escape, Key::Escape);
            Stage::FixedBlankCopy
        }
        Stage::FixedBlankCopy => {
            if world.resource::<FileManagerState>().context_menu_open() { return Ok(None); }
            if !world.resource::<super::file_manager::FixedFileSelection>().selected.is_empty() {
                return Err("右键空白取消菜单后恢复了之前的固定栏物理文件选择".into());
            }
            if run.typed == 0 { key_state(world, window, KeyCode::ControlLeft, Key::Control, ButtonState::Pressed); run.typed = 1; return Ok(None); }
            if run.typed == 1 { key(world, window, KeyCode::KeyC, Key::Character("c".into())); run.typed = 2; return Ok(None); }
            key_state(world, window, KeyCode::ControlLeft, Key::Control, ButtonState::Released);
            Stage::FixedBlankVerify
        }
        Stage::FixedBlankVerify => {
            let state = world.resource::<FileManagerState>();
            if !state.notice.contains("请先选择") { return Ok(None); }
            if !world.resource::<super::file_manager::FixedFileSelection>().selected.is_empty() { return Err("空白复制命令借用了旧文件选择".into()); }
            run.steps.insert("fixed_blank_cancel_keeps_empty_selection".into(), json!({"input":"选择Cargo.toml→空白右键→Esc→Ctrl+C", "notice":state.notice}));
            run.pointer = None;
            Stage::FixedEntry
        }
        Stage::FixedEntry => {
            if !world.resource::<FileManagerState>().context_menu_open() && run.menu_step == 0 {
                let tools = world.query::<(&FileManagerButton, &ComputedNode, &InheritedVisibility)>().iter(world)
                    .filter(|(button, node, visible)| button.window.is_none() && node.size().min_element() > 1. && visible.get()
                        && matches!(button.action, FileAction::NewWindow | FileAction::Copy | FileAction::Cut | FileAction::Paste
                            | FileAction::Delete | FileAction::NewFolder | FileAction::Rename)).count();
                if tools != 0 { return Err("固定文件栏仍存在常驻整理按钮".into()); }
                run.steps.insert("fixed_sidebar_toolbar_removed".into(), json!(true));
            }
            if !context_command(world, run, window, None, FileAction::NewWindow)? { return Ok(None); }
            run.steps.insert("fixed_sidebar_context_menu".into(), json!({"action":"NewWindow", "input":"真实右键按下/释放与菜单左键按下/释放"}));
            Stage::FixedRootReady
        }
        Stage::FixedRootReady => {
            let state = world.resource::<FileManagerState>();
            let Some(browser) = state.windows.iter().find(|browser| !browser.loading) else { return Ok(None); };
            run.source_window = Some(browser.id);
            run.steps.insert("fixed_sidebar_entry".into(), json!({"source":"Files页目录空白右键的真实NewWindow菜单项", "window":browser.id,"initial_path":browser.path}));
            if same_path(&browser.path, &run.project.join("assets")) { Stage::EnterSource }
            else if same_path(&browser.path, &run.project) { Stage::EnterAssets }
            else { return Err(format!("固定文件栏新窗没有打开当前工程目录：{}", browser.path.display())); }
        }
        Stage::EnterAssets => {
            let path = run.project.join("assets");
            if path_ready(world, source, &path) { return Ok(Some(Stage::EnterSource)); }
            let row = visible(world, |row: &FileRow| row.window == source && row.path.as_deref() == Some(path.as_path()));
            click(world, &mut run.click, window, row);
            return Ok(None);
        }
        Stage::EnterSource => {
            if path_ready(world, source, &run.source) { return Ok(Some(Stage::SourceReady)); }
            let path = run.source.clone();
            let row = visible(world, |row: &FileRow| row.window == source && row.path.as_deref() == Some(path.as_path()));
            click(world, &mut run.click, window, row);
            return Ok(None);
        }
        Stage::SourceReady => {
            let state = world.resource::<FileManagerState>();
            let Some(browser) = state.windows.iter().find(|browser| same_path(&browser.path, &run.source) && !browser.loading) else { return Ok(None); };
            if browser.entries.len() != 4 { return Ok(None); }
            run.source_window = Some(browser.id);
            Stage::OpenSecond
        }
        Stage::OpenSecond => {
            if !context_command(world, run, window, Some(source), FileAction::NewWindow)? { return Ok(None); }
            Stage::SecondReady
        }
        Stage::SecondReady => {
            let state = world.resource::<FileManagerState>();
            let Some(browser) = state.windows.iter().find(|browser| browser.id != source && same_path(&browser.path, &run.source) && !browser.loading) else { return Ok(None); };
            run.target_window = Some(browser.id);
            Stage::TargetUp
        }
        Stage::TargetUp => {
            if !button(world, run, window, Some(target), FileAction::Up) { return Ok(None); }
            Stage::TargetEnter
        }
        Stage::TargetEnter => {
            if !path_ready(world, target, &run.project.join("assets")) { return Ok(None); }
            let path = run.destination.clone();
            let row = visible(world, |row: &FileRow| row.window == target && row.path.as_deref() == Some(path.as_path()));
            if !click(world, &mut run.click, window, row) { return Ok(None); }
            Stage::TargetReady
        }
        Stage::TargetReady => {
            if !path_ready(world, target, &run.destination) {
                let path = run.destination.clone();
                let row = visible(world, |row: &FileRow| row.window == target && row.path.as_deref() == Some(path.as_path()));
                click(world, &mut run.click, window, row);
                return Ok(None);
            }
            if !path_ready(world, source, &run.source) { return Err("第二个窗口导航改写了首窗目录".into()); }
            run.steps.insert("independent_navigation".into(), windows_proof(world));
            Stage::CopySelect
        }
        Stage::CopySelect => {
            let path = run.source.join("copy-me.txt");
            if !activate(world, run, window, source) || !select(world, run, window, source, &path) { return Ok(None); }
            Stage::CopyCommand
        }
        Stage::CopyCommand => {
            if !context_command(world, run, window, Some(source), FileAction::Copy)? { return Ok(None); }
            Stage::CopyTarget
        }
        Stage::CopyTarget => { if !activate(world, run, window, target) { return Ok(None); } Stage::CopyPaste }
        Stage::CopyPaste => { if !context_command(world, run, window, Some(target), FileAction::Paste)? { return Ok(None); } Stage::CopyWait }
        Stage::CopyWait => { if !receipt(world, run, FileOperationKind::Copy, "copy")? { return Ok(None); } Stage::CutSelect }
        Stage::CutSelect => {
            let path = run.source.join("move-me.txt");
            if !activate(world, run, window, source) || !select(world, run, window, source, &path) { return Ok(None); }
            Stage::CutCommand
        }
        Stage::CutCommand => { if !context_command(world, run, window, Some(source), FileAction::Cut)? { return Ok(None); } Stage::CutTarget }
        Stage::CutTarget => { if !activate(world, run, window, target) { return Ok(None); } Stage::CutPaste }
        Stage::CutPaste => { if !context_command(world, run, window, Some(target), FileAction::Paste)? { return Ok(None); } Stage::CutWait }
        Stage::CutWait => { if !receipt(world, run, FileOperationKind::Move, "cut_paste_move")? { return Ok(None); } Stage::NestedSource }
        Stage::NestedSource => { if !activate(world, run, window, source) { return Ok(None); } Stage::NestedDrag }
        Stage::NestedDrag => {
            let path = run.source.join("嵌套");
            let row = visible(world, |row: &FileRow| row.window == source && row.path.as_deref() == Some(path.as_path()));
            let destination = {
                let state = world.resource::<FileManagerState>();
                let Some(browser) = state.windows.iter().find(|browser| browser.id == target) else { return Ok(None); };
                // 窗口采用级联初始位置；取目标底边露出区，窄桌面两窗等宽时也避开上层源窗。
                (browser.rect.max - Vec2::splat(10.)).as_dvec2()
                    * world.get::<Window>(window).unwrap().scale_factor() as f64
            };
            if run.drag.is_none() { key_state(world, window, KeyCode::ControlLeft, Key::Control, ButtonState::Pressed); }
            if !drag(world, &mut run.drag, window, row, destination) { return Ok(None); }
            key_state(world, window, KeyCode::ControlLeft, Key::Control, ButtonState::Released);
            Stage::NestedWait
        }
        Stage::NestedWait => { if !receipt(world, run, FileOperationKind::Copy, "cross_window_ctrl_drag_directory")? { return Ok(None); } Stage::NewFolderCommand }
        Stage::NewFolderCommand => {
            if !activate(world, run, window, target) || !context_command(world, run, window, Some(target), FileAction::NewFolder)? { return Ok(None); }
            Stage::NameNewFolder
        }
        Stage::NameNewFolder => { if !name_prompt(world, run, window, "新建整理目录")? { return Ok(None); } Stage::NewFolderWait }
        Stage::NewFolderWait => { if !receipt(world, run, FileOperationKind::CreateFolder, "create_folder")? { return Ok(None); } Stage::NewTextCommand }
        Stage::NewTextCommand => { if !context_command(world, run, window, Some(target), FileAction::NewText)? { return Ok(None); } Stage::NameNewText }
        Stage::NameNewText => { if !name_prompt(world, run, window, "中文笔记.txt")? { return Ok(None); } Stage::NewTextWait }
        Stage::NewTextWait => { if !receipt(world, run, FileOperationKind::CreateFile, "create_text_file")? { return Ok(None); } Stage::NewMarkdownCommand }
        Stage::NewMarkdownCommand => { if !context_command(world, run, window, Some(target), FileAction::NewMarkdown)? { return Ok(None); } Stage::NameNewMarkdown }
        Stage::NameNewMarkdown => { if !name_prompt(world, run, window, "中文说明.md")? { return Ok(None); } Stage::NewMarkdownWait }
        Stage::NewMarkdownWait => { if !receipt(world, run, FileOperationKind::CreateFile, "create_markdown_file")? { return Ok(None); } Stage::NewGlbCommand }
        Stage::NewGlbCommand => { if !context_command(world, run, window, Some(target), FileAction::NewGlb)? { return Ok(None); } Stage::NameNewGlb }
        Stage::NameNewGlb => { if !name_prompt(world, run, window, "中文空场景.glb")? { return Ok(None); } Stage::NewGlbWait }
        Stage::NewGlbWait => { if !receipt(world, run, FileOperationKind::CreateFile, "create_glb_file")? { return Ok(None); } Stage::VerifyNewFiles }
        Stage::VerifyNewFiles => {
            let Some(proof) = evidence::new_files(&mut run.new_files, &run.destination)? else { return Ok(None); };
            run.steps.insert("new_files_reread".into(), proof);
            Stage::RenameSelect
        }
        Stage::RenameSelect => {
            let path = run.destination.join("copy-me.txt");
            if !select(world, run, window, target, &path) { return Ok(None); }
            Stage::RenameCommand
        }
        Stage::RenameCommand => { if !context_command(world, run, window, Some(target), FileAction::Rename)? { return Ok(None); } Stage::NameRename }
        Stage::NameRename => { if !name_prompt(world, run, window, "renamed.txt")? { return Ok(None); } Stage::RenameWait }
        Stage::RenameWait => { if !receipt(world, run, FileOperationKind::Rename, "rename")? { return Ok(None); } Stage::DeleteSelect }
        Stage::DeleteSelect => {
            let path = run.source.join("remove-me.txt");
            if !activate(world, run, window, source) || !select(world, run, window, source, &path) { return Ok(None); }
            Stage::DeleteCommand
        }
        Stage::DeleteCommand => { if !context_command(world, run, window, Some(source), FileAction::Delete)? { return Ok(None); } Stage::DeleteConfirm }
        Stage::DeleteConfirm => {
            if receipt(world, run, FileOperationKind::Delete, "recycle_delete")? { return Ok(Some(Stage::DragWindow)); }
            let confirm = visible(world, |button: &FileManagerButton| button.action == FileAction::Confirm);
            if !click(world, &mut run.click, window, confirm) { return Ok(None); }
            Stage::DeleteWait
        }
        Stage::DeleteWait => { if !receipt(world, run, FileOperationKind::Delete, "recycle_delete")? { return Ok(None); } Stage::DragWindow }
        Stage::DragWindow => {
            if !activate(world, run, window, target) { return Ok(None); }
            let before = *run.rect_before.get_or_insert_with(|| browser_rect(world, target).unwrap());
            let title = visible(world, |title: &FileTitleBar| title.window == target);
            let position = title.and_then(|entity| world.get::<UiGlobalTransform>(entity)).map(|value| value.translation.as_dvec2());
            let Some(position) = position else { return Ok(None); };
            let factor = world.get::<Window>(window).unwrap().scale_factor() as f64;
            let destination = run.drag.as_ref().map_or(position + DVec2::new(78., 32.) * factor, |(_, _, destination, _)| *destination);
            if !drag(world, &mut run.drag, window, title, destination) { return Ok(None); }
            let after = browser_rect(world, target).ok_or("拖动后的文件窗口消失")?;
            if before.min.distance(after.min) < 8. { return Err("真实标题栏拖动没有改变窗口位置".into()); }
            run.steps.insert("drag_window".into(), json!({"before":rect_proof(before),"after":rect_proof(after)}));
            run.rect_before = None;
            Stage::ResizeWindow
        }
        Stage::ResizeWindow => {
            let before = *run.rect_before.get_or_insert_with(|| browser_rect(world, target).unwrap());
            let handle = visible(world, |handle: &FileResizeHandle| handle.window == target && handle.edges == 10);
            let position = handle.and_then(|entity| world.get::<UiGlobalTransform>(entity)).map(|value| value.translation.as_dvec2());
            let Some(position) = position else { return Ok(None); };
            let factor = world.get::<Window>(window).unwrap().scale_factor() as f64;
            let destination = run.drag.as_ref().map_or(position + DVec2::new(95., 36.) * factor, |(_, _, destination, _)| *destination);
            if !drag(world, &mut run.drag, window, handle, destination) { return Ok(None); }
            let after = browser_rect(world, target).ok_or("缩放后的文件窗口消失")?;
            if before.size().distance(after.size()) < 8. { return Err("真实边角拖动没有改变窗口尺寸".into()); }
            run.steps.insert("resize_window".into(), json!({"before":rect_proof(before),"after":rect_proof(after)}));
            run.restored = Some(after);
            Stage::CaptureDesktop
        }
        Stage::CaptureDesktop => {
            let Some(proof) = evidence::capture(world, &mut run.capture, &run.work.join("files-desktop.png"))? else { return Ok(None); };
            run.screenshots.insert("files-desktop.png".into(), proof);
            Stage::Maximize
        }
        Stage::Maximize => { if !button(world, run, window, Some(target), FileAction::Maximize) { return Ok(None); } Stage::VerifyMaximize }
        Stage::VerifyMaximize => {
            let state = world.resource::<FileManagerState>();
            let Some(browser) = state.windows.iter().find(|browser| browser.id == target) else { return Ok(None); };
            if !browser.maximized || browser.rect.size().distance(state.bounds.size()) > 2. { return Ok(None); }
            run.steps.insert("maximize".into(), json!({"rect":rect_proof(browser.rect),"bounds":rect_proof(state.bounds)}));
            Stage::RestoreSize
        }
        Stage::RestoreSize => {
            if !button(world, run, window, Some(target), FileAction::Maximize) { return Ok(None); }
            Stage::Minimize
        }
        Stage::Minimize => {
            let state = world.resource::<FileManagerState>();
            let browser = state.windows.iter().find(|browser| browser.id == target).ok_or("还原窗口缺失")?;
            if browser.maximized || Some(browser.rect) != run.restored { return Err("最大化还原没有恢复普通窗口矩形".into()); }
            if !button(world, run, window, Some(target), FileAction::Minimize) { return Ok(None); }
            Stage::VerifyMinimize
        }
        Stage::VerifyMinimize => {
            let state = world.resource::<FileManagerState>();
            let browser = state.windows.iter().find(|browser| browser.id == target).ok_or("最小化错误地关闭了浏览会话")?;
            if !browser.minimized { return Ok(None); }
            if !same_path(&browser.path, &run.destination) { return Err("最小化丢失文件目录状态".into()); }
            run.steps.insert("minimize_keeps_session".into(), json!({"window":target,"path":browser.path}));
            Stage::RestoreTask
        }
        Stage::RestoreTask => {
            let task = visible(world, |task: &FileTaskButton| task.window == target);
            if !click(world, &mut run.click, window, task) { return Ok(None); }
            Stage::VerifyRestore
        }
        Stage::VerifyRestore => {
            let state = world.resource::<FileManagerState>();
            let browser = state.windows.iter().find(|browser| browser.id == target).ok_or("任务栏恢复窗口缺失")?;
            if browser.minimized || state.focused != Some(target) { return Ok(None); }
            if !same_path(&browser.path, &run.destination) || Some(browser.rect) != run.restored { return Err("任务栏恢复没有保留目录和窗口矩形".into()); }
            run.steps.insert("taskbar_restore".into(), windows_proof(world));
            Stage::CaptureRestored
        }
        Stage::CaptureRestored => {
            let Some(proof) = evidence::capture(world, &mut run.capture, &run.work.join("files-restored.png"))? else { return Ok(None); };
            run.screenshots.insert("files-restored.png".into(), proof);
            Stage::CloseWindow
        }
        Stage::CloseWindow => { if !button(world, run, window, Some(target), FileAction::Close) { return Ok(None); } Stage::VerifyClose }
        Stage::VerifyClose => {
            let state = world.resource::<FileManagerState>();
            if state.windows.iter().any(|browser| browser.id == target) { return Ok(None); }
            if !state.windows.iter().any(|browser| browser.id == source && same_path(&browser.path, &run.source)) { return Err("关闭目标窗口连带关闭或改写了另一浏览会话".into()); }
            if !run.blocked_seen { return Err("文件窗口交互期间没有观察到原场景鼠标保护门".into()); }
            run.steps.insert("close_independent_window".into(), json!(true));
            Stage::Assets
        }
        Stage::Assets => { if !assets::advance(world, run, window)? { return Ok(None); } Stage::Complete }
        Stage::Complete => return Ok(None),
    };
    Ok(Some(next))
}

fn browser_rect(world: &World, id: u64) -> Option<Rect> {
    world.get_resource::<FileManagerState>()?.windows.iter().find(|browser| browser.id == id).map(|browser| browser.rect)
}

fn path_ready(world: &World, id: u64, path: &Path) -> bool {
    world.get_resource::<FileManagerState>().is_some_and(|state| state.windows.iter().any(|browser| browser.id == id && same_path(&browser.path, path) && !browser.loading))
}

fn same_path(left: &Path, right: &Path) -> bool { crate::ui::external_files::platform_path(left) == crate::ui::external_files::platform_path(right) }

fn windows_proof(world: &World) -> Value {
    world.get_resource::<FileManagerState>().map_or(Value::Null, |state| json!({"focused":state.focused,"notice":state.notice,
        "windows":state.windows.iter().map(|browser| json!({"id":browser.id,"path":browser.path,"rect":rect_proof(browser.rect),
            "minimized":browser.minimized,"maximized":browser.maximized,"loading":browser.loading,"error":browser.error,
            "selected":browser.selection,"entries":browser.entries.iter().map(|entry| &entry.name).collect::<Vec<_>>() })).collect::<Vec<_>>() }))
}

fn rect_proof(rect: Rect) -> Value { json!({"min":[rect.min.x,rect.min.y],"max":[rect.max.x,rect.max.y]}) }

fn descendant_text(world: &World, parent: Entity) -> String {
    let mut pending = vec![parent];
    let mut value = String::new();
    for _ in 0..64 {
        let Some(entity) = pending.pop() else { break; };
        if let Some(text) = world.get::<Text>(entity) { value.push_str(&text.0); }
        if let Some(children) = world.get::<Children>(entity) { pending.extend(children.iter()); }
    }
    value
}

fn receipt(world: &World, run: &mut Run, kind: FileOperationKind, step: &str) -> Result<bool, String> {
    let Some(snapshot) = world.get_resource::<FileOperationService>().and_then(|service| service.snapshot()) else { return Ok(false); };
    let Some(receipt) = snapshot.completed.iter().find(|receipt| receipt.id > run.receipt && receipt.kind == kind) else { return Ok(false); };
    if receipt.cancelled || receipt.error.is_some() || receipt.completed.is_empty() {
        return Err(format!("文件操作 {:?} 未完成：{:?}", kind, receipt.error));
    }
    run.receipt = receipt.id;
    run.steps.insert(step.into(), json!({"id":receipt.id,"kind":format!("{:?}",receipt.kind),"completed":receipt.completed.iter()
        .map(|change| json!({"source":change.source,"destination":change.destination})).collect::<Vec<_>>() }));
    Ok(true)
}

fn activate(world: &mut World, run: &mut Run, window: Entity, id: u64) -> bool {
    let active = world.resource::<FileManagerState>().focused == Some(id)
        && world.resource::<FileManagerState>().windows.iter().any(|browser| browser.id == id && !browser.minimized);
    if active && !run.click.is_some_and(|(entity, _)| world.get::<FileTaskButton>(entity).is_some()) { return true; }
    let task = visible(world, |task: &FileTaskButton| task.window == id);
    click(world, &mut run.click, window, task);
    false
}

fn select(world: &mut World, run: &mut Run, window: Entity, id: u64, path: &Path) -> bool {
    // 有的控件在按下帧已经发布选择，仍必须完成原按键的释放，不能带着按住状态进入下一命令。
    if run.click.is_none() && world.resource::<FileManagerState>().windows.iter().any(|browser| browser.id == id && browser.selection.contains(path)) { return true; }
    let row = visible(world, |row: &FileRow| row.window == id && row.path.as_deref() == Some(path));
    click(world, &mut run.click, window, row);
    false
}

fn fixed_blank_position(world: &mut World) -> Option<DVec2> {
    let view = world.get_resource::<super::project_browser::ProjectBrowserState>()?.view;
    let entity = visible(world, |page: &super::project_browser::ProjectBrowserContent| page.0 == view)?;
    let node = world.get::<ComputedNode>(entity)?;
    let center = world.get::<UiGlobalTransform>(entity)?.translation;
    // 当前目录路径文字属于文件正文，位置避开刷新按钮、文件行和文本输入框。
    Some((center - node.size() * 0.5 + Vec2::new(18., 8.)).as_dvec2())
}

fn context_position(world: &mut World, id: Option<u64>, action: FileAction) -> Option<DVec2> {
    let Some(id) = id else { return fixed_blank_position(world); };
    if matches!(action, FileAction::Copy | FileAction::Cut | FileAction::Rename | FileAction::Delete) {
        let path = world.get_resource::<FileManagerState>()?.windows.iter().find(|browser| browser.id == id)?.selection.first()?.clone();
        let entity = visible(world, |row: &FileRow| row.window == id && row.path.as_deref() == Some(path.as_path()))?;
        return world.get::<UiGlobalTransform>(entity).map(|transform| transform.translation.as_dvec2());
    }
    let entity = visible(world, |list: &FileList| list.window == id)?;
    let node = world.get::<ComputedNode>(entity)?;
    let center = world.get::<UiGlobalTransform>(entity)?.translation;
    Some((center + Vec2::new(0., (node.size().y * 0.5 - 14.).max(0.))).as_dvec2())
}

fn menu_width(world: &mut World, submenu: bool) -> Result<Option<Value>, String> {
    let Some(panel) = visible(world, |panel: &FileContextPanel| panel.submenu == submenu) else { return Ok(None); };
    let node = world.get::<ComputedNode>(panel).expect("可见菜单已布局");
    let inverse = node.inverse_scale_factor;
    let actual = node.size().x * inverse;
    let viewport = world.query_filtered::<&ComputedNode, With<crate::components::SceneEditorRoot>>().single(world)
        .map_err(|error| error.to_string())?;
    let viewport_width = viewport.size().x * viewport.inverse_scale_factor;
    let mut longest = (0., "");
    let mut measured_rows = 0;
    // 标签可能因菜单高度而需要滚动，但隐藏的未安装 IDE 不能参与量宽。
    for (parent, text, layout) in world.query_filtered::<(&ChildOf, &Text, &TextLayoutInfo), With<FileContextLabel>>().iter(world) {
        let row = parent.parent();
        if !world.get::<ChildOf>(row).is_some_and(|parent| parent.parent() == panel)
            || world.get::<Node>(row).is_none_or(|node| node.display == Display::None)
        {
            continue;
        }
        if layout.glyphs.is_empty() || layout.size.x <= 0. { return Ok(None); }
        let width = layout.size.x * inverse;
        if width > longest.0 { longest = (width, text.as_str()); }
        measured_rows += 1;
    }
    if measured_rows < if submenu { 4 } else { 11 } { return Ok(None); }
    let (text_width, longest_label) = longest;
    // 8px 行内边距、4px 面板内边距、1px 边框各两侧。实际布局可有一物理像素舍入。
    let expected = (text_width + 26.).ceil().min((viewport_width - 8.).max(1.));
    if (actual - expected).abs() > (1. + inverse) {
        return Err(format!("{}宽度未按实际文字排布：最长项「{longest_label}」{text_width:.2}px，期望 {expected:.2}px，实际 {actual:.2}px",
            if submenu { "新建子菜单" } else { "文件右键菜单" }));
    }
    Ok(Some(json!({"panel_logical_px":actual,"text_logical_px":text_width,"expected_logical_px":expected,
        "longest_label":longest_label,"measured_rows":measured_rows,"ui_scale":inverse.recip()})))
}

fn context_command(world: &mut World, run: &mut Run, window: Entity, id: Option<u64>, action: FileAction) -> Result<bool, String> {
    if run.menu_step == 0 {
        let Some(position) = context_position(world, id, action) else { return Ok(false); };
        run.pointer = Some(position);
        if !right_click(world, &mut run.right, window, position) { return Ok(false); }
        if !world.resource::<FileManagerState>().context_menu_open() { return Err(format!("{:?} 没有经真实右键打开菜单", action)); }
        run.menu_step = 1;
        return Ok(false);
    }
    let creation = matches!(action, FileAction::NewFolder | FileAction::NewText | FileAction::NewMarkdown | FileAction::NewGlb);
    if creation && run.menu_step == 1 {
        let entity = visible(world, |button: &FileManagerButton| button.window.is_none() && button.action == FileAction::NewMenu);
        if let Some(position) = entity.and_then(|entity| world.get::<UiGlobalTransform>(entity)).map(|transform| transform.translation.as_dvec2()) { run.pointer = Some(position); }
        if !click(world, &mut run.click, window, entity) { return Ok(false); }
        run.menu_step = 2;
        return Ok(false);
    }
    let entity = visible(world, |button: &FileManagerButton| button.window.is_none() && button.action == action);
    let Some(entity) = entity else { return Ok(false); };
    if !creation {
        run.pointer = world.get::<UiGlobalTransform>(entity).map(|transform| transform.translation.as_dvec2());
    }
    let capture_name = if id.is_none() && action == FileAction::NewWindow { Some("files-context-menu.png") }
        else if creation && action == FileAction::NewFolder { Some("files-new-submenu.png") } else { None };
    if let Some(name) = capture_name.filter(|name| !run.screenshots.contains_key(*name)) {
        let actions = if creation { vec![FileAction::NewFolder, FileAction::NewText, FileAction::NewMarkdown, FileAction::NewGlb] }
            else { vec![FileAction::NewWindow, FileAction::Copy, FileAction::Cut, FileAction::Paste, FileAction::Rename, FileAction::Delete, FileAction::NewMenu] };
        let mut labels = Vec::new();
        for action in actions {
            let Some(item) = visible(world, |button: &FileManagerButton| button.window.is_none() && button.action == action) else { return Ok(false); };
            labels.push(descendant_text(world, item));
        }
        let Some(width) = menu_width(world, creation)? else { return Ok(false); };
        let Some(mut proof) = evidence::capture(world, &mut run.capture, &run.work.join(name))? else { return Ok(false); };
        proof["menu_labels"] = json!(labels);
        proof["measured_width"] = width;
        proof["input"] = json!("真实右键和菜单左键 WindowEvent");
        run.screenshots.insert(name.into(), proof);
    }
    // 截图期间保持“新建”父项悬停；等 GPU 回执完成后才移入目标子项执行。
    run.pointer = world.get::<UiGlobalTransform>(entity).map(|transform| transform.translation.as_dvec2());
    if !click(world, &mut run.click, window, Some(entity)) { return Ok(false); }
    run.pointer = None;
    run.menu_step = 0;
    Ok(true)
}

fn right_click(world: &mut World, pending: &mut Option<(DVec2, u8)>, window: Entity, position: DVec2) -> bool {
    let (position, frame) = *pending.get_or_insert((position, 0));
    move_cursor(world, window, position);
    match frame {
        0 => {}
        1 => window_event(world, MouseButtonInput { window, button: MouseButton::Right, state: ButtonState::Pressed }),
        2 => window_event(world, MouseButtonInput { window, button: MouseButton::Right, state: ButtonState::Released }),
        _ => { *pending = None; return true; }
    }
    *pending = Some((position, frame + 1));
    false
}

fn button(world: &mut World, run: &mut Run, window: Entity, id: Option<u64>, action: FileAction) -> bool {
    let target = visible(world, |button: &FileManagerButton| button.window == id && button.action == action);
    click(world, &mut run.click, window, target)
}

fn name_prompt(world: &mut World, run: &mut Run, window: Entity, value: &str) -> Result<bool, String> {
    let confirming = (run.typed == 3).then_some(run.click).flatten().map(|(entity, _)| entity)
        .filter(|entity| world.get::<FileManagerButton>(*entity).is_some_and(|button| button.action == FileAction::Confirm));
    if let Some(entity) = confirming {
        // 确认按钮按下后文本焦点可以正常移走；此时必须把原按钮的释放送完，不能回抢输入框。
        return Ok(click(world, &mut run.click, window, Some(entity)));
    }
    let Some(input) = visible(world, |_: &FileNameInput| true) else { return Ok(false); };
    if world.resource::<InputFocus>().get() != Some(input) || run.click.is_some_and(|(entity, _)| entity == input) {
        raw_click(world, &mut run.click, window, input);
        return Ok(false);
    }
    if run.typed == 0 {
        key(world, window, KeyCode::End, Key::End);
        run.typed = 1;
        return Ok(false);
    }
    if run.typed == 1 {
        let current = world.get::<EditableText>(input).map(|text| text.value().to_string()).unwrap_or_default();
        for _ in current.chars() { key(world, window, KeyCode::Backspace, Key::Backspace); }
        run.typed = 2;
        return Ok(false);
    }
    if run.typed == 2 {
        if world.get::<EditableText>(input).is_none_or(|text| !text.value().to_string().is_empty()) { return Ok(false); }
        // 清空旧值与提交中文分开等到实际编辑器采用，不能依赖不同事件系统的同帧先后顺序。
        window_event(world, Ime::Commit { window, value: value.into() });
        run.typed = 3;
        return Ok(false);
    }
    if world.get::<EditableText>(input).is_none_or(|text| text.value().to_string() != value) { return Ok(false); }
    let confirm = visible(world, |button: &FileManagerButton| button.action == FileAction::Confirm);
    Ok(click(world, &mut run.click, window, confirm))
}

fn visible<C: Component>(world: &mut World, predicate: impl Fn(&C) -> bool) -> Option<Entity> {
    world.query::<(Entity, &C, &ComputedNode, &InheritedVisibility)>().iter(world)
        .find(|(_, component, node, visibility)| predicate(component) && node.size().min_element() > 1. && visibility.get())
        .map(|(entity, ..)| entity)
}

fn click(world: &mut World, pending: &mut Option<(Entity, bool)>, window: Entity, target: Option<Entity>) -> bool {
    let Some(entity) = target else { release_pending_click(world, pending, window); return false; };
    if pending.is_some_and(|(previous, _)| previous != entity) {
        release_pending_click(world, pending, window);
        return false;
    }
    let Some(position) = world.get::<UiGlobalTransform>(entity).map(|transform| transform.translation.as_dvec2()) else {
        release_pending_click(world, pending, window);
        return false;
    };
    move_cursor(world, window, position);
    match *pending {
        None => *pending = Some((entity, false)),
        Some((_, false)) if world.get::<UiInteraction>(entity) == Some(&UiInteraction::Hovered) => {
            mouse(world, window, ButtonState::Pressed); *pending = Some((entity, true));
        }
        Some((_, true)) if world.get::<UiInteraction>(entity) == Some(&UiInteraction::Pressed) => {
            mouse(world, window, ButtonState::Released); *pending = None; return true;
        }
        _ => {}
    }
    false
}

fn raw_click(world: &mut World, pending: &mut Option<(Entity, bool)>, window: Entity, entity: Entity) -> bool {
    if pending.is_some_and(|(previous, _)| previous != entity) {
        release_pending_click(world, pending, window);
        return false;
    }
    let Some(position) = world.get::<UiGlobalTransform>(entity).map(|transform| transform.translation.as_dvec2()) else {
        release_pending_click(world, pending, window);
        return false;
    };
    move_cursor(world, window, position);
    match *pending {
        None => *pending = Some((entity, false)),
        Some((_, false)) => { mouse(world, window, ButtonState::Pressed); *pending = Some((entity, true)); }
        Some((_, true)) => { mouse(world, window, ButtonState::Released); *pending = None; return true; }
    }
    false
}

fn release_pending_click(world: &mut World, pending: &mut Option<(Entity, bool)>, window: Entity) {
    // 布局重挂或目标换绑不能遗留内部按住状态；先收口旧Press，再允许后续帧重新定位。
    if pending.take().is_some_and(|(_, pressed)| pressed) { mouse(world, window, ButtonState::Released); }
}

fn drag(world: &mut World, pending: &mut Option<(Entity, DVec2, DVec2, u8)>, window: Entity, target: Option<Entity>, destination: DVec2) -> bool {
    if pending.is_none() {
        let Some(entity) = target else { return false; };
        let Some(transform) = world.get::<UiGlobalTransform>(entity) else { return false; };
        *pending = Some((entity, transform.translation.as_dvec2(), destination, 0));
    }
    let Some((_, origin, destination, frame)) = *pending else { return false; };
    let position = if frame < 2 { origin } else { origin.lerp(destination, ((frame - 1) as f64 / 4.).min(1.)) };
    move_cursor(world, window, position);
    if frame == 1 { mouse(world, window, ButtonState::Pressed); }
    if frame == 6 { mouse(world, window, ButtonState::Released); }
    if frame >= 7 { *pending = None; return true; }
    if let Some(value) = pending { value.3 += 1; }
    false
}

fn key(world: &mut World, window: Entity, code: KeyCode, logical: Key) {
    key_state(world, window, code, logical.clone(), ButtonState::Pressed);
    key_state(world, window, code, logical, ButtonState::Released);
}

fn key_state(world: &mut World, window: Entity, key_code: KeyCode, logical_key: Key, state: ButtonState) {
    window_event(world, KeyboardInput { window, key_code, logical_key, state, repeat: false, text: None });
}

fn mouse(world: &mut World, window: Entity, state: ButtonState) {
    window_event(world, MouseButtonInput { window, button: MouseButton::Left, state });
}

fn move_cursor(world: &mut World, window: Entity, physical: DVec2) {
    let mut target = world.get_mut::<Window>(window).unwrap();
    target.set_physical_cursor_position(Some(physical));
    let position = physical.as_vec2() / target.scale_factor();
    window_event(world, CursorMoved { window, position, delta: None });
}

fn window_event(world: &mut World, event: impl Message + Clone + Into<WindowEvent>) {
    let window_event: WindowEvent = event.clone().into();
    world.write_message(window_event);
    world.write_message(event);
}
