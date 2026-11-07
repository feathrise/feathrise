//! 自有夹具中的构建方式实窗回归；驱动原菜单，核对实际进程/基线/GPU与未保存作者草稿。
//! 不测冷构建速度，不清缓存；原按钮的内部输入不能代表实体键鼠或系统焦点已验收。

use bevy::{prelude::*, render::view::screenshot::{Screenshot, ScreenshotCaptured}, tasks::IoTaskPool};
use crate::{native_play::ProjectBuildBackend as Backend, viewport_mode::{ViewportMode, ViewportModeState}};
use feathrise_editor_map::standalone::{self, embedded_validation as author};
use serde_json::{Value, json};
use std::{path::{Path, PathBuf}, time::{Duration, Instant}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase { Open, Local, Added, Await, Menu, Return, Finish, Done }

#[derive(Resource)]
struct Plan {
    directory: PathBuf,
    phase: Phase,
    changed: Instant,
    index: usize,
    frames: u64,
    pulse: Option<Entity>,
    author: Option<author::AuthorProbe>,
    steps: Vec<Value>,
    error: Option<String>,
    guest: Option<async_channel::Receiver<Result<Option<Value>, String>>>,
    screenshot: Option<async_channel::Receiver<Result<Value, String>>>,
    completion: Option<async_channel::Receiver<Result<(), String>>>,
    build_command: Option<String>,
}

impl Plan {
    fn advance(&mut self, phase: Phase) { self.phase = phase; self.changed = Instant::now(); }
    fn backend(&self) -> Backend { if self.index == 1 { Backend::Native } else { Backend::Dx } }
}

pub(super) fn active() -> bool {
    matches!(std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref(), Ok("build-backend" | "bundled-dx" | "dx-full"))
}

pub(super) fn full_build() -> bool {
    std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref() == Ok("dx-full")
}

pub(super) fn install(app: &mut App, directory: PathBuf) -> Result<(), String> {
    if directory.join("build-backend.json").exists() { return Err("拒绝覆盖已有构建方式验收".into()); }
    // 调用者已检查本轮owner和provenance，普通作者会话不能进入此分支。
    author::install(app);
    app.insert_resource(Plan { directory, phase: Phase::Open, changed: Instant::now(), index: 0,
        frames: 0, pulse: None, author: None, steps: Vec::new(), error: None,
        guest: None, screenshot: None, completion: None, build_command: None })
        .add_systems(Update, super::input_ownership.before(feathrise_editor_core::MapEditorSystemSet::Input))
        .add_systems(Last, drive);
    Ok(())
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut plan: Mut<Plan>| {
        if let Some(entity) = plan.pulse.take() { author::release_pulse(world, entity); }
        plan.frames += 1;
        if plan.phase == Phase::Done {
            if let Some(completion) = &plan.completion {
                match completion.try_recv() {
                    Ok(result) => {
                        if let Err(error) = result { eprintln!("BUILD_BACKEND_VALIDATION_FAILED {error}"); plan.error = Some(error); }
                        world.write_message(if plan.error.is_none() { AppExit::Success } else { AppExit::error() });
                        plan.completion = None;
                    }
                    Err(async_channel::TryRecvError::Closed) => { world.write_message(AppExit::error()); plan.completion = None; }
                    Err(async_channel::TryRecvError::Empty) => {}
                }
            }
            return;
        }
        let budget = if plan.phase == Phase::Await { 1800 } else { 120 };
        let result = if plan.changed.elapsed() > Duration::from_secs(budget) {
            Err(format!("构建方式验收超时：{:?}，第{}次运行", plan.phase, plan.index + 1))
        } else { advance(world, &mut plan) };
        if let Err(error) = result {
            eprintln!("BUILD_BACKEND_VALIDATION_FAILED {error}");
            plan.error = Some(error);
            plan.advance(Phase::Finish);
        }
    });
}

fn advance(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    match plan.phase {
        Phase::Open if plan.frames > 30 => {
            if Backend::selected(world) != Backend::Dx { return Err("新作者会话没有默认DX".into()); }
            standalone::request_open_map(world, "M1-SAMPLE")?;
            plan.advance(Phase::Local);
        }
        Phase::Local if standalone::preview_ready_for(world, "M1-SAMPLE") => {
            plan.author = Some(author::probe(world)?);
            author::request_cuboid(world, [7., 1., -2.])?;
            plan.advance(Phase::Added);
        }
        Phase::Added => {
            let current = author::probe(world)?;
            if !current.dirty || current.undo_len != plan.author.as_ref().unwrap().undo_len + 1 { return Ok(()); }
            if press(world, plan, "工程 3D")? { plan.author = Some(current); plan.advance(Phase::Await); }
        }
        Phase::Await => {
            let status = world.resource::<super::EmbeddedStatus>();
            if let Some(error) = &status.error { return Err(error.clone()); }
            if full_build() && plan.build_command.is_none() {
                plan.build_command = world.resource::<crate::project_log::ProjectLogState>()
                    .validation_build_command(crate::project_log::LogTaskKind::Project3d).map(str::to_owned);
            }
            if crate::embedded_viewport::connection_state(world) != crate::embedded_viewport::ConnectionState::Connected
                || status.frames < 12 { return Ok(()); }
            let runtime = world.non_send::<super::super::resources::Runtime>();
            let expected = plan.backend();
            if Backend::selected(world) != expected || runtime.attempted_backend != Some(expected)
                || runtime.hotpatch.baseline.is_some() != (expected == Backend::Dx && !full_build()) {
                return Err("实际工程构建方式/热更新基线与菜单选择不符".into());
            }
            if full_build() && !plan.build_command.as_deref().is_some_and(is_full_dx_command) {
                return Err(format!("普通 Debug 工程未通过 DX 完整构建：{:?}", plan.build_command));
            }
            if plan.steps.iter().any(|step|step["pid"] == status.pid) { return Err("更换构建方式没有替换旧工程进程".into()); }
            preserve_author(world, plan)?;
            if !capture_guest(world, plan)? { return Ok(()); }
            let title = format!("Debug · {} ▼", expected.label());
            if press(world, plan, &title)? { plan.advance(Phase::Menu); }
        }
        Phase::Menu if plan.changed.elapsed() > Duration::from_millis(200) => {
            let Some(proof) = screenshot(world, plan)? else { return Ok(()); };
            plan.steps.last_mut().ok_or("菜单截图缺少运行证据")?["menu"] = proof;
            if plan.index + 1 < run_count() {
                let target = if plan.index == 0 { Backend::Native } else { Backend::Dx };
                if !press(world, plan, target.label())? { return Err("原菜单缺少可见的构建方式按钮".into()); }
                plan.index += 1;
                plan.advance(Phase::Await);
            } else {
                if !press(world, plan, "Debug · DX ▼")? { return Err("无法关闭原启动配置菜单".into()); }
                plan.advance(Phase::Return);
            }
        }
        Phase::Return => {
            if world.resource::<ViewportModeState>().selected != ViewportMode::SavedWorld {
                press(world, plan, "编辑器 3D")?;
            } else if !world.resource::<super::EmbeddedStatus>().active && standalone::preview_ready_for(world, "M1-SAMPLE") {
                preserve_author(world, plan)?;
                plan.advance(Phase::Finish);
            }
        }
        Phase::Finish => {
            if !crate::viewport_mode::ready_to_exit(world, false)? { return Ok(()); }
            let report = json!({"schema":1,"passed":plan.error.is_none(),"steps":plan.steps,"error":plan.error,
                "owner_checked":true,"default_backend":"dx","author_draft_preserved":plan.error.is_none(),
                "os_input_verified":false,"input":"original menu EditorButtonClick -> production request -> actual build and Job",
                "not_verified":["cold build timing", "physical mouse and native focus", "Release and vendor feature combinations"]});
            let directory = plan.directory.clone();
            let (sender, receiver) = async_channel::bounded(1);
            plan.completion = Some(receiver);
            IoTaskPool::get().spawn(async move {
                let result = serde_json::to_vec_pretty(&report).map_err(|error|error.to_string())
                    .and_then(|bytes| std::fs::write(directory.join("build-backend.json"), bytes).map_err(|error|error.to_string()));
                let _ = sender.try_send(result);
            }).detach();
            plan.advance(Phase::Done);
        }
        _ => {}
    }
    Ok(())
}

fn preserve_author(world: &World, plan: &Plan) -> Result<(), String> {
    let current = author::probe(world)?;
    let expected = plan.author.as_ref().ok_or("缺少作者基线")?;
    if current.placements != expected.placements || current.undo_len != expected.undo_len
        || current.redo_len != expected.redo_len || current.save_sequence != expected.save_sequence || !current.dirty {
        return Err("切换构建方式改变了作者草稿、保存序号或撤销历史".into());
    }
    Ok(())
}

/// 限定原可见按钮与当前布局；菜单隐藏时保留的旧ComputedNode不算可点入口。
fn press(world: &mut World, plan: &mut Plan, label: &str) -> Result<bool, String> {
    let labels: Vec<_> = world.query::<(Entity, &Text)>().iter(world)
        .filter_map(|(entity, text)|(text.trim().trim_start_matches('✓').trim() == label).then_some(entity)).collect();
    let mut matches = Vec::new();
    for entity in labels {
        let mut current = Some(entity);
        let mut button = None;
        let mut visible = true;
        for _ in 0..128 {
            let Some(entity) = current else { break; };
            visible &= world.get::<Node>(entity).is_none_or(|node|node.display != Display::None)
                && world.get::<InheritedVisibility>(entity).is_none_or(|value|value.get());
            if world.get::<Button>(entity).is_some() { button = Some(entity); }
            current = world.get::<ChildOf>(entity).map(ChildOf::parent);
        }
        if visible && let Some(button) = button
            && world.get::<bevy::ui::InteractionDisabled>(button).is_none()
            && world.get::<ComputedNode>(button).is_some_and(|node|node.size().min_element() > 0.) { matches.push(button); }
    }
    matches.sort(); matches.dedup();
    if matches.len() > 1 { return Err(format!("原按钮不唯一：{label}")); }
    let Some(entity) = matches.first().copied() else { return Ok(false); };
    world.entity_mut(entity).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    plan.pulse = Some(entity);
    Ok(true)
}

fn capture_guest(world: &World, plan: &mut Plan) -> Result<bool, String> {
    if plan.steps.len() > plan.index { return Ok(true); }
    if let Some(receiver) = &plan.guest {
        match receiver.try_recv() {
            Ok(result) => {
                plan.guest = None;
                if let Some(proof) = result? {
                    let status = world.resource::<super::EmbeddedStatus>();
                    if proof["game"]["pid"] != status.pid || proof["game"]["revision"] != status.applied_revision {
                        return Err("GPU证据返回时工程实例已变化".into());
                    }
                    let baseline = world.non_send::<super::super::resources::Runtime>().hotpatch.baseline.is_some();
                    let mut step = json!({"backend":plan.backend().key(),"pid":status.pid,
                        "hotpatch_baseline":baseline,"starts":status.starts,"completed":status.completed,
                        "game":proof,"author":author::probe(world)?});
                    if let Some(command) = &plan.build_command { step["build_command"] = json!(command); }
                    plan.steps.push(step);
                    println!("BUILD_BACKEND_VALIDATION_READY {}", plan.backend().key());
                    return Ok(true);
                }
            }
            Err(async_channel::TryRecvError::Closed) => return Err("工程GPU取证槽已关闭".into()),
            Err(async_channel::TryRecvError::Empty) => {}
        }
        return Ok(false);
    }
    let status = world.resource::<super::EmbeddedStatus>();
    let (pid, revision) = (status.pid, status.applied_revision);
    let geometry = world.resource::<crate::viewport_mode::ViewportGeometry>().clone();
    let author = serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string())?;
    let directory = plan.directory.clone();
    let index = plan.index;
    let (sender, receiver) = async_channel::bounded(1);
    plan.guest = Some(receiver);
    IoTaskPool::get().spawn(async move {
        let result = (|| {
            let Some(mut evidence) = super::code_iteration_capture(&directory.join("game"), pid, revision, 0, &author, &geometry)? else { return Ok(None); };
            let bytes = std::fs::read(directory.join(format!("game/{pid}-{revision}-editing.png"))).map_err(|error|error.to_string())?;
            let Some(pixels) = crate::window_chrome::preview_validation::snapshot_pixel_identity(&bytes, &evidence["game"])? else { return Ok(None); };
            let name = format!("{index}-game.png");
            std::fs::write(directory.join(&name), bytes).map_err(|error|error.to_string())?;
            evidence["snapshot"] = json!(name); evidence["snapshot_pixels"] = pixels;
            Ok(Some(evidence))
        })();
        let _ = sender.try_send(result);
    }).detach();
    Ok(false)
}

fn screenshot(world: &mut World, plan: &mut Plan) -> Result<Option<Value>, String> {
    if let Some(receiver) = &plan.screenshot {
        return match receiver.try_recv() {
            Ok(result) => { plan.screenshot = None; result.map(Some) }
            Err(async_channel::TryRecvError::Closed) => Err("菜单截图槽已关闭".into()),
            Err(async_channel::TryRecvError::Empty) => Ok(None),
        };
    }
    let name = format!("{}-menu.png", plan.index);
    let path = plan.directory.join(&name);
    let (sender, receiver) = async_channel::bounded(1);
    plan.screenshot = Some(receiver);
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
        let image = event.image.clone(); let path = path.clone(); let name = name.clone(); let sender = sender.clone();
        IoTaskPool::get().spawn(async move {
            let result = image.try_into_dynamic().map_err(|error|error.to_string())
                .and_then(|image|image.to_rgb8().save(&path).map_err(|error|error.to_string()))
                .map(|_|json!({"source":"Bevy ScreenshotCaptured","path":name}));
            let _ = sender.try_send(result);
        }).detach();
    });
    Ok(None)
}

pub(super) fn verify(output: &Path, report: &Value) -> Result<Vec<Value>, String> {
    let steps = report["steps"].as_array().filter(|steps|steps.len() == run_count()).ok_or("缺少实际构建方式回执")?;
    let mut evidence = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        if step["backend"] != if index == 1 { "native" } else { "dx" }
            || step["hotpatch_baseline"] != (index != 1 && !full_build()) || step["starts"] != index + 1 || step["completed"] != index {
            return Err("DX/原生顺序、fat基线或旧Job回收次数错误".into());
        }
        if full_build() && !step["build_command"].as_str().is_some_and(is_full_dx_command) {
            return Err("普通 Debug 夹具缺少实际 DX 完整构建命令".into());
        }
        let png = std::fs::read(output.join(format!("{index}-game.png"))).map_err(|error|error.to_string())?;
        let pixels = crate::window_chrome::preview_validation::snapshot_pixel_identity(&png, &step["game"]["game"])?
            .ok_or("工程截图与实际GPU回执不符")?;
        if pixels != step["game"]["snapshot_pixels"] { return Err("工程截图已被覆盖".into()); }
        let menu = std::fs::read(output.join(format!("{index}-menu.png"))).map_err(|error|error.to_string())?;
        if !menu.starts_with(b"\x89PNG\r\n\x1a\n") { return Err("缺少实际菜单PNG".into()); }
        evidence.push(json!({"backend":step["backend"],"pid":step["pid"],"pixels":pixels}));
    }
    Ok(evidence)
}

fn is_full_dx_command(command: &str) -> bool {
    command.to_ascii_lowercase().contains("dx.exe") && command.contains(" build --windows ")
        && command.contains("--profile render-dev") && !command.contains("--fat-binary")
}

/// 内嵌工具和无热更新专项各验证一次真实 DX 启动；原构建方式切换仍验证三次。
fn run_count() -> usize {
    if matches!(std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref(), Ok("bundled-dx" | "dx-full")) { 1 } else { 3 }
}
