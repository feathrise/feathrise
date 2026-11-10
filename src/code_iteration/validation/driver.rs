//! 通过原创建/撤销、原文档输入/保存、真实诊断行与应用按钮驱动本轮专项，不写成功状态。

use feathrise_editor_ui::UiInteraction;
use super::{close_cancel, evidence, resources::{Phase, Plan}};
use crate::{code_iteration::{CodeIterationState, components::{ApplyVersion, CodeIterationAction}, types::CodeIterationPhase},
    embedded_viewport::EmbeddedStatus, viewport_mode::{RequestViewportMode, ViewportMode, ViewportModeState}};
use bevy::{input_focus::{FocusCause, InputFocus}, prelude::*, tasks::{IoTaskPool, futures::check_ready}, text::{EditableText, TextEdit}, window::WindowCloseRequested};
use feathrise_editor_scene::standalone::{self, SourceLocationAction, embedded_validation as author};
use serde_json::{Value, json};
use std::{io::Write, path::PathBuf, time::Duration};

const CONTROLLER: &str = "src/controller.rs";
const NOTE: &str = "src/author_notes.rs";
const SPEED_12: &str = "const CODE_ITERATION_WALK_SPEED: f32 = 12.0;";
const SPEED_BAD: &str = "const CODE_ITERATION_WALK_SPEED: f32 = code_iteration_invalid_number;";
const SPEED_36: &str = "const CODE_ITERATION_WALK_SPEED: f32 = 36.0;";

pub(super) fn drive(world: &mut World) {
    world.resource_scope(|world, mut plan: Mut<Plan>| {
        // 旁路只观察原UI发布的请求，不发消息或补发点击；合成脉冲不等于业务已受理。
        if let Some(generation) = plan.apply_requests.read(world.resource::<Messages<crate::code_iteration::RequestApplyCode>>())
            .map(|request| request.generation).last()
        { plan.apply_request_generation = Some(generation); }
        if plan.phase == Phase::Applying && let Some(button) = plan.pulse {
            plan.apply_button_evidence = Some(apply_button_probe(world, button));
        }
        if let Some(button) = plan.pulse.take() { author::release_pulse(world, button); }
        plan.frames = plan.frames.saturating_add(1);
        if plan.phase == Phase::Finished { return; }
        if plan.phase == Phase::Report {
            if let Some(result) = plan.completion.as_mut().and_then(check_ready) {
                plan.completion = None;
                if let Err(error) = result {
                    eprintln!("FEATHRISE_CODE_ITERATION_VALIDATION_REPORT_FAILED {error}");
                    world.write_message(AppExit::error());
                    plan.phase = Phase::Finished;
                    return;
                }
                close_cancel::release_hold(world);
                if let Err(error) = crate::native_project_close::embedded_validation::request(world) {
                    eprintln!("FEATHRISE_CODE_ITERATION_VALIDATION_CLOSE_FAILED {error}");
                    world.write_message(AppExit::error());
                    plan.phase = Phase::Finished;
                    return;
                }
                plan.advance(Phase::Closing);
            }
            return;
        }
        if plan.phase == Phase::Closing {
            match crate::native_project_close::embedded_validation::finish_fixture(world) {
                Ok(Some(button)) => plan.pulse = Some(button),
                Ok(None) => (),
                Err(error) => eprintln!("FEATHRISE_CODE_ITERATION_VALIDATION_CLOSE_WAIT {error}"),
            }
            return; // 原request_exit确认后台、游戏与日志线程全部收口后才发AppExit。
        }
        let budget = if plan.phase == Phase::FunctionHotpatch { 2400 } else if matches!(plan.phase, Phase::Embedded | Phase::CompileFailed | Phase::CandidateReady
            | Phase::CloseRecovered) { 1200 } else { 180 };
        let result = if plan.changed.elapsed() > Duration::from_secs(budget) {
            Err(format!("代码迭代专项阶段{:?}超过{budget}秒：{}", plan.phase, world.resource::<CodeIterationState>().status))
        } else { advance(world, &mut plan) };
        if let Err(error) = result {
            eprintln!("FEATHRISE_CODE_ITERATION_VALIDATION_FAILED {}", json!({"phase":format!("{:?}",plan.phase),"error":error}));
            plan.error = Some(error);
            write_report(world, &mut plan);
        }
    });
}

fn advance(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    if plan.phase == Phase::FunctionHotpatch { return super::function_hotpatch::exercise(world, plan); }
    if close_cancel::handles(plan.phase) { return close_cancel::exercise(world, plan); }
    if plan.old_pid != 0 && !matches!(plan.phase, Phase::Applying | Phase::AfterCapture) {
        same_old(world, plan)?;
    }
    match plan.phase {
        Phase::Open if plan.frames >= 30 => {
            world.write_message(RequestViewportMode(ViewportMode::SavedWorld));
            standalone::request_open_scene(world, "M1-SAMPLE")?;
            plan.advance(Phase::Local);
        }
        Phase::Local if standalone::preview_ready_for(world, "M1-SAMPLE")
            && world.resource::<ViewportModeState>().selected == ViewportMode::SavedWorld => {
            plan.initial = Some(json!(author::probe(world)?));
            world.write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
            plan.advance(Phase::Embedded);
        }
        Phase::Embedded => {
            // 真实构建／连接已经失败时直接保存其原因；GPU等待不能把终态错误拖成1200秒超时。
            if let Some(error) = &world.resource::<EmbeddedStatus>().error {
                return Err(format!("工程3D首次启动失败：{error}"));
            }
            // 原创建入口会丢弃失焦帧的请求；等待真实Winit焦点，不改focused冒充窗口已激活。
            if !primary_window_focused(world) { return Ok(()); }
            let Some(proof) = evidence::capture(world, plan, 12.)? else { return Ok(()); };
            let status = world.resource::<EmbeddedStatus>();
            plan.old_pid = status.pid; plan.old_world = status.world_id; plan.old_frame = status.frames;
            record(world, plan, "old_version_12_real_component_and_gpu", Some(proof))?;
            author::request_cuboid(world, [7., 1., -2.])?;
            plan.advance(Phase::FirstAdded);
        }
        Phase::FirstAdded if evidence::current_snapshot(world) => {
            let probe = author::probe(world)?;
            let initial = plan.initial.as_ref().unwrap();
            if probe.placements.len() as u64 != initial["placements"].as_array().unwrap().len() as u64 + 1
                || probe.undo_len as u64 != initial["undo_len"].as_u64().unwrap() + 1 { return Ok(()); }
            // 等首份快照期间焦点可能改变；第二个一次性创建请求同样等待真实作者窗口获焦。
            if !primary_window_focused(world) { return Ok(()); }
            plan.first_id = probe.last_added_id;
            author::request_cuboid(world, [9., 1., -2.])?;
            plan.advance(Phase::SecondAdded);
        }
        Phase::SecondAdded if evidence::current_snapshot(world) => {
            let probe = author::probe(world)?;
            if probe.undo_len as u64 != plan.initial.as_ref().unwrap()["undo_len"].as_u64().unwrap() + 2 { return Ok(()); }
            if let Some(button) = author::pulse_history(world, false)? { plan.pulse = Some(button); plan.advance(Phase::Undo); }
        }
        Phase::Undo if evidence::current_snapshot(world) => {
            let probe = author::probe(world)?;
            if !probe.dirty || probe.redo_len != 1 || probe.undo_len as u64 != plan.initial.as_ref().unwrap()["undo_len"].as_u64().unwrap() + 1 { return Ok(()); }
            author::tool_previews::select(world, plan.first_id.as_deref())?;
            record(world, plan, "unsaved_scene_selection_and_undo_redo_prepared", None)?;
            open_source(world, plan, NOTE);
            plan.advance(Phase::NoteOpen);
        }
        Phase::NoteOpen => {
            if replace_source(world, plan, NOTE, "基线", "未保存草稿")? { plan.advance(Phase::NoteEdit); }
        }
        Phase::NoteEdit if source_equals(world, plan, NOTE, true) => {
            let Some(probe) = standalone::source_location_probe(world) else { return Ok(()); };
            if probe["path"] != NOTE || probe["dirty"] != true || probe["native_matches"] != true
                || probe["source"].as_str() != Some("// 作者文本草稿：未保存草稿\n")
                || probe["undo_len"].as_u64().unwrap_or(0) == 0 { return Ok(()); }
            plan.note_baseline = Some(probe.clone());
            record(world, plan, "unsaved_text_native_input_and_history_prepared", Some(probe))?;
            if plan.function_hotpatch {
                super::function_hotpatch::begin(world, plan)?;
            } else if close_cancel::selected(world) {
                close_cancel::begin(world, plan)?;
            } else {
                open_source(world, plan, CONTROLLER);
                plan.advance(Phase::ControllerOpen);
            }
        }
        Phase::ControllerOpen => {
            if replace_source(world, plan, CONTROLLER, SPEED_12, SPEED_BAD)? { plan.advance(Phase::InvalidBuffer); }
        }
        Phase::InvalidBuffer if source_equals(world, plan, CONTROLLER, true) => {
            if pulse_source(world, plan, CONTROLLER, SourceLocationAction::Save)? { plan.advance(Phase::SaveInvalid); }
        }
        Phase::SaveInvalid if source_equals(world, plan, CONTROLLER, false) => {
            plan.old_frame = world.resource::<EmbeddedStatus>().frames;
            record(world, plan, "invalid_rust_saved_through_original_button", None)?;
            plan.advance(Phase::CompileFailed);
        }
        Phase::CompileFailed => {
            let state = world.resource::<CodeIterationState>();
            if state.phase != CodeIterationPhase::Failed { return Ok(()); }
            let Some((index, location)) = state.diagnostics.iter().enumerate().find_map(|(index, diagnostic)| {
                diagnostic.location.as_ref().filter(|location| location.path == PathBuf::from(CONTROLLER))
                    .map(|location| (index, json!(location)))
            }) else { return Err("真实编译错误缺少controller.rs源码位置".into()); };
            plan.invalid_generation = state.generation;
            if let Some(recovery) = &plan.close_cancel_evidence
                && state.generation <= recovery["resumed_generation"].as_u64().unwrap_or(u64::MAX)
            { return Err("取消后的再次保存没有推进真实源码代次".into()); }
            plan.diagnostic_index = Some(index);
            plan.diagnostic_location = Some(location);
            if world.resource::<EmbeddedStatus>().frames <= plan.old_frame { return Ok(()); }
            let diagnostics = json!({"diagnostics":state.diagnostics.as_ref()});
            record(world, plan, "compile_error_preserved_old_pid_world_and_frames", Some(diagnostics))?;
            if pulse_code(world, plan, false)? { plan.advance(Phase::Diagnostics); }
        }
        Phase::Diagnostics => {
            let index = plan.diagnostic_index.unwrap();
            let Some(button) = super::super::diagnostics_ui::validation::diagnostic_row_entity(world, index) else { return Ok(()); };
            if !evidence::screenshot(world, plan, "compile-error-diagnostics.png")? { return Ok(()); }
            pulse(world, plan, button);
            plan.advance(Phase::Located);
        }
        Phase::Located => {
            let Some(probe) = standalone::source_location_probe(world) else { return Ok(()); };
            let location = plan.diagnostic_location.as_ref().unwrap();
            if probe["path"] != CONTROLLER || probe["native_matches"] != true || probe["focused"] != true
                || probe["line"] != location["line"] || probe["column"] != location["column"] { return Ok(()); }
            record(world, plan, "diagnostic_original_row_located_native_unicode_position", Some(probe))?;
            if replace_source(world, plan, CONTROLLER, SPEED_BAD, SPEED_36)? { plan.advance(Phase::FixedBuffer); }
        }
        Phase::FixedBuffer if source_equals(world, plan, CONTROLLER, true) => {
            if pulse_source(world, plan, CONTROLLER, SourceLocationAction::Save)? { plan.advance(Phase::SaveFixed); }
        }
        Phase::SaveFixed if source_equals(world, plan, CONTROLLER, false) => {
            plan.old_frame = world.resource::<EmbeddedStatus>().frames;
            record(world, plan, "speed_36_saved_through_original_button", None)?;
            plan.advance(Phase::CandidateReady);
        }
        Phase::CandidateReady => {
            let state = world.resource::<CodeIterationState>();
            if state.phase != CodeIterationPhase::Ready || state.generation <= plan.invalid_generation
                || state.pending.as_ref().is_none_or(|version| version.identity.generation != state.generation)
                || world.resource::<EmbeddedStatus>().frames <= plan.old_frame { return Ok(()); }
            plan.apply_generation = state.generation;
            record(world, plan, "fixed_code_complete_build_ready_old_game_retained", None)?;
            open_source(world, plan, NOTE);
            plan.advance(Phase::ReturnNote);
        }
        Phase::ReturnNote => {
            source_wait(world, plan);
            let Some(probe) = standalone::source_location_probe(world) else { return Ok(()); };
            if probe["path"] != NOTE || probe["native_matches"] != true { return Ok(()); }
            verify_note(&probe, plan.note_baseline.as_ref().unwrap())?;
            if pulse_source(world, plan, NOTE, SourceLocationAction::Scene)? { plan.advance(Phase::ReturnScene); }
        }
        Phase::ReturnScene => {
            source_wait(world, plan);
            if !standalone::preview_ready_for(world, "M1-SAMPLE") || !evidence::current_snapshot(world) { return Ok(()); }
            let Some(probe) = standalone::source_location_probe(world) else { return Ok(()); };
            if probe["central_active"] == true { return Ok(()); }
            // 场景重新显示后，布局、投影及下一帧原生几何按原系统逐次发布。
            // 只冻结连续三帧一致的真实基线；之后的保留断言仍逐字段严格比较。
            let baseline = evidence::author_state(world)?;
            if plan.baseline.as_ref() != Some(&baseline) {
                plan.baseline = Some(baseline);
                plan.baseline_stable_frames = 0;
                return Ok(());
            }
            plan.baseline_stable_frames += 1;
            if plan.baseline_stable_frames < 3 { return Ok(()); }
            plan.advance(Phase::BeforeCapture);
        }
        Phase::BeforeCapture => {
            let Some(proof) = evidence::capture(world, plan, 12.)? else { return Ok(()); };
            if !evidence::screenshot(world, plan, "before-apply-editor.png")? { return Ok(()); }
            evidence::preserved(world, plan.baseline.as_ref().unwrap())?;
            if pulse_code(world, plan, true)? {
                record(world, plan, "explicit_apply_original_button_pulsed_with_full_author_baseline", Some(proof))?;
                plan.advance(Phase::Applying);
            }
        }
        Phase::Applying => {
            let state = world.resource::<CodeIterationState>();
            if state.phase == CodeIterationPhase::Failed { return Err(format!("新版本应用失败：{}", state.status)); }
            let accepted = state.applying.as_ref().is_some_and(|flight| flight.token == plan.apply_generation)
                || state.applied.as_ref().is_some_and(|identity| identity.generation == plan.apply_generation);
            if !accepted && plan.changed.elapsed() > Duration::from_secs(2) {
                return Err(format!("原应用按钮脉冲后两秒未获真实ApplyFlight受理：{}", apply_wait_probe(world, plan)));
            }
            if accepted && !plan.apply_accepted {
                plan.apply_accepted = true;
                let receipt = json!({"requested_generation":plan.apply_request_generation,"generation":plan.apply_generation});
                record(world, plan, "explicit_apply_original_button_accepted_by_business_flight", Some(receipt))?;
            }
            let state = world.resource::<CodeIterationState>();
            if state.phase != CodeIterationPhase::Applied || state.applied.as_ref().is_none_or(|identity| identity.generation != plan.apply_generation) { return Ok(()); }
            let status = world.resource::<EmbeddedStatus>();
            if !status.ready || status.playing || status.pid == plan.old_pid || status.world_id == plan.old_world
                || status.pid == 0 || status.world_id == 0 || !evidence::current_snapshot(world) { return Ok(()); }
            plan.advance(Phase::AfterCapture);
        }
        Phase::AfterCapture => {
            if plan.changed.elapsed() < Duration::from_millis(400) { return Ok(()); }
            evidence::preserved(world, plan.baseline.as_ref().unwrap())?;
            let Some(proof) = evidence::capture(world, plan, 36.)? else { return Ok(()); };
            if !evidence::screenshot(world, plan, "after-apply-editor.png")? { return Ok(()); }
            record(world, plan, "new_pid_world_actual_speed_36_gpu_and_all_author_state_preserved", Some(proof))?;
            write_report(world, plan);
        }
        _ => (),
    }
    Ok(())
}

fn apply_button_probe(world: &World, button: Entity) -> Value {
    json!({"entity":format!("{button:?}"),"generation":world.get::<ApplyVersion>(button).and_then(|bound|bound.0),
        "clicked":world.get::<feathrise_editor_ui::EditorButtonClick>(button).map(|click|click.is_clicked()),
        "click_state":world.get::<feathrise_editor_ui::EditorButtonClick>(button).map(|click|format!("{click:?}")),
        "interaction":world.get::<UiInteraction>(button).map(|interaction|format!("{interaction:?}")),
        "disabled":world.get::<bevy::ui::InteractionDisabled>(button).is_some()})
}

fn apply_wait_probe(world: &World, plan: &Plan) -> Value {
    let state = world.resource::<CodeIterationState>();
    let runtime = world.non_send::<super::super::resources::CodeIterationRuntime>();
    json!({"expected_generation":plan.apply_generation,"observed_request_generation":plan.apply_request_generation,
        "current_generation":state.generation,"pending_generation":state.pending.as_ref().map(|version|version.identity.generation),
        "phase":format!("{:?}",state.phase),"status":state.status,"diagnostics_open":state.diagnostics_open,
        "backend_current":state.pending.as_ref().is_some_and(|version|runtime.backend.as_ref().is_some_and(|backend|backend.is_current(&version.identity))),
        "lifecycle":format!("{:?}",runtime.lifecycle),
        "close_captures_input":world.get_resource::<crate::native_project_close::NativeProjectCloseState>().is_some_and(|close|close.captures_input()),
        "host_overlay_active":world.get_resource::<feathrise_editor_core::EditorHostOverlay>().is_some_and(|overlay|overlay.active),
        "close_message_present":world.get_resource::<Messages<WindowCloseRequested>>().is_some_and(|messages|!messages.is_empty()),
        "button_after_original_update_before_validation_release":plan.apply_button_evidence})
}

fn primary_window_focused(world: &mut World) -> bool {
    world.query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .single(world).is_ok_and(|window| window.focused)
}

fn same_old(world: &World, plan: &Plan) -> Result<(), String> {
    let game = world.resource::<EmbeddedStatus>();
    if !game.active || !game.ready || game.pid != plan.old_pid || game.world_id != plan.old_world || game.error.is_some() {
        return Err(format!("显式应用前原工程实例被中断或替换：{game:?}"));
    }
    Ok(())
}

pub(super) fn open_source(world: &mut World, plan: &Plan, path: &str) {
    let context = world.get_resource::<feathrise_editor_scene::workbench::WorkbenchModel>()
        .map_or(world.resource::<CodeIterationState>().source_context, |model| model.context);
    world.write_message(standalone::OpenProjectSourceLocation { context, project_root: plan.project.root.clone(), path: path.into(), line: 1, column: 1 });
}

pub(super) fn pulse(world: &mut World, plan: &mut Plan, button: Entity) {
    world.entity_mut(button).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    plan.pulse = Some(button);
}

pub(super) fn pulse_source(world: &mut World, plan: &mut Plan, path: &str, action: SourceLocationAction) -> Result<bool, String> {
    let Some(button) = standalone::source_location_action_entity(world, path, action) else { return Ok(false); };
    pulse(world, plan, button);
    Ok(true)
}

pub(super) fn pulse_code(world: &mut World, plan: &mut Plan, apply: bool) -> Result<bool, String> {
    let button = world.query::<(Entity, &CodeIterationAction, &ApplyVersion, &Node, &ComputedNode, &InheritedVisibility)>()
        .iter(world).find_map(|(entity, action, version, node, computed, visible)| {
            let matches = if apply { matches!(action, CodeIterationAction::Apply) && version.0 == Some(plan.apply_generation) }
                else { matches!(action, CodeIterationAction::Diagnostics) };
            (matches && node.display != Display::None && !computed.is_empty() && visible.get()).then_some(entity)
        });
    let Some(button) = button else { return Ok(false); };
    pulse(world, plan, button);
    Ok(true)
}

pub(super) fn replace_source(world: &mut World, plan: &mut Plan, path: &str, from: &str, to: &str) -> Result<bool, String> {
    let Some(probe) = standalone::source_location_probe(world) else {
        if plan.changed.elapsed() > Duration::from_secs(1) && plan.frames % 300 == 0 {
            eprintln!("FEATHRISE_CODE_ITERATION_SOURCE_WAIT {}", standalone::source_location_navigation_probe(world));
        }
        return Ok(false);
    };
    if probe["path"] != path { return Ok(false); }
    if probe["edit_mode"] != true { let _ = pulse_source(world, plan, path, SourceLocationAction::Edit)?; return Ok(false); }
    if probe["native_matches"] != true || probe["pending_edits"].as_u64().unwrap_or(1) != 0 { return Ok(false); }
    let Some(entity) = standalone::source_location_input_entity(world, path) else { return Ok(false); };
    let source = probe["source"].as_str().ok_or("本轮小源码没有完整原文档正文")?;
    if source.len() > 16 * 1024 || source.matches(from).count() != 1 { return Err(format!("本轮源码替换锚点不是唯一或超预算：{path} {from}")); }
    let replacement = source.replacen(from, to, 1);
    world.resource_mut::<InputFocus>().set(entity, FocusCause::Navigated);
    let mut input = world.get_mut::<EditableText>(entity).ok_or("原源码输入刚刚失效")?;
    input.queue_edit(TextEdit::SelectAll);
    input.queue_edit(TextEdit::Insert(replacement.clone().into()));
    plan.source_target = Some(replacement);
    Ok(true)
}

pub(super) fn source_equals(world: &mut World, plan: &Plan, path: &str, dirty: bool) -> bool {
    standalone::source_location_probe(world).is_some_and(|probe| probe["path"] == path
        && probe["source"].as_str() == plan.source_target.as_deref() && probe["dirty"] == dirty
        && probe["native_matches"] == true && probe["pending_edits"].as_u64() == Some(0)
        && (!dirty || probe["revision"].as_u64().unwrap_or(0) > probe["saved_revision"].as_u64().unwrap_or(0)))
}

pub(super) fn verify_note(actual: &Value, expected: &Value) -> Result<(), String> {
    for field in ["path", "source", "dirty", "revision", "saved_revision", "undo_len", "redo_len", "disk_digest"] {
        if actual[field] != expected[field] { return Err(format!("源码编译或诊断丢失了作者文本{field}")); }
    }
    Ok(())
}

/// 仅停留在回程门时每5秒读一次固定状态，避免返回动作失配后只留下阶段名而没有实际条件。
pub(super) fn source_wait(world: &mut World, plan: &mut Plan) {
    if plan.source_wait.elapsed() < Duration::from_secs(5) { return; }
    plan.source_wait = std::time::Instant::now();
    let probe = standalone::source_location_probe(world);
    let source = probe.as_ref().map(|probe| json!({"path":probe["path"],"native_matches":probe["native_matches"],
        "input_entity":probe["input_entity"],"visible":probe["visible"],"central_active":probe["central_active"],
        "pending_edits":probe["pending_edits"],"focused":probe["focused"]}));
    let scene_button = standalone::source_location_action_entity(world, NOTE, SourceLocationAction::Scene).map(Entity::to_bits);
    let navigation = standalone::source_location_navigation_probe(world);
    eprintln!("FEATHRISE_CODE_ITERATION_SOURCE_WAIT {}", json!({"phase":format!("{:?}",plan.phase),"source":source,
        "scene_button":scene_button,"preview_ready":standalone::preview_ready_for(world,"M1-SAMPLE"),
        "snapshot_current":evidence::current_snapshot(world),"navigation":navigation}));
}

pub(super) fn record(world: &mut World, plan: &mut Plan, stage: &str, proof: Option<Value>) -> Result<(), String> {
    if plan.steps.last().is_some_and(|step| step["stage"] == stage) { return Ok(()); }
    if plan.steps.len() >= if plan.function_hotpatch { 48 } else { 24 } { return Err("本轮代码迭代阶段证据超过固定预算".into()); }
    let game = world.resource::<EmbeddedStatus>();
    let game = json!({"pid":game.pid,"world_id":game.world_id,"ready":game.ready,"playing":game.playing,
        "revision":game.applied_revision,"frames":game.frames,"ticks":game.ticks});
    let code = world.resource::<CodeIterationState>();
    let code = json!({"phase":format!("{:?}",code.phase),"generation":code.generation,"status":code.status});
    plan.steps.push(json!({"stage":stage,"elapsed_ms":plan.started.elapsed().as_secs_f64()*1000.,
        "game":game,"code":code,"author":author::probe(world)?,"source":standalone::source_location_probe(world),"proof":proof}));
    println!("FEATHRISE_CODE_ITERATION_VALIDATION_STEP {stage}");
    Ok(())
}

pub(super) fn write_report(world: &mut World, plan: &mut Plan) {
    crate::embedded_viewport::validation::code_iteration_cancel_return_click(world);
    if plan.phase == Phase::Report { return; }
    close_cancel::prepare_report(world);
    if close_cancel::selected(world) && plan.close_cancel_evidence.is_none() && plan.error.is_none() {
        plan.error = Some("已选择关闭取消专项，但未完成真实后台恢复证据".into());
    }
    if plan.function_hotpatch && plan.function_hotpatch_evidence.is_none() && plan.error.is_none() {
        plan.error = Some("已选择函数体热更新专项，但未取得同实例连续状态及新代次GPU证据".into());
    }
    let observation = json!({"author":author::probe(world).ok(),
        "embedded":format!("{:?}",world.resource::<EmbeddedStatus>()),
        "snapshot_wait":standalone::embedded_snapshot::waiting_reason(world),
        "snapshot_error":world.resource::<standalone::embedded_snapshot::EmbeddedSnapshotState>().error,
        "snapshot_revision":world.resource::<standalone::embedded_snapshot::EmbeddedSnapshotState>().revision,
        // Winit焦点与原生前台许可独立记录，避免把“预览已运行但未获得输入”误判为启动失败。
        "viewport_geometry":world.get_resource::<crate::viewport_mode::ViewportGeometry>().map(|geometry| json!({
            "focused":geometry.focused,"visible":geometry.visible,"revision":geometry.revision,
            "parent_hwnd":geometry.parent_hwnd,"parent_pid":geometry.parent_pid,"rect":geometry.rect})),
        "window_focused":world.query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
            .single(world).ok().map(|window|window.focused)});
    let report = json!({"schema":1,"scope":if plan.function_hotpatch {"owned-function-hotpatch-formal-preview-real-window"} else {"owned-code-iteration-first-stage-real-window"},
        "passed":plan.error.is_none(),"owner_checked":true,"host_pid":std::process::id(),
        "project_id":plan.project.id,"project_root":plan.project.root,"target":plan.target,
        "steps":plan.steps,"baseline":plan.baseline,"error":plan.error,"final_observation":observation,
        "apply_request_generation":plan.apply_request_generation,"apply_accepted":plan.apply_accepted,
        "apply_button_after_original_update":plan.apply_button_evidence,
        "close_cancel_selected":close_cancel::selected(world),
        "close_cancel":plan.close_cancel_evidence.clone().or_else(|| close_cancel::progress(world)),
        "function_hotpatch_selected":plan.function_hotpatch,"function_hotpatch":plan.function_hotpatch_evidence,
        "actual_runtime_proof":if plan.function_hotpatch {"same actual World/Entity/Resource/Local/Time/material + SDK epoch and independent GPU PNG"} else {"fixture system reads real PrimarySceneView FreeCamera component + EmbeddedProjectState identities"},
        "source_save":"original EditableText queue -> document history -> original Save button -> real disk save -> Backend watcher",
        "source_navigation":standalone::source_location_navigation_probe(world),
        "not_verified":["physical mouse and keyboard feel","arbitrary gameplay state migration","all project plugin author domains","large-map performance"],
        "logs":world.get_resource::<crate::project_log::ProjectLogState>().map(|logs|logs.validation_tail())});
    let path = plan.report.clone();
    plan.completion = Some(IoTaskPool::get().spawn(async move {
        let bytes = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
        std::fs::OpenOptions::new().write(true).create_new(true).open(path)
            .and_then(|mut file| file.write_all(&bytes)).map_err(|error| error.to_string())
    }));
    plan.advance(Phase::Report);
}
