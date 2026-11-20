//! 常驻工程真实窗口验收。只给本轮自有夹具添加原生 Bevy 对象，再沿原按钮查看场景树。

use super::{EmbeddedStatus, orchestration};
use crate::{
    scene_format_validation::click_text,
    viewport_mode::{ViewportMode, ViewportModeState},
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use feathrise_editor_scene::standalone::{
    self, embedded_scene_tree::{received_rows, rendered_rows}, embedded_validation::release_pulse,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) const REPORT: &str = "project-scene-tree.json";
const SCREENSHOT: &str = "project-scene-tree.png";

pub(super) fn active() -> bool {
    std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref() == Ok("scene-tree")
}

pub(super) fn prepare(root: &Path) -> Result<(), String> {
    let project = orchestration::owner(root)?;
    let path = project.root.join("src/controller.rs");
    let source = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let startup = ".add_systems(Startup, spawn_observer)";
    if source.matches(startup).count() != 1 {
        return Err("夹具需要原空模板的 Startup 注册".into());
    }
    let mut source = source.replace(
        startup,
        ".add_systems(Startup, (spawn_observer, scene_tree_fixture))",
    );
    source.push_str(r#"

// 由工程自己的 Startup 创建，故意不登记到作者场景。
fn scene_tree_fixture(mut commands: Commands, assets: Res<AssetServer>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let camera = commands.spawn((Name::new("备用相机"), Camera3d::default(), Camera { is_active: false, ..default() })).id();
    let anonymous = commands.spawn((Transform::default(), Visibility::Inherited, ChildOf(camera))).id();
    // 离屏父节点不应占行，正常分组仍应显示在备用相机下面。
    let offscreen = commands.spawn((
        Name::new("离屏父相机"), Camera3d::default(), Camera { is_active: false, ..default() },
        bevy::camera::RenderTarget::None { size: UVec2::ONE }, ChildOf(anonymous),
    )).id();
    let parent = commands.spawn((Name::new("原生分组"), Transform::default(), Visibility::Inherited, ChildOf(offscreen))).id();
    let mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let material = materials.add(Color::srgb(0.2, 0.6, 0.8));
    commands.spawn((
        Name::new("原生立方体"),
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_xyz(2.0, 1.0, 0.0),
        ChildOf(parent),
    ));
    commands.spawn((Name::new("原生灯光"), PointLight::default(), ChildOf(parent)));
    let instance = commands.spawn((
        WorldAssetRoot(assets.load(bevy::gltf::GltfAssetLabel::Scene(0).from_asset("scene/probe.glb"))),
        Transform::from_xyz(-2.0, 1.0, 0.0),
        ChildOf(parent),
    )).id();
    // 模型内部的命名网格和大量无名网格都实际在 World 中，检查完整回包是否舍去这些行。
    commands.spawn((Name::new("实例内部网格"), Mesh3d(mesh.clone()), ChildOf(instance)));
    for offset in 0..16 {
        commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::from_xyz(50.0 + offset as f32, 0.0, 0.0)));
    }
}
"#);
    fs::write(path, source).map_err(|error| error.to_string())?;
    let settings = project.context.ensure_data_directory()?.join("launch.json");
    fs::write(
        settings,
        serde_json::to_vec(&json!({
            "format": 1, "mode": "saved_world", "profile": "debug", "backend": "cargo"
        }))
        .unwrap(),
    )
    .map_err(|error| error.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Open,
    Navigator,
    Project,
    Observe,
    Capture,
    Editor,
    Finish,
}

#[derive(Resource)]
struct Run {
    directory: PathBuf,
    phase: Phase,
    started: Instant,
    frames: u32,
    ready_at: Option<Instant>,
    pulse: Option<Entity>,
    picture: Option<async_channel::Receiver<Result<(), String>>>,
    report: Option<Task<Result<(), String>>>,
    observed: Value,
    error: Option<String>,
}

pub(super) fn install(app: &mut App, directory: PathBuf) -> Result<(), String> {
    if directory.join(REPORT).exists() || directory.join(SCREENSHOT).exists() {
        return Err("工程场景树验收不覆盖上次结果".into());
    }
    app.insert_resource(Run {
        directory,
        phase: Phase::Open,
        started: Instant::now(),
        frames: 0,
        ready_at: None,
        pulse: None,
        picture: None,
        report: None,
        observed: Value::Null,
        error: None,
    })
    .add_systems(Last, drive);
    Ok(())
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        run.frames = run.frames.saturating_add(1);
        if let Some(entity) = run.pulse.take() {
            release_pulse(world, entity);
        }
        if run.phase == Phase::Finish {
            if let Some(result) = run.report.as_mut().and_then(check_ready) {
                let passed = result.is_ok() && run.error.is_none();
                world.write_message(if passed {
                    AppExit::Success
                } else {
                    AppExit::error()
                });
                run.report = None;
            }
            return;
        }
        let result = if run.started.elapsed() > Duration::from_secs(1800) {
            let status = world.resource::<EmbeddedStatus>();
            Err(format!(
                "工程场景树验收等待 {:?} 超时，工程状态 {status:?}",
                run.phase
            ))
        } else {
            advance(world, &mut run)
        };
        match result {
            Ok(false) => return,
            Ok(true) => (),
            Err(error) => {
                eprintln!("PROJECT_SCENE_TREE_VALIDATION_FAILED {error}");
                run.error = Some(error);
            }
        }
        run.phase = Phase::Finish;
        let path = run.directory.join(REPORT);
        let report = json!({
            "passed": run.error.is_none(), "error": run.error, "observed": run.observed,
            "original_mode_and_navigator_buttons": true, "os_input_verified": false,
        });
        run.report = Some(IoTaskPool::get().spawn(async move {
            fs::write(
                path,
                serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())
        }));
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<bool, String> {
    let click = |world: &mut World, chinese, english| {
        click_text(world, chinese).or_else(|| click_text(world, english))
    };
    match run.phase {
        Phase::Open if run.frames >= 30 => {
            standalone::request_open_scene(world, "M1-SAMPLE")?;
            run.phase = Phase::Navigator;
        }
        Phase::Navigator if standalone::preview_ready_for(world, "M1-SAMPLE") => {
            if let Some(entity) = click(world, "场景树", "Scene Tree") {
                run.pulse = Some(entity);
                run.phase = Phase::Project;
            }
        }
        Phase::Project => {
            if let Some(entity) = click(world, "工程 3D", "Project 3D") {
                run.pulse = Some(entity);
                run.phase = Phase::Observe;
            }
        }
        Phase::Observe => {
            let status = world.resource::<EmbeddedStatus>();
            if let Some(error) = &status.error {
                return Err(error.clone());
            }
            let (pid, world_id, revision) = (status.pid, status.world_id, status.applied_revision);
            if !status.ready || revision == 0 {
                return Ok(false);
            }
            let ready_at = *run.ready_at.get_or_insert_with(Instant::now);
            let rows = rendered_rows(world);
            let find = |fragment: &str| {
                rows.iter()
                    .find(|(text, _)| text.contains(fragment))
                    .map(|(_, indent)| *indent)
            };
            let camera = find("主相机").or_else(|| find("Main camera"));
            let (Some(group), Some(cube), Some(light)) =
                (find("原生分组"), find("原生立方体"), find("原生灯光"))
            else {
                let runtime = world.get_non_send::<super::super::resources::Runtime>();
                if ready_at.elapsed() > Duration::from_secs(20) || runtime.is_some_and(|runtime| !runtime.tree.supported) {
                    let tree = runtime.map(|runtime| &runtime.tree);
                    let viewport = world.resource::<crate::viewport_mode::ViewportGeometry>();
                    let navigator = feathrise_editor_scene::standalone::embedded_scene_tree::visible(world);
                    return Err(format!("工程对象未进入左栏：{rows:?}，场景树 {tree:?}，导航 {navigator}，视口可见 {}", viewport.visible));
                }
                return Ok(false);
            };
            if camera.is_none()
                || find("备用相机").is_none()
                || group != percent(4)
                || cube != percent(8)
                || light != percent(8)
                || find("场景实例 · probe.glb").or_else(|| find("Scene instance · probe.glb")) != Some(percent(8))
            {
                return Err(format!("工程对象实际显示的相机或父子缩进不正确：{rows:?}"));
            }
            let received = received_rows(world);
            if received.iter().any(|row| {
                row.category[0] == "离屏相机"
                    || row.name.as_deref().is_some_and(|name| {
                        name.starts_with("DynamicCelestial")
                            || name.starts_with("场景显式实例候选_")
                            || matches!(name, "离屏父相机" | "实例内部网格" | "SunDirectionalLight" | "MoonDirectionalLight")
                    })
                    || (row.category[0] == "渲染对象" || row.category[0] == "节点") && row.name.is_none()
            }) {
                return Err("工程对象回包包含离屏相机、环境派生实体、模型内部网格、无名零件或空候选分组".into());
            }
            run.observed = json!({"pid":pid, "world_id":world_id, "revision":revision,
                "object_count":received.len(), "scene_objects_filtered":true,
                "source_objects":received.iter().map(|row| row.name.as_deref().unwrap_or(row.category[0])).collect::<Vec<_>>(),
                "rows":rows.iter().map(|(text, indent)| json!([text, format!("{indent:?}")])).collect::<Vec<_>>()});
            let path = run.directory.join(SCREENSHOT);
            let (sender, receiver) = async_channel::bounded(1);
            run.picture = Some(receiver);
            world.spawn(Screenshot::primary_window()).observe(
                move |event: On<ScreenshotCaptured>| {
                    let (image, path, sender) = (event.image.clone(), path.clone(), sender.clone());
                    IoTaskPool::get()
                        .spawn(async move {
                            let result = image
                                .try_into_dynamic()
                                .map_err(|error| error.to_string())
                                .and_then(|image| {
                                    image
                                        .to_rgb8()
                                        .save(path)
                                        .map_err(|error| error.to_string())
                                });
                            let _ = sender.try_send(result);
                        })
                        .detach();
                },
            );
            run.phase = Phase::Capture;
        }
        Phase::Capture => match run.picture.as_ref().unwrap().try_recv() {
            Ok(result) => {
                result?;
                run.picture = None;
                run.phase = Phase::Editor;
            }
            Err(async_channel::TryRecvError::Empty) => (),
            Err(async_channel::TryRecvError::Closed) => {
                return Err("工程场景树截图没有 GPU 回读".into());
            }
        },
        Phase::Editor => {
            if world.resource::<ViewportModeState>().selected != ViewportMode::SavedWorld {
                run.pulse = click(world, "编辑器 3D", "Editor 3D");
            } else if !world.resource::<EmbeddedStatus>().active {
                if !rendered_rows(world).is_empty() {
                    return Err("切回编辑器 3D 后仍有工程对象行".into());
                }
                run.observed["cleared_after_return"] = json!(true);
                return Ok(true);
            }
        }
        _ => (),
    }
    Ok(false)
}

pub(super) fn verify(directory: &Path, report: &Value) -> Result<Vec<Value>, String> {
    let screenshot = directory.join(SCREENSHOT);
    let bytes = fs::read(&screenshot).map_err(|error| format!("缺少工程场景树实窗图：{error}"))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || report["observed"]["cleared_after_return"] != true
        || report["observed"]["scene_objects_filtered"] != true
    {
        return Err("工程场景树实窗截图或切回编辑器后的清除证据无效".into());
    }
    Ok(vec![json!({"screenshot":screenshot, "observed":report["observed"]})])
}
