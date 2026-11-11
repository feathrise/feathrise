//! 自有工程实窗验收：驱动构建配置、日志查找与原关闭入口，保存实际窗口截图。

use crate::{
    embedded_viewport::EmbeddedStatus, native_play::NativePlayStatus,
    native_project_close::embedded_validation as close, project::Project,
    project_log::ProjectLogState,
};
use bevy::{
    ecs::message::Message,
    input::{ButtonState, keyboard::{Key, KeyboardInput}, mouse::MouseButtonInput},
    input_focus::InputFocus,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
    text::{EditableText, TextLayoutInfo},
    ui::{InteractionDisabled, UiGlobalTransform},
    window::{CursorMoved, PrimaryWindow, WindowEvent},
};
use feathrise_editor_scene::standalone::embedded_validation::release_pulse;
use feathrise_editor_scene::workbench::WorkbenchProjectLogButton;
use feathrise_editor_ui::{
    EditorButtonClick, InteractionButton, UiInteraction, localization::EditorLocale,
};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

const ENVIRONMENT: &str = "FEATHRISE_BUILD_ENVIRONMENT_VALIDATION_DIR";

#[derive(Resource)]
struct Run {
    output: PathBuf,
    project_root: PathBuf,
    started: Instant,
    phase: Phase,
    pressed: Option<Entity>,
    missing_rust: bool,
    rust_dialog_seen: bool,
}

enum Phase {
    OpenMenu,
    CheckEnvironment,
    WaitForLog,
    ReopenLog(Value),
    FindShortcut(Value),
    SearchFocus(Value),
    SearchText { log: Value, field: Entity },
    SearchRefocus { log: Value, field: Entity, step: SearchClick },
    SaveEvidence(async_channel::Receiver<Result<(), String>>),
    Close,
    Finished,
}

enum SearchClick {
    OutsideDown,
    OutsideUp,
    MoveBack,
    FieldDown,
    FieldUp,
    Type,
    Verify,
}

pub(super) fn install(app: &mut App, project: &Project) -> Result<(), String> {
    let Some(directory) = std::env::var_os(ENVIRONMENT).map(PathBuf::from) else {
        return Ok(());
    };
    let output = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let preferences = std::env::var_os("FEATHRISE_PREFERENCES_FILE").map(PathBuf::from);
    let plugins = std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS").map(PathBuf::from);
    // 只认本轮工程的兄弟输出和独占归属；在装入自动操作前核对偏好与插件隔离。
    if !directory.is_absolute()
        || project.root.parent() != Some(output.as_path())
        || preferences != Some(directory.join("editor.json"))
        || plugins != Some(directory.join("isolated-global-plugins"))
        || std::env::var_os("FEATHRISE_DX_PATH")
            != Some(project.root.join("missing-dx.exe").into_os_string())
    {
        return Err("构建环境验收要求本轮工程、隔离偏好和插件目录、指定的缺失 DX 路径".into());
    }
    let mut owner = String::new();
    File::open(output.join(".feathrise-build-environment-owner"))
        .map_err(|error| error.to_string())?
        .take(129)
        .read_to_string(&mut owner)
        .map_err(|error| error.to_string())?;
    if owner != project.id
        || output.join("build-environment.png").exists()
        || project.root.join("missing-dx.exe").exists()
    {
        return Err("构建环境验收归属不匹配，或缺失工具及截图条件已变化".into());
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join(".started"))
        .map_err(|error| format!("构建环境验收要求新的独占输出目录：{error}"))?;
    let missing_rust = output.join(".missing-rust").is_file();
    if missing_rust {
        let empty = output.join("empty-rust");
        if ["USERPROFILE", "CARGO_HOME", "RUSTUP_HOME"].into_iter()
            .any(|key| std::env::var_os(key).map(PathBuf::from) != Some(directory.join("empty-rust")))
            || ["EDITOR_HOST_CARGO", "CARGO", "RUSTC"].into_iter().any(|key| std::env::var_os(key).is_some())
            || empty.canonicalize().ok().as_deref() != Some(empty.as_path())
            || std::fs::read_dir(&empty).map_err(|error| error.to_string())?.next().is_some()
        {
            return Err("Rust 缺失验收要求本轮独占的空 Rust 安装目录和默认工具选择".into());
        }
        crate::native_process::isolate_registry_for_validation();
    }
    app.insert_resource(bevy::winit::WinitSettings::continuous())
        .insert_resource(Run {
            output,
            project_root: project.root.clone(),
            started: Instant::now(),
            phase: Phase::OpenMenu,
            pressed: None,
            missing_rust,
            rust_dialog_seen: false,
        })
        .add_systems(Last, drive);
    Ok(())
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        if let Some(button) = run.pressed.take() {
            release_pulse(world, button);
        }
        if matches!(run.phase, Phase::Finished) {
            return;
        }
        let result = if run.started.elapsed() > Duration::from_secs(90) {
            Err("构建环境实窗验收等待原按钮、日志、截图或关闭超时".into())
        } else if crate::build_process::project_build_active()
            || world.resource::<NativePlayStatus>().active
            || world.resource::<EmbeddedStatus>().active
        {
            Err("环境诊断期间出现工程构建或运行".into())
        } else {
            advance(world, &mut run)
        };
        if let Err(error) = result {
            eprintln!("构建环境实窗验收失败：{error}");
            world.write_message(AppExit::error());
            run.phase = Phase::Finished;
        }
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<(), String> {
    match &mut run.phase {
        Phase::OpenMenu => {
            if world.query_filtered::<&Window, With<PrimaryWindow>>().single(world)
                .is_ok_and(|window| run.missing_rust || window.focused)
                && let Some(button) = press(world, "启动配置") {
                run.pressed = Some(button);
                run.phase = Phase::CheckEnvironment;
            }
        }
        Phase::CheckEnvironment => {
            if let Some(button) = press(world, "检查构建环境") {
                run.pressed = Some(button);
                run.phase = Phase::WaitForLog;
            }
        }
        Phase::WaitForLog => {
            let locale = world
                .get_resource::<EditorLocale>()
                .copied()
                .unwrap_or_default();
            let log = world.resource::<ProjectLogState>().validation_tail();
            let lines = log["lines"].as_array().ok_or("无法读取工程日志回执")?;
            let contains = |value: &str| {
                lines.iter().any(|line| {
                    line["text"]
                        .as_str()
                        .is_some_and(|text| text.contains(value))
                })
            };
            if !contains(locale.text("build_environment.scope")) {
                return Ok(());
            }
            if run.missing_rust {
                if !contains("找不到原生工具EXE：cargo") {
                    return Err("空工具环境仍找到 Cargo，无法验收安装提示".into());
                }
                if !run.rust_dialog_seen {
                    let owner = world.query_filtered::<&bevy::window::RawHandleWrapper, With<PrimaryWindow>>()
                        .single(world).ok().and_then(crate::workspace_window::primary_hwnd);
                    if let Some(owner) = owner {
                        run.rust_dialog_seen = crate::environment_diagnostics::rust_install::dismiss_for_validation(owner, locale)?;
                    }
                    return Ok(());
                }
                // 确认真正收到关闭回执，再回读缺失工具的工程日志。
                if !crate::environment_diagnostics::rust_install::dismissed_for_validation() {
                    return Ok(());
                }
            }
            for marker in [
                locale.text("build_environment.title"),
                "Cargo",
                "rustc",
                "missing-dx.exe",
            ] {
                if !contains(marker) {
                    return Err(format!("诊断报告缺少 {marker}"));
                }
            }
            let project_directory = run.project_root.to_string_lossy();
            if !contains(project_directory.trim_start_matches(r"\\?\")) {
                return Err("报告的检查目录与本轮工程不一致".into());
            }
            // 等待原日志正文真正获得布局，再从窗口回读画面。
            if !world
                .query::<(&Name, &EditableText, &TextLayoutInfo, &InheritedVisibility)>()
                .iter(world)
                .any(|(name, text, layout, visibility)| {
                    name.as_str() == "工程日志正文"
                        && visibility.get()
                        && !layout.glyphs.is_empty()
                        && text.value().to_string().contains("DX")
                })
            {
                return Ok(());
            }
            if run.missing_rust {
                run.phase = Phase::SaveEvidence(capture(world, run, log));
            } else if let Some(button) = toggle_log(world) {
                run.pressed = Some(button);
                run.phase = Phase::ReopenLog(log);
            }
        }
        Phase::ReopenLog(log) => {
            if let Some(button) = toggle_log(world) {
                run.pressed = Some(button);
                run.phase = Phase::FindShortcut(log.clone());
            }
        }
        Phase::FindShortcut(log) => {
            let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world)
                .map_err(|error| error.to_string())?;
            let log_focus = world.resource::<InputFocus>().get()
                .and_then(|entity| world.get::<Name>(entity))
                .is_some_and(|name| name.as_str() == "工程日志正文");
            if !log_focus { return Err("通过底栏重开日志后，正文没有接管焦点".into()); }
            keyboard(world, window, KeyCode::ControlLeft, Key::Control, ButtonState::Pressed, None);
            keyboard(world, window, KeyCode::KeyF, Key::Character("f".into()), ButtonState::Pressed, None);
            run.phase = Phase::SearchFocus(log.clone());
        }
        Phase::SearchFocus(log) => {
            let (window, focused) = world.query_filtered::<(Entity, &Window), With<PrimaryWindow>>()
                .single(world).map(|(window, state)| (window, state.focused))
                .map_err(|error| error.to_string())?;
            let field = world.query_filtered::<(Entity, &Name, &ComputedNode), With<EditableText>>()
                .iter(world).find_map(|(entity, name, node)|
                    (name.as_str() == "工程日志查找框" && !node.is_empty()).then_some(entity));
            let Some(field) = field else { return Ok(()); };
            let focus = world.resource::<InputFocus>().get();
            if focus != Some(field) {
                return Err(format!("Ctrl+F 后没有文本焦点（窗口={focused}，实际={focus:?}，输入框={field:?}）"));
            }
            keyboard(world, window, KeyCode::KeyF, Key::Character("f".into()), ButtonState::Released, None);
            keyboard(world, window, KeyCode::ControlLeft, Key::Control, ButtonState::Released, None);
            // 使用窗口后端的键盘消息。官方焦点分发、文本过滤与搜索索引都由生产系统执行。
            for (key_code, letter) in [(KeyCode::KeyR, "r"), (KeyCode::KeyU, "u"), (KeyCode::KeyS, "s"), (KeyCode::KeyT, "t")] {
                keyboard(world, window, key_code, Key::Character(letter.into()), ButtonState::Pressed, Some(letter));
                keyboard(world, window, key_code, Key::Character(letter.into()), ButtonState::Released, None);
            }
            run.phase = Phase::SearchText { log: log.clone(), field };
        }
        Phase::SearchText { log, field } => {
            let value = world.get::<EditableText>(*field).ok_or("日志查找框已失效")?.value().to_string();
            if value != "rust" {
                let focus = world.resource::<InputFocus>().get();
                return Err(format!("官方键盘输入未进入日志查找框（实际={value:?}，焦点={focus:?}）"));
            }
            let window = primary_window(world)?;
            move_search_cursor(world, window, *field, false)?;
            run.phase = Phase::SearchRefocus { log: log.clone(), field: *field, step: SearchClick::OutsideDown };
        }
        Phase::SearchRefocus { log, field, step } => {
            if let Some(next) = advance_search_click(world, *field, step)? {
                *step = next;
            } else {
                let log = log.clone();
                run.phase = Phase::SaveEvidence(capture(world, run, log));
            }
        }
        Phase::SaveEvidence(receiver) => match receiver.try_recv() {
            Ok(result) => {
                result?;
                close::request(world)?;
                run.phase = Phase::Close;
            }
            Err(async_channel::TryRecvError::Closed) => return Err("未收到实际窗口截图回执".into()),
            Err(async_channel::TryRecvError::Empty) => {}
        },
        Phase::Close => {
            if let Some(button) = close::finish_fixture(world)? {
                run.pressed = Some(button);
            }
        }
        Phase::Finished => {}
    }
    Ok(())
}

fn press(world: &mut World, label: &str) -> Option<Entity> {
    let button = world.query_filtered::<(Entity, &Name, &ComputedNode, &InheritedVisibility), (With<InteractionButton>, Without<InteractionDisabled>)>()
        .iter(world).find_map(|(entity, name, computed, visibility)|
            (name.as_str() == label && visibility.get() && !computed.is_empty()).then_some(entity))?;
    pulse(world, button);
    Some(button)
}

fn toggle_log(world: &mut World) -> Option<Entity> {
    let button = world.query_filtered::<(Entity, &ComputedNode, &InheritedVisibility), With<WorkbenchProjectLogButton>>()
        .iter(world).find_map(|(entity, node, visible)| (!node.is_empty() && visible.get()).then_some(entity))?;
    pulse(world, button);
    Some(button)
}

fn pulse(world: &mut World, button: Entity) {
    world.entity_mut(button).insert((UiInteraction::Pressed, EditorButtonClick::triggered()));
}

fn keyboard(world: &mut World, window: Entity, key_code: KeyCode, logical_key: Key, state: ButtonState, text: Option<&str>) {
    window_event(world, KeyboardInput { key_code, logical_key, state, text: text.map(Into::into), repeat: false, window });
}

fn window_event(world: &mut World, event: impl Message + Clone + Into<WindowEvent>) {
    world.write_message(event.clone().into());
    world.write_message(event);
}

fn primary_window(world: &mut World) -> Result<Entity, String> {
    world.query_filtered::<Entity, With<PrimaryWindow>>().single(world)
        .map_err(|error| error.to_string())
}

fn move_search_cursor(world: &mut World, window: Entity, field: Entity, inside: bool) -> Result<(), String> {
    let center = world.get::<UiGlobalTransform>(field).ok_or("日志查找框没有窗口坐标")?.translation;
    let size = world.get::<ComputedNode>(field).ok_or("日志查找框没有布局")?.size();
    // 点到输入框右半部，现有关键词较短，原生光标应落到词尾；外侧目标位于面板上方。
    let position = if inside { center + Vec2::new(size.x * 0.25, 0.) } else { Vec2::new(center.x, center.y * 0.5) };
    let mut settings = world.get_mut::<Window>(window).ok_or("主窗口已失效")?;
    settings.set_physical_cursor_position(Some(position.as_dvec2()));
    let position = position / settings.scale_factor();
    window_event(world, CursorMoved { window, position, delta: None });
    Ok(())
}

fn advance_search_click(world: &mut World, field: Entity, step: &SearchClick) -> Result<Option<SearchClick>, String> {
    use SearchClick::*;

    let window = primary_window(world)?;
    // 每帧只投递一个鼠标阶段，使生产拾取和焦点请求按真实点击顺序处理。
    let (next, state) = match step {
        OutsideDown => (OutsideUp, ButtonState::Pressed),
        OutsideUp => (MoveBack, ButtonState::Released),
        MoveBack => {
            if world.resource::<InputFocus>().get() == Some(field) {
                return Err("点击面板外后，日志查找框仍占用文本焦点".into());
            }
            move_search_cursor(world, window, field, true)?;
            return Ok(Some(FieldDown));
        }
        FieldDown => (FieldUp, ButtonState::Pressed),
        FieldUp => (Type, ButtonState::Released),
        Type => {
            let focus = world.resource::<InputFocus>().get();
            if focus != Some(field) {
                return Err(format!("鼠标点回日志查找框后未恢复焦点（实际={focus:?}，输入框={field:?}）"));
            }
            keyboard(world, window, KeyCode::KeyC, Key::Character("c".into()), ButtonState::Pressed, Some("c"));
            keyboard(world, window, KeyCode::KeyC, Key::Character("c".into()), ButtonState::Released, None);
            return Ok(Some(Verify));
        }
        Verify => {
            let value = world.get::<EditableText>(field).ok_or("日志查找框已失效")?.value().to_string();
            if value != "rustc" {
                return Err(format!("鼠标点回日志查找框后无法继续输入（实际={value:?}）"));
            }
            return Ok(None);
        }
    };
    window_event(world, MouseButtonInput { window, button: MouseButton::Left, state });
    Ok(Some(next))
}

fn capture(
    world: &mut World,
    run: &Run,
    log: Value,
) -> async_channel::Receiver<Result<(), String>> {
    let (sender, receiver) = async_channel::bounded(1);
    let directory = run.output.clone();
    let project_root = run.project_root.clone();
    let rust_install_dialog_dismissed = run.missing_rust;
    let mut log = Some(log);
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
        let Some(log) = log.take() else {
            return;
        };
        let image = event.image.clone();
        let directory = directory.clone();
        let project_root = project_root.clone();
        let sender = sender.clone();
        IoTaskPool::get().spawn(async move {
            let result = (|| -> Result<(), String> {
                let image = image.try_into_dynamic().map_err(|error| error.to_string())?;
                image.save(directory.join("build-environment.png")).map_err(|error| error.to_string())?;
                let data = json!({"scope":"real-window-build-environment", "project_root":project_root,
                    "pid":std::process::id(), "project_build_active":false, "log":log,
                    "rust_install_dialog_dismissed":rust_install_dialog_dismissed,
                    "log_search":(!rust_install_dialog_dismissed).then(|| json!({
                        "shortcut":"Ctrl+F","typed":"rustc","mouse_refocused":true,
                        "input":"WindowEvent + CursorMoved + MouseButtonInput + KeyboardInput"}))});
                let bytes = serde_json::to_vec_pretty(&data).map_err(|error| error.to_string())?;
                OpenOptions::new().write(true).create_new(true).open(directory.join("build-environment.json"))
                    .and_then(|mut file| file.write_all(&bytes)).map_err(|error| error.to_string())
            })();
            let _ = sender.try_send(result);
        }).detach();
    });
    receiver
}
