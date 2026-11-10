//! 自有工程实窗验收：驱动原构建配置按钮，读取工程日志、GPU 截图和原关闭回执。

use crate::{
    embedded_viewport::EmbeddedStatus, native_play::NativePlayStatus,
    native_project_close::embedded_validation as close, project::Project,
    project_log::ProjectLogState,
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
    text::{EditableText, TextLayoutInfo},
    ui::InteractionDisabled,
};
use feathrise_editor_map::standalone::embedded_validation::release_pulse;
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
}

enum Phase {
    OpenMenu,
    CheckEnvironment,
    WaitForLog,
    SaveEvidence(async_channel::Receiver<Result<(), String>>),
    Close,
    Finished,
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
    app.insert_resource(bevy::winit::WinitSettings::continuous())
        .insert_resource(Run {
            output,
            project_root: project.root.clone(),
            started: Instant::now(),
            phase: Phase::OpenMenu,
            pressed: None,
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
    match &run.phase {
        Phase::OpenMenu => {
            if let Some(button) = press(world, "启动配置") {
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
            run.phase = Phase::SaveEvidence(capture(world, run, log));
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
    world
        .entity_mut(button)
        .insert((UiInteraction::Pressed, EditorButtonClick::triggered()));
    Some(button)
}

fn capture(
    world: &mut World,
    run: &Run,
    log: Value,
) -> async_channel::Receiver<Result<(), String>> {
    let (sender, receiver) = async_channel::bounded(1);
    let directory = run.output.clone();
    let project_root = run.project_root.clone();
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
                    "pid":std::process::id(), "project_build_active":false, "log":log});
                let bytes = serde_json::to_vec_pretty(&data).map_err(|error| error.to_string())?;
                OpenOptions::new().write(true).create_new(true).open(directory.join("build-environment.json"))
                    .and_then(|mut file| file.write_all(&bytes)).map_err(|error| error.to_string())
            })();
            let _ = sender.try_send(result);
        }).detach();
    });
    receiver
}
