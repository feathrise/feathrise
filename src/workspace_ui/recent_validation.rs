//! 最近工程真实首页专项。只驱动本轮独占配置上的鼠标消息，实际保存和重开仍由宿主处理。

use super::{
    Page, WorkspaceCommand, WorkspaceModel, WorkspaceProbe,
    components::{RecentAction, RecentActionKind},
};
use bevy::{
    ecs::message::MessageCursor,
    input::{ButtonState, mouse::MouseButtonInput},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
    ui::InteractionDisabled,
    window::{CursorMoved, PrimaryWindow, WindowEvent},
};
use feathrise_editor_ui::UiInteraction;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const REPORT: &str = "recent-projects.json";
const SCREENSHOT: &str = "recent-projects.png";
const NAME: &str = "同名工程";
const TOGGLES: [bool; 3] = [true, false, true];
const MAX_CAPTURE_ATTEMPTS: u8 = 16;
const CAPTURE_RETRY_INTERVAL: Duration = Duration::from_millis(250);
const MIN_UI_COLORS: usize = 65;

#[derive(Resource)]
struct Plan {
    directory: PathBuf,
    preferences: PathBuf,
    paths: [PathBuf; 2],
    reopen: bool,
    started: Instant,
    resized: bool,
    step: usize,
    phase: u8,
    ready_frames: u8,
    command_seen: bool,
    proofs: Vec<Value>,
    commands: MessageCursor<WorkspaceCommand>,
    capture_attempts: u8,
    next_capture: Instant,
    result: Option<async_channel::Receiver<Result<bool, String>>>,
}

pub(crate) fn install(app: &mut App, directory: PathBuf, mode: &OsStr) -> Result<(), String> {
    let reopen = mode == "reopen";
    if (mode != "exercise" && !reopen)
        || directory.file_name() != Some(mode)
        || fs::read_dir(&directory).map_err(error)?.next().is_some()
    {
        return Err("最近工程专项需要新的 exercise/reopen 目录与对应模式".into());
    }
    let root = directory.parent().ok_or("最近工程专项缺少夹具根")?;
    let preferences = root.join("editor.json");
    let configured =
        std::env::var_os("FEATHRISE_PREFERENCES_FILE").ok_or("必须显式隔离最近工程偏好")?;
    let plugins =
        std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS").ok_or("必须显式隔离全局插件")?;
    if Path::new(&configured).canonicalize().map_err(error)? != preferences
        || Path::new(&plugins).canonicalize().map_err(error)?
            != root.join("isolated-global-plugins")
    {
        return Err("最近工程专项的配置与插件必须位于本轮夹具根".into());
    }
    let model = app.world().resource::<WorkspaceModel>();
    let (first, second) = if reopen { (0, 1) } else { (1, 0) };
    if model.project.is_some()
        || model.recent.len() != 2
        || model.recent.iter().any(|entry| entry.name != NAME)
    {
        return Err("最近工程专项只接受两个本轮同名记录".into());
    }
    let paths = [
        model.recent[first].path.clone(),
        model.recent[second].path.clone(),
    ];
    for (path, worktree) in paths.iter().zip(["worktree-a", "worktree-b"]) {
        let expected = root.join(worktree).join("project");
        if path.canonicalize().map_err(error)? != expected || !expected.is_dir() {
            return Err("最近工程记录必须属于本轮自建工作树".into());
        }
    }
    let source = read_json(&preferences)?;
    check_disk(&source, &paths, reopen)?;
    if reopen {
        let previous = read_json(&root.join("exercise").join(REPORT))?;
        if previous["passed"] != true
            || previous["gpu_capture"]["has_ui_pixels"] != true
            || previous["preferences"] != json!(preferences)
            || previous["pid"] == std::process::id()
        {
            return Err("重开验收必须读取另一进程已经验证的同一份偏好".into());
        }
    } else if source["recent_projects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry.get("pinned").is_some())
    {
        return Err("首次验收须由没有 pinned 字段的旧偏好开始".into());
    }
    write_new(
        &directory.join("owner.json"),
        &json!({"pid":std::process::id(), "root":root}),
    )?;
    app.insert_resource(Plan {
        directory,
        preferences,
        paths,
        reopen,
        started: Instant::now(),
        resized: false,
        step: if reopen { TOGGLES.len() } else { 0 },
        phase: 0,
        ready_frames: 0,
        command_seen: false,
        proofs: Vec::new(),
        commands: default(),
        capture_attempts: 0,
        next_capture: Instant::now(),
        result: None,
    })
    .add_systems(First, input.before(bevy::picking::PickingSystems::Input))
    .add_systems(Last, drive);
    Ok(())
}

fn window_event(world: &mut World, event: impl Message + Clone + Into<WindowEvent>) {
    world.write_message::<WindowEvent>(event.clone().into());
    world.write_message(event);
}

fn input(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<Plan>() else {
        return;
    };
    issue_input(world, &mut plan);
    world.insert_resource(plan);
}

fn issue_input(world: &mut World, plan: &mut Plan) {
    let Ok((window, mut settings)) = world
        .query_filtered::<(Entity, &mut Window), With<PrimaryWindow>>()
        .single_mut(world)
    else {
        return;
    };
    if !plan.resized {
        settings.resolution.set(900., 680.);
        plan.resized = true;
        return;
    }
    if (settings.width() - 900.).abs() > 2. || plan.phase >= 3 {
        return;
    }
    let Some(&pinned) = TOGGLES.get(plan.step) else {
        return;
    };
    if world.resource::<WorkspaceModel>().busy
        || world.resource::<WorkspaceProbe>().page != Page::Start
    {
        return;
    }
    let target = world
        .query::<(
            &RecentAction,
            &UiInteraction,
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
            Has<InteractionDisabled>,
        )>()
        .iter(world)
        .find_map(|(action, interaction, node, at, visibility, disabled)| {
            (action.kind == RecentActionKind::SetPinned(pinned)
                && action.path == plan.paths[0]
                && visibility.get()
                && !node.is_empty()
                && !disabled)
                .then_some((at.translation, *interaction))
        });
    let Some((position, interaction)) = target else {
        return;
    };
    let mut settings = world.get_mut::<Window>(window).unwrap();
    settings.set_physical_cursor_position(Some(position.as_dvec2()));
    let position = position / settings.scale_factor();
    window_event(
        world,
        CursorMoved {
            window,
            position,
            delta: None,
        },
    );
    // 与窗口后端相同的两路事件。生产拾取生成悬停/按下，再由原按钮生成释放点击。
    let state = match (plan.phase, interaction) {
        (0, _) => {
            plan.phase = 1;
            return;
        }
        (1, UiInteraction::Hovered) => ButtonState::Pressed,
        (2, UiInteraction::Pressed) => ButtonState::Released,
        _ => return,
    };
    plan.phase += 1;
    window_event(
        world,
        MouseButtonInput {
            button: MouseButton::Left,
            state,
            window,
        },
    );
}

fn drive(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<Plan>() else {
        return;
    };
    match tick(world, &mut plan) {
        Ok(false) => world.insert_resource(plan),
        Ok(true) => {
            world.write_message(AppExit::Success);
        }
        Err(error) => {
            eprintln!(
                "最近工程专项失败，步骤 {}，鼠标阶段 {}：{error}",
                plan.step, plan.phase
            );
            world.write_message(AppExit::error());
        }
    }
}

fn tick(world: &mut World, plan: &mut Plan) -> Result<bool, String> {
    if plan.started.elapsed() > Duration::from_secs(75) {
        return Err("等待真实首页、保存回执或截图超时".into());
    }
    if let Some(result) = &plan.result {
        return match result.try_recv() {
            Ok(Ok(false)) => {
                plan.result = None;
                plan.next_capture = Instant::now() + CAPTURE_RETRY_INTERVAL;
                if plan.capture_attempts == MAX_CAPTURE_ATTEMPTS {
                    return Err(
                        "GPU 截图重试达到上限，首页仍无足够的实际绘制像素，已保留各次原图".into(),
                    );
                }
                Ok(false)
            }
            Ok(result) => result,
            Err(async_channel::TryRecvError::Empty) => Ok(false),
            Err(async_channel::TryRecvError::Closed) => Err("最近工程取证未收到回执".into()),
        };
    }
    let pinned = TOGGLES.get(plan.step).copied().unwrap_or(true);
    for command in plan
        .commands
        .read(world.resource::<Messages<WorkspaceCommand>>())
    {
        if plan.phase != 3
            || plan.command_seen
            || !matches!(command,
            WorkspaceCommand::SetRecentPinned { path, pinned: value } if path == &plan.paths[0] && *value == pinned)
        {
            return Err(format!("固定按钮产生了其他操作：{command:?}"));
        }
        plan.command_seen = true;
    }
    let model = world.resource::<WorkspaceModel>();
    if model.project.is_some() {
        return Err("最近工程验收期间意外打开工程".into());
    }
    let (a, b) = if pinned { (0, 1) } else { (1, 0) };
    if model.busy
        || model.recent.len() != 2
        || model.recent[a].path != plan.paths[0]
        || model.recent[a].pinned != pinned
        || model.recent[b].path != plan.paths[1]
        || model.recent[b].pinned
    {
        return Ok(false);
    }
    if plan.step < TOGGLES.len() {
        if plan.command_seen && model.status == "编辑器个人设置已保存" {
            plan.proofs.push(json!({"pinned":pinned,"actual_button_command":true,"status":model.status,"first":model.recent[0].path}));
            plan.step += 1;
            plan.phase = 0;
            plan.command_seen = false;
        }
        return Ok(false);
    }
    if Instant::now() < plan.next_capture {
        return Ok(false);
    }
    if let Some(view) = inspect_rows(world, &plan.paths) {
        plan.ready_frames = plan.ready_frames.saturating_add(1);
        // 等待实际布局连续可用，再捕获窄窗渲染目标。
        if plan.ready_frames >= 3 {
            capture(world, plan, view);
        }
    } else {
        plan.ready_frames = 0;
    }
    Ok(false)
}

fn inspect_rows(world: &mut World, paths: &[PathBuf; 2]) -> Option<Value> {
    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .ok()?;
    if (window.width() - 900.).abs() > 2. {
        return None;
    }
    let physical = [window.physical_width(), window.physical_height()];
    let mut rows = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let (children, at) = world
            .query::<(
                &RecentAction,
                &Children,
                &UiGlobalTransform,
                &InheritedVisibility,
            )>()
            .iter(world)
            .find_map(|(action, children, at, visible)| {
                (action.kind == RecentActionKind::Open && &action.path == path && visible.get())
                    .then_some((children, at))
            })?;
        let texts = children
            .iter()
            .filter_map(|entity| {
                let node = world.get::<ComputedNode>(entity)?;
                (!node.is_empty())
                    .then(|| world.get::<Text>(entity).map(|text| text.0.as_str()))
                    .flatten()
            })
            .collect::<Vec<_>>();
        let short_path = format!(
            "…\\worktree-{}\\project",
            if index == 0 { 'a' } else { 'b' }
        );
        let full_path = crate::project::display_path(path);
        if !texts.contains(&NAME)
            || !texts.contains(&short_path.as_str())
            || !texts.contains(&full_path.as_str())
        {
            return None;
        }
        rows.push(
            json!({"name":NAME,"short_path":short_path,"full_path":full_path,"y":at.translation.y}),
        );
    }
    if rows[0]["y"].as_f64()? >= rows[1]["y"].as_f64()? {
        return None;
    }
    Some(json!({"physical":physical,"rows":rows}))
}

fn capture(world: &mut World, plan: &mut Plan, view: Value) {
    plan.capture_attempts += 1;
    let attempt = plan.capture_attempts;
    let path = plan.directory.clone();
    let preferences = plan.preferences.clone();
    let paths = plan.paths.clone();
    let evidence = json!({"passed":true,"pid":std::process::id(),"reopen":plan.reopen,
        "preferences":preferences,"steps":plan.proofs,"view":view,"screenshot":SCREENSHOT});
    let (sender, receiver) = async_channel::bounded(1);
    plan.result = Some(receiver);
    world
        .spawn(Screenshot::primary_window())
        .observe(move |event: On<ScreenshotCaptured>| {
            let (path, preferences, paths, mut evidence, sender) = (
                path.clone(),
                preferences.clone(),
                paths.clone(),
                evidence.clone(),
                sender.clone(),
            );
            let image = event.image.clone();
            IoTaskPool::get()
                .spawn(async move {
                    let result = (|| {
                        check_disk(&read_json(&preferences)?, &paths, true)?;
                        let image = image.try_into_dynamic().map_err(error)?;
                        if evidence["view"]["physical"] != json!([image.width(), image.height()]) {
                            return Err("GPU回读与实际窄窗尺寸不同".into());
                        }
                        // ECS 布局已就绪时，渲染管线仍可能只完成清屏。采样实际回读像素，冷帧另存后重试。
                        let pixels = image.to_rgba8();
                        let mut colors = HashSet::with_capacity(MIN_UI_COLORS);
                        let has_ui_pixels = pixels.pixels().step_by(11).any(|pixel| {
                            colors.insert(pixel.0);
                            colors.len() >= MIN_UI_COLORS
                        });
                        let screenshot = if has_ui_pixels {
                            path.join(SCREENSHOT)
                        } else {
                            path.join(format!("recent-projects-cold-{attempt:02}.png"))
                        };
                        let mut file = OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(screenshot)
                            .map_err(error)?;
                        image
                            .write_to(
                                &mut file,
                                bevy::image::ImageFormat::Png
                                    .as_image_crate_format()
                                    .unwrap(),
                            )
                            .map_err(error)?;
                        file.sync_all().map_err(error)?;
                        if !has_ui_pixels {
                            return Ok(false);
                        }
                        evidence["gpu_capture"] = json!({
                            "has_ui_pixels":true,"sampled_unique_colors_at_least":colors.len(),
                            "attempts":attempt,"rejected_originals":attempt-1,
                        });
                        write_new(&path.join(REPORT), &evidence)?;
                        Ok(true)
                    })();
                    let _ = sender.try_send(result);
                })
                .detach();
        });
}

fn check_disk(disk: &Value, paths: &[PathBuf; 2], pinned: bool) -> Result<(), String> {
    let (a, b) = if pinned { (0, 1) } else { (1, 0) };
    let rows = disk["recent_projects"]
        .as_array()
        .ok_or("隔离偏好缺少最近工程")?;
    if disk["format"] != 1
        || rows.len() != 2
        || rows[a]["path"] != json!(paths[0])
        || rows[a]["pinned"].as_bool().unwrap_or(false) != pinned
        || rows[b]["path"] != json!(paths[1])
        || rows[b]["pinned"].as_bool().unwrap_or(false)
        || rows[a]["last_opened_unix_seconds"] != 1
        || rows[b]["last_opened_unix_seconds"] != 2
    {
        return Err("真实偏好的目录、固定状态、顺序或打开时间错误".into());
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = crate::user_preferences::read_optional(path)?.ok_or("缺少隔离偏好或验收报告")?;
    serde_json::from_slice(&bytes).map_err(error)
}

fn write_new(path: &Path, value: &Value) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(error)?;
    serde_json::to_writer_pretty(&mut file, value).map_err(error)?;
    file.flush().and_then(|()| file.sync_all()).map_err(error)
}

fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
