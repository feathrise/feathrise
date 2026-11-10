//! 在真实作者 App 中逐项驱动生产操作。帧内只观察小夹具，截图编码和报告写入由后台任务处理。

use super::{CHILD, NATIVE_TIMEOUT, PARENT, WINDOW_REPORT, fixture::ScenePair, write_json_new};
use crate::{
    embedded_viewport::{ConnectionState, EmbeddedStatus, connection_state},
    native_project_close::embedded_validation as close,
    native_project_menu::{NativeProjectMenuState, save_startup_from_workspace},
    project_export::{messages::RequestExport, resources::ExportState},
    viewport_mode::{RequestViewportMode, ViewportGeometry, ViewportMode, ViewportModeState},
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
    ui::InteractionDisabled,
    window::PrimaryWindow,
};
use feathrise_editor_scene::standalone::{
    self, ProjectSaveProgress, embedded_snapshot::EmbeddedSnapshotState,
    embedded_validation::release_pulse, scene_format_validation as author,
};
use feathrise_editor_ui::{EditorButtonClick, InteractionButton, UiInteraction};
use feathrise_scene_content::SceneId;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, GetForegroundWindow, GetWindowThreadProcessId,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    CreateChild,
    CreateChildMenu,
    ChildForm,
    ChildReady,
    ChildObject,
    ChildSaved,
    CreateParent,
    CreateParentMenu,
    ParentForm,
    ParentReady,
    SelectSource,
    OpenTools,
    AddReference,
    SaveParent,
    ParentSaved,
    StartupSaved,
    Reopen,
    ShowTree,
    Capture,
    Export,
    PreviewReady,
    PreviewPlaying,
    Write,
    Close,
    Done,
}

#[derive(Resource)]
struct Plan {
    step: Step,
    since: Instant,
    directory: PathBuf,
    reopen: bool,
    scenes: Option<ScenePair>,
    child: Option<SceneId>,
    pressed: Option<Entity>,
    creation_pointer: Option<(Entity, bool)>,
    save_before: u64,
    preview_session: Option<(u32, u64, u64)>,
    preview_focus_started: Option<Instant>,
    native_failure_since: Option<Instant>,
    proof: Value,
    capture: Option<async_channel::Receiver<Result<Value, String>>>,
    writer: Option<Task<Result<(), String>>>,
    failed: bool,
}

pub(super) fn install(app: &mut App, directory: PathBuf, scenes: Option<ScenePair>) {
    let reopen = scenes.is_some();
    app.insert_resource(Plan {
        step: if reopen {
            Step::Reopen
        } else {
            Step::CreateChild
        },
        since: Instant::now(),
        directory,
        reopen,
        scenes,
        child: None,
        pressed: None,
        creation_pointer: None,
        save_before: 0,
        preview_session: None,
        preview_focus_started: None,
        native_failure_since: None,
        proof: json!({"os_input_verified":false,"pid":std::process::id()}),
        capture: None,
        writer: None,
        failed: false,
    })
    .add_systems(Last, drive);
}

impl Plan {
    fn go(&mut self, next: Step) {
        self.step = next;
        self.since = Instant::now();
    }

    fn scenes(&self) -> &ScenePair {
        self.scenes.as_ref().expect("此阶段已冻结场景身份")
    }

    fn finish(&mut self, error: Option<String>) {
        self.failed = error.is_some();
        self.proof["passed"] = json!(!self.failed);
        self.proof["error"] = json!(error);
        let report = self.directory.join(WINDOW_REPORT);
        let proof = self.proof.clone();
        self.writer = Some(IoTaskPool::get().spawn(async move { write_json_new(&report, &proof) }));
        self.go(Step::Write);
    }
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut plan: Mut<Plan>| {
        if let Some(pressed) = plan.pressed.take() {
            release_pulse(world, pressed);
        }
        let budget = if matches!(plan.step, Step::Export | Step::PreviewReady) {
            1500
        } else {
            90
        };
        let result =
            if plan.step != Step::Done && plan.since.elapsed() > Duration::from_secs(budget) {
                Err(format!(
                    "场景格式窗口验收在 {:?} 超时：{}",
                    plan.step,
                    standalone::author_status(world).unwrap_or("作者状态尚未初始化")
                ))
            } else {
                advance(world, &mut plan)
            };
        if let Err(error) = result {
            eprintln!("{error}");
            if matches!(plan.step, Step::Close | Step::Write | Step::Done) {
                world.write_message(AppExit::error());
                plan.go(Step::Done);
            } else {
                plan.proof["failed_step"] = json!(format!("{:?}", plan.step));
                plan.proof["last_ui"] = ui_diagnostics(world, plan.creation_pointer);
                plan.finish(Some(error));
            }
        }
    });
}

fn advance(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    let observed = author::observe(world)?;
    let current_is = |id: &SceneId| observed.current.as_ref() == Some(id);
    let lookup = |name: &str| {
        observed
            .catalog
            .as_ref()?
            .iter()
            .find_map(|(id, label)| (label == name).then(|| id.clone()))
    };
    match plan.step {
        Step::CreateChild | Step::CreateParent if observed.open && observed.catalog.is_some() => {
            if !observed.directory_visible {
                plan.pressed = click_text(world, "场景");
            } else if observed.directory_ready {
                let root = author::creation_root(world)?;
                // 新场景只从根目录的右键浮层进入。真实拾取冻结当前目录投影并打开原菜单。
                if click_creation(world, plan, root, MouseButton::Right) {
                    plan.go(if plan.step == Step::CreateChild {
                        Step::CreateChildMenu
                    } else {
                        Step::CreateParentMenu
                    });
                }
            }
        }
        Step::CreateChildMenu | Step::CreateParentMenu if observed.directory_menu_active => {
            let button = author::creation_menu_button(world)?;
            if click_creation(world, plan, button, MouseButton::Left) {
                // 释放后由生产菜单走静默帧并转发原按钮，继续等待生产表单出现。
                plan.go(if plan.step == Step::CreateChildMenu {
                    Step::ChildForm
                } else {
                    Step::ParentForm
                });
            }
        }
        Step::ChildForm | Step::ParentForm if observed.dialog => {
            let child = plan.step == Step::ChildForm;
            if let Some(button) =
                author::submit_creation(world, if child { CHILD } else { PARENT })?
            {
                plan.pressed = Some(button);
                plan.go(if child {
                    Step::ChildReady
                } else {
                    Step::ParentReady
                });
            }
        }
        Step::ChildReady => {
            if let Some(child) =
                lookup(CHILD).filter(|id| standalone::preview_ready_for(world, id.as_str()))
            {
                author::request_cuboid(world, &child)?;
                plan.child = Some(child);
                plan.go(Step::ChildObject);
            }
        }
        Step::ChildObject if observed.placement_count == 1 && observed.dirty => {
            if standalone::project_close_snapshot(world).pending.is_none() {
                plan.save_before = observed.save_sequence;
                standalone::request_save_all(world)?;
                plan.go(Step::ChildSaved);
            }
        }
        Step::ChildSaved if saved(world, &observed, plan.save_before)? => {
            plan.proof["child_save_sequence"] = json!(observed.save_sequence);
            plan.go(Step::CreateParent);
        }
        Step::ParentReady => {
            if let Some(parent) =
                lookup(PARENT).filter(|id| standalone::preview_ready_for(world, id.as_str()))
            {
                let child = plan.child.clone().ok_or("缺少新建子场景身份")?;
                if parent == child {
                    return Err("两个场景错误共用身份".into());
                }
                plan.scenes = Some(ScenePair { parent, child });
                plan.go(Step::SelectSource);
            }
        }
        Step::SelectSource if current_is(&plan.scenes().parent) => {
            if let Some(button) = author::select_source(world, &plan.scenes().child)? {
                plan.pressed = Some(button);
                plan.go(Step::OpenTools);
            }
        }
        Step::OpenTools if observed.selected_source.as_ref() == Some(&plan.scenes().child) => {
            if let Some(button) = click_text(world, "交互") {
                plan.pressed = Some(button);
                plan.go(Step::AddReference);
            }
        }
        Step::AddReference => {
            if let Some(button) = click_text(world, "将目录所选场景添加为子场景") {
                plan.pressed = Some(button);
                plan.go(Step::SaveParent);
            }
        }
        Step::SaveParent if has_reference(&observed, plan.scenes()) && observed.dirty => {
            if standalone::project_close_snapshot(world).pending.is_none() {
                plan.save_before = observed.save_sequence;
                standalone::request_save_all(world)?;
                plan.go(Step::ParentSaved);
            }
        }
        Step::ParentSaved if saved(world, &observed, plan.save_before)? => {
            plan.proof["parent_save_sequence"] = json!(observed.save_sequence);
            save_startup_from_workspace(world);
            if !world.resource::<NativeProjectMenuState>().saving() {
                return Err("正式启动场景保存没有受理当前场景".into());
            }
            plan.go(Step::StartupSaved);
        }
        Step::StartupSaved => {
            let state = world.resource::<NativeProjectMenuState>();
            if let Some(error) = state.save_failed() {
                return Err(error.into());
            }
            if !state.saving() {
                plan.proof["startup_save_completed"] = json!(true);
                plan.go(Step::ShowTree);
            }
        }
        Step::Reopen if observed.open && lookup(PARENT).as_ref() == Some(&plan.scenes().parent) => {
            standalone::request_open_scene(world, plan.scenes().parent.as_str())?;
            plan.go(Step::ShowTree);
        }
        Step::ShowTree
            if has_reference(&observed, plan.scenes())
                && !observed.dirty
                && standalone::preview_ready_for(world, plan.scenes().parent.as_str()) =>
        {
            if let Some(button) = click_text(world, "场景树") {
                plan.pressed = Some(button);
                plan.proof["parent_scene_id"] = json!(plan.scenes().parent);
                plan.proof["child_scene_id"] = json!(plan.scenes().child);
                plan.go(Step::Capture);
            }
        }
        Step::Capture if observed.tree_visible => {
            if let Some(proof) = capture(world, plan)? {
                plan.proof["screenshot"] = proof;
                if plan.reopen {
                    let destination = plan
                        .directory
                        .parent()
                        .ok_or("验收目录缺少工作根")?
                        .join("delivery");
                    world.write_message(RequestExport { destination });
                    plan.go(Step::Export);
                } else {
                    plan.finish(None);
                }
            }
        }
        Step::Export => {
            let export = world.resource::<ExportState>();
            if let Some(error) = &export.failed {
                return Err(error.clone());
            }
            if let Some(receipt) = export.receipt.as_ref() {
                plan.proof["export"] = json!(receipt);
                world.write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
                plan.go(Step::PreviewReady);
            }
        }
        Step::PreviewReady => {
            // 驱动在 Last 读取，原视口切换已在下一帧 Update 受理或写回业务拒绝原因。
            let mode = world.resource::<ViewportModeState>();
            if mode.selected != ViewportMode::EmbeddedWorld {
                plan.proof["preview_failure"] =
                    json!({"selected_mode":mode.selected,"mode_status":mode.status});
                return Err(format!("未切换到工程 3D：{}", mode.status));
            }
            let status = world.resource::<EmbeddedStatus>();
            if let Some(error) = &status.error {
                return Err(error.clone());
            }
            let snapshot = world.resource::<EmbeddedSnapshotState>();
            if let Some(error) = &snapshot.error {
                plan.proof["preview_failure"] = json!({"scene_id":snapshot.scene_id,"author_revision":snapshot.revision,"snapshot_error":error});
                return Err(format!("工程 3D 场景快照失败：{error}"));
            }
            // Connected 校验完整快照身份与采用回执。只握手成功不能代表新场景已进入工程 World。
            if connection_state(world) == ConnectionState::Connected && status.applied_revision > 0
            {
                if snapshot.scene_id.as_deref() != Some(plan.scenes().parent.as_str())
                    || status.pid == 0
                    || status.pid == std::process::id()
                    || status.world_id == 0
                {
                    return Err("工程进程采用的快照或独立进程身份错误".into());
                }
                let session = (status.pid, status.world_id, status.applied_revision);
                let focus = preview_focus(world);
                // 首次就绪在夹具目录留一个标记，真实桌面可据此给本轮作者窗口前台。
                if plan.preview_focus_started.is_none() {
                    plan.preview_focus_started = Some(Instant::now());
                    let path = plan.directory.join("preview-ready.json");
                    let proof = focus.proof(&plan.scenes().parent, session);
                    IoTaskPool::get()
                        .spawn(async move {
                            if let Err(error) = write_json_new(&path, &proof) {
                                eprintln!("保存原生预览就绪诊断失败：{error}");
                            }
                        })
                        .detach();
                }
                // 只读原生前台。Bevy focus 作为交叉证据，不修改 Window 或绕开 SDK 激活门。
                if !focus.author_foreground {
                    if plan
                        .preview_focus_started
                        .is_some_and(|started| started.elapsed() >= Duration::from_secs(75))
                    {
                        plan.proof["preview_before_play"] =
                            focus.proof(&plan.scenes().parent, session);
                        return Err("工程 3D 已采用场景，作者窗口未在75秒内成为真实前台".into());
                    }
                    return Ok(());
                }
                plan.proof["preview_before_play"] = focus.proof(&plan.scenes().parent, session);
                plan.preview_session = Some(session);
                let request = standalone::current_saved_play_request(
                    world,
                    feathrise_editor_core::ProjectBuildProfile::Debug,
                );
                world.write_message(request);
                plan.go(Step::PreviewPlaying);
            }
        }
        Step::PreviewPlaying => {
            let status = world.resource::<EmbeddedStatus>();
            if let Some(error) = &status.error {
                if error.contains("原生窗口交接未在10秒内完成") {
                    plan.proof["native_handoff_timeout_path"] =
                        json!(plan.directory.join(NATIVE_TIMEOUT));
                    // SDK 超时回读经异步 stderr 到达，给后台旁录短暂收口预算。
                    if plan
                        .native_failure_since
                        .get_or_insert_with(Instant::now)
                        .elapsed()
                        < Duration::from_millis(750)
                    {
                        return Ok(());
                    }
                }
                return Err(error.clone());
            }
            if status.playing
                && status.frames > 0
                && connection_state(world) == ConnectionState::Connected
            {
                let (pid, world_id, revision) =
                    plan.preview_session.ok_or("没有采用完整场景快照的回执")?;
                if status.pid != pid
                    || status.world_id != world_id
                    || status.applied_revision < revision
                {
                    return Err("同一工程场景的预览丢失了原已采用的快照".into());
                }
                plan.proof["preview"] = json!({"protocol_version":feathrise_editor_protocol::PROTOCOL_VERSION,
                    "scene_id":plan.scenes().parent,"pid":status.pid,"world_id":status.world_id,
                    "applied_revision":status.applied_revision,"frames":status.frames,"playing":status.playing});
                plan.finish(None);
            }
        }
        Step::Write => {
            if let Some(result) = plan.writer.as_mut().and_then(check_ready) {
                plan.writer = None;
                result?;
                if plan.failed {
                    world.write_message(AppExit::error());
                    plan.go(Step::Done);
                } else {
                    close::request(world)?;
                    plan.go(Step::Close);
                }
            }
        }
        Step::Close => {
            plan.pressed = click_text(world, "退出编辑器");
        }
        _ => {}
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct PreviewFocus {
    author_hwnd: usize,
    author_root_hwnd: usize,
    foreground_hwnd: usize,
    foreground_pid: u32,
    author_foreground: bool,
    bevy_focused: Option<bool>,
    viewport_focused: bool,
}

impl PreviewFocus {
    fn proof(&self, scene: &SceneId, (pid, world_id, revision): (u32, u64, u64)) -> Value {
        let mut proof = json!(self);
        proof["source"] = json!("Win32 GetForegroundWindow + Bevy Window");
        proof["author_pid"] = json!(std::process::id());
        proof["game_pid"] = json!(pid);
        proof["world_id"] = json!(world_id);
        proof["scene_id"] = json!(scene);
        proof["applied_revision"] = json!(revision);
        proof
    }
}

fn preview_focus(world: &mut World) -> PreviewFocus {
    let geometry = world.resource::<ViewportGeometry>();
    let author_hwnd = geometry.parent_hwnd;
    let viewport_focused = geometry.focused;
    let bevy_focused = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .ok()
        .map(|window| window.focused);
    let mut foreground_pid = 0;
    // 只核对生产常驻会话实际使用的作者父窗口，不激活窗口或派发实体输入。
    let (author_root, foreground) = unsafe {
        let root = GetAncestor(author_hwnd as HWND, GA_ROOT);
        let foreground = GetForegroundWindow();
        if !foreground.is_null() {
            GetWindowThreadProcessId(foreground, &mut foreground_pid);
        }
        (root, foreground)
    };
    PreviewFocus {
        author_hwnd,
        author_root_hwnd: author_root as usize,
        foreground_hwnd: foreground as usize,
        foreground_pid,
        author_foreground: !author_root.is_null() && author_root == foreground,
        bevy_focused,
        viewport_focused,
    }
}

fn click_creation(
    world: &mut World,
    plan: &mut Plan,
    candidate: Option<Entity>,
    button: MouseButton,
) -> bool {
    let Ok(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
    else {
        return false;
    };
    crate::locale_validation::click_entity(
        world,
        &mut plan.creation_pointer,
        window,
        candidate,
        button,
    )
}

fn ui_diagnostics(world: &mut World, pending: Option<(Entity, bool)>) -> Value {
    let Ok(observed) = author::observe(world) else {
        return Value::Null;
    };
    let root = author::creation_root(world).ok().flatten();
    let action = author::creation_menu_button(world).ok().flatten();
    let focused = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .ok()
        .map(|window| window.focused);
    let interaction = |entity: Option<Entity>| {
        entity
            .and_then(|entity| world.get::<UiInteraction>(entity))
            .map(|state| format!("{state:?}"))
    };
    json!({"open": observed.open, "window_focused": focused, "current_scene_id": observed.current,
        "catalog_entries": observed.catalog.as_ref().map(Vec::len), "dialog": observed.dialog,
        "directory_visible": observed.directory_visible, "directory_ready": observed.directory_ready,
        "directory_menu_active": observed.directory_menu_active, "root_interaction": interaction(root),
        "new_scene_interaction": interaction(action), "pointer_pressed": pending.map(|(_, pressed)| pressed),
        "tree_visible": observed.tree_visible})
}

fn has_reference(observed: &author::Observation, pair: &ScenePair) -> bool {
    observed.current.as_ref() == Some(&pair.parent)
        && observed.subscenes.as_slice() == std::slice::from_ref(&pair.child)
}

fn saved(world: &World, observed: &author::Observation, before: u64) -> Result<bool, String> {
    match standalone::project_save_progress(world) {
        ProjectSaveProgress::Failed { detail } => Err(detail),
        ProjectSaveProgress::Succeeded => Ok(!observed.dirty && observed.save_sequence > before),
        _ => Ok(false),
    }
}

fn click_text(world: &mut World, label: &str) -> Option<Entity> {
    let entity = world
        .query::<(Entity, &Text)>()
        .iter(world)
        .filter(|(_, text)| text.as_str() == label)
        .find_map(|(mut entity, _)| {
            loop {
                if !visible(world, entity) {
                    return None;
                }
                if world.get::<InteractionButton>(entity).is_some() {
                    return world
                        .get::<InteractionDisabled>(entity)
                        .is_none()
                        .then_some(entity);
                }
                entity = world.get::<ChildOf>(entity)?.parent();
            }
        })?;
    world
        .entity_mut(entity)
        .insert((UiInteraction::Pressed, EditorButtonClick::triggered()));
    Some(entity)
}

fn visible(world: &World, entity: Entity) -> bool {
    world
        .get::<ComputedNode>(entity)
        .is_some_and(|node| node.size().min_element() > 2.)
        && world
            .get::<Node>(entity)
            .is_none_or(|node| node.display != Display::None)
        && world
            .get::<InheritedVisibility>(entity)
            .is_none_or(|visibility| visibility.get())
}

fn capture(world: &mut World, plan: &mut Plan) -> Result<Option<Value>, String> {
    if let Some(receiver) = &plan.capture {
        return match receiver.try_recv() {
            Ok(result) => result.map(Some),
            Err(async_channel::TryRecvError::Empty) => Ok(None),
            Err(async_channel::TryRecvError::Closed) => Err("场景格式截图没有GPU回执".into()),
        };
    }
    // 等真实树内容出现后再回读，避免只截到切换页签前的目录或加载遮罩。
    let mut labels = [false; 3];
    for (entity, text) in world.query::<(Entity, &Text)>().iter(world) {
        if !visible(world, entity) {
            continue;
        }
        match text.as_str() {
            "场景" => labels[0] = true,
            "场景树" => labels[1] = true,
            "工程" => labels[2] = true,
            _ => {}
        }
    }
    if !labels.into_iter().all(|visible| visible)
        || !author::reference_row_visible(world, &plan.scenes().parent, CHILD)
        || plan.since.elapsed() < Duration::from_millis(300)
    {
        return Ok(None);
    }
    let path = plan.directory.join("scene-tree.png");
    let (sender, receiver) = async_channel::bounded(1);
    plan.capture = Some(receiver);
    world
        .spawn(Screenshot::primary_window())
        .observe(move |event: On<ScreenshotCaptured>| {
            let image = event.image.clone();
            let path = path.clone();
            let sender = sender.clone();
            IoTaskPool::get()
                .spawn(async move {
                    let result = (|| -> Result<Value, String> {
                        let pixels = image.data.as_ref().ok_or("场景格式截图无GPU回读数据")?;
                        if image.width() < 320 || image.height() < 240 {
                            return Err("场景格式截图尺寸无效".into());
                        }
                        let proof = json!({"source":"Bevy ScreenshotCaptured","path":path,
                    "width":image.width(),"height":image.height(),
                    "sha256":Sha256::digest(pixels).iter().map(|byte| format!("{byte:02x}")).collect::<String>()});
                        image
                            .try_into_dynamic()
                            .map_err(|error| error.to_string())?
                            .to_rgb8()
                            .save(path)
                            .map_err(|error| error.to_string())?;
                        Ok(proof)
                    })();
                    let _ = sender.try_send(result);
                })
                .detach();
        });
    Ok(None)
}
