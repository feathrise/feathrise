//! 自有工程的关闭取消专项：只延迟最后退出，读取真实后台退役与恢复，再走原保存、诊断和应用链。

use super::{driver, evidence, resources::{Phase, Plan}};
use crate::{
    code_iteration::{CodeIterationState, resources::{BackendLifecycle, CodeIterationRuntime}, types::CodeIterationPhase},
    embedded_viewport::EmbeddedStatus,
    native_project_close::embedded_validation as close,
    viewport_mode::{RequestViewportMode, ViewportMode, ViewportModeState},
};
use bevy::prelude::*;
use feathrise_editor_scene::standalone;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const NOTE: &str = "src/author_notes.rs";
const CONTROLLER: &str = "src/controller.rs";

#[derive(Resource, Default)]
pub(super) struct CloseCancelCheck {
    hold: bool,
    baseline: Option<Value>,
    original_pid: u32,
    original_world: u64,
    initial_generation: u64,
    retired_generation: u64,
    resumed_generation: u64,
    cancelled: Option<Instant>,
    proofs: Vec<Value>,
}

pub(super) fn selected(world: &World) -> bool { world.contains_resource::<CloseCancelCheck>() }
pub(super) fn held(world: &World) -> bool {
    world.get_resource::<CloseCancelCheck>().is_some_and(|check| check.hold)
}

pub(super) fn progress(world: &World) -> Option<Value> {
    let check = world.get_resource::<CloseCancelCheck>()?;
    Some(json!({"selected":true,"passed":false,"hold":check.hold,
        "retired_generation":check.retired_generation,"resumed_generation":check.resumed_generation,
        "proofs":check.proofs}))
}

pub(super) fn begin(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    let generation = {
        let runtime = world.non_send::<CodeIterationRuntime>();
        if runtime.lifecycle != BackendLifecycle::Running { return Err("关闭专项开始时自动编译未处于运行态".into()); }
        runtime.backend.as_ref().ok_or("关闭专项缺少真实自动编译后台")?.current_generation()
    };
    let baseline = evidence::author_state(world)?;
    let status = world.resource::<EmbeddedStatus>();
    let (pid, world_id) = (status.pid, status.world_id);
    if !status.ready || pid == 0 || world_id == 0 { return Err("关闭专项必须从真实已连接工程开始".into()); }
    let mut check = world.resource_mut::<CloseCancelCheck>();
    check.hold = true;
    check.baseline = Some(baseline);
    check.original_pid = pid;
    check.original_world = world_id;
    check.initial_generation = generation;
    drop(check);
    close::request(world)?;
    plan.advance(Phase::CloseConfirm);
    Ok(())
}

pub(super) fn handles(phase: Phase) -> bool {
    matches!(phase, Phase::CloseConfirm | Phase::CloseDrain | Phase::CloseCancelled
        | Phase::CloseRecovered | Phase::CloseReconnectLocal | Phase::CloseSceneCapture | Phase::CloseReturnNote)
}

pub(super) fn exercise(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    world.resource_scope(|world, mut check: Mut<CloseCancelCheck>| {
        if let Some(error) = &world.non_send::<CodeIterationRuntime>().failure {
            return Err(format!("关闭取消专项遇到真实后台收口失败：{error}"));
        }
        match plan.phase {
            Phase::CloseConfirm => {
                let Some(prompt) = close::visible_prompt(world) else { return Ok(()); };
                if !evidence::screenshot(world, plan, "close-cancel-confirm.png")? { return Ok(()); }
                let Some(button) = close::discard(world)? else { return Ok(()); };
                plan.pulse = Some(button);
                check.proofs.push(json!({"stage":"original_dirty_close_confirmed","prompt":prompt}));
                driver::record(world, plan, "close_cancel_original_discard_confirmed", None)?;
                plan.advance(Phase::CloseDrain);
            }
            Phase::CloseDrain => {
                let retired = {
                    let runtime = world.non_send::<CodeIterationRuntime>();
                    if runtime.lifecycle != BackendLifecycle::Closing || runtime.backend.is_some() { return Ok(()); }
                    runtime.last_generation
                };
                let Some(prompt) = close::draining_prompt(world) else { return Ok(()); };
                if world.get_resource::<Messages<AppExit>>().is_some_and(|exits| !exits.is_empty()) {
                    return Err("后台收尾观察期间已发出退出，不能再声称可以取消".into());
                }
                if retired < check.initial_generation { return Err("后台最终退役代次倒退".into()); }
                if !evidence::screenshot(world, plan, "close-cancel-real-backend-retired.png")? { return Ok(()); }
                check.retired_generation = retired;
                let proof = json!({"stage":"real_backend_retired_before_cancel","prompt":prompt,
                    "backend_absent":true,"lifecycle":"Closing","initial_generation":check.initial_generation,
                    "retired_generation":retired,"exit_queued":false,
                    "gate":"owned validation delays only final exit after real is_finished + finish"});
                check.proofs.push(proof);
                driver::record(world, plan, "close_cancel_real_backend_retired_with_original_cancel_visible", None)?;
                plan.pulse = Some(close::cancel(world)?);
                plan.advance(Phase::CloseCancelled);
            }
            Phase::CloseCancelled => {
                if !close::idle(world) { return Ok(()); }
                check.hold = false;
                check.cancelled.get_or_insert_with(Instant::now);
                evidence::preserved(world, check.baseline.as_ref().unwrap())?;
                let note = standalone::source_location_probe(world).ok_or("取消后原脏文本不再可读")?;
                driver::verify_note(&note, plan.note_baseline.as_ref().unwrap())?;
                if !evidence::screenshot(world, plan, "close-cancel-author-restored.png")? { return Ok(()); }
                check.proofs.push(json!({"stage":"original_cancel_returned_to_edit","source":note,
                    "author_and_documents_preserved":true,"close_idle_after_release":true}));
                driver::record(world, plan, "close_cancel_original_cancel_preserved_author_and_text", None)?;
                plan.advance(Phase::CloseRecovered);
            }
            Phase::CloseRecovered => {
                let runtime = world.non_send::<CodeIterationRuntime>();
                if runtime.lifecycle != BackendLifecycle::Running || runtime.backend.is_none() {
                    if check.cancelled.is_some_and(|cancelled| cancelled.elapsed() > Duration::from_secs(10)) {
                        return Err("取消关闭后10秒仍未重建自动编译后台".into());
                    }
                    return Ok(());
                }
                let state = world.resource::<CodeIterationState>();
                if state.phase == CodeIterationPhase::Failed {
                    return Err(format!("取消后的当前源码补偿编译失败：{}", state.status));
                }
                let Some(candidate) = state.pending.as_ref() else { return Ok(()); };
                if state.phase != CodeIterationPhase::Ready || state.generation <= check.retired_generation
                    || candidate.identity.generation != state.generation
                    || !runtime.backend.as_ref().unwrap().is_current(&candidate.identity) { return Ok(()); }
                check.resumed_generation = state.generation;
                let generation = state.generation;
                let profile = format!("{:?}", runtime.profile);
                if !driver::pulse_source(world, plan, NOTE, standalone::SourceLocationAction::Scene)? { return Ok(()); }
                let proof = json!({"stage":"latest_saved_source_rebuilt_after_resume",
                    "generation":generation,"retired_generation":check.retired_generation,"profile":profile,
                    "candidate_matches_new_backend":true});
                check.proofs.push(proof);
                driver::record(world, plan, "close_cancel_compensation_build_ready_in_new_generation", None)?;
                // 已结束的运行实例沿原显式重连入口恢复。两次选择必须跨帧，原处理器每帧只取最后请求。
                world.write_message(RequestViewportMode(ViewportMode::SavedWorld));
                plan.advance(Phase::CloseReconnectLocal);
            }
            Phase::CloseReconnectLocal => {
                let mode = world.resource::<ViewportModeState>();
                if mode.selected != ViewportMode::SavedWorld || world.resource::<EmbeddedStatus>().active { return Ok(()); }
                let revision = mode.revision;
                world.write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
                check.proofs.push(json!({"stage":"explicit_original_viewport_reconnect_requested",
                    "confirmed_mode":"SavedWorld","confirmed_selection_revision":revision,
                    "old_runtime_inactive":true,"requested_mode":"EmbeddedWorld",
                    "route":"original RequestViewportMode SavedWorld then EmbeddedWorld in separate frames",
                    "automatic_reconnect_claimed":false}));
                driver::record(world, plan, "close_cancel_original_viewport_reconnect_requested", None)?;
                plan.advance(Phase::CloseSceneCapture);
            }
            Phase::CloseSceneCapture => {
                if world.resource::<ViewportModeState>().selected != ViewportMode::EmbeddedWorld { return Ok(()); }
                if !standalone::preview_ready_for(world, "M1-SAMPLE") || !evidence::current_snapshot(world) { return Ok(()); }
                let Some(proof) = evidence::capture(world, plan, 12.)? else { return Ok(()); };
                let status = world.resource::<EmbeddedStatus>();
                let (pid, world_id, frame) = (status.pid, status.world_id, status.frames);
                if pid == check.original_pid || world_id == check.original_world {
                    return Err("已经真实退役的旧工程被冒充为取消后重连的工程".into());
                }
                if !evidence::screenshot(world, plan, "close-cancel-reconnected-editor.png")? { return Ok(()); }
                // 关闭已真实退役旧运行实例。只在新实例实际组件/GPU通过后更新后续诊断与应用链的基线。
                plan.old_pid = pid;
                plan.old_world = world_id;
                plan.old_frame = frame;
                let proof = json!({"stage":"new_runtime_reconnected_with_same_saved_code",
                    "original_pid":check.original_pid,"original_world":check.original_world,
                    "recovered_pid":pid,"recovered_world":world_id,"proof":proof});
                check.proofs.push(proof);
                driver::record(world, plan, "close_cancel_reconnected_real_speed_12_component_and_gpu", None)?;
                driver::open_source(world, plan, NOTE);
                plan.advance(Phase::CloseReturnNote);
            }
            Phase::CloseReturnNote => {
                driver::source_wait(world, plan);
                let Some(note) = standalone::source_location_probe(world) else { return Ok(()); };
                if note["path"] != NOTE || note["native_matches"] != true { return Ok(()); }
                driver::verify_note(&note, plan.note_baseline.as_ref().unwrap())?;
                plan.close_cancel_evidence = Some(json!({"selected":true,"passed":true,
                    "original_session":{"pid":check.original_pid,"world_id":check.original_world},
                    "retired_generation":check.retired_generation,"resumed_generation":check.resumed_generation,
                    "proofs":check.proofs,"later_source_flow":"requires original saved error, location, fixed candidate and actual application phases"}));
                driver::open_source(world, plan, CONTROLLER);
                plan.advance(Phase::ControllerOpen);
            }
            _ => (),
        }
        Ok(())
    })
}

/// 专项失败仍先通过原取消入口撤回在途关闭，防止退出先于失败报告落盘；不伪造收口结果。
pub(super) fn prepare_report(world: &mut World) {
    if held(world) { crate::native_project_close::cancel_workspace_close(world); }
}

pub(super) fn release_hold(world: &mut World) {
    if let Some(mut check) = world.get_resource_mut::<CloseCancelCheck>() { check.hold = false; }
}
