//! 无工程首页缩放专项：只向原按钮投递官方输入，核对布局、真实偏好和GPU回执。
//! 调用方先创建独占root、root/.home-scale-fixture-owner（指定固定字节）、隔离editor.json及
//! 带原owner的global目录；FEATHRISE_HOME_SCALE_VALIDATION_DIR指向root/exercise或root/reopen。
//! 两个mode共用同一份自有配置，各自报告目录必须为空；先等exercise正常退出，再启动reopen。
use feathrise_editor_ui::UiInteraction;
mod io;
mod probe;
mod resources;
mod types;
use resources::*;
use types::*;
use super::{WorkspaceCommand, WorkspaceModel};
use async_channel::TryRecvError;
use bevy::{input::{ButtonState, mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel}}, prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured}, window::PrimaryWindow};
use serde_json::{Value, json};
use std::{fs::{self, OpenOptions}, io::Write, path::{Path, PathBuf}, time::{Duration, Instant}};

const OWNER: &[u8] = b"feathrise-home-scale-validation-v1";
const MAX_CAPTURE_ATTEMPTS: u8 = 16;
const CAPTURE_RETRY_INTERVAL: Duration = Duration::from_millis(250);
const CAPTURE_BUDGET: Duration = Duration::from_secs(8);

fn ensure(condition: bool, message: &str) -> Result<(), String> { condition.then_some(()).ok_or_else(|| message.to_owned()) }

/// 首页报告专用的文件身份投影留在本模块，共享 IO 被窗口验收复用时只编译通用读写。
fn stamp(value: &Option<FileStamp>) -> Value {
    value.as_ref().map_or(json!({"exists":false}), |value| json!({"exists":true,"bytes":value.bytes.len(),
        "sha256":value.sha256,"modified_ns":value.modified.duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64).ok()}))
}

pub(super) fn install(app: &mut App) {
    let Some(directory) = std::env::var_os("FEATHRISE_HOME_SCALE_VALIDATION_DIR").map(PathBuf::from) else { return; };
    setup(app, directory).expect("首页缩放专项的独占目录或隔离合同不符");
}

fn setup(app: &mut App, directory: PathBuf) -> Result<(), String> {
    ensure(!std::env::args_os().any(|argument| argument == "--project" || argument == "--native-editor"), "首页专项不能打开工程或接管作者实例")?;
    ensure(crate::workspace_window::EmbedTarget::from_env()?.is_none(), "首页专项不能进入嵌入作者")?;
    for other in ["FEATHRISE_ABOUT_VALIDATION_DIR", "FEATHRISE_CHROME_VALIDATION_DIR", "FEATHRISE_PLUGIN_HOST_VALIDATION_DIR", "FEATHRISE_WORKSPACE_SMOKE_DIR"] {
        ensure(std::env::var_os(other).is_none(), "首页缩放专项不能与其他自动驱动叠加")?;
    }
    ensure(directory.is_absolute() && io::ordinary(&directory)?.is_dir(), "先创建本轮空报告目录")?;
    let directory = directory.canonicalize().map_err(|error| error.to_string())?;
    let preferences = PathBuf::from(std::env::var_os("FEATHRISE_PREFERENCES_FILE").ok_or("必须隔离个人配置")?);
    ensure(preferences.is_absolute() && preferences.file_name().is_some_and(|name| name == "editor.json"), "专项配置必须为独占root/editor.json")?;
    let root = preferences.parent().ok_or("配置缺少父目录")?.canonicalize().map_err(|error| error.to_string())?;
    ensure(io::ordinary(&root)?.is_dir() && directory.parent() == Some(root.as_path()), "报告必须是隔离配置root的直接子目录")?;
    let marker = io::read_now(&root.join(".home-scale-fixture-owner"))?.ok_or("调用方必须先写本轮root owner")?;
    ensure(marker.bytes == OWNER, "本轮root owner不符")?;
    let preferences = root.join("editor.json");
    let initial = io::read_now(&preferences)?.ok_or("先写只含本轮默认目录/空最近列表的隔离editor.json")?;
    let source: Value = serde_json::from_slice(&initial.bytes).map_err(|error| error.to_string())?;
    ensure(source["format"] == 1 && source["recent_projects"].as_array().is_some_and(|values| values.is_empty()), "隔离开始页不能带用户最近工程")?;
    let default_directory = Path::new(source["default_project_directory"].as_str().ok_or("隔离配置缺少本轮默认目录")?);
    ensure(default_directory.is_absolute() && io::ordinary(default_directory)?.is_dir()
        && default_directory.canonicalize().map_err(|error| error.to_string())?.starts_with(&root), "默认工程目录必须位于本轮root内")?;
    let global = PathBuf::from(std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS").ok_or("必须隔离全局插件")?);
    ensure(global.is_absolute() && io::ordinary(&global)?.is_dir(), "全局插件根必须为自有普通目录")?;
    let global = global.canonicalize().map_err(|error| error.to_string())?;
    ensure(global.starts_with(&root) && global != root, "全局插件根必须位于本轮root内")?;
    ensure(io::read_now(&global.join(".feathrise-plugin-fixture-owner"))?.is_some_and(|value| value.bytes == b"feathrise-single-binary-validation-v1"), "全局插件根缺少原夹具owner")?;
    let mode = match std::env::var("FEATHRISE_HOME_SCALE_VALIDATION_MODE").as_deref() {
        Ok("exercise") | Err(std::env::VarError::NotPresent) => Mode::Exercise,
        Ok("reopen") => Mode::Reopen, _ => return Err("mode只能是exercise/reopen".into()),
    };
    ensure(directory.file_name().is_some_and(|name| name == mode.name()), "报告目录名必须与exercise/reopen mode一致")?;
    ensure(fs::read_dir(&directory).map_err(|error| error.to_string())?.next().is_none(), "报告目录必须为空，不能覆盖旧证据")?;
    let author = preferences.with_extension("author.json");
    if mode == Mode::Exercise {
        ensure(!author.try_exists().map_err(|error| error.to_string())?, "exercise只能从没有作者配置的新夹具开始")?;
    } else {
        let previous = io::read_now(&root.join("exercise/evidence.json"))?.ok_or("重开前必须已有本轮exercise报告")?;
        let previous: Value = serde_json::from_slice(&previous.bytes).map_err(|error| error.to_string())?;
        ensure(previous["passed"] == true && previous["mode"] == "exercise" && previous["preferences"] == json!(preferences), "exercise尚未通过或配置身份不同")?;
        ensure(io::read_now(&author)?.is_some(), "重开必须读取原业务已经保存的作者配置")?;
    }
    ensure(app.world().resource::<WorkspaceModel>().project.is_none(), "不能接管用户工程")?;
    let mut owner = OpenOptions::new().write(true).create_new(true).open(directory.join("owner.json")).map_err(|error| error.to_string())?;
    owner.write_all(&serde_json::to_vec_pretty(&json!({"schema":1,"kind":"home-scale-validation","mode":mode.name(),
        "pid":std::process::id(),"root":root,"directory":directory,"preferences":preferences})).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let now = Instant::now();
    app.insert_resource(Plan { directory, preferences, author, mode, started: now, stage_started: now, stage: 0, frames: 0,
        target: None, mouse_phase: 0, click_seen: false, issued: false, layout_frames: 0, last_view: None, view: None,
        read: None, next_read: now, disk_ready: false, last_stamp: None, captured: false, capture: None,
        capture_attempts: 0, capture_started: None, next_capture: now,
        wheel_cooldown: 0, wheel_events: 0, failure_started: None, failure_capture: None, failure_diagnostics: None,
        proofs: Vec::new(), captures: Vec::new(), failure: None, report: None, report_written: false, exit_phase: 0, commands: default() })
        // 在现代拾取读取窗口消息前提供输入，三态及点击仍由生产系统生成。
        .add_systems(First, input.before(bevy::picking::PickingSystems::Input))
        .add_systems(Last, drive);
    Ok(())
}

/// 复现真实窗口后端的双通道投递，保证InputPlugin和现代拾取读取同一份输入。
fn window_input(world: &mut World, input: impl bevy::ecs::message::Message + Clone + Into<bevy::window::WindowEvent>) {
    let event: bevy::window::WindowEvent = input.clone().into();
    world.write_message(input);
    world.write_message(event);
}

fn input(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<Plan>() else { return; };
    if plan.failure.is_none() && plan.report.is_none() && !plan.report_written
        && let Err(error) = issue_input(world, &mut plan) { plan.failure = Some(error); }
    world.insert_resource(plan);
}

fn issue_input(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    let Some(step) = step(plan.mode, plan.stage) else { return Ok(()); };
    let Ok(window) = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world) else { return Ok(()); };
    let settings_open = world.resource::<feathrise_editor_scene::workbench::WorkbenchHostPageState>().settings_open;
    let opening_or_closing = matches!(step.target, Some(Target::Open | Target::Close)) && plan.mouse_phase < 3;
    // 打开/完成只负责自己的按下松开；之后让滚轮保有正文padding内的指针，不能每帧移回旧按钮。
    if step.settings_open && settings_open && plan.mouse_phase != 2 && !opening_or_closing
        && issue_scroll(world, plan, window)? { return Ok(()); }
    let Some(target) = step.target else { return Ok(()); };
    if plan.mouse_phase >= 3 { return Ok(()); }
    if plan.target.is_none() { plan.target = probe::button(world, target); }
    let Some(entity) = plan.target else { return Ok(()); };
    let Some(rect) = probe::bounds(world, entity) else { return Ok(()); };
    world.get_mut::<Window>(window).ok_or("原主窗口消失")?.set_physical_cursor_position(Some(rect.center().as_dvec2()));
    let position = rect.center() / world.get::<Window>(window).unwrap().scale_factor();
    window_input(world, bevy::window::CursorMoved { window, position, delta: None });
    match plan.mouse_phase {
        0 => plan.mouse_phase = 1,
        1 if world.get::<UiInteraction>(entity) == Some(&UiInteraction::Hovered) => {
            window_input(world, MouseButtonInput { button: MouseButton::Left, state: ButtonState::Pressed, window });
            plan.mouse_phase = 2;
        }
        2 => {
            ensure(world.get::<UiInteraction>(entity) == Some(&UiInteraction::Pressed), "官方鼠标没有进入原按钮按下态")?;
            window_input(world, MouseButtonInput { button: MouseButton::Left, state: ButtonState::Released, window });
            plan.mouse_phase = 3;
        }
        _ => {}
    }
    Ok(())
}

fn issue_scroll(world: &mut World, plan: &mut Plan, window: Entity) -> Result<bool, String> {
    let Some((body, pointer, delta)) = probe::scroll_request(world)? else { return Ok(false); };
    world.get_mut::<Window>(window).ok_or("原主窗口消失")?.set_physical_cursor_position(Some(pointer.as_dvec2()));
    let position = pointer / world.get::<Window>(window).unwrap().scale_factor();
    window_input(world, bevy::window::CursorMoved { window, position, delta: None });
    if plan.wheel_cooldown > 0 { plan.wheel_cooldown -= 1; return Ok(true); }
    if !world.get::<bevy::ui::RelativeCursorPosition>(body).is_some_and(|cursor|cursor.cursor_over()) { return Ok(true); }
    ensure(plan.wheel_events < 32, "原正文滚轮32步内未能把缩放组滚入视口")?;
    window_input(world, MouseWheel { unit: MouseScrollUnit::Pixel, x: 0., y: delta, window,
        phase: bevy::input::touch::TouchPhase::Moved });
    plan.wheel_events += 1;
    plan.wheel_cooldown = 2;
    if plan.mouse_phase < 2 { plan.mouse_phase = 0; plan.target = None; }
    plan.proofs.push(json!({"stage":step(plan.mode,plan.stage).map(|step|step.name),"official_wheel":true,
        "pixel_delta_y":delta,"pointer_physical":[pointer.x,pointer.y],"body":format!("{body:?}"),
        "body_cursor_over_observed":true,"pointer_in_original_body_padding":true,"wheel_step":plan.wheel_events}));
    Ok(true)
}

fn poll<T>(receiver: &Reply<T>) -> Result<Option<T>, String> {
    match receiver.try_recv() {
        Ok(result) => result.map(Some), Err(TryRecvError::Empty) => Ok(None), Err(TryRecvError::Closed) => Err("专项后台槽未返回完整回执".into()),
    }
}

fn disk_values(value: &FileStamp) -> Result<Values, String> {
    let source: Value = serde_json::from_slice(&value.bytes).map_err(|error| error.to_string())?;
    ensure(source["format"] == 1, "作者配置格式不符")?;
    let editor = &source["editor"];
    Ok(Values { manual: editor["ui_scale"].as_f64().ok_or("实际作者配置缺少ui_scale")? as f32,
        automatic: editor["ui_scale_auto"].as_bool().ok_or("实际作者配置缺少ui_scale_auto")?,
        offset: editor["ui_scale_auto_offset"].as_f64().ok_or("实际作者配置缺少ui_scale_auto_offset")? as f32 })
}

fn poll_disk(plan: &mut Plan, step: Step) -> Result<(), String> {
    if let Some(receiver) = &plan.read && let Some(observed) = poll(receiver)? {
        plan.read = None;
        if step.no_write { ensure(observed == plan.last_stamp, "重复输入、打开/完成或纯窗口变化额外改写了作者配置（字节或mtime变化）")?; }
        let matching = match &observed {
            Some(value) => step.values.same(disk_values(value)?),
            None => plan.mode == Mode::Exercise && plan.stage <= 1 && step.values.same(Values::manual(1.)),
        };
        if matching {
            plan.disk_ready = true;
            plan.last_stamp = observed;
        } else { plan.disk_ready = false; }
        plan.next_read = Instant::now() + Duration::from_millis(80);
    }
    if plan.read.is_none() && Instant::now() >= plan.next_read && (!plan.disk_ready || step.no_write) {
        plan.read = Some(io::read(plan.author.clone()));
    }
    Ok(())
}

fn capture(world: &mut World, plan: &mut Plan, name: &str) -> Result<(), String> {
    ensure(plan.capture.is_none(), "一次只能有一份缩放截图在途")?;
    let physical = {
        let window = world.query_filtered::<&Window, With<PrimaryWindow>>().single(world).map_err(|error| error.to_string())?;
        [window.physical_width(), window.physical_height()]
    };
    ensure(plan.capture_attempts < MAX_CAPTURE_ATTEMPTS, "真实UI像素重试已达到16次上限")?;
    plan.capture_attempts += 1;
    if plan.capture_started.is_none() { plan.capture_started = Some(Instant::now()); }
    let path = plan.directory.join(format!("{name}-attempt-{:02}.png", plan.capture_attempts));
    plan.proofs.push(json!({"stage":name,"capture_requested":path,"attempt":plan.capture_attempts}));
    let view = plan.view.clone().ok_or("截图前缺少原布局回读")?;
    let (sender, receiver) = async_channel::bounded(1);
    plan.capture = Some(receiver);
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
        io::image(event.image.clone(), path.clone(), physical, view.clone(), sender.clone());
    });
    plan.captured = true;
    Ok(())
}

fn next(plan: &mut Plan) {
    plan.stage += 1;
    plan.frames = 0;
    plan.stage_started = Instant::now();
    plan.target = None; plan.mouse_phase = 0; plan.click_seen = false; plan.issued = false;
    plan.layout_frames = 0; plan.last_view = None; plan.view = None;
    plan.read = None; plan.disk_ready = false; plan.next_read = Instant::now(); plan.captured = false;
    plan.capture_attempts = 0; plan.capture_started = None; plan.next_capture = Instant::now();
    plan.wheel_cooldown = 0; plan.wheel_events = 0;
}

fn advance(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    ensure(world.resource::<WorkspaceModel>().project.is_none(), "首页专项不能创建或打开工程")?;
    ensure(plan.started.elapsed() < Duration::from_secs(180), "首页缩放专项超时")?;
    for command in plan.commands.read(world.resource::<Messages<WorkspaceCommand>>()) {
        ensure(matches!(command, WorkspaceCommand::QuitApp) && plan.exit_phase > 0, "设置操作泄漏了工程/退出命令")?;
    }
    if let Some(receiver) = &plan.capture && let Some(mut proof) = poll(receiver)? {
        let accepted = proof["accepted"].as_bool().ok_or("截图回执缺少真实像素判定")?;
        proof["stage"] = json!(step(plan.mode, plan.stage).map(|step|step.name));
        proof["attempt"] = json!(plan.capture_attempts);
        plan.captures.push(proof);
        plan.capture = None;
        plan.captured = accepted;
        if !accepted {
            // 不改变颜色/尺寸标准；冷帧留存后，只在小而明确的预算内再请求下一张真实GPU帧。
            plan.next_capture = Instant::now() + CAPTURE_RETRY_INTERVAL;
            ensure(plan.capture_attempts < MAX_CAPTURE_ATTEMPTS, "真实UI像素仍未就绪，16次捕获上限已到；所有原图与颜色统计已保留")?;
        }
    }
    ensure(plan.capture_started.is_none_or(|started| started.elapsed() < CAPTURE_BUDGET),
        "等待真实UI像素超过8秒预算；已捕获的冷帧原图和颜色统计均保留，未计为通过")?;
    let Some(step) = step(plan.mode, plan.stage) else { return Ok(()); };
    ensure(plan.stage_started.elapsed() < Duration::from_secs(if plan.stage == 0 { 45 } else { 20 }),
           &format!("阶段{}没有获得原输入、UiScale、布局或真实落盘回执；actual={:?}", step.name, probe::values(world)))?;
    plan.frames += 1;
    if let Some(target) = plan.target && plan.mouse_phase == 3 && !plan.click_seen
        && world.get::<feathrise_editor_ui::EditorButtonClick>(target).is_some_and(|click| click.is_clicked()) {
        plan.click_seen = true;
        plan.proofs.push(json!({"stage":step.name,"target":step.target.map(|target|target.label()),
            "original_button":format!("{target:?}"),"official_mouse_press_release":true,"production_button_click_observed":true}));
    }
    if step.target.is_some() && !plan.click_seen { return Ok(()); }
    if let Some((width, height)) = step.resize && !plan.issued {
        world.query_filtered::<&mut Window, With<PrimaryWindow>>().single_mut(world).map_err(|error| error.to_string())?.resolution.set(width, height);
        plan.issued = true;
        return Ok(());
    }
    if let Some(mut view) = probe::view(world, step)? {
        if step.resize.is_some() {
            // 缩小可能前后都夹到60%，必须用本轮真实旧读数证明窗口变化确实改变了自动倍率。
            let previous = plan.proofs.iter().rev().find(|proof| proof["stage"] == "automatic-minus" && proof["completed"] == true)
                .and_then(|proof| proof["view"]["ui_scale"].as_f64()).ok_or("resize前缺少automatic-minus的实际倍率")?;
            let actual = view["ui_scale"].as_f64().ok_or("resize后缺少实际UiScale")?;
            let expected = view["expected_effective"].as_f64().ok_or("resize后缺少窗口适配公式结果")?;
            ensure((actual - previous).abs() > 0.005 && (actual - expected).abs() < 0.001,
                "窗口变化没有改变实际自动倍率，或实际倍率与窗口适配公式不同")?;
            view["resize_previous_actual_ui_scale"] = json!(previous);
        }
        plan.layout_frames = if plan.last_view.as_ref() == Some(&view) { plan.layout_frames + 1 } else { 1 };
        plan.last_view = Some(view.clone()); plan.view = Some(view);
    } else { plan.layout_frames = 0; plan.last_view = None; return Ok(()); }
    poll_disk(plan, step)?;
    let quiet = !step.no_write || plan.stage_started.elapsed() >= Duration::from_millis(700);
    if plan.frames < 8 || plan.layout_frames < 2 || !plan.disk_ready || !quiet { return Ok(()); }
    if step.capture && !plan.captured {
        if plan.capture.is_none() && Instant::now() >= plan.next_capture { capture(world, plan, step.name)?; }
        return Ok(());
    }
    if plan.capture.is_some() { return Ok(()); }
    // 不留未完成的观察槽进入下一阶段，避免旧读回执被误认为下一次输入已保存。
    if plan.read.is_some() { return Ok(()); }
    plan.proofs.push(json!({"stage":step.name,"completed":true,"expected":step.values,"view":plan.view,
        "author_file":stamp(&plan.last_stamp),"no_extra_write_observed":step.no_write,
        "first_input_not_issued":plan.mode == Mode::Reopen && plan.stage == 0}));
    eprintln!("首页缩放专项 {} / {} 完成", plan.mode.name(), step.name);
    next(plan);
    Ok(())
}

fn publish(plan: &mut Plan) {
    let passed = plan.failure.is_none() && step(plan.mode, plan.stage).is_none();
    let report = json!({"schema":1,"passed":passed,"error":plan.failure,"mode":plan.mode.name(),"pid":std::process::id(),
        "preferences":plan.preferences,"author_preferences":plan.author,"stage":plan.stage,"proofs":plan.proofs,"captures":plan.captures,
        "failure_diagnostics":plan.failure_diagnostics,
        "scope":"real primary window without project; original settings controls and production persistence",
        "physical_input_verified":false,"final_manual_scale_expected":1.25,
        "capture_retry_policy":{"max_attempts_per_stage":MAX_CAPTURE_ATTEMPTS,"minimum_retry_interval_ms":250,
            "time_budget_ms":8000,"unchanged_pixel_standard":"actual unique colors > 64 and actual image dimensions match current window","rejected_originals_preserved":true},
        "normal_exit":"after report acknowledgement request WorkspaceCommand::QuitApp and authorize original owned close confirmation; actual process exit observed externally",
        "not_verified":["OS实体键鼠与跨显示器DPI变化","工程作者/试玩全生命周期","未保存文档的人工关闭选择"]});
    plan.report = Some(io::report(plan.directory.join("evidence.json"), report));
}

/// 失败仍保留真实GPU帧和只读现场；诊断图不参与阶段通过判定，也不修改原错误。
fn failure_evidence(world: &mut World, plan: &mut Plan) -> bool {
    if plan.failure_started.is_none() {
        let diagnostics = probe::diagnostics(world);
        let stage = step(plan.mode,plan.stage);
        plan.failure_started = Some(Instant::now());
        plan.failure_diagnostics = Some(json!({"stage":stage.map(|step|step.name),"expected":stage.map(|step|step.values),
            "mouse_phase":plan.mouse_phase,"target":plan.target.map(|entity|format!("{entity:?}")),"wheel_events":plan.wheel_events,
            "original_ui":diagnostics}));
        let Ok(window) = world.query_filtered::<&Window,With<PrimaryWindow>>().single(world) else {
            plan.failure_diagnostics.as_mut().unwrap()["screenshot_error"] = json!("失败时原主窗口已经不存在");
            return true;
        };
        let physical = [window.physical_width(),window.physical_height()];
        let path = plan.directory.join(format!("failure-stage-{:02}.png",plan.stage));
        plan.failure_diagnostics.as_mut().unwrap()["requested_screenshot"] = json!(path);
        let view = plan.failure_diagnostics.clone().unwrap();
        let (sender,receiver) = async_channel::bounded(1);
        plan.failure_capture = Some(receiver);
        world.spawn(Screenshot::primary_window()).observe(move |event:On<ScreenshotCaptured>| {
            io::image(event.image.clone(),path.clone(),physical,view.clone(),sender.clone());
        });
    }
    if let Some(receiver) = &plan.failure_capture {
        match poll(receiver) {
            Ok(Some(mut image)) => {
                image["diagnostic_only"] = json!(true);
                plan.captures.push(image);
                plan.failure_capture = None;
                return true;
            }
            Err(error) => {
                plan.failure_diagnostics.as_mut().unwrap()["screenshot_error"] = json!(error);
                plan.failure_capture = None;
                return true;
            }
            Ok(None) => {}
        }
    } else { return true; }
    if plan.failure_started.is_some_and(|started|started.elapsed() > Duration::from_secs(5)) {
        plan.failure_diagnostics.as_mut().unwrap()["screenshot_timeout_seconds"] = json!(5);
        return true;
    }
    false
}

fn drive(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<Plan>() else { return; };
    if plan.failure.is_none() && !plan.report_written && plan.report.is_none()
        && let Err(error) = advance(world, &mut plan) { plan.failure = Some(error); }
    if (plan.failure.is_some() || step(plan.mode, plan.stage).is_none()) && plan.report.is_none() && !plan.report_written {
        // 若输入中途失败，仍先归还本轮按键；不伪造原Interaction或业务成功状态。
        if plan.mouse_phase == 2 && let Ok(window) = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world) {
            window_input(world, MouseButtonInput { button: MouseButton::Left, state: ButtonState::Released, window });
            plan.mouse_phase = 3;
        }
        if plan.failure.is_none() || failure_evidence(world, &mut plan) { publish(&mut plan); }
    }
    if let Some(receiver) = &plan.report {
        match poll(receiver) {
            Ok(Some(())) => { plan.report = None; plan.report_written = true; }
            Err(error) => { eprintln!("首页缩放证据写入失败：{error}"); plan.report = None; plan.report_written = true; plan.failure = Some(error); }
            Ok(None) => {}
        }
    }
    if plan.report_written && world.resource::<WorkspaceModel>().project.is_none() {
        if plan.exit_phase == 0 {
            world.write_message(WorkspaceCommand::QuitApp);
            plan.exit_phase = 1;
        } else if world.resource::<crate::native_project_close::NativeProjectCloseState>().active() {
            use bevy::ecs::system::RunSystemOnce;
            if let Err(error) = world.run_system_once(crate::native_project_close::confirm_workspace_validation_close) {
                eprintln!("自有首页原关闭确认失败：{error}");
            }
            plan.exit_phase = 2;
        }
    }
    world.insert_resource(plan);
}
