//! 原停止／模式／历史按钮驱动和只读生命周期验收；不直接调用停止函数或写入完成状态。

use super::{Completion, ENVIRONMENT, REPORT_FILE, PURPOSE, RELEASE_FILE, evidence, fixture, resources::{HistorySettlement, Phase, Plan}};
use super::super::super::{EmbeddedStatus, resources::{Runtime, Settings}};
use crate::viewport_mode::{ViewportMode, ViewportModeState};
use bevy::{prelude::*, tasks::{IoTaskPool, futures::check_ready}};
use feathrise_editor_map::standalone::{self, embedded_snapshot, embedded_validation as author};
use serde_json::{Value, json};
use std::{path::{Path, PathBuf}, time::{Duration, Instant}};

const POSITION: [f32; 3] = [7., 1., -2.];

pub(crate) fn install(app: &mut App, root: &Path) -> Result<Option<Completion>, String> {
    let Some(directory) = std::env::var_os(ENVIRONMENT).map(PathBuf::from) else { return Ok(None); };
    let (project, owner) = fixture::owned(root, &directory)?;
    if directory.join(REPORT_FILE).exists() || ["FEATHRISE_EMBEDDED_SMOKE_DIR", "FEATHRISE_CODE_ITERATION_REPORT",
        "FEATHRISE_EXPORT_VALIDATION_REPORT", "FEATHRISE_PLUGIN_SMOKE_DIR", "NEW_EDITOR_NATIVE_SMOKE_DIR"]
        .iter().any(|name|std::env::var_os(name).is_some()) {
        return Err("停止编译验收要求独占新报告及单一显式场景".into());
    }
    let target = app.world().resource::<Settings>().target.canonicalize().map_err(|error|error.to_string())?;
    let preferences = std::env::var_os("FEATHRISE_PREFERENCES_FILE").map(PathBuf::from).ok_or("停止编译验收缺少隔离偏好路径")?;
    if owner["target"] != json!(target) || owner["preferences"] != json!(preferences)
        || preferences.parent() != Some(directory.as_path())
        || fixture::read_json(&preferences.with_extension("viewport.json"))? != json!({"format":1,"mode":"embedded_world"})
        || app.world().resource::<ViewportModeState>().selected != ViewportMode::EmbeddedWorld
        || app.world().resource::<ViewportModeState>().revision != 1 {
        return Err("停止编译验收必须由本轮工程3D偏好直接打开，不能注入初始模式请求替代".into());
    }
    author::install(app);
    // 仅自有实窗验收保持连续更新，避免后台焦点策略让60帧观察依赖桌面焦点偶然性。
    app.insert_resource(bevy::winit::WinitSettings::continuous());
    let completion = Completion::new(directory.clone());
    app.insert_resource(Plan { project, directory, target, nonce:owner["nonce"].as_str().ok_or("缺少本轮随机身份")?.into(),
        phase:Phase::Open, changed:Instant::now(), started:Instant::now(), frames:0, pulse:None, initial:None,
        author:None, presentation:None, history_settlement:None, original_session:None, control:None, witness:None,
        witness_control:crate::build_process::BuildControl::default(), build_io:None,
        io:None, screenshot:None, writer:None, steps:Vec::new(), error:None, completion:completion.clone() })
        .add_systems(Last, drive);
    println!("FEATHRISE_EMBEDDED_BUILD_CANCEL_START {}", json!({"project_root":root,"directory":owner["directory"],"nonce":owner["nonce"]}));
    Ok(Some(completion))
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut plan: Mut<Plan>| {
        if let Some(pulse) = plan.pulse.take() { author::release_pulse(world, pulse); }
        plan.frames = plan.frames.saturating_add(1);
        if plan.phase == Phase::Finished { return; }
        if plan.phase == Phase::Write {
            if let Some(result) = plan.writer.as_mut().and_then(check_ready) {
                plan.writer = None;
                if let Err(error) = result {
                    eprintln!("FEATHRISE_EMBEDDED_BUILD_CANCEL_WRITE_FAILED {error}");
                    world.write_message(AppExit::error());
                    plan.advance(Phase::Finished);
                } else if let Err(error) = crate::native_project_close::embedded_validation::request(world) {
                    eprintln!("FEATHRISE_EMBEDDED_BUILD_CANCEL_CLOSE_FAILED {error}");
                    world.write_message(AppExit::error());
                    plan.advance(Phase::Finished);
                } else { plan.advance(Phase::Closing); }
            }
            return;
        }
        if plan.phase == Phase::Closing {
            if let Ok(Some((button,_))) = evidence::original_button(world, "退出编辑器") {
                evidence::pulse(world, button); plan.pulse = Some(button);
            } else if let Ok(Some(button)) = crate::native_project_close::embedded_validation::discard(world) { plan.pulse = Some(button); }
            if plan.changed.elapsed() > Duration::from_secs(180) {
                eprintln!("FEATHRISE_EMBEDDED_BUILD_CANCEL_CLOSE_TIMEOUT");
                world.write_message(AppExit::error());
                plan.advance(Phase::Finished);
            }
            return;
        }
        if plan.phase == Phase::FailureDrain {
            plan.witness_control.cancel();
            if let Some(active) = world.non_send::<Runtime>().active.as_ref() { active.control.cancel(); }
            if world.non_send::<Runtime>().active.is_none() || plan.changed.elapsed() > Duration::from_secs(30) { report(&mut plan); }
            return;
        }
        let budget = if matches!(plan.phase, Phase::Build | Phase::Guest | Phase::StartingRetry) { 1500 }
            else if matches!(plan.phase, Phase::Drain | Phase::DrainRetry) { 30 } else { 180 };
        let result = if plan.changed.elapsed() > Duration::from_secs(budget) { Err(format!("停止编译验收阶段{:?}超过{budget}秒", plan.phase)) }
            else { advance(world, &mut plan) };
        if let Err(error) = result {
            eprintln!("FEATHRISE_EMBEDDED_BUILD_CANCEL_FAILED {}", json!({"phase":format!("{:?}",plan.phase),"error":error,"status":status(world)}));
            plan.error = Some(error);
            plan.witness_control.cancel();
            plan.advance(Phase::FailureDrain);
        }
    });
}

fn advance(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    if let Some(error) = &world.resource::<EmbeddedStatus>().error { return Err(format!("真实工程视口失败：{error}")); }
    if world.non_send::<Runtime>().cleanup_failure.is_some() { return Err("真实常驻工作槽未确认清理，停止验收不能继续".into()); }
    match plan.phase {
        Phase::Open if plan.frames >= 30 => {
            let state = world.resource::<ViewportModeState>();
            let status = world.resource::<EmbeddedStatus>();
            if state.selected != ViewportMode::EmbeddedWorld || state.revision != 1 { return Err("打开时没有保持工程3D个人偏好".into()); }
            if !status.active { return Ok(()); }
            if status.ready || status.starts != 1 || status.completed != 0 { return Err("尚未取得首轮真实编译窗口，不能把旧实例计为本轮启动".into()); }
            record(world, "opened_with_embedded_preference", json!({"initial_mode_input_injected":false}), plan);
            standalone::request_open_map(world, "M1-SAMPLE")?;
            plan.advance(Phase::Map);
        }
        Phase::Map if standalone::preview_ready_for(world, "M1-SAMPLE") => {
            plan.initial = Some(author_value(world)?);
            author::request_cuboid(world, POSITION)?;
            plan.advance(Phase::Draft);
        }
        Phase::Draft if plan.changed.elapsed() >= Duration::from_secs(1) => {
            let current = author::probe(world)?;
            let initial = plan.initial.as_ref().ok_or("缺少原创建前基线")?;
            if current.placements.len() != initial["placements"].as_array().ok_or("创建前摆放实例缺失")?.len() + 1
                || current.undo_len != initial["undo_len"].as_u64().ok_or("创建前历史缺失")? as usize + 1
                || !current.dirty || current.last_added_id.is_none() { return Ok(()); }
            if json!(current.save_sequence) != initial["save_sequence"] { return Err("未保存夹具创建意外触发保存".into()); }
            if !super::super::presentation_evidence::mode_ready(world, false)? { return Ok(()); }
            plan.author = Some(serde_json::to_value(current).map_err(|error|error.to_string())?);
            plan.presentation = Some(serde_json::to_value(author::presentation_probe(world)?).map_err(|error|error.to_string())?);
            record(world, "unsaved_author_draft_prepared", json!({"saved_author":plan.initial,"creation":"original CuboidCreationRequest"}), plan);
            let (root,target,project,nonce) = (plan.project.root.clone(),plan.target.clone(),plan.project.id.clone(),plan.nonce.clone());
            let control = plan.witness_control.clone();
            plan.build_io = Some(IoTaskPool::get().spawn(async move { evidence::build_witness(root,target,project,nonce,control) }));
            plan.advance(Phase::Build);
        }
        Phase::Build => {
            if let Some(result) = plan.build_io.as_mut().and_then(check_ready) {
                plan.build_io = None;
                plan.witness = Some(result?);
                plan.advance(Phase::CaptureBuild);
            }
        }
        Phase::CaptureBuild => {
            ensure_author(world, plan)?;
            ensure_real_build(world, plan)?;
            let Some((_,button)) = evidence::original_button(world, "停止编译")? else { return Ok(()); };
            if let Some(capture) = evidence::screenshot(world, plan, "compiling-stop-visible.png")? {
                record(world, "real_cargo_build_script_running_and_stop_button_gpu", json!({"capture":capture,
                    "button":button,"build":plan.witness.as_ref().unwrap().proof}), plan);
                plan.advance(Phase::Stop);
            }
        }
        Phase::Stop => {
            ensure_author(world, plan)?;
            ensure_real_build(world, plan)?;
            let Some((button,ui)) = evidence::original_button(world, "停止编译")? else { return Ok(()); };
            let runtime = world.non_send::<Runtime>();
            let active = runtime.active.as_ref().ok_or("原停止前真实工作槽消失")?;
            let control = active.control.clone();
            let session = runtime.identity.as_ref().ok_or("原停止前真实会话身份消失")?.session;
            plan.original_session = Some(session);
            plan.control = Some(control);
            record(world, "original_stop_compilation_pressed", json!({"input":"original visible StopCompilation Button + EditorButtonClick",
                "button":ui,"control_cancelled_before":false,"thread_finished_before":false,
                "session":session,"build":plan.witness.as_ref().unwrap().proof}), plan);
            evidence::pulse(world, button); plan.pulse = Some(button);
            plan.advance(Phase::Drain);
        }
        Phase::Drain => {
            ensure_author(world, plan)?;
            ensure_project_selected(world)?;
            let control = plan.control.as_ref().ok_or("取消前控制牌未绑定")?;
            let runtime = world.non_send::<Runtime>();
            if !control.is_cancelled() || !runtime.compilation_stopped {
                if plan.changed.elapsed() > Duration::from_secs(5) { return Err("原停止按钮未取消精确控制牌".into()); }
                return Ok(());
            }
            if runtime.identity.is_some() || runtime.pending_play || runtime.restart_requested || world.resource::<EmbeddedStatus>().ready {
                return Err("停止后仍采用旧身份／Ready或保留自动重启意图".into());
            }
            if runtime.active.is_some() || world.resource::<EmbeddedStatus>().active { return Ok(()); }
            if control.cleanup_failed() || world.resource::<EmbeddedStatus>().completed != 1 || world.resource::<EmbeddedStatus>().starts != 1 {
                return Err("原取消未真实归还唯一线程槽，或清理失败".into());
            }
            let process = evidence::process_exit(plan.witness.as_ref().ok_or("缺少精确真实编译进程句柄")?)?.ok_or("工作槽归还时真实Cargo构建脚本尚未退出")?;
            record(world, "cancelled_cargo_worker_and_process_really_drained", json!({"control_cancelled":true,"cleanup_failed":false,
                "build_process":process,"compilation_stopped":runtime.compilation_stopped,"identity":runtime.identity,
                "pending_play":runtime.pending_play,"restart_requested":runtime.restart_requested}), plan);
            plan.advance(Phase::CaptureStopped);
        }
        Phase::CaptureStopped => {
            ensure_author(world, plan)?;
            ensure_stopped_idle(world, ViewportMode::EmbeddedWorld)?;
            ensure_project_selected(world)?;
            if evidence::original_button(world, "停止编译")?.is_some() { return Err("真实收口后停止编译按钮仍可点击".into()); }
            if let Some(capture) = evidence::screenshot(world, plan, "compilation-stopped.png")? {
                record(world, "actual_stopped_ui_gpu", json!({"capture":capture}), plan);
                plan.advance(Phase::StableStopped);
            }
        }
        Phase::StableStopped => {
            ensure_author(world, plan)?;
            ensure_stopped_idle(world, ViewportMode::EmbeddedWorld)?;
            ensure_project_selected(world)?;
            // 原停止只撤销构建；持续观察保留的工程3D，并确认原“预览”入口已恢复可用。
            if plan.changed.elapsed() >= Duration::from_secs(3) && plan.frames >= 60 {
                if let Some((_,button)) = evidence::original_button(world, "预览")? {
                    record(world, "project_3d_stable_without_automatic_restart", json!({"observed_frames":plan.frames,
                        "observed_ms":plan.changed.elapsed().as_secs_f64()*1000.,"retry_button":button}), plan);
                    plan.advance(Phase::Local);
                }
            }
        }
        Phase::Local => {
            ensure_author(world, plan)?;
            ensure_stopped_idle(world, ViewportMode::EmbeddedWorld)?;
            ensure_project_selected(world)?;
            if let Some((button,ui)) = evidence::original_button(world, "编辑器 3D")? {
                record(world, "original_editor_3d_selected", json!({"input":"original Editor3D Button + EditorButtonClick","button":ui}), plan);
                evidence::pulse(world, button); plan.pulse = Some(button);
                plan.advance(Phase::WaitLocal);
            }
        }
        Phase::WaitLocal if world.resource::<ViewportModeState>().selected == ViewportMode::SavedWorld
            && standalone::preview_ready_for(world, "M1-SAMPLE") => {
            ensure_author(world, plan)?;
            ensure_local_idle(world)?;
            if super::super::presentation_evidence::mode_ready(world, false)? { plan.advance(Phase::StableLocal); }
        }
        Phase::StableLocal => {
            ensure_author(world, plan)?;
            ensure_local_idle(world)?;
            if plan.changed.elapsed() >= Duration::from_secs(3) && plan.frames >= 60 {
                if let Some(button) = author::pulse_history(world, false)? {
                    let presentation = author::presentation_probe(world)?;
                    record(world, "editor_3d_stable_without_automatic_restart", json!({"observed_frames":plan.frames,
                        "observed_ms":plan.changed.elapsed().as_secs_f64()*1000.,"presentation":presentation}), plan);
                    plan.pulse = Some(button); plan.advance(Phase::Undo);
                }
            }
        }
        Phase::Undo => {
            ensure_local_idle(world)?;
            let current = author_value(world)?;
            let before = plan.author.as_ref().ok_or("缺少未保存草稿基线")?;
            if current["placements"].as_array().is_some_and(|values|values.len() + 1 == before["placements"].as_array().unwrap().len())
                && current["undo_len"].as_u64().unwrap_or(0) + 1 == before["undo_len"].as_u64().unwrap_or(0)
                && current["redo_len"].as_u64().unwrap_or(0) == before["redo_len"].as_u64().unwrap_or(0) + 1 {
                if current["save_sequence"] != before["save_sequence"] { return Err("停止后的原撤销触发保存".into()); }
                if let Some(button) = author::pulse_history(world, true)? {
                    record(world, "original_editor_undo_after_stop", json!({"input":"original Undo Button + EditorButtonClick"}), plan);
                    plan.pulse = Some(button); plan.advance(Phase::Redo);
                }
            }
        }
        Phase::Redo => {
            ensure_local_idle(world)?;
            let current = author_value(world)?;
            let expected = plan.author.as_ref().ok_or("缺少重做恢复基线")?;
            if ["placements", "dirty", "save_sequence", "undo_len", "redo_len", "last_added_id"].iter().all(|field|current[*field] == expected[*field]) {
                if current["revision"].as_u64().unwrap_or(0) <= expected["revision"].as_u64().unwrap_or(0) { return Err("撤销／重做没有产生真实作者新修订".into()); }
                plan.author = Some(current);
                let presentation = history_presentation(world, plan)?;
                plan.history_settlement = Some(HistorySettlement { presentation:presentation.clone(), stable_since:Instant::now(),
                    stable_frames:1, transitions:vec![json!({"elapsed_ms":plan.started.elapsed().as_secs_f64()*1000.,
                        "selection":presentation["selection"]})] });
                // 原撤销先删除作者记录，旧场景树签名可能在重做首帧之后才撤销悬空选择。
                // 这里只认可作者修订已经采用；不能把该帧尚在收口的呈现当作重试恢复基线。
                plan.advance(Phase::RedoSettle);
            }
        }
        Phase::RedoSettle => {
            ensure_local_idle(world)?;
            if plan.author.as_ref() != Some(&author_value(world)?) {
                return Err("原重做采用后作者记录、修订或历史仍在变化，不能冻结恢复基线".into());
            }
            let presentation = history_presentation(world, plan)?;
            let ready = standalone::preview_ready_for(world, "M1-SAMPLE") && standalone::saved_play_blocker(world).is_none()
                && super::super::presentation_evidence::mode_ready(world, false)?;
            let settlement = plan.history_settlement.as_mut().ok_or("缺少原历史呈现收口观测")?;
            if settlement.presentation != presentation {
                if settlement.transitions.len() >= 8 { return Err("原历史后的选择持续变化，超过有界收口观测预算".into()); }
                settlement.transitions.push(json!({"elapsed_ms":plan.started.elapsed().as_secs_f64()*1000.,
                    "selection":presentation["selection"]}));
                settlement.presentation = presentation;
                settlement.stable_since = Instant::now();
                settlement.stable_frames = 0;
            }
            if !ready {
                settlement.stable_since = Instant::now();
                settlement.stable_frames = 0;
                return Ok(());
            }
            settlement.stable_frames = settlement.stable_frames.saturating_add(1);
            if settlement.stable_frames >= 8 && settlement.stable_since.elapsed() >= Duration::from_millis(250) {
                let proof = json!({"ready":true,"stable_frames":settlement.stable_frames,
                    "stable_ms":settlement.stable_since.elapsed().as_secs_f64()*1000.,"selection_transitions":settlement.transitions,
                    "before_history":plan.presentation,"after_history":settlement.presentation});
                // 明确的原Undo/Redo已经完成并稳定；后续停止/重试仍严格保持这个实际作者现场。
                plan.presentation = Some(settlement.presentation.clone());
                record(world, "original_editor_redo_after_stop", json!({"input":"original Redo Button + EditorButtonClick",
                    "history_settlement":proof}), plan);
                plan.advance(Phase::CaptureLocal);
            }
        }
        Phase::CaptureLocal => {
            ensure_author(world, plan)?;
            ensure_local_idle(world)?;
            if let Some(capture) = evidence::screenshot(world, plan, "editor3d-usable.png")? {
                record(world, "editor_3d_usable_actual_gpu", json!({"capture":capture}), plan);
                plan.advance(Phase::Release);
            }
        }
        Phase::Release => {
            ensure_local_idle(world)?;
            if let Some(result) = plan.io.as_mut().and_then(check_ready) {
                plan.io = None;
                record(world, "owned_retry_gate_released_after_drain", result?.ok_or("重试放行缺少真实回执")?, plan);
                plan.advance(Phase::Retry);
            } else if plan.io.is_none() {
                let (root,directory) = (plan.project.root.clone(),plan.directory.clone());
                plan.io = Some(IoTaskPool::get().spawn(async move {
                    fixture::owned(&root, &directory)?;
                    fixture::write_bytes_new(&root.join(RELEASE_FILE), b"release only after cancelled worker and exact process drained")?;
                    Ok(Some(json!({"source":"owned fixture-only build.rs retry release","after_real_drain":true})))
                }));
            }
        }
        Phase::Retry => {
            ensure_author(world, plan)?;
            ensure_local_idle(world)?;
            if let Some((button,ui)) = evidence::original_button(world, "工程 3D")? {
                record(world, "original_project_3d_retry_pressed", json!({"input":"original Project3D Button + EditorButtonClick","button":ui}), plan);
                evidence::pulse(world, button); plan.pulse = Some(button);
                plan.advance(Phase::StartingRetry);
            }
        }
        Phase::StartingRetry => {
            let status = world.resource::<EmbeddedStatus>();
            let runtime = world.non_send::<Runtime>();
            if status.active && status.starts == 2 && runtime.active.is_some() {
                let active = runtime.active.as_ref().unwrap();
                let session = runtime.identity.as_ref().ok_or("重试缺少新会话身份")?.session;
                if active.control.is_cancelled() || runtime.compilation_stopped || Some(session) == plan.original_session || status.completed != 1 {
                    return Err("显式重试沿用取消控制牌、旧会话或未归还的旧工作槽".into());
                }
                record(world, "explicit_retry_started_new_real_slot", json!({"session":session,"control_cancelled":false}), plan);
                plan.advance(Phase::Guest);
            }
        }
        Phase::Guest => {
            ensure_author(world, plan)?;
            let status = world.resource::<EmbeddedStatus>();
            let (pid, revision) = (status.pid, status.applied_revision);
            let capture = world.resource::<embedded_snapshot::EmbeddedSnapshotState>();
            if !status.ready || status.playing || status.applied_revision == 0 || !embedded_snapshot::ready_is_current(world)
                || capture.ready.as_ref().is_none_or(|snapshot|status.applied_revision < snapshot.author_revision)
                || plan.changed.elapsed() < Duration::from_secs(2) || !super::super::presentation_evidence::mode_ready(world, false)? { return Ok(()); }
            if let Some(result) = plan.io.as_mut().and_then(check_ready) {
                plan.io = None;
                if let Some(guest) = result? {
                    record(world, "retry_adopted_preserved_author_and_real_gpu", json!({"host_window_region":guest["window"],"guest":guest}), plan);
                    plan.advance(Phase::CaptureRetry);
                }
            } else if plan.io.is_none() && plan.frames % 10 == 0 {
                let (directory,expected,geometry) = (plan.directory.clone(),
                    plan.author.as_ref().unwrap().clone(),world.resource::<crate::viewport_mode::ViewportGeometry>().clone());
                plan.io = Some(IoTaskPool::get().spawn(async move { evidence::guest_capture(directory,pid,revision,expected,geometry) }));
            }
        }
        Phase::CaptureRetry => {
            ensure_author(world, plan)?;
            if let Some(capture) = evidence::screenshot(world, plan, "retry-engine-ready.png")? {
                record(world, "retry_host_ui_gpu", json!({"capture":capture,"scope":"host only; actual project pixels in retry-game.png"}), plan);
                plan.advance(Phase::ReturnLocal);
            }
        }
        Phase::ReturnLocal => {
            ensure_author(world, plan)?;
            if let Some((button,_)) = evidence::original_button(world, "编辑器 3D")? {
                evidence::pulse(world, button); plan.pulse = Some(button);
                plan.advance(Phase::DrainRetry);
            }
        }
        Phase::DrainRetry if world.resource::<ViewportModeState>().selected == ViewportMode::SavedWorld
            && !world.resource::<EmbeddedStatus>().active && world.non_send::<Runtime>().active.is_none()
            && standalone::preview_ready_for(world, "M1-SAMPLE") => {
            ensure_author(world, plan)?;
            if world.resource::<EmbeddedStatus>().starts != 2 || world.resource::<EmbeddedStatus>().completed != 2 { return Err("重试返回后槽位计数不对应两次真实任务".into()); }
            record(world, "back_to_editor_3d_and_retry_worker_drained", json!({}), plan);
            report(plan);
        }
        _ => (),
    }
    Ok(())
}

fn ensure_real_build(world: &World, plan: &Plan) -> Result<(), String> {
    let runtime = world.non_send::<Runtime>();
    let status = world.resource::<EmbeddedStatus>();
    let active = runtime.active.as_ref().ok_or("原停止前真实编译工作槽丢失")?;
    if !status.active || status.ready || status.starts != 1 || status.completed != 0 || active.thread.is_finished()
        || active.control.is_cancelled() || evidence::process_exit(plan.witness.as_ref().ok_or("真实Cargo脚本句柄缺失")?)?.is_some() {
        return Err("原停止前真实Cargo/工作线程不再运行，不能以迟到取消计成功".into());
    }
    Ok(())
}

fn ensure_local_idle(world: &World) -> Result<(), String> {
    ensure_stopped_idle(world, ViewportMode::SavedWorld)
}

fn ensure_project_selected(world: &World) -> Result<(), String> {
    let selected = world.resource::<ViewportModeState>();
    if selected.selected != ViewportMode::EmbeddedWorld || selected.revision != 1 {
        return Err("原停止按钮改变了工程3D选择或个人偏好修订".into());
    }
    Ok(())
}

fn ensure_stopped_idle(world: &World, selected: ViewportMode) -> Result<(), String> {
    let status = world.resource::<EmbeddedStatus>();
    let runtime = world.non_send::<Runtime>();
    if world.resource::<ViewportModeState>().selected != selected || status.active || runtime.active.is_some()
        || status.starts != 1 || status.completed != 1 || !runtime.compilation_stopped || runtime.pending_play || runtime.restart_requested {
        return Err("停止后的观察期出现模式误切、自动重启、未收口槽位或旧运行意图".into());
    }
    Ok(())
}

fn author_value(world: &World) -> Result<Value, String> { serde_json::to_value(author::probe(world)?).map_err(|error|error.to_string()) }

fn history_presentation(world: &mut World, plan: &Plan) -> Result<Value, String> {
    let current = serde_json::to_value(author::presentation_probe(world)?).map_err(|error|error.to_string())?;
    let before = plan.presentation.as_ref().ok_or("缺少原历史前呈现基线")?;
    for field in ["roots", "headers", "camera", "navigation", "panes", "layout"] {
        if current[field] != before[field] { return Err(format!("原Undo/Redo意外改变作者{field}现场；预期={}，实际={}", before[field], current[field])); }
    }
    for field in ["map", "directory_map", "portal", "view", "orthographic"] {
        if current["selection"][field] != before["selection"][field] {
            return Err(format!("原Undo/Redo意外改变作者选择的{field}；预期={}，实际={}", before["selection"][field], current["selection"][field]));
        }
    }
    if !current["selection"]["placement"].is_null() && current["selection"]["placement"] != before["selection"]["placement"] {
        return Err(format!("原Undo/Redo选择了不属于已删恢复对象的其他实例；预期={}或null，实际={}",
            before["selection"]["placement"], current["selection"]["placement"]));
    }
    Ok(current)
}

fn ensure_author(world: &mut World, plan: &Plan) -> Result<(), String> {
    if plan.author.as_ref() != Some(&author_value(world)?) { return Err("停止编译或显式重试修改了未保存作者草稿、修订或历史".into()); }
    let current = serde_json::to_value(author::presentation_probe(world)?).map_err(|error|error.to_string())?;
    let before = plan.presentation.as_ref().ok_or("缺少作者现场呈现基线")?;
    for field in ["camera", "navigation", "selection", "panes", "layout"] {
        if current[field] != before[field] { return Err(format!("停止编译或重试改变作者{field}现场；预期={}，实际={}", before[field], current[field])); }
    }
    Ok(())
}

fn status(world: &World) -> Value {
    let status = world.resource::<EmbeddedStatus>();
    json!({"active":status.active,"ready":status.ready,"pid":status.pid,"world_id":status.world_id,
        "playing":status.playing,"applied_revision":status.applied_revision,"frames":status.frames,"ticks":status.ticks,
        "starts":status.starts,"completed":status.completed,"error":status.error})
}

fn record(world: &World, stage: &str, extra: Value, plan: &mut Plan) {
    let state = world.resource::<ViewportModeState>();
    let mut proof = json!({"stage":stage,"selected":state.selected,"selection_revision":state.revision,"status":status(world),
        "runtime_slot_present":world.non_send::<Runtime>().active.is_some(),"author":author::probe(world).ok(),
        "pid":world.resource::<EmbeddedStatus>().pid,"world_id":world.resource::<EmbeddedStatus>().world_id,
        "applied_revision":world.resource::<EmbeddedStatus>().applied_revision,
        "mode":if world.resource::<EmbeddedStatus>().playing {"playing"} else {"editing"},
        "elapsed_ms":plan.started.elapsed().as_secs_f64()*1000.});
    // 工程实例就绪不能替代独立代码监听的状态；只读摘取本阶段真实错误，不清理或覆盖失败投影。
    if let Some(code) = world.get_resource::<crate::code_iteration::CodeIterationState>() {
        let diagnostics: Vec<_> = code.diagnostics.iter().take(8).map(|diagnostic| json!({
            "level":diagnostic.level,"message":bounded_evidence(&diagnostic.message, 1024),
            "message_truncated":diagnostic.message.len() > 1024,"code":diagnostic.code,"location":diagnostic.location,
        })).collect();
        proof["code_iteration"] = json!({"phase":format!("{:?}", code.phase),
            "status":bounded_evidence(&code.status, 8 * 1024),"status_truncated":code.status.len() > 8 * 1024,
            "diagnostic_revision":code.diagnostic_revision,"diagnostic_count":code.diagnostics.len(),"diagnostics":diagnostics});
    }
    if let Some(logs) = world.get_resource::<crate::project_log::ProjectLogState>() {
        let mut tail = logs.validation_tail();
        if let Some(lines) = tail["lines"].as_array_mut() {
            let excess = lines.len().saturating_sub(16);
            lines.drain(..excess);
        }
        proof["project_log_tail"] = tail;
    }
    if let Some(extra) = extra.as_object() { proof.as_object_mut().unwrap().extend(extra.clone()); }
    plan.steps.push(proof);
    println!("FEATHRISE_EMBEDDED_BUILD_CANCEL_STEP {stage}");
}

fn bounded_evidence(text: &str, maximum: usize) -> &str {
    let mut end = text.len().min(maximum);
    while !text.is_char_boundary(end) { end -= 1; }
    &text[..end]
}

fn report(plan: &mut Plan) {
    let passed = plan.error.is_none() && plan.steps.last().is_some_and(|step|step["stage"] == "back_to_editor_3d_and_retry_worker_drained");
    let report = json!({"schema":1,"scope":PURPOSE,"passed":passed,"project_id":plan.project.id,"project_root":plan.project.root,
        "directory":plan.directory,"nonce":plan.nonce,"host_pid":std::process::id(),"steps":plan.steps,"error":plan.error,
        "physical_input_verified":false,"host_screenshot_scope":"parent GPU only; separate immutable retry-game.png captures actual project pixels"});
    let path = plan.directory.join(REPORT_FILE);
    plan.writer = Some(IoTaskPool::get().spawn(async move { fixture::write_new(&path, &report) }));
    plan.advance(Phase::Write);
}
