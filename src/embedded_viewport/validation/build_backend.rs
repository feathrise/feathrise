//! 自有夹具中的构建方式实窗回归；驱动原菜单，核对实际进程/基线/GPU与未保存作者草稿。
//! 不测冷构建速度，不清缓存；原按钮的内部输入不能代表实体键鼠或系统焦点已验收。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use bevy::{prelude::*, render::view::screenshot::{Screenshot, ScreenshotCaptured}, tasks::IoTaskPool};
use crate::{native_play::ProjectBuildBackend as Backend, viewport_mode::{ViewportMode, ViewportModeState}};
use feathrise_editor_map::standalone::{self, embedded_validation as author};
use feathrise_editor_core::{ProjectBuildProfile, ProjectLaunchSettings};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::{Path, PathBuf}, time::{Duration, Instant}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase { Open, Local, Layout, Added, Await, Menu, Return, Finish, Done }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsStage { Select, Reopen }

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
    settings_stage: Option<SettingsStage>,
    initial_backend: Backend,
    initial_profile: ProjectBuildProfile,
    selection_clicks: usize,
    window_resized: bool,
    layout: Option<Value>,
}

impl Plan {
    fn advance(&mut self, phase: Phase) { self.phase = phase; self.changed = Instant::now(); }
    fn backend(&self) -> Backend { if native_only() || self.index == 1 { Backend::Native } else { Backend::Dx } }
}

pub(super) fn active() -> bool {
    matches!(std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref(), Ok("build-backend" | "bundled-dx" | "dx-full" | "native-cargo" | "cargo-settings"))
}

pub(super) fn cargo_settings() -> bool {
    std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref() == Ok("cargo-settings")
}

pub(super) fn full_build() -> bool {
    std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref() == Ok("dx-full")
}

fn native_only() -> bool {
    matches!(std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref(), Ok("native-cargo" | "cargo-settings"))
}

pub(super) fn install(app: &mut App, directory: PathBuf) -> Result<(), String> {
    if directory.join("build-backend.json").exists() { return Err("拒绝覆盖已有构建方式验收".into()); }
    // 阶段只由本轮协调器传给独立子进程；重开轮绝不通过再次点选来掩盖恢复失败。
    let settings_stage = if cargo_settings() {
        Some(match std::env::var("FEATHRISE_CARGO_SETTINGS_STAGE").as_deref() {
            Ok("select") => SettingsStage::Select,
            Ok("reopen") => SettingsStage::Reopen,
            _ => return Err("工程配置专项缺少协调器的选择/重开阶段".into()),
        })
    } else { None };
    let initial_backend = Backend::selected(app.world());
    let initial_profile = app.world().resource::<ProjectLaunchSettings>().profile;
    // 调用者已检查本轮owner和provenance，普通作者会话不能进入此分支。
    author::install(app);
    app.insert_resource(Plan { directory, phase: Phase::Open, changed: Instant::now(), index: 0,
        frames: 0, pulse: None, author: None, steps: Vec::new(), error: None,
        guest: None, screenshot: None, completion: None, build_command: None,
        settings_stage, initial_backend, initial_profile, selection_clicks: 0,
        window_resized: false, layout: None })
        .add_systems(Update, super::input_ownership.before(feathrise_editor_core::MapEditorSystemSet::Input))
        .add_systems(Last, drive);
    Ok(())
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut plan: Mut<Plan>| {
        if let Some(entity) = plan.pulse.take() { author::release_pulse(world, entity); }
        plan.frames += 1;
        if plan.settings_stage.is_some() && !plan.window_resized && plan.frames <= 30 {
            // 修改真实主窗口尺寸后等待正常布局；不直接改 ComputedNode 或假造窄视口坐标。
            if let Ok(mut window) = world.query_filtered::<&mut Window, With<bevy::window::PrimaryWindow>>().single_mut(world) {
                window.resolution.set(1100., 800.);
                plan.window_resized = true;
            }
        }
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
            let expected = if plan.settings_stage == Some(SettingsStage::Reopen) { Backend::Native } else { Backend::Dx };
            if plan.initial_backend != expected || Backend::selected(world) != expected {
                return Err("新作者会话没有恢复预期的初始构建方式".into());
            }
            if plan.initial_profile != ProjectBuildProfile::Debug || world.resource::<ProjectLaunchSettings>().profile != ProjectBuildProfile::Debug {
                return Err("新作者会话没有恢复 Debug 配置".into());
            }
            if plan.settings_stage.is_some() && world.resource::<super::EmbeddedStatus>().starts != 0 {
                return Err("工程配置专项在原按钮输入前意外启动了工程".into());
            }
            standalone::request_open_map(world, "M1-SAMPLE")?;
            plan.advance(Phase::Local);
        }
        Phase::Local if standalone::preview_ready_for(world, "M1-SAMPLE") => {
            plan.author = Some(author::probe(world)?);
            if plan.settings_stage.is_some() { plan.advance(Phase::Layout); }
            else { author::request_cuboid(world, [7., 1., -2.])?; plan.advance(Phase::Added); }
        }
        Phase::Layout => {
            let layout = narrow_layout(world)?;
            let Some(capture) = screenshot(world, plan, "narrow-layout.png")? else { return Ok(()); };
            plan.layout = Some(json!({"geometry":layout,"screenshot":capture}));
            author::request_cuboid(world, [7., 1., -2.])?;
            plan.advance(Phase::Added);
        }
        Phase::Added => {
            let current = author::probe(world)?;
            if !current.dirty || current.undo_len != plan.author.as_ref().unwrap().undo_len + 1 { return Ok(()); }
            // 原生专项先通过原菜单选择构建方式，再由原工程 3D 按钮启动。
            if native_only() && Backend::selected(world) != Backend::Native {
                if plan.settings_stage == Some(SettingsStage::Reopen) { return Err("重开作者丢失已保存的 Cargo，拒绝重新点选掩盖问题".into()); }
                if press(world, plan, Backend::Native.label())? { plan.selection_clicks += 1; }
                else { press(world, plan, "Debug · DX ▼")?; }
                return Ok(());
            }
            if press(world, plan, "工程 3D")? { plan.author = Some(current); plan.advance(Phase::Await); }
        }
        Phase::Await => {
            let expected = plan.backend();
            let status = world.resource::<super::EmbeddedStatus>();
            if let Some(error) = &status.error { return Err(error.clone()); }
            if plan.build_command.is_none() {
                // 旧 Job 尚未收口时日志仍可能属于上轮 DX，不能把它冻结成原生构建证据。
                plan.build_command = world.resource::<crate::project_log::ProjectLogState>()
                    .validation_build_command(crate::project_log::LogTaskKind::Project3d)
                    .filter(|command| expected != Backend::Native || is_native_debug_command(command)).map(str::to_owned);
            }
            if crate::embedded_viewport::connection_state(world) != crate::embedded_viewport::ConnectionState::Connected
                || status.frames < 12 { return Ok(()); }
            let runtime = world.non_send::<super::super::resources::Runtime>();
            if Backend::selected(world) != expected || runtime.attempted_backend != Some(expected)
                || runtime.hotpatch.baseline.is_some() != (expected == Backend::Dx && !full_build()) {
                return Err("实际工程构建方式/热更新基线与菜单选择不符".into());
            }
            if full_build() && !plan.build_command.as_deref().is_some_and(is_full_dx_command) {
                return Err(format!("普通 Debug 工程未通过 DX 完整构建：{:?}", plan.build_command));
            }
            if expected == Backend::Native && !plan.build_command.as_deref().is_some_and(is_native_debug_command) {
                return Err(format!("原生 Debug 未使用普通 Cargo 开发缓存：{:?}", plan.build_command));
            }
            if plan.steps.iter().any(|step|step["pid"] == status.pid) { return Err("更换构建方式没有替换旧工程进程".into()); }
            preserve_author(world, plan)?;
            if !capture_guest(world, plan)? { return Ok(()); }
            let title = format!("Debug · {} ▼", expected.label());
            if press(world, plan, &title)? { plan.advance(Phase::Menu); }
        }
        Phase::Menu if plan.changed.elapsed() > Duration::from_millis(200) => {
            let Some(proof) = screenshot(world, plan, &format!("{}-menu.png", plan.index))? else { return Ok(()); };
            plan.steps.last_mut().ok_or("菜单截图缺少运行证据")?["menu"] = proof;
            if plan.index + 1 < run_count() {
                let target = if plan.index == 0 { Backend::Native } else { Backend::Dx };
                if !press(world, plan, target.label())? { return Err("原菜单缺少可见的构建方式按钮".into()); }
                plan.index += 1;
                plan.build_command = None;
                plan.advance(Phase::Await);
            } else {
                let title = format!("Debug · {} ▼", plan.backend().label());
                if !press(world, plan, &title)? { return Err("无法关闭原启动配置菜单".into()); }
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
            let status = world.resource::<super::EmbeddedStatus>();
            let runtime = world.non_send::<super::super::resources::Runtime>();
            let jobs_drained = !status.active && status.completed == run_count() as u64
                && runtime.active.is_none() && runtime.cleanup_failure.is_none();
            let report = json!({"schema":1,"passed":plan.error.is_none(),"steps":plan.steps,"error":plan.error,
                "owner_checked":true,"host_pid":std::process::id(),"default_backend":plan.initial_backend,
                "default_profile":plan.initial_profile,"backend_selection_clicks":plan.selection_clicks,
                "settings_stage":plan.settings_stage.map(|stage|match stage { SettingsStage::Select => "select", SettingsStage::Reopen => "reopen" }),
                "narrow_layout":plan.layout,"game_jobs_drained":jobs_drained,
                "author_draft_preserved":plan.error.is_none(),
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

/// 仅在本轮固定夹具的一次截图阶段读取原节点；Name用于验收定位，不进入产品布局逻辑。
fn narrow_layout(world: &mut World) -> Result<Value, String> {
    let roots: Vec<_> = world.query::<(Entity, &Name, &ChildOf, &Node, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>()
        .iter(world).filter(|(_, name, ..)| matches!(name.as_str(), "地图编辑器XYZ方向轴" | "视口着色"))
        .map(|(entity, name, parent, node, computed, transform, visible)|
            (entity, name.as_str().to_owned(), parent.parent(), node.clone(), computed.size(), transform.translation, visible.get()))
        .collect();
    let find = |name: &str| {
        let matches: Vec<_> = roots.iter().filter(|root|root.1 == name).collect();
        if matches.len() != 1 { return Err(format!("窄窗缺少唯一原节点：{name}")); }
        Ok(matches[0])
    };
    let axis = find("地图编辑器XYZ方向轴")?;
    let shading = find("视口着色")?;
    if axis.2 != shading.2 { return Err("XYZ 与着色栏不属于同一真实中央视口".into()); }
    let viewport = axis.2;
    let node = world.get::<ComputedNode>(viewport).ok_or("中央视口尚未完成真实布局")?;
    let center = world.get::<UiGlobalTransform>(viewport).ok_or("中央视口缺少真实坐标")?.translation;
    let rect = Rect::from_center_size(center, node.size());
    let inverse_scale = node.inverse_scale_factor();
    if !inverse_scale.is_finite() || inverse_scale <= 0. || !rect.min.is_finite() || !rect.max.is_finite()
        || rect.size().min_element() <= 0. || rect.width() * inverse_scale >= 610. {
        return Err("工程配置专项未形成中央逻辑宽度小于 610 的真实窄视口".into());
    }
    let projection = world.resource::<standalone::viewport_projection::ViewportHostProjection>();
    if projection.layout != Some((node.size(), center)) { return Err("测得的XYZ父节点不是生产系统当前投影的视口".into()); }
    let mut controls = Vec::new();
    for (root, top) in [(shading, 8.), (axis, 42.)] {
        let bounds = Rect::from_center_size(root.5, root.4);
        let top_inset = (bounds.min.y - rect.min.y) * inverse_scale;
        let right_inset = (rect.max.x - bounds.max.x) * inverse_scale;
        if !root.6 || root.3.display == Display::None || root.3.position_type != PositionType::Absolute
            || root.3.top != px(top) || root.3.right != px(8) || root.3.bottom != Val::Auto
            || !bounds.min.is_finite() || !bounds.max.is_finite() || bounds.size().min_element() <= 0.
            || bounds.min.x < rect.min.x || bounds.max.y > rect.max.y
            || (top_inset - top).abs() > 2. || (right_inset - 8.).abs() > 2. {
            return Err(format!("窄窗原控件未保持右上角：{}，top={top_inset} right={right_inset}", root.1));
        }
        controls.push(json!({"name":root.1,"entity":format!("{:?}",root.0),
            "physical_rect":[bounds.min.x,bounds.min.y,bounds.max.x,bounds.max.y],
            "logical_top_inset":top_inset,"logical_right_inset":right_inset}));
    }
    let window = world.query_filtered::<&Window, With<bevy::window::PrimaryWindow>>().single(world)
        .map_err(|_|"工程配置专项缺少唯一真实主窗口")?;
    if (window.width() - 1100.).abs() > 1. || (window.height() - 800.).abs() > 1. {
        return Err("真实作者主窗口没有采用 1100×800 的验收尺寸".into());
    }
    Ok(json!({"source":"production ComputedNode + UiGlobalTransform after layout",
        "window_logical_size":[window.width(),window.height()],"window_physical_size":[window.physical_width(),window.physical_height()],
        "viewport_entity":format!("{viewport:?}"),"viewport_physical_rect":[rect.min.x,rect.min.y,rect.max.x,rect.max.y],
        "viewport_logical_width":rect.width()*inverse_scale,"inverse_scale_factor":inverse_scale,"controls":controls}))
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
            if world.get::<InteractionButton>(entity).is_some() { button = Some(entity); }
            current = world.get::<ChildOf>(entity).map(ChildOf::parent);
        }
        if visible && let Some(button) = button
            && world.get::<bevy::ui::InteractionDisabled>(button).is_none()
            && world.get::<ComputedNode>(button).is_some_and(|node|node.size().min_element() > 0.) { matches.push(button); }
    }
    matches.sort(); matches.dedup();
    if matches.len() > 1 { return Err(format!("原按钮不唯一：{label}")); }
    let Some(entity) = matches.first().copied() else { return Ok(false); };
    world.entity_mut(entity).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
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

fn screenshot(world: &mut World, plan: &mut Plan, name: &str) -> Result<Option<Value>, String> {
    if let Some(receiver) = &plan.screenshot {
        return match receiver.try_recv() {
            Ok(result) => { plan.screenshot = None; result.map(Some) }
            Err(async_channel::TryRecvError::Closed) => Err("菜单截图槽已关闭".into()),
            Err(async_channel::TryRecvError::Empty) => Ok(None),
        };
    }
    let name = name.to_owned();
    let path = plan.directory.join(&name);
    let (sender, receiver) = async_channel::bounded(1);
    plan.screenshot = Some(receiver);
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
        let image = event.image.clone(); let path = path.clone(); let name = name.clone(); let sender = sender.clone();
        IoTaskPool::get().spawn(async move {
            let result = (|| {
                let image = image.try_into_dynamic().map_err(|error|error.to_string())?;
                let (width, height) = (image.width(), image.height());
                image.to_rgb8().save(&path).map_err(|error|error.to_string())?;
                let bytes = std::fs::read(&path).map_err(|error|error.to_string())?;
                let hash: String = Sha256::digest(bytes).iter().map(|byte|format!("{byte:02x}")).collect();
                Ok(json!({"source":"Bevy ScreenshotCaptured","path":name,"width":width,"height":height,"png_sha256":hash}))
            })();
            let _ = sender.try_send(result);
        }).detach();
    });
    Ok(None)
}

pub(super) fn verify(output: &Path, report: &Value) -> Result<Vec<Value>, String> {
    if cargo_settings() {
        let reopened = match report["settings_stage"].as_str() {
            Some("select") => false,
            Some("reopen") => true,
            _ => return Err("工程配置专项缺少真实作者阶段".into()),
        };
        if report["default_backend"] != (if reopened { "cargo" } else { "dx" })
            || report["default_profile"] != "debug" || report["backend_selection_clicks"] != (if reopened { 0 } else { 1 })
            || report["game_jobs_drained"] != true || report["author_draft_preserved"] != true
            || report["host_pid"].as_u64().unwrap_or(0) == 0 {
            return Err("工程配置选择、默认恢复、作者草稿或真实Job收口证据不符".into());
        }
        let layout = &report["narrow_layout"];
        if layout["geometry"]["viewport_logical_width"].as_f64().is_none_or(|width| !(0.0..610.0).contains(&width))
            || layout["geometry"]["controls"].as_array().is_none_or(|controls|controls.len() != 2)
            || layout["screenshot"]["source"] != "Bevy ScreenshotCaptured" || layout["screenshot"]["path"] != "narrow-layout.png" {
            return Err("工程配置专项缺少真实窄窗和XYZ右上布局证据".into());
        }
        let png = std::fs::read(output.join("narrow-layout.png")).map_err(|error|error.to_string())?;
        let hash: String = Sha256::digest(&png).iter().map(|byte|format!("{byte:02x}")).collect();
        if !png.starts_with(b"\x89PNG\r\n\x1a\n") || layout["screenshot"]["png_sha256"] != hash
            || layout["screenshot"]["width"] != layout["geometry"]["window_physical_size"][0]
            || layout["screenshot"]["height"] != layout["geometry"]["window_physical_size"][1] {
            return Err("真实窄窗截图缺失或已被覆盖".into());
        }
    }
    let steps = report["steps"].as_array().filter(|steps|steps.len() == run_count()).ok_or("缺少实际构建方式回执")?;
    let mut evidence = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let expected = if native_only() || index == 1 { "native" } else { "dx" };
        if step["backend"] != expected
            || step["hotpatch_baseline"] != (expected == "dx" && !full_build()) || step["starts"] != index + 1 || step["completed"] != index {
            return Err("DX/原生顺序、fat基线或旧Job回收次数错误".into());
        }
        if full_build() && !step["build_command"].as_str().is_some_and(is_full_dx_command) {
            return Err("普通 Debug 夹具缺少实际 DX 完整构建命令".into());
        }
        if step["backend"] == "native" && !step["build_command"].as_str().is_some_and(is_native_debug_command) {
            return Err("原生 Debug 夹具缺少使用 dev 缓存的实际 Cargo 命令".into());
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

fn is_native_debug_command(command: &str) -> bool {
    command.contains(" build ") && command.contains("--profile dev")
        && !command.contains("--profile render-dev") && !command.contains("--fat-binary")
}

/// 单方式专项验证一次真实启动，构建方式切换专项验证三次。
fn run_count() -> usize {
    if matches!(std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref(), Ok("bundled-dx" | "dx-full" | "native-cargo" | "cargo-settings")) { 1 } else { 3 }
}
