//! 自有空工程的数值手势专项：真实窗口原字段、后台保存回执和第二进程重开，普通构建不装配。

use super::preferences;
use crate::project::Project;
use bevy::{
    ecs::message::MessageUpdateSystems,
    input::{ButtonState, keyboard::{Key, KeyboardInput}, mouse::MouseButtonInput},
    picking::PickingSystems,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
    ui::CalculatedClip,
    window::{CursorMoved, PrimaryWindow, WindowCloseRequested, WindowEvent},
};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, InteractionButton, NumericInputDisabled,
    NumericInputScrubConfig, NumericInputScrubSettingsKey, NumericInputValue,
};
use serde_json::json;
use std::{fs, io::Write, path::PathBuf, time::{Duration, Instant}};

const ENV: &str = "FEATHRISE_NUMERIC_INPUT_VALIDATION_DIR";
const OWNER: &[u8] = b"feathrise-numeric-input-validation-v1";

#[derive(Resource)]
struct Run {
    root: PathBuf,
    file: PathBuf,
    reopen: bool,
    step: u8,
    started: Instant,
    field: Option<Entity>,
    key: String,
    anchor: Vec2,
    click: Option<(Entity, bool)>,
    capture: Option<async_channel::Receiver<Result<bool, String>>>,
    attempts: u8,
}

pub(super) fn install(app: &mut App, project: &Project) -> Result<(), String> {
    let Some(root) = std::env::var_os(ENV).map(PathBuf::from) else { return Ok(()); };
    if !root.is_absolute() { return Err("数值验收要求自有绝对目录".into()); }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let plugins = root.join("plugins");
    let preferences = std::env::var_os("FEATHRISE_PREFERENCES_FILE").map(PathBuf::from);
    let preferences = preferences.and_then(|path| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)));
    let plugin_root = std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS")
        .and_then(|path| PathBuf::from(path).canonicalize().ok());
    if fs::read(root.join(".numeric-owner")).ok().as_deref() != Some(OWNER)
        || project.root != root.join("project")
        || preferences != Some(root.join("prefs.json"))
        || plugin_root != Some(plugins.clone())
        || fs::read(plugins.join(".feathrise-plugin-fixture-owner")).ok().as_deref()
            != Some(b"feathrise-single-binary-validation-v1")
    {
        return Err("数值验收的工程、偏好和插件必须全部属于本轮夹具".into());
    }
    let reopen = std::env::var_os("FEATHRISE_NUMERIC_INPUT_VALIDATION_REOPEN").is_some();
    let name = if reopen { "reopen" } else { "exercise" };
    if root.join(format!("{name}.json")).exists() || root.join(format!("{name}.png")).exists()
        || reopen && !root.join("exercise.json").is_file()
        || !reopen && project.context.editor_data.join("numeric-input.json").exists()
    {
        return Err("数值验收不能覆盖已有证据或复用旧工程速度".into());
    }
    if !reopen {
        let assets = project.context.asset_roots.first().ok_or("数值验收缺少资产根")?;
        if project.context.asset_roots.len() != 1 || *assets != root.join("project/assets")
            || fs::read_dir(assets).map_err(|error| error.to_string())?.next().is_some()
        {
            return Err("数值验收必须从本轮自有空资产目录开始".into());
        }
        feathrise_project::seed::create_empty(assets)?;
    }
    app.insert_resource(bevy::winit::WinitSettings::continuous())
        .insert_resource(Run {
            root, file: project.context.editor_data.join("numeric-input.json"), reopen,
            step: 0, started: Instant::now(), field: None, key: String::new(), anchor: Vec2::ZERO,
            click: None, capture: None, attempts: 0,
        })
        .add_systems(First, drive.after(MessageUpdateSystems).before(PickingSystems::Input));
    Ok(())
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        if run.step == 255 { return; }
        let previous = run.step;
        if let Err(error) = advance(world, &mut run) {
            let _ = report(world, &run, false, Some(&error));
            error!("数值输入专项 step={}: {error}", run.step);
            run.step = 255;
            world.write_message(AppExit::error());
        } else if run.step != previous {
            run.started = Instant::now();
        }
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<(), String> {
    if run.started.elapsed() > Duration::from_secs(45) {
        return Err("等待原字段、输入、保存或截图回执超时".into());
    }
    let Some(window) = world.query_filtered::<Entity, With<PrimaryWindow>>().iter(world).next() else { return Ok(()); };
    if run.step == 0 {
        let settings = button_named(world, &["设置", "Settings"]);
        if crate::locale_validation::click_entity(world, &mut run.click, window, settings, MouseButton::Left) {
            run.step = 1;
        }
        return Ok(());
    }
    if run.step == 1 {
        let field = world.query::<(Entity, &NumericInputScrubSettingsKey)>()
            .iter(world).find(|(_, key)| key.0.contains("/移动步长/")).map(|(entity, key)| (entity, key.0.clone()));
        let Some((field, key)) = field else { return Ok(()); };
        if world.get::<NumericInputDisabled>(field).is_some() || !expose_field(world, field) {
            return Ok(());
        }
        run.field = Some(field);
        run.key = key;
        let speed = world.get::<NumericInputScrubConfig>(field).ok_or("原字段缺少拖动配置")?.sensitivity;
        if (speed - if run.reopen { 2.0 } else { 1.0 }).abs() > 0.0001 {
            return Err(format!("工程速度未正确恢复：{speed}"));
        }
        if run.reopen {
            ensure(value(world, field)? == 2.74, "重开未恢复原吸附数值")?;
            run.step = 20;
        } else {
            run.anchor = center(world, window, field)?;
            pointer(world, window, run.anchor);
            run.step = 2;
        }
        return Ok(());
    }
    let field = run.field.ok_or("验收丢失原字段身份")?;
    match run.step {
        2 => {
            mouse(world, window, MouseButton::Left, ButtonState::Pressed);
            run.step = 3;
        }
        3 => {
            ensure(world.resource::<ActiveNumericInput>().entity.is_none(), "左键按下过早进入字符模式")?;
            ensure(world.resource::<ActiveNumericInputScrub>().entity == Some(field), "左键未捕获原字段")?;
            mouse(world, window, MouseButton::Left, ButtonState::Released);
            run.step = 4;
        }
        4 => {
            ensure(world.resource::<ActiveNumericInput>().entity == Some(field), "无拖动松开未进入字符模式")?;
            key(world, window, KeyCode::Digit2, Key::Character("2.5".into()), Some("2.5"));
            run.step = 5;
        }
        5 => {
            key(world, window, KeyCode::Enter, Key::Enter, None);
            run.step = 6;
        }
        6 => {
            ensure(value(world, field)? == 2.5 && world.resource::<EditorSettings>().editor_snapping.translation_meters == 2.5,
                "字符输入未经过原业务提交")?;
            run.anchor = center(world, window, field)?;
            pointer(world, window, run.anchor);
            mouse(world, window, MouseButton::Left, ButtonState::Pressed);
            run.step = 7;
        }
        7 => {
            ensure(world.resource::<ActiveNumericInput>().entity.is_none(), "左拖误入字符模式")?;
            let point = offset(world, window, run.anchor, 24.0);
            pointer(world, window, point);
            run.step = 8;
        }
        8 => {
            ensure(value(world, field)? == 2.74, "左拖未按24物理像素实时调整原值")?;
            mouse(world, window, MouseButton::Left, ButtonState::Released);
            run.step = 9;
        }
        9 => {
            ensure(world.resource::<ActiveNumericInput>().entity.is_none(), "左拖松开误入字符模式")?;
            ensure(world.resource::<EditorSettings>().editor_snapping.translation_meters == 2.74, "左拖未提交原吸附设置")?;
            run.anchor = center(world, window, field)?;
            pointer(world, window, run.anchor);
            mouse(world, window, MouseButton::Right, ButtonState::Pressed);
            run.step = 10;
        }
        10 => {
            ensure(world.resource::<ActiveNumericInputScrub>().entity == Some(field), "右键未捕获原字段")?;
            let point = offset(world, window, run.anchor, 48.0);
            pointer(world, window, point);
            run.step = 11;
        }
        11 => {
            ensure((world.get::<NumericInputScrubConfig>(field).unwrap().sensitivity - 2.0).abs() < 0.0001,
                "右拖48物理像素未调整为2倍")?;
            ensure(!world.resource::<EditorSettings>().numeric_scrub_speeds.contains_key(&run.key),
                "右键尚未释放就提交工程速度")?;
            ensure(value(world, field)? == 2.74, "右拖错误改变业务数值")?;
            if capture(world, run, "speed")? {
                mouse(world, window, MouseButton::Right, ButtonState::Released);
                run.step = 12;
            }
        }
        12 => {
            ensure(world.resource::<EditorSettings>().numeric_scrub_speeds.get(&run.key) == Some(&2.0),
                "右键松开未发布工程速度")?;
            // 等真实单飞写者归还完整回执；不能只看ECS中的倍率就宣称已保存。
            if preferences::ready_to_exit(world, false)? {
                run.step = 20;
            }
        }
        20 => {
            let name = if run.reopen { "reopen" } else { "exercise" };
            if capture(world, run, name)? {
                report(world, run, true, None)?;
                run.step = 21;
            }
        }
        21 => {
            let done = button_named(world, &["完成", "Done"]);
            if crate::locale_validation::click_entity(world, &mut run.click, window, done, MouseButton::Left) {
                run.step = 22;
            }
        }
        22 if world.get::<NumericInputDisabled>(field).is_some() => {
            window_input(world, WindowCloseRequested { window });
            run.step = 23;
        }
        22 => {}
        23 => {
            let label = world.resource::<feathrise_editor_ui::localization::EditorLocale>().editor_label("退出编辑器").to_owned();
            let exit = button_named(world, &[&label]);
            if crate::locale_validation::click_entity(world, &mut run.click, window, exit, MouseButton::Left) {
                run.step = 24;
            }
        }
        24 => {} // 原关闭按钮及生产事务负责退出，未退出会按上方期限明确失败。
        _ => return Err("未知数值验收阶段".into()),
    }
    Ok(())
}

fn ensure(condition: bool, error: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(error.into()) }
}

fn value(world: &World, field: Entity) -> Result<f32, String> {
    world.get::<NumericInputValue>(field).and_then(|value| value.0.as_f32()).ok_or("原字段不是浮点数".into())
}

fn button_named(world: &mut World, labels: &[&str]) -> Option<Entity> {
    let texts: Vec<_> = world.query::<(Entity, &Text)>().iter(world)
        .filter(|(_, text)| labels.contains(&text.as_str())).map(|(entity, _)| entity).collect();
    for mut entity in texts {
        for _ in 0..16 {
            if world.get::<InteractionButton>(entity).is_some()
                && world.get::<InheritedVisibility>(entity).is_some_and(|visible| visible.get())
                && world.get::<ComputedNode>(entity).is_some_and(|node| !node.is_empty())
            { return Some(entity); }
            entity = match world.get::<ChildOf>(entity) { Some(parent) => parent.parent(), None => break };
        }
    }
    None
}

fn expose_field(world: &mut World, field: Entity) -> bool {
    let Some(point) = world.get::<UiGlobalTransform>(field).map(|transform| transform.translation) else { return false; };
    if world.get::<InheritedVisibility>(field).is_none_or(|visible| !visible.get())
        || world.get::<ComputedNode>(field).is_none_or(|node| node.is_empty())
    { return false; }
    if world.get::<CalculatedClip>(field).is_none_or(|clip| clip.contains_point(point)) { return true; }
    let mut entity = field;
    for _ in 0..32 {
        let Some(parent) = world.get::<ChildOf>(entity) else { break; };
        entity = parent.parent();
        if world.get::<ScrollPosition>(entity).is_some() {
            let offset = world.get::<UiGlobalTransform>(entity).zip(world.get::<ComputedNode>(entity))
                .map(|(transform, node)| (point.y - transform.translation.y) * node.inverse_scale_factor());
            if let Some(offset) = offset {
                world.get_mut::<ScrollPosition>(entity).unwrap().y += offset;
            }
            break;
        }
    }
    false
}

fn center(world: &World, window: Entity, field: Entity) -> Result<Vec2, String> {
    let point = world.get::<UiGlobalTransform>(field).ok_or("字段未排版")?.translation;
    Ok(point.round() / world.get::<Window>(window).ok_or("原窗口已退役")?.scale_factor())
}

fn offset(world: &World, window: Entity, point: Vec2, physical: f32) -> Vec2 {
    point + Vec2::X * physical / world.get::<Window>(window).unwrap().scale_factor()
}

fn window_input<T: bevy::ecs::message::Message + Clone + Into<WindowEvent>>(world: &mut World, input: T) {
    let event: WindowEvent = input.clone().into();
    world.write_message(event);
    world.write_message(input);
}

fn pointer(world: &mut World, window: Entity, position: Vec2) {
    let mut target = world.get_mut::<Window>(window).unwrap();
    let delta = target.cursor_position().map(|previous| position - previous);
    target.set_cursor_position(Some(position));
    window_input(world, CursorMoved { window, position, delta });
}

fn mouse(world: &mut World, window: Entity, button: MouseButton, state: ButtonState) {
    window_input(world, MouseButtonInput { window, button, state });
}

fn key(world: &mut World, window: Entity, key_code: KeyCode, logical_key: Key, text: Option<&str>) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        window_input(world, KeyboardInput { window, key_code, logical_key: logical_key.clone(), state,
            text: text.filter(|_| state == ButtonState::Pressed).map(Into::into), repeat: false });
    }
}

fn capture(world: &mut World, run: &mut Run, name: &str) -> Result<bool, String> {
    if let Some(receiver) = &run.capture {
        return match receiver.try_recv() {
            Ok(result) => { run.capture = None; if result.as_ref().is_ok_and(|ready| *ready) { run.attempts = 0; } result },
            Err(async_channel::TryRecvError::Empty) => Ok(false),
            Err(async_channel::TryRecvError::Closed) => Err("数值验收GPU回执丢失".into()),
        };
    }
    if run.attempts >= 12 { return Err("数值验收仅读回清屏画面".into()); }
    run.attempts += 1;
    let path = run.root.join(format!("{name}.png"));
    let (sender, receiver) = async_channel::bounded(1);
    run.capture = Some(receiver);
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
        let image = event.image.clone();
        let (path, sender) = (path.clone(), sender.clone());
        IoTaskPool::get().spawn(async move {
            let result = image.try_into_dynamic().map_err(|error| error.to_string()).and_then(|image| {
                let pixels = image.to_rgba8();
                let mut colors = std::collections::HashSet::new();
                let ready = pixels.pixels().step_by(11).any(|pixel| { colors.insert(pixel.0); colors.len() > 64 });
                if ready { image.save(path).map_err(|error| error.to_string())?; }
                Ok(ready)
            });
            let _ = sender.try_send(result);
        }).detach();
    });
    Ok(false)
}

fn report(world: &World, run: &Run, passed: bool, error: Option<&str>) -> Result<(), String> {
    let name = if run.reopen { "reopen" } else { "exercise" };
    let report = json!({"passed":passed,"error":error,"step":run.step,"pid":std::process::id(),
        "input":"WindowEvent and independent Bevy input messages","os_input_verified":false,
        "project_numeric_file":run.file,"field_key":run.key,
        "field_value":run.field.and_then(|field| value(world, field).ok()),
        "field_speed":run.field.and_then(|field| world.get::<NumericInputScrubConfig>(field)).map(|config| config.sensitivity),
        "saved_speed":world.resource::<EditorSettings>().numeric_scrub_speeds.get(&run.key)});
    fs::OpenOptions::new().write(true).create_new(true).open(run.root.join(format!("{name}.json")))
        .and_then(|mut file| file.write_all(serde_json::to_string_pretty(&report).unwrap().as_bytes()))
        .map_err(|error| error.to_string())
}
