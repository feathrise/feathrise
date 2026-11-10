//! 人形绑骨真窗口验收。只在显式 validation 及本轮自有工程启用，走原按钮、子进程和发布流程。

use crate::{
    asset_catalog::{EditorAssetCatalog, request_asset_reveal},
    resources::MapEditorState,
    ui::{
        asset_preview::{
            OpenAssetPreview,
            AssetPreviewState,
            animation::{rig_animation_diagnostic, rig_animation_probe},
            rigging::{RigAction, RigLandmarkDot, RiggingState},
            types::AssetPreviewCanvas,
        },
        sidebars::MapEditorSidebarState,
    },
};
use bevy::{
    app::AppExit,
    input::{ButtonState, mouse::MouseButtonInput},
    math::DVec2,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
    ui::{InteractionDisabled, UiGlobalTransform},
    window::{CursorMoved, PrimaryWindow, WindowEvent},
};
use feathrise_editor_ui::UiInteraction;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

const SOURCE: &str = "人形.glb";
const OUTPUT: &str = "人形_已绑骨.glb";
const IDLE: &str = "01-待机.glb";
const WALK: &str = "02-行走.glb";
const FIXTURE_ENV: &str = "FEATHRISE_RIG_VALIDATION_DIR";
const OWNER: &[u8] = b"feathrise-rigging-validation-v1";

#[derive(Clone, Copy, Debug)]
enum Phase {
    Open,
    Selected,
    Launch,
    Inspect,
    AimMarker,
    PressMarker,
    DragMarker,
    ReleaseMarker,
    MarkersImage,
    Bind,
    Published,
    PickFile,
    PickWalk,
    Preview,
    Motion,
    MotionImage,
    Verify,
}

#[derive(Resource)]
struct Run {
    root: PathBuf,
    assets: PathBuf,
    reopen: bool,
    started: Instant,
    phase_started: Instant,
    phase: Phase,
    source_hashes: [String; 3],
    previous_output: Option<String>,
    click: Option<(Entity, bool)>,
    drag: Option<(DVec2, [f32; 3])>,
    dragged_marker: bool,
    capture: Option<Mutex<mpsc::Receiver<Result<(), String>>>>,
    sample: Option<(f32, Vec<(String, Quat)>)>,
    motion: Option<Value>,
    verify: Option<Task<Result<Value, String>>>,
    launch_snapshot: bool,
}

pub fn install(app: &mut App, project: &Path, assets: &Path) -> Result<(), String> {
    let Some(root) = std::env::var_os(FIXTURE_ENV).map(PathBuf::from) else {
        return Ok(());
    };
    if !root.is_absolute() {
        return Err("绑骨验收必须使用本轮自有绝对目录".into());
    }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let project = project.canonicalize().map_err(|error| error.to_string())?;
    let assets = assets.canonicalize().map_err(|error| error.to_string())?;
    let isolated_preferences = std::env::var_os("FEATHRISE_PREFERENCES_FILE").map(PathBuf::from);
    let isolated_plugins = std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS")
        .and_then(|path| PathBuf::from(path).canonicalize().ok());
    if project != root.join("project")
        || assets != project.join("assets")
        || fs::read(root.join(".rigging-owner")).ok().as_deref() != Some(OWNER)
        || !isolated_preferences.is_some_and(|path| {
            path.file_name()
                .is_some_and(|name| name == "preferences.json")
                && path
                    .parent()
                    .and_then(|parent| parent.canonicalize().ok())
                    .as_ref()
                    == Some(&root)
        })
        || isolated_plugins.as_ref() != Some(&root.join("plugins"))
        || fs::read(root.join("plugins/.feathrise-plugin-fixture-owner"))
            .ok()
            .as_deref()
            != Some(b"feathrise-single-binary-validation-v1")
    {
        return Err("绑骨验收的工程、作者偏好或全局插件缺少本轮隔离归属".into());
    }
    let reopen = std::env::var_os("FEATHRISE_RIG_VALIDATION_REOPEN").is_some();
    let report = if reopen {
        "reopen.json"
    } else {
        "exercise.json"
    };
    if root.join(report).exists() {
        return Err("绑骨验收报告已存在，请使用新目录".into());
    }
    let previous_output = if reopen {
        let bytes = fs::read(root.join("exercise.json")).map_err(|error| error.to_string())?;
        let exercise: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        let expected = exercise["model_sha256"]
            .as_str()
            .ok_or("缺少上一轮已生成模型摘要")?;
        if exercise["passed"] != true || hash(&assets, OUTPUT)? != expected {
            return Err("重开前落盘模型与上次验收不同".into());
        }
        Some(expected.to_owned())
    } else {
        if fs::symlink_metadata(assets.join(OUTPUT)).is_ok() {
            return Err("绑骨夹具已经存在结果，请使用新工程".into());
        }
        None
    };
    app.insert_resource(Run {
        source_hashes: hashes(&assets)?,
        root,
        assets,
        reopen,
        previous_output,
        started: Instant::now(),
        phase_started: Instant::now(),
        phase: Phase::Open,
        click: None,
        capture: None,
        drag: None,
        dragged_marker: false,
        sample: None,
        motion: None,
        verify: None,
        launch_snapshot: false,
    })
    .add_systems(Update, fixture_focus.before(crate::MapEditorSystemSet::Input))
    .add_systems(Last, drive);
    Ok(())
}

fn fixture_focus(mut window: Query<&mut Window, With<PrimaryWindow>>) {
    // 独占夹具的内部鼠标输入使用原焦点门。验证不抢占操作系统的实际前台。
    if let Ok(mut window) = window.single_mut() && !window.focused {
        window.focused = true;
    }
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        let short_phase = matches!(
            run.phase,
            Phase::Launch | Phase::PressMarker | Phase::DragMarker | Phase::ReleaseMarker | Phase::Motion
        );
        let result = if run.started.elapsed() > Duration::from_secs(250)
            || short_phase && run.phase_started.elapsed() > Duration::from_secs(40)
        {
            Err(format!(
                "等待 {:?} 阶段超过限定时间。{}",
                run.phase, rig_animation_diagnostic(world).unwrap_or_default()
            ))
        } else {
            advance(world, &mut run)
        };
        match result {
            Ok(Some(proof)) => {
                let name = if run.reopen {
                    "reopen.json"
                } else {
                    "exercise.json"
                };
                match report(&run.root.join(name), &proof) {
                    Ok(()) => {
                        world.write_message(AppExit::Success);
                    }
                    Err(error) => {
                        error!("写绑骨验收报告失败：{error}");
                        world.write_message(AppExit::error());
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                let name = if run.reopen {
                    "reopen-failure.json"
                } else {
                    "exercise-failure.json"
                };
                let detail = diagnostics(world, &run);
                let _ = report(&run.root.join(name), &json!({
                    "phase": format!("{:?}", run.phase), "error": error, "detail": detail,
                }));
                error!("绑骨验收 {:?}：{error}", run.phase);
                world.write_message(AppExit::error());
            }
        }
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<Option<Value>, String> {
    let Some(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next()
    else {
        return Ok(None);
    };
    let model = if run.reopen { OUTPUT } else { SOURCE };
    let action = if run.reopen { WALK } else { IDLE };
    if let Some(state) = world.get_resource::<RiggingState>() {
        if let Some(error) = state.validation_status().error {
            return Err(error.to_owned());
        }
    }
    let phase = run.phase;
    let next = match phase {
        Phase::Open => {
            let Some(editor) = world.get_resource::<MapEditorState>() else {
                return Ok(None);
            };
            if !editor.open || editor.pending_map_transition.is_some() {
                return Ok(None);
            }
            // 用正式引用定位事务选择源文件，等待资产目录采用后再走原双击打开命令。
            world.resource_scope(|world, mut catalog: Mut<EditorAssetCatalog>| {
                world.resource_scope(|world, mut editor: Mut<MapEditorState>| {
                    request_asset_reveal(
                        &mut catalog,
                        &mut editor,
                        &mut world.resource_mut::<MapEditorSidebarState>(),
                        model,
                    )
                })
            })?;
            Phase::Selected
        }
        Phase::Selected
            if world.resource::<MapEditorState>().selected_asset.as_deref() == Some(model) =>
        {
            world.write_message(OpenAssetPreview { path: model.into() });
            Phase::Launch
        }
        Phase::Launch => {
            if click(world, run, window, RigAction::Launch) {
                Phase::Inspect
            } else {
                if !run.launch_snapshot && run.phase_started.elapsed() > Duration::from_secs(5) {
                    run.launch_snapshot = true;
                    let path = run.root.join("launch.png");
                    world.spawn(Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(path));
                    eprintln!("绑骨入口等待详情 {}", diagnostics(world, run));
                }
                return Ok(None);
            }
        }
        Phase::Inspect => {
            let status = world.resource::<RiggingState>().validation_status();
            if !status.open || status.busy || status.source != Some(model) {
                return Ok(None);
            }
            if run.reopen {
                Phase::PickFile
            } else if status.landmarks == 17 {
                if status.animation != Some(IDLE) {
                    return Err("待机参考动画未从工程目录选中".into());
                }
                Phase::AimMarker
            } else {
                return Ok(None);
            }
        }
        Phase::AimMarker => {
            let dot = world
                .query::<(&RigLandmarkDot, &UiGlobalTransform, &ComputedNode)>()
                .iter(world)
                .find_map(|(dot, transform, node)| {
                    (dot.0 == 5 && node.size().min_element() > 1.0)
                        .then_some(transform.affine().translation.as_dvec2())
                });
            let Some(dot) = dot else { return Ok(None) };
            let before = world
                .resource::<RiggingState>()
                .validation_landmark(5)
                .ok_or("缺少左手腕关节点")?;
            run.drag = Some((dot, before));
            move_cursor(world, window, dot);
            Phase::PressMarker
        }
        Phase::PressMarker => {
            let hover = world
                .query_filtered::<&UiInteraction, With<AssetPreviewCanvas>>()
                .iter(world)
                .any(|interaction| *interaction == UiInteraction::Hovered);
            if !hover {
                return Ok(None);
            }
            window_event(
                world,
                MouseButtonInput {
                    window,
                    button: MouseButton::Left,
                    state: ButtonState::Pressed,
                },
            );
            Phase::DragMarker
        }
        Phase::DragMarker => {
            if !world.resource::<RiggingState>().owns_mouse() {
                return Ok(None);
            }
            let (dot, _) = run.drag.ok_or("缺少关节点拖动起点")?;
            let scale = f64::from(world.get::<Window>(window).unwrap().scale_factor());
            move_cursor(world, window, dot + DVec2::new(6.0, -4.0) * scale);
            Phase::ReleaseMarker
        }
        Phase::ReleaseMarker => {
            let (_, before) = run.drag.ok_or("缺少关节点拖动起点")?;
            let after = world
                .resource::<RiggingState>()
                .validation_landmark(5)
                .ok_or("缺少左手腕关节点")?;
            if Vec3::from_array(after).distance(Vec3::from_array(before)) < 1e-5 {
                return Ok(None);
            }
            window_event(
                world,
                MouseButtonInput {
                    window,
                    button: MouseButton::Left,
                    state: ButtonState::Released,
                },
            );
            run.dragged_marker = true;
            Phase::MarkersImage
        }
        Phase::MarkersImage if capture(world, run, "landmarks.png")? => Phase::Bind,
        Phase::Bind if click(world, run, window, RigAction::Bind) => Phase::Published,
        Phase::Published
            if world.resource::<RiggingState>().validation_status().output == Some(OUTPUT) =>
        {
            Phase::Preview
        }
        Phase::PickFile if click(world, run, window, RigAction::PickFile) => Phase::PickWalk,
        Phase::PickWalk if click(world, run, window, RigAction::Choose(1)) => Phase::Preview,
        Phase::Preview => {
            let status = world.resource::<RiggingState>().validation_status();
            if status.animation != Some(action) || status.clip != 0 {
                return Ok(None);
            }
            if !click(world, run, window, RigAction::Preview) {
                return Ok(None);
            }
            Phase::Motion
        }
        Phase::Motion => {
            let Some(probe) = rig_animation_probe(world) else {
                return Ok(None);
            };
            if probe.model_path != OUTPUT
                || probe.source.file_name() != Some(std::ffi::OsStr::new(action))
                || probe.clip_index != 0
            {
                return Err("实际发布的模型、动作源或片段身份不匹配".into());
            }
            let Some((time, joints)) = &run.sample else {
                run.sample = Some((probe.time, probe.joints));
                return Ok(None);
            };
            let changed = probe
                .joints
                .iter()
                .zip(joints)
                .filter(|((name, rotation), (before, old))| {
                    name == before && rotation.dot(*old).abs() < 0.99999
                })
                .count();
            if (probe.time - time).abs() < 0.08 || changed == 0 {
                return Ok(None);
            }
            run.motion = Some(json!({
                "animation": action, "clip": probe.clip_name, "time": probe.time,
                "changed_joints": changed, "bound_joints": probe.joints.len(),
                "visible_model_root": format!("{:?}", probe.root),
            }));
            Phase::MotionImage
        }
        Phase::MotionImage => {
            let filename = if run.reopen { "walk.png" } else { "idle.png" };
            if !capture(world, run, filename)? {
                return Ok(None);
            }
            Phase::Verify
        }
        Phase::Verify => {
            if let Some(task) = &mut run.verify {
                return match check_ready(task) {
                    Some(result) => result.map(Some),
                    None => Ok(None),
                };
            }
            let assets = run.assets.clone();
            let source_hashes = run.source_hashes.clone();
            let previous_output = run.previous_output.clone();
            let mut motion = run.motion.take().ok_or("缺少实际动画证据")?;
            motion["landmark_drag_verified"] = json!(run.dragged_marker);
            run.verify = Some(IoTaskPool::get().spawn(async move {
                verify_disk(&assets, &source_hashes, previous_output.as_deref(), motion)
            }));
            return Ok(None);
        }
        _ => return Ok(None),
    };
    eprintln!("人形绑骨验收 {:?} -> {next:?}", run.phase);
    run.phase = next;
    run.phase_started = Instant::now();
    Ok(None)
}

fn diagnostics(world: &mut World, run: &Run) -> Value {
    let preview = world.get_resource::<AssetPreviewState>().map(|preview| json!({
        "path": preview.selected_path, "content": format!("{:?}", preview.content),
        "status": preview.status, "rendering": preview.render_visible,
        "model": preview.model.as_ref().map(|model| json!({
            "generation": model.generation, "scene_ready": model.scene_ready,
            "meshes": model.scene_mesh_count, "target": format!("{:?}", model.target),
        })),
    }));
    let buttons: Vec<_> = world.query::<(Entity, &RigAction, &Node, &ComputedNode, &UiInteraction, Option<&InteractionDisabled>)>()
        .iter(world).filter(|(_, action, _, _, _, _)| **action == RigAction::Launch)
        .map(|(entity, _, node, computed, interaction, disabled)| json!({
            "entity": format!("{entity:?}"), "display": format!("{:?}", node.display),
            "size": computed.size().to_array(), "interaction": format!("{interaction:?}"),
            "disabled": disabled.is_some(),
        })).collect();
    let window = world.query_filtered::<&Window, With<PrimaryWindow>>().iter(world).next()
        .map(|window| json!({"focused": window.focused, "cursor": window.cursor_position().map(|value| value.to_array())}));
    json!({"preview": preview, "buttons": buttons, "click": format!("{:?}", run.click), "window": window})
}

fn click(world: &mut World, run: &mut Run, window: Entity, action: RigAction) -> bool {
    let entity = match run.click {
        Some((entity, _)) => Some(entity),
        None => world
            .query::<(
                Entity,
                &RigAction,
                &ComputedNode,
                Option<&InteractionDisabled>,
            )>()
            .iter(world)
            .find(|(_, marker, node, disabled)| {
                **marker == action && node.size().min_element() > 1.0 && disabled.is_none()
            })
            .map(|(entity, _, _, _)| entity),
    };
    let Some(entity) = entity else { return false };
    let Some(transform) = world.get::<UiGlobalTransform>(entity) else {
        return false;
    };
    let physical = transform.affine().translation.as_dvec2();
    move_cursor(world, window, physical);
    match run.click {
        None => run.click = Some((entity, false)),
        Some((_, false)) if world.get::<UiInteraction>(entity) == Some(&UiInteraction::Hovered) => {
            window_event(
                world,
                MouseButtonInput {
                    window,
                    button: MouseButton::Left,
                    state: ButtonState::Pressed,
                },
            );
            run.click = Some((entity, true));
        }
        Some((_, true)) if world.get::<UiInteraction>(entity) == Some(&UiInteraction::Pressed) => {
            window_event(
                world,
                MouseButtonInput {
                    window,
                    button: MouseButton::Left,
                    state: ButtonState::Released,
                },
            );
            run.click = None;
            return true;
        }
        _ => {}
    }
    false
}

fn move_cursor(world: &mut World, window: Entity, physical: DVec2) {
    let mut target = world.get_mut::<Window>(window).unwrap();
    target.set_physical_cursor_position(Some(physical));
    let position = physical.as_vec2() / target.scale_factor();
    window_event(
        world,
        CursorMoved {
            window,
            position,
            delta: None,
        },
    );
}

fn window_event(world: &mut World, event: impl Message + Clone + Into<WindowEvent>) {
    let window_event: WindowEvent = event.clone().into();
    world.write_message(window_event);
    world.write_message(event);
}

fn capture(world: &mut World, run: &mut Run, filename: &str) -> Result<bool, String> {
    if let Some(receiver) = &mut run.capture {
        return match receiver
            .get_mut()
            .map_err(|error| error.to_string())?
            .try_recv()
        {
            Ok(result) => {
                run.capture = None;
                result.map(|()| true)
            }
            Err(mpsc::TryRecvError::Empty) => Ok(false),
            Err(mpsc::TryRecvError::Disconnected) => Err("实际 GPU 截图没有回执".into()),
        };
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let path = run.root.join(filename);
    run.capture = Some(Mutex::new(receiver));
    world
        .spawn(Screenshot::primary_window())
        .observe(move |event: On<ScreenshotCaptured>| {
            let (image, path, sender) = (event.image.clone(), path.clone(), sender.clone());
            IoTaskPool::get()
                .spawn(async move {
                    let result = image
                        .try_into_dynamic()
                        .map_err(|error| error.to_string())
                        .and_then(|image| image.save(path).map_err(|error| error.to_string()));
                    let _ = sender.send(result);
                })
                .detach();
        });
    Ok(false)
}

fn hash(assets: &Path, name: &str) -> Result<String, String> {
    let path = assets.join(name);
    if path.canonicalize().ok().as_ref() != Some(&path) {
        return Err(format!("绑骨验收资产不是本轮普通文件：{name}"));
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn hashes(assets: &Path) -> Result<[String; 3], String> {
    Ok([
        hash(assets, SOURCE)?,
        hash(assets, IDLE)?,
        hash(assets, WALK)?,
    ])
}

fn verify_disk(
    assets: &Path,
    original: &[String; 3],
    previous: Option<&str>,
    motion: Value,
) -> Result<Value, String> {
    if &hashes(assets)? != original {
        return Err("绑骨流程改动了原模型或原动作文件".into());
    }
    let model_sha256 = hash(assets, OUTPUT)?;
    if previous.is_some_and(|previous| previous != model_sha256) {
        return Err("重开后已生成模型字节被改写".into());
    }
    let glb = gltf::Gltf::open(assets.join(OUTPUT)).map_err(|error| error.to_string())?;
    let joints = glb.skins().next().map_or(0, |skin| skin.joints().count());
    if joints != 65
        || glb.animations().len() != 0
        || !glb
            .nodes()
            .any(|node| node.mesh().is_some() && node.skin().is_some())
    {
        return Err("新 GLB 未保存预期的独立角色和蒙皮".into());
    }
    Ok(json!({
        "passed": true, "os_input_verified": false, "reopened": previous.is_some(),
        "model": OUTPUT, "model_sha256": model_sha256, "source_sha256": original,
        "joint_count": joints, "embedded_clips": 0, "motion": motion,
    }))
}

fn report(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|error| error.to_string())
}
