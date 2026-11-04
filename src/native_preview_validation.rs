//! 同HWND原生预览的自有工程验收：原作者历史、可靠模式回执与外层真实OS输入分开取证。
//! 自动操作只在显式validation的native-preview场景安装；不会写业务成功、锁鼠或窗口模式。
#[path = "native_preview_validation/gpu_evidence.rs"]
mod gpu_evidence;
#[path = "native_preview_validation/io.rs"]
mod io;
#[path = "native_preview_validation/resources.rs"]
mod resources;

use crate::{
    embedded_viewport::{EmbeddedStatus, validation as evidence},
    viewport_mode::{RequestViewportMode, ViewportMode},
};
use bevy::{
    input::{ButtonState, mouse::MouseButtonInput},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    ui::InteractionDisabled,
    window::{PrimaryWindow, RawHandleWrapper},
};
use feathrise_editor_protocol::Mode;
use feathrise_editor_map::standalone::{self, embedded_snapshot, embedded_validation as author};
use resources::{Phase, Plan};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn ensure(value: bool, reason: &str) -> Result<(), String> {
    value.then_some(()).ok_or_else(|| reason.into())
}

pub(super) fn ordinary_path(path: &Path) -> Result<(), String> {
    io::ordinary(path).map(|_| ())
}

pub(crate) fn install(app: &mut App, root: &Path, directory: PathBuf) -> Result<(), String> {
    let project = evidence::code_iteration_owner(root)?;
    let marker = io::read_now(&root.join(".native-preview-rpg-adapter.json"))?
        .ok_or("原生专项缺少私有RPG夹具身份")?;
    let token = std::env::var("FEATHRISE_NATIVE_PREVIEW_TOKEN").map_err(|_| "原生专项缺少token")?;
    ensure(
        marker["token"] == token
            && marker["project_id"] == project.id
            && marker["output"] == json!(directory.join("runtime")),
        "RPG夹具与本轮token、工程或取证目录不匹配",
    )?;
    ensure(
        directory.is_absolute()
            && io::ordinary(&directory)?.is_dir()
            && !directory.join("native-preview.json").exists(),
        "原生专项必须使用本轮独占目录",
    )?;
    let embed = crate::workspace_window::EmbedTarget::from_env()?
        .ok_or("原生专项必须通过正常工作台打开作者")?;
    author::install(app);
    app.insert_resource(Plan {
        directory,
        project: project.root,
        token,
        root: embed.parent,
        root_pid: embed.pid,
        pid: 0,
        world_id: 0,
        phase: Phase::Open,
        started: Instant::now(),
        changed: Instant::now(),
        frames: 0,
        initial: None,
        baseline: None,
        editor_baseline: None,
        editor_image: None,
        editor_capture: None,
        added: None,
        pulse: None,
        click: None,
        click_step: 0,
        frame_before: 0,
        ticks_before: 0,
        paused_since: None,
        gate_name: None,
        gate_write: None,
        gate_written: false,
        gate_read: None,
        gate_retry: Instant::now(),
        steps: Vec::new(),
        report_write: None,
        report_written: false,
        failure: None,
        diagnostic_write: None,
        diagnostic_count: 0,
        diagnostic_next: Instant::now() + Duration::from_secs(10),
        diagnostic_error: None,
    })
    .add_systems(First, input)
    .add_systems(Last, drive);
    Ok(())
}

fn rect(world: &World, entity: Entity) -> Option<Rect> {
    if world.get::<InteractionDisabled>(entity).is_some()
        || !world.get::<InheritedVisibility>(entity)?.get()
    {
        return None;
    }
    let node = world.get::<ComputedNode>(entity)?;
    let pose = world.get::<UiGlobalTransform>(entity)?;
    (!node.is_empty()).then(|| Rect::from_center_size(pose.translation, node.size()))
}

/// 只为作者原预览按钮注入官方鼠标协议；原生游戏输入全部由外层SendInput负责。
fn input(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<Plan>() else {
        return;
    };
    let result = (|| -> Result<(), String> {
        if plan.failure.is_some()
            || !matches!(
                plan.phase,
                Phase::Launch
                    | Phase::LaunchReturn
                    | Phase::LaunchAfterCancel
                    | Phase::LaunchAfterAltF4
            )
            || plan.click_step >= 3
        {
            return Ok(());
        }
        if plan.click.is_none() {
            plan.click = world
                .query_filtered::<Entity, With<feathrise_editor_core::ProjectLaunchButton>>()
                .iter(world)
                .find(|entity| rect(world, *entity).is_some());
        }
        let Some(entity) = plan.click else {
            return Ok(());
        };
        let Some(bounds) = rect(world, entity) else {
            return Ok(());
        };
        let window = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .map_err(|error| error.to_string())?;
        world
            .get_mut::<Window>(window)
            .ok_or("作者窗口已消失")?
            .set_physical_cursor_position(Some(bounds.center().as_dvec2()));
        match plan.click_step {
            0 => plan.click_step = 1,
            1 if world.get::<Interaction>(entity) == Some(&Interaction::Hovered) => {
                world.write_message(MouseButtonInput {
                    button: MouseButton::Left,
                    state: ButtonState::Pressed,
                    window,
                });
                plan.click_step = 2;
            }
            2 => {
                ensure(
                    world.get::<Interaction>(entity) == Some(&Interaction::Pressed),
                    "原预览按钮未收到按下",
                )?;
                world.write_message(MouseButtonInput {
                    button: MouseButton::Left,
                    state: ButtonState::Released,
                    window,
                });
                plan.click_step = 3;
            }
            _ => {}
        }
        Ok(())
    })();
    if let Err(error) = result {
        plan.failure = Some(error);
    }
    world.insert_resource(plan);
}

fn advance_to(plan: &mut Plan, phase: Phase) {
    plan.phase = phase;
    plan.changed = Instant::now();
    plan.gate_name = None;
    plan.gate_write = None;
    plan.gate_written = false;
    plan.gate_read = None;
    plan.gate_retry = Instant::now();
    plan.click = None;
    plan.click_step = 0;
    plan.paused_since = None;
    plan.diagnostic_count = 0;
    plan.diagnostic_next = Instant::now() + Duration::from_secs(10);
}

fn status(world: &World) -> Value {
    let state = world.resource::<EmbeddedStatus>();
    json!({"active":state.active,"ready":state.ready,"pid":state.pid,"world_id":state.world_id,
        "playing":state.playing,"mode":state.observed_mode,"revision":state.applied_revision,
        "frames":state.frames,"ticks":state.ticks,"error":state.error})
}

fn unchanged(world: &World, plan: &Plan) -> Result<(), String> {
    if let Some(baseline) = &plan.baseline {
        ensure(
            *baseline
                == serde_json::to_value(author::probe(world)?)
                    .map_err(|error| error.to_string())?,
            "预览或窗口操作改变了作者草稿、保存身份或历史",
        )?;
    }
    let state = world.resource::<EmbeddedStatus>();
    if plan.pid != 0 {
        ensure(
            state.pid == plan.pid && state.world_id == plan.world_id,
            "原生预览往返或取消退出更换了PID/World",
        )?;
    }
    Ok(())
}

fn record(world: &mut World, plan: &mut Plan, stage: &str) -> Result<(), String> {
    unchanged(world, plan)?;
    let presentation = serde_json::to_value(author::presentation_probe(world)?)
        .map_err(|error| error.to_string())?;
    if stage == "resident_editing_ready" {
        plan.editor_baseline = Some(presentation.clone());
    }
    if let Some(baseline) = &plan.editor_baseline {
        ensure(presentation == *baseline, "独立预览改变了原作者工作台、相机、布局或选择")?;
        ensure(presentation["roots"].as_array().is_some_and(|roots| !roots.is_empty()
            && roots.iter().all(|root| root["display"] != "None" && root["inherited_visible"] == true))
            && presentation["headers"].as_array().is_some_and(|headers| !headers.is_empty()
                && headers.iter().all(|header| header["inherited_visible"] == true))
            && presentation["camera"]["active"] == true && presentation["layout"].is_array(),
            "独立预览期间原工作台和作者视口必须实际可见")?;
        let chrome = crate::window_chrome::preview_chrome_probe(world).ok_or("原作者顶栏缺失")?;
        ensure(chrome["active_header"] == chrome["regular_header"]
            && chrome["preview_header"].is_null() && chrome["camera"].is_null(),
            "预览仍创建或切换了旧替代标题栏")?;
        ensure(!world.resource::<standalone::EmbeddedAuthorInput>().blocked(),
            "稳定预览仍吞掉了原工作台的全部输入")?;
    }
    plan.steps
        .push(json!({"stage":stage,"author":author::probe(world)?,"game":status(world),"editor_presentation":presentation}));
    println!(
        "FEATHRISE_EMBEDDED_NATIVE_PREVIEW {}",
        json!({"stage":stage,"game":status(world)})
    );
    Ok(())
}

fn editor_image(world: &mut World, plan: &mut Plan) -> Result<bool, String> {
    if plan.editor_capture.is_some() { return Ok(true); }
    if let Some(reply) = &plan.editor_image {
        if let Some(proof) = io::poll(reply)? {
            ensure(proof["accepted"] == true, "原工作台GPU图尺寸或实际UI像素未通过")?;
            plan.editor_capture = Some(proof);
            plan.editor_image = None;
            return Ok(true);
        }
        return Ok(false);
    }
    let window = world.query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world).map_err(|error| error.to_string())?;
    let physical = [window.physical_width(), window.physical_height()];
    let view = json!({"game":status(world),"presentation":author::presentation_probe(world)?,
        "chrome":crate::window_chrome::preview_chrome_probe(world)});
    let path = plan.directory.join("editor-during-native-preview.png");
    let (sender, receiver) = async_channel::bounded(1);
    plan.editor_image = Some(receiver);
    world.spawn(Screenshot::primary_window()).observe(move |event: On<ScreenshotCaptured>| {
        io::image(event.image.clone(), path.clone(), physical, view.clone(), sender.clone());
    });
    Ok(false)
}

fn button_value(world: &World, entity: Entity, id: &str) -> Option<Value> {
    world.get::<Button>(entity)?;
    let bounds = rect(world, entity)?;
    Some(
        json!({"id":id,"entity":entity.to_bits(),"physical_rect":[bounds.min.x,bounds.min.y,bounds.width(),bounds.height()],
        "physical_center":bounds.center().to_array(),"visible":true,"enabled":true}),
    )
}

/// 取消按钮仍是原确认层的原实体；通过文字与父Button查找只读布局，不触发点击成功组件。
fn cancel_button(world: &mut World) -> Option<Value> {
    let candidates: Vec<Entity> = world
        .query::<(&Text, &ChildOf)>()
        .iter(world)
        .filter_map(|(text, parent)| (text.0 == "取消 (Esc)").then_some(parent.parent()))
        .collect();
    candidates.into_iter().find_map(|mut entity| {
        // 共享控件允许标签内再包布局节点；最多走四层，只导出实际Button，不把文字容器当按钮。
        for _ in 0..4 {
            if let Some(value) = button_value(world, entity, "cancel") {
                return Some(value);
            }
            entity = world.get::<ChildOf>(entity)?.parent();
        }
        None
    })
}

fn gate(
    world: &mut World,
    plan: &mut Plan,
    name: &str,
    final_action: bool,
) -> Result<bool, String> {
    if name == "playing-input" && !editor_image(world, plan)? {
        return Ok(false);
    }
    if let Some(reply) = &plan.gate_write
        && io::poll(reply)?.is_some()
    {
        plan.gate_write = None;
        plan.gate_written = true;
    }
    if !plan.gate_written && plan.gate_write.is_none() {
        let own = world
            .query_filtered::<&RawHandleWrapper, With<PrimaryWindow>>()
            .single(world)
            .map_err(|error| error.to_string())?;
        let own = crate::workspace_window::primary_hwnd(own).ok_or("作者没有HWND")?;
        let mut actions = Vec::new();
        if let Some(cancel) = cancel_button(world) {
            actions.push(cancel);
        }
        if let Some(finish) =
            crate::native_project_close::embedded_validation::fixture_finish_target(world)?
            && let Some(finish) = button_value(world, finish, "finish")
        {
            actions.push(finish);
        }
        let close_target = world
            .get_resource::<crate::native_project_close::NativeProjectCloseState>()
            .map(|state| state.target());
        let value = json!({"schema":1,"token":plan.token,"stage":name,"root_hwnd":plan.root,"root_pid":plan.root_pid,
            "author_hwnd":own,"author_pid":std::process::id(),"game_pid":plan.pid,"world_id":plan.world_id,
            "game":status(world),"author":author::probe(world)?,"actions":actions,"close_target":close_target,
            "chrome":crate::window_chrome::preview_chrome_probe(world),"runtime_directory":plan.directory.join("runtime"),
            "final_action":final_action,"input":"real OS SendInput; original production buttons"});
        plan.gate_name = Some(name.into());
        plan.gate_write = Some(io::write(
            plan.directory.join(format!("{name}.ready.json")),
            value,
        ));
        return Ok(false);
    }
    if !plan.gate_written || final_action {
        return Ok(false);
    }
    if let Some(reply) = &plan.gate_read
        && let Some(value) = io::poll(reply)?
    {
        plan.gate_read = None;
        plan.gate_retry = Instant::now() + Duration::from_millis(80);
        if let Some(ack) = value {
            ensure(
                ack["schema"] == 1
                    && ack["token"] == plan.token
                    && ack["stage"] == name
                    && ack["passed"] == true
                    && ack["input"] == "Win32 SendInput and OS queries",
                "原生门回执身份或来源不符",
            )?;
            plan.steps
                .push(json!({"native_input_gate":name,"external_real_input_ack":ack}));
            return Ok(true);
        }
    }
    if plan.gate_read.is_none() && Instant::now() >= plan.gate_retry {
        plan.gate_read = Some(io::read(plan.directory.join(format!("{name}.ack.json"))));
    }
    Ok(false)
}

fn settled_editing(world: &World, plan: &mut Plan) -> Result<bool, String> {
    unchanged(world, plan)?;
    let state = world.resource::<EmbeddedStatus>();
    if state.playing
        || state.observed_mode != Some(Mode::Editing)
        || state.frames <= plan.frame_before
    {
        return Ok(false);
    }
    if plan.paused_since.is_none() {
        plan.paused_since = Some(Instant::now());
        plan.ticks_before = state.ticks;
        plan.frame_before = state.frames;
    }
    ensure(
        state.ticks == plan.ticks_before,
        "返回Editing后玩法tick仍在推进",
    )?;
    Ok(plan
        .paused_since
        .is_some_and(|time| time.elapsed() >= Duration::from_secs(1))
        && state.frames >= plan.frame_before + 15)
}

fn prompt_ready(world: &mut World) -> Result<bool, String> {
    if crate::native_project_close::embedded_validation::visible_prompt(world).is_none() {
        return Ok(false);
    }
    ensure(
        world
            .resource::<crate::native_project_close::NativeProjectCloseState>()
            .target()
            == crate::native_project_close::CloseTarget::Editor,
        "Editor X没有请求退出编辑器",
    )?;
    Ok(cancel_button(world).is_some()
        && crate::native_project_close::embedded_validation::fixture_finish_target(world)?
            .is_some())
}

fn advance(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    if plan.phase == Phase::Finished {
        return Ok(());
    }
    plan.frames = plan.frames.saturating_add(1);
    ensure(
        plan.started.elapsed() < Duration::from_secs(2400)
            && plan.changed.elapsed()
                < Duration::from_secs(if matches!(plan.phase, Phase::Embedded) {
                    1800
                } else {
                    180
                }),
        "原生预览验收阶段超时",
    )?;
    if let Some(error) = &world.resource::<EmbeddedStatus>().error {
        return Err(format!("工程会话失败：{error}"));
    }
    if plan.baseline.is_some() {
        unchanged(world, plan)?;
    }
    // 先复制固定阶段，避免match守卫借住Plan字段而阻止门轮询更新其单飞槽。
    let phase = plan.phase;
    match phase {
        Phase::Open if plan.frames >= 30 => {
            world.write_message(RequestViewportMode(ViewportMode::SavedWorld));
            standalone::request_open_map(world, "M1-SAMPLE")?;
            advance_to(plan, Phase::Local);
        }
        Phase::Local if standalone::preview_ready_for(world, "M1-SAMPLE") => {
            plan.initial = Some(
                serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?,
            );
            author::request_cuboid(world, [7., 1., -2.])?;
            advance_to(plan, Phase::Added);
        }
        Phase::Added => {
            let current =
                serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?;
            let initial = plan.initial.as_ref().unwrap();
            if current["dirty"] == true
                && current["placements"].as_array().unwrap().len()
                    == initial["placements"].as_array().unwrap().len() + 1
            {
                ensure(
                    current["save_sequence"] == initial["save_sequence"],
                    "创建夹具隐式保存了作者草稿",
                )?;
                plan.added = Some(current);
                plan.pulse = author::pulse_history(world, false)?;
                if plan.pulse.is_some() {
                    advance_to(plan, Phase::Undo);
                }
            }
        }
        Phase::Undo => {
            let current =
                serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?;
            if current["placements"] == plan.initial.as_ref().unwrap()["placements"] {
                plan.pulse = author::pulse_history(world, true)?;
                if plan.pulse.is_some() {
                    advance_to(plan, Phase::Redo);
                }
            }
        }
        Phase::Redo => {
            let current =
                serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?;
            if current["placements"] == plan.added.as_ref().unwrap()["placements"] {
                ensure(
                    current["undo_len"] == plan.added.as_ref().unwrap()["undo_len"]
                        && current["redo_len"] == 0
                        && current["save_sequence"]
                            == plan.initial.as_ref().unwrap()["save_sequence"],
                    "原Undo/Redo历史或保存身份不符",
                )?;
                plan.baseline = Some(current);
                record(world, plan, "unsaved_author_and_history_prepared")?;
                world.write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
                advance_to(plan, Phase::Embedded);
            }
        }
        Phase::Embedded => {
            let state = world.resource::<EmbeddedStatus>();
            if state.ready
                && state.observed_mode == Some(Mode::Editing)
                && embedded_snapshot::ready_is_current(world)
                // ready_is_current核完整SourceIdentity；此处只与同一捕获域的快照修订对齐，不能混用作者Undo修订。
                && world.resource::<embedded_snapshot::EmbeddedSnapshotState>().ready.as_ref()
                    .is_some_and(|snapshot| snapshot.map_id == "M1-SAMPLE"
                        && state.applied_revision == snapshot.author_revision)
                && state.frames > 4
            {
                plan.pid = state.pid;
                plan.world_id = state.world_id;
                record(world, plan, "resident_editing_ready")?;
                advance_to(plan, Phase::EditingReady);
            }
        }
        Phase::EditingReady if gate(world, plan, "editing-ready", false)? => {
            advance_to(plan, Phase::Launch)
        }
        Phase::Launch
        | Phase::LaunchReturn
        | Phase::LaunchAfterCancel
        | Phase::LaunchAfterAltF4 => {
            let state = world.resource::<EmbeddedStatus>();
            if plan.click_step == 3
                && state.playing
                && state.observed_mode == Some(Mode::Playing)
                && state.ticks > plan.ticks_before
            {
                plan.frame_before = state.frames;
                let next = match plan.phase {
                    Phase::Launch => Phase::PlayingInput,
                    Phase::LaunchReturn => Phase::PlayingReturn,
                    Phase::LaunchAfterCancel => Phase::PlayingAfterCancel,
                    _ => Phase::PlayingAfterAltF4,
                };
                record(
                    world,
                    plan,
                    match next {
                        Phase::PlayingInput => "playing_native_ready",
                        Phase::PlayingReturn => "playing_before_hud_return",
                        Phase::PlayingAfterCancel => "playing_after_editor_cancel",
                        _ => "playing_after_same_batch_alt_f4",
                    },
                )?;
                advance_to(plan, next);
            }
        }
        Phase::PlayingInput if gate(world, plan, "playing-input", false)? => {
            advance_to(plan, Phase::ReturnedX)
        }
        Phase::ReturnedX if settled_editing(world, plan)? => {
            ensure(
                crate::native_project_close::embedded_validation::idle(world),
                "预览原生X错误触发了Editor关闭事务",
            )?;
            record(world, plan, "native_x_returned_editing_same_world")?;
            advance_to(plan, Phase::EditingAfterX);
        }
        Phase::EditingAfterX if gate(world, plan, "editing-after-x", false)? => {
            advance_to(plan, Phase::LaunchReturn)
        }
        Phase::PlayingReturn if gate(world, plan, "playing-return", false)? => {
            advance_to(plan, Phase::ReturnedHud)
        }
        Phase::ReturnedHud if settled_editing(world, plan)? => {
            record(world, plan, "hud_returned_editing_same_world")?;
            advance_to(plan, Phase::EditorClose);
        }
        Phase::EditorClose if gate(world, plan, "editor-close", false)? => {
            advance_to(plan, Phase::EditorPrompt)
        }
        // 发布后Cancel会隐藏原确认；仍轮询同一门ACK，随后再由Cancelled核对真实Idle/隐藏/暂停事实。
        Phase::EditorPrompt
            if plan.gate_name.as_deref() == Some("editor-confirm") || prompt_ready(world)? =>
        {
            if gate(world, plan, "editor-confirm", false)? {
                advance_to(plan, Phase::Cancelled);
            }
        }
        Phase::Cancelled
            if crate::native_project_close::embedded_validation::idle(world)
                && crate::native_project_close::embedded_validation::hidden_prompt(world)
                && settled_editing(world, plan)? =>
        {
            record(world, plan, "editor_exit_cancelled_same_healthy_world")?;
            advance_to(plan, Phase::LaunchAfterCancel);
        }
        Phase::PlayingAfterCancel if gate(world, plan, "playing-after-cancel", false)? => {
            advance_to(plan, Phase::ReturnedAfterCancel)
        }
        Phase::ReturnedAfterCancel if settled_editing(world, plan)? => {
            record(world, plan, "cancelled_editor_resumed_and_returned")?;
            record(world, plan, "same_batch_alt_f4_returned_editing")?;
            advance_to(plan, Phase::LaunchAfterAltF4);
        }
        Phase::PlayingAfterAltF4 if gate(world, plan, "playing-after-alt-f4", false)? => {
            // 外层先核重入偏好，再真实Min并点原Editor X；这里不向游戏伪造Editing。
            advance_to(plan, Phase::MinimizedEditorPrompt);
        }
        Phase::MinimizedEditorPrompt
            if plan.gate_name.as_deref() == Some("minimized-editor-confirm")
                || (world.resource::<EmbeddedStatus>().observed_mode == Some(Mode::Editing)
                    && !world.resource::<EmbeddedStatus>().playing
                    && prompt_ready(world)?) =>
        {
            if gate(world, plan, "minimized-editor-confirm", false)? {
                advance_to(plan, Phase::MinimizedCancelled);
            }
        }
        Phase::MinimizedCancelled
            if crate::native_project_close::embedded_validation::idle(world)
                && crate::native_project_close::embedded_validation::hidden_prompt(world)
                && settled_editing(world, plan)? =>
        {
            record(
                world,
                plan,
                "minimized_preview_returned_and_editor_exit_cancelled",
            )?;
            advance_to(plan, Phase::FinalClose);
        }
        Phase::FinalClose if gate(world, plan, "final-editor-close", false)? => {
            advance_to(plan, Phase::FinalPrompt)
        }
        Phase::FinalPrompt if prompt_ready(world)? => {
            // 报告先落盘，外层才获准点击本轮夹具原“不保存并退出”。退出成功仍由协调器/Job独立证明。
            if !plan.report_written {
                if plan.report_write.is_none() {
                    record(world, plan, "final_original_editor_confirmation_ready")?;
                    plan.report_write = Some(io::write(
                        plan.directory.join("native-preview.json"),
                        report(world, plan, true),
                    ));
                }
                if let Some(reply) = &plan.report_write
                    && io::poll(reply)?.is_some()
                {
                    plan.report_write = None;
                    plan.report_written = true;
                }
            }
            if plan.report_written {
                let _ = gate(world, plan, "final-editor-confirm", true)?;
                if plan.gate_written {
                    plan.phase = Phase::Finished;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn author_observation(world: &World) -> Value {
    author::probe(world).map_or_else(
        |error| json!({"error":error}),
        |probe| {
            serde_json::to_value(probe).unwrap_or_else(|error| json!({"error":error.to_string()}))
        },
    )
}

fn log_tail(world: &World) -> Value {
    world
        .get_resource::<crate::project_log::ProjectLogState>()
        .map_or(Value::Null, |log| log.validation_tail())
}

/// 只读等待取证不参与业务通过条件；主帧只复制本夹具的有限DTO，文件写入始终单飞。
fn diagnostic(world: &World, plan: &mut Plan) {
    if let Some(reply) = &plan.diagnostic_write {
        match io::poll(reply) {
            Ok(None) => return,
            Ok(Some(())) => plan.diagnostic_write = None,
            Err(error) => {
                plan.diagnostic_error = Some(error);
                plan.diagnostic_write = None;
            }
        }
    }
    if plan.phase == Phase::Finished
        || plan.diagnostic_count >= 3
        || Instant::now() < plan.diagnostic_next
    {
        return;
    }
    plan.diagnostic_count += 1;
    plan.diagnostic_next = Instant::now() + Duration::from_secs(10);
    let snapshot = json!({"schema":1,"scope":"native-preview-readonly-wait","token":plan.token,
        "phase":format!("{:?}",plan.phase),"sequence":plan.diagnostic_count,"per_phase_limit":3,
        "elapsed_ms":plan.started.elapsed().as_millis() as u64,"phase_elapsed_ms":plan.changed.elapsed().as_millis() as u64,
        "author_probe":author_observation(world),"embedded_status":status(world),"error":plan.failure,
        "project_log_tail":log_tail(world),"diagnostic_error":plan.diagnostic_error,
        "input":{"history_pulse":plan.pulse.map(Entity::to_bits),"preview_click":plan.click.map(Entity::to_bits),"click_step":plan.click_step},
        "io":{"gate_name":plan.gate_name,"gate_write_pending":plan.gate_write.is_some(),"gate_read_pending":plan.gate_read.is_some()}});
    plan.diagnostic_write = Some(io::write(
        plan.directory.join(format!(
            "native-preview-wait-{:?}-{:02}.json",
            plan.phase, plan.diagnostic_count
        )),
        snapshot,
    ));
}

fn report(world: &World, plan: &Plan, passed: bool) -> Value {
    json!({"schema":1,"scope":"owned-native-preview-window","passed":passed,"token":plan.token,
        "owner_checked":true,"project_root":plan.project,"root_pid":plan.root_pid,"author_pid":std::process::id(),
        "game_pid":plan.pid,"world_id":plan.world_id,"steps":plan.steps,"error":plan.failure,
        "editor_gpu_capture":plan.editor_capture,
        "phase":format!("{:?}",plan.phase),"elapsed_ms":plan.started.elapsed().as_millis() as u64,
        "phase_elapsed_ms":plan.changed.elapsed().as_millis() as u64,"author_probe":author_observation(world),
        "embedded_status":status(world),"project_log_tail":log_tail(world),"diagnostic_error":plan.diagnostic_error,
        "alt_f4_scope":"one SendInput batch; no extra Alt consumption in accepted close frame; earlier physical Alt timing is not verified",
        "final_exit_requires_process_result":true,"physical_mouse_feel_verified":false})
}

fn drive(world: &mut World) {
    let Some(mut plan) = world.remove_resource::<Plan>() else {
        return;
    };
    // pulse在前一Last发布，必须等本帧原Update消费后再释放，First清除会吞掉真实Undo/Redo。
    if let Some(pulse) = plan.pulse.take() {
        author::release_pulse(world, pulse);
    }
    if plan.failure.is_none()
        && let Err(error) = advance(world, &mut plan)
    {
        plan.failure = Some(error);
    }
    diagnostic(world, &mut plan);
    if plan.failure.is_some() && !plan.report_written {
        if plan.report_write.is_none() {
            eprintln!(
                "FEATHRISE_EMBEDDED_NATIVE_PREVIEW_FAILED {:?}: {:?}",
                plan.phase, plan.failure
            );
            plan.report_write = Some(io::write(
                plan.directory.join("native-preview.json"),
                report(world, &plan, false),
            ));
        }
        if let Some(reply) = &plan.report_write
            && io::poll(reply).is_ok_and(|value| value.is_some())
        {
            plan.report_written = true;
        }
    }
    world.insert_resource(plan);
}

/// 外层协调器核对本轮阶段与原生门；旧preview-chrome的成功不能被新合同消费。
pub(crate) fn verify(output: &Path, report: &Value) -> Result<Vec<Value>, String> {
    ensure(
        report["scope"] == "owned-native-preview-window"
            && report["owner_checked"] == true
            && report["final_exit_requires_process_result"] == true,
        "缺少独立原生预览专项身份",
    )?;
    let steps = report["steps"].as_array().ok_or("原生专项缺少阶段")?;
    for name in [
        "unsaved_author_and_history_prepared",
        "resident_editing_ready",
        "playing_native_ready",
        "native_x_returned_editing_same_world",
        "playing_before_hud_return",
        "hud_returned_editing_same_world",
        "editor_exit_cancelled_same_healthy_world",
        "playing_after_editor_cancel",
        "cancelled_editor_resumed_and_returned",
        "same_batch_alt_f4_returned_editing",
        "playing_after_same_batch_alt_f4",
        "minimized_preview_returned_and_editor_exit_cancelled",
        "final_original_editor_confirmation_ready",
    ] {
        ensure(
            steps.iter().filter(|step| step["stage"] == name).count() == 1,
            "原生预览阶段不完整或重复",
        )?;
    }
    for name in [
        "editing-ready",
        "playing-input",
        "editing-after-x",
        "playing-return",
        "editor-close",
        "editor-confirm",
        "playing-after-cancel",
        "playing-after-alt-f4",
        "minimized-editor-confirm",
        "final-editor-close",
    ] {
        let matching: Vec<_> = steps
            .iter()
            .filter(|step| step["native_input_gate"] == name)
            .collect();
        ensure(
            matching.len() == 1
                && matching[0]["external_real_input_ack"]["passed"] == true
                && matching[0]["external_real_input_ack"]["token"] == report["token"],
            "原生门缺少本轮真实输入回执",
        )?;
    }
    gpu_evidence::verify(output, report)
}
