//! 正式预览函数体热更新专项：原保存／诊断／应用按钮、真实原生HUD返回，以及同实例运行状态和新代次GPU。
//! 仅本轮独占夹具安装探针；文件、PNG解码和窗口核验复用唯一后台槽，作者帧不等待。

use super::{driver, evidence, resources::{Phase, Plan}};
use crate::{code_iteration::{CodeIterationState, types::CodeIterationPhase}, embedded_viewport::{EmbeddedStatus, validation as embedded}};
use bevy::{asset::RenderAssetUsages, image::{CompressedImageFormats, ImageSampler, ImageType}, prelude::*, tasks::{IoTaskPool, Task, futures::check_ready}};
use feathrise_editor_map::standalone::{self, SourceLocationAction, embedded_validation as author};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::{Path, PathBuf}, time::{Duration, Instant}};

const CONTROLLER: &str = "src/controller.rs";
const NOTE: &str = "src/author_notes.rs";
const ONE: &str = "fn live_step() -> f32 { 1.0 }";
const TWO: &str = "fn live_step() -> f32 { 2.0 }";
const THREE: &str = "fn live_step() -> f32 { 3.0 }";
// 保持首期数字表达式形状，交由真实 Rust 类型检查报 E0308；错误恢复本身也须走同一薄编译工具链。
const BAD: &str = "fn live_step() -> f32 { 1_u32 }";
const SOURCE: &str = "actual-function-hotpatch-World-Entity-Resource-Local-Time-and-material";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step { InitialMap, InitialPlaying, ReturnEditing, Controller, InvalidBuffer, SavedInvalid, Diagnostics,
    Located, FixedBuffer, SavedFixed, Candidate, ReturnNote, ReturnMap, Apply, Applied, Resumed, FinalMap }

pub(super) struct Check {
    step: Step,
    changed: Instant,
    epoch: u64,
    next: u64,
    minimum_frame: u64,
    baseline: Option<Value>,
    task: Option<Task<Result<Option<Value>, String>>>,
    retry: Instant,
    proof: Option<Value>,
    captured: Option<Value>,
    samples: Vec<Value>,
    error_evidence: Option<Value>,
    return_sent: bool,
    author: Option<Value>,
    accept_recorded: bool,
}

impl Check {
    fn advance(&mut self, step: Step) {
        self.step = step;
        self.changed = Instant::now();
        self.proof = None;
        self.captured = None;
        self.retry = Instant::now();
        println!("FEATHRISE_FUNCTION_HOTPATCH_PHASE {step:?} epoch={}", self.epoch);
    }
}

pub(super) fn begin(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    if plan.hotpatch_check.is_some() { return Err("函数体专项不得重复初始化".into()); }
    if world.resource::<EmbeddedStatus>().patch_epoch != 0 { return Err("新建正式预览实例的补丁代次不是零".into()); }
    if !driver::pulse_source(world, plan, NOTE, SourceLocationAction::Map)? { return Ok(()); }
    plan.hotpatch_check = Some(Box::new(Check { step: Step::InitialMap, changed: Instant::now(), epoch: 0, next: 2,
        minimum_frame: 0, baseline: None, task: None, retry: Instant::now(), proof: None, captured: None, samples: Vec::new(),
        error_evidence: None, return_sent: false, author: None, accept_recorded: false }));
    plan.advance(Phase::FunctionHotpatch);
    Ok(())
}

pub(super) fn exercise(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    let mut check = plan.hotpatch_check.take().ok_or("函数体专项缺少有限状态机")?;
    let result = advance(world, plan, &mut check);
    plan.hotpatch_check = Some(check);
    result
}

fn advance(world: &mut World, plan: &mut Plan, check: &mut Check) -> Result<(), String> {
    let status = world.resource::<EmbeddedStatus>();
    if !status.active || !status.ready || status.pid != plan.old_pid || status.world_id != plan.old_world || status.error.is_some() {
        return Err(format!("函数体补丁改变或中断原PID／World：{status:?}"));
    }
    let budget = if matches!(check.step, Step::SavedInvalid | Step::Candidate) { 1200 } else { 180 };
    if check.changed.elapsed() > Duration::from_secs(budget) {
        return Err(format!("函数体专项{:?}超时（return_sent={}）：{}", check.step, check.return_sent,
            world.resource::<CodeIterationState>().status));
    }
    match check.step {
        Step::InitialMap if map_ready(world) => {
            let Some(proof) = capture(world, plan, check, false, 1., 0)? else { return Ok(()); };
            check.baseline = Some(proof["runtime"].clone());
            push_sample(check, &proof)?;
            driver::record(world, plan, "function_hotpatch_initial_same_instance_and_red_gpu", Some(proof))?;
            if press_preview(world, plan)? { check.advance(Step::InitialPlaying); }
        }
        Step::InitialPlaying if world.resource::<EmbeddedStatus>().playing => {
            let Some(proof) = capture(world, plan, check, true, 1., 30)? else { return Ok(()); };
            continuity(check.baseline.as_ref().unwrap(), &proof["runtime"], true)?;
            push_sample(check, &proof)?;
            driver::record(world, plan, "function_hotpatch_playing_state_accumulated_before_edit", Some(proof.clone()))?;
            check.minimum_frame = frame(&proof)?;
            check.proof = Some(proof);
            check.return_sent = false;
            check.step = Step::ReturnEditing;
            check.captured = None;
            check.changed = Instant::now();
        }
        Step::ReturnEditing => {
            if !check.return_sent {
                let proof = check.proof.as_ref().ok_or("原生HUD返回缺少本轮窗口证明")?;
                check.return_sent = embedded::code_iteration_return_to_edit(world, &proof["window"], plan.old_pid)?;
                return Ok(());
            }
            if world.resource::<EmbeddedStatus>().playing || !map_ready(world) { return Ok(()); }
            // 原返回按钮只改变模式，不能替换实例或清空玩法状态；等本次返回后的独立GPU帧。
            let minimum_ticks = last_runtime(check)?["tick"].as_u64().unwrap_or(0);
            check.proof = None;
            let Some(proof) = capture(world, plan, check, false, (check.epoch + 1) as f64, minimum_ticks)? else { return Ok(()); };
            continuity(last_runtime(check)?, &proof["runtime"], false)?;
            push_sample(check, &proof)?;
            driver::record(world, plan, "function_hotpatch_original_hud_returned_same_runtime", Some(proof))?;
            if check.epoch == 2 { check.advance(Step::FinalMap); }
            else { driver::open_source(world, plan, CONTROLLER); check.advance(Step::Controller); }
        }
        Step::Controller => {
            let (from, to, next) = if check.epoch == 0 { (ONE, BAD, Step::InvalidBuffer) } else { (TWO, THREE, Step::FixedBuffer) };
            if driver::replace_source(world, plan, CONTROLLER, from, to)? { check.advance(next); }
        }
        Step::InvalidBuffer if driver::source_equals(world, plan, CONTROLLER, true) => {
            if driver::pulse_source(world, plan, CONTROLLER, SourceLocationAction::Save)? {
                plan.old_frame = world.resource::<EmbeddedStatus>().frames;
                check.advance(Step::SavedInvalid);
            }
        }
        Step::SavedInvalid if driver::source_equals(world, plan, CONTROLLER, false) => {
            let state = world.resource::<CodeIterationState>();
            if state.phase != CodeIterationPhase::Failed || world.resource::<EmbeddedStatus>().frames <= plan.old_frame { return Ok(()); }
            let Some((index, location)) = state.diagnostics.iter().enumerate().find_map(|(index, diagnostic)| {
                diagnostic.location.as_ref().filter(|location| location.path == PathBuf::from(CONTROLLER)).map(|location| (index, json!(location)))
            }) else { return Err("真实函数体编译错误缺少可定位源码诊断".into()); };
            plan.invalid_generation = state.generation;
            plan.diagnostic_index = Some(index); plan.diagnostic_location = Some(location);
            check.error_evidence = Some(json!({"diagnostics":state.diagnostics.as_ref(),"generation":state.generation,
                "old_pid":plan.old_pid,"old_world":plan.old_world,"frames_continued":world.resource::<EmbeddedStatus>().frames}));
            if driver::pulse_code(world, plan, false)? { check.advance(Step::Diagnostics); }
        }
        Step::Diagnostics => {
            let Some(button) = super::super::diagnostics_ui::validation::diagnostic_row_entity(world, plan.diagnostic_index.unwrap()) else { return Ok(()); };
            if !evidence::screenshot(world, plan, "function-hotpatch-compile-error.png")? { return Ok(()); }
            driver::pulse(world, plan, button); check.advance(Step::Located);
        }
        Step::Located => {
            let Some(probe) = standalone::source_location_probe(world) else { return Ok(()); };
            let location = plan.diagnostic_location.as_ref().unwrap();
            if probe["path"] != CONTROLLER || probe["focused"] != true || probe["native_matches"] != true
                || probe["line"] != location["line"] || probe["column"] != location["column"] { return Ok(()); }
            driver::record(world, plan, "function_hotpatch_compile_error_old_runtime_and_original_location", Some(probe))?;
            if driver::replace_source(world, plan, CONTROLLER, BAD, TWO)? { check.advance(Step::FixedBuffer); }
        }
        Step::FixedBuffer if driver::source_equals(world, plan, CONTROLLER, true) => {
            if driver::pulse_source(world, plan, CONTROLLER, SourceLocationAction::Save)? { check.advance(Step::SavedFixed); }
        }
        Step::SavedFixed if driver::source_equals(world, plan, CONTROLLER, false) => {
            driver::record(world, plan, &format!("function_hotpatch_step_{}_saved_original_button", check.next), None)?;
            check.advance(Step::Candidate);
        }
        Step::Candidate => {
            let state = world.resource::<CodeIterationState>();
            // 原监听有去抖；修复保存后的首帧仍可能显示上一代真实错误，不能把它误判为新构建失败。
            if state.phase == CodeIterationPhase::Failed && state.generation > plan.invalid_generation {
                return Err(format!("函数体候选编译失败：{}", state.status));
            }
            if state.phase != CodeIterationPhase::Ready || state.generation <= plan.invalid_generation
                || state.pending.as_ref().is_none_or(|version| version.identity.generation != state.generation) { return Ok(()); }
            if !matches!(&state.pending.as_ref().unwrap().artifact, crate::code_iteration::backend::PreparedArtifact::Patch(_)) {
                return Err("登记的纯标量函数体修改被构建为重启版本，不能计原地补丁通过".into());
            }
            plan.apply_generation = state.generation; plan.apply_accepted = false;
            driver::record(world, plan, &format!("function_hotpatch_step_{}_candidate_ready_old_instance_retained", check.next), None)?;
            driver::open_source(world, plan, NOTE); check.advance(Step::ReturnNote);
        }
        Step::ReturnNote => {
            driver::source_wait(world, plan);
            let Some(probe) = standalone::source_location_probe(world) else { return Ok(()); };
            if probe["path"] != NOTE || probe["native_matches"] != true { return Ok(()); }
            driver::verify_note(&probe, plan.note_baseline.as_ref().unwrap())?;
            if driver::pulse_source(world, plan, NOTE, SourceLocationAction::Map)? { check.advance(Step::ReturnMap); }
        }
        Step::ReturnMap if map_ready(world) => {
            let baseline = evidence::author_state(world)?;
            if check.author.as_ref() != Some(&baseline) {
                check.author = Some(baseline.clone()); plan.baseline_stable_frames = 0; return Ok(());
            }
            plan.baseline_stable_frames = plan.baseline_stable_frames.saturating_add(1);
            if plan.baseline_stable_frames < 3 { return Ok(()); }
            plan.baseline = Some(baseline);
            let minimum_ticks = last_runtime(check)?["tick"].as_u64().unwrap_or(0);
            let Some(proof) = capture(world, plan, check, false, (check.epoch + 1) as f64, minimum_ticks)? else { return Ok(()); };
            continuity(last_runtime(check)?, &proof["runtime"], false)?;
            push_sample(check, &proof)?;
            if !evidence::screenshot(world, plan, &format!("function-hotpatch-before-{}.png", check.next))? { return Ok(()); }
            evidence::preserved(world, plan.baseline.as_ref().unwrap())?;
            if driver::pulse_code(world, plan, true)? {
                check.minimum_frame = frame(&proof)?;
                check.accept_recorded = false;
                driver::record(world, plan, &format!("function_hotpatch_step_{}_explicit_apply_original_button", check.next), Some(proof))?;
                check.advance(Step::Apply);
            }
        }
        Step::Apply => {
            let state = world.resource::<CodeIterationState>();
            if state.phase == CodeIterationPhase::Failed { return Err(format!("原地补丁应用失败：{}", state.status)); }
            let accepted = state.applying.as_ref().is_some_and(|flight| flight.token == plan.apply_generation)
                || state.applied.as_ref().is_some_and(|identity| identity.generation == plan.apply_generation);
            if !accepted && check.changed.elapsed() > Duration::from_secs(2) { return Err("原应用按钮未获真实补丁事务受理".into()); }
            if accepted && !check.accept_recorded {
                plan.apply_accepted = true; check.accept_recorded = true;
                driver::record(world, plan, &format!("function_hotpatch_step_{}_actual_apply_flight", check.next), None)?;
            }
            let state = world.resource::<CodeIterationState>();
            if state.phase != CodeIterationPhase::Applied || state.applied.as_ref().is_none_or(|identity| identity.generation != plan.apply_generation) { return Ok(()); }
            let status = world.resource::<EmbeddedStatus>();
            if status.playing || status.patch_epoch != check.epoch + 1 { return Err("补丁回执未保持Editing模式或真实采用代次未递增一次".into()); }
            check.epoch += 1; check.advance(Step::Applied);
        }
        Step::Applied if map_ready(world) => {
            if check.changed.elapsed() < Duration::from_millis(400) { return Ok(()); }
            evidence::preserved(world, plan.baseline.as_ref().unwrap())?;
            let minimum_ticks = last_runtime(check)?["tick"].as_u64().unwrap_or(0);
            let Some(proof) = capture(world, plan, check, false, check.next as f64, minimum_ticks)? else { return Ok(()); };
            continuity(last_runtime(check)?, &proof["runtime"], false)?;
            push_sample(check, &proof)?;
            if !evidence::screenshot(world, plan, &format!("function-hotpatch-after-{}.png", check.next))? { return Ok(()); }
            driver::record(world, plan, &format!("function_hotpatch_step_{}_same_world_state_and_new_gpu", check.next), Some(proof))?;
            if press_preview(world, plan)? { check.advance(Step::Resumed); }
        }
        Step::Resumed if world.resource::<EmbeddedStatus>().playing => {
            let minimum_ticks = last_runtime(check)?["tick"].as_u64().unwrap_or(0).saturating_add(30);
            let Some(proof) = capture(world, plan, check, true, check.next as f64, minimum_ticks)? else { return Ok(()); };
            continuity(last_runtime(check)?, &proof["runtime"], true)?;
            push_sample(check, &proof)?;
            driver::record(world, plan, &format!("function_hotpatch_step_{}_original_preview_resumed_with_new_behavior", check.next), Some(proof.clone()))?;
            check.minimum_frame = frame(&proof)?; check.proof = Some(proof); check.return_sent = false;
            check.next += 1; check.step = Step::ReturnEditing; check.captured = None; check.changed = Instant::now();
        }
        Step::FinalMap if map_ready(world) => {
            if check.changed.elapsed() < Duration::from_millis(400) { return Ok(()); }
            // 返回后的作者布局按原系统恢复，文本草稿、选择和撤销现场仍逐字段核对。
            evidence::preserved(world, plan.baseline.as_ref().unwrap())?;
            if check.epoch != 2 || check.error_evidence.is_none() { return Err("正式函数体专项缺少两次实际补丁或编译错误恢复".into()); }
            plan.function_hotpatch_evidence = Some(json!({"source":SOURCE,"scope":"Editing explicit patch then original preview resume",
                "pid":plan.old_pid,"world_id":plan.old_world,"actual_patch_epochs":check.epoch,
                "runtime_samples":check.samples,"compile_error_recovery":check.error_evidence,
                "state_contract":"same actual World/Entity/Resource/Local/Startup/position/Time; original preview resumes new scalar behavior",
                "not_claimed":["applying directly during Playing","arbitrary Rust body or layout changes","DLL unloading","all rendering feature combinations"]}));
            driver::record(world, plan, "function_hotpatch_two_patches_full_author_and_runtime_state_preserved", None)?;
            driver::write_report(world, plan);
        }
        _ => (),
    }
    Ok(())
}

fn map_ready(world: &mut World) -> bool {
    !world.resource::<EmbeddedStatus>().playing && standalone::preview_ready_for(world, "M1-SAMPLE")
        && evidence::current_snapshot(world)
        && standalone::source_location_probe(world).is_some_and(|probe| probe["central_active"] != true)
}

fn press_preview(world: &mut World, plan: &mut Plan) -> Result<bool, String> {
    let button = world.query_filtered::<Entity, With<feathrise_editor_core::ProjectLaunchButton>>()
        .single(world).map_err(|_| "缺少唯一真实预览按钮")?;
    if world.get::<bevy::ui::InteractionDisabled>(button).is_some() { return Ok(false); }
    driver::pulse(world, plan, button); Ok(true)
}

fn last_runtime(check: &Check) -> Result<&Value, String> {
    check.samples.last().map(|proof| &proof["runtime"]).ok_or_else(|| "状态连续性缺少上一真实样本".into())
}

fn push_sample(check: &mut Check, proof: &Value) -> Result<(), String> {
    if check.samples.last() == Some(proof) { return Ok(()); }
    if check.samples.len() >= 16 { return Err("函数体专项运行证据超过16份固定预算".into()); }
    check.samples.push(proof.clone()); Ok(())
}

fn frame(proof: &Value) -> Result<u64, String> { proof["game"]["frame"].as_u64().ok_or_else(|| "GPU证据缺少实际帧号".into()) }

fn continuity(before: &Value, after: &Value, advanced: bool) -> Result<(), String> {
    for field in ["pid", "world_id", "actual_world", "entity", "sentinel", "startup"] {
        if before[field] != after[field] { return Err(format!("原地补丁／预览往返改变了真实状态{field}：{} -> {}", before[field], after[field])); }
    }
    if after["startup"] != 1 || after["local"] != after["tick"] { return Err("Startup重跑或系统Local与资源tick不再连续".into()); }
    for field in ["tick", "local", "position", "real_ms", "virtual_ms"] {
        let old = before[field].as_f64().ok_or_else(|| format!("旧运行样本缺少{field}"))?;
        let new = after[field].as_f64().filter(|value| value.is_finite()).ok_or_else(|| format!("新运行样本缺少{field}"))?;
        if new < old || (advanced && matches!(field, "tick" | "local" | "position") && new <= old) {
            return Err(format!("原地补丁重置或预览未继续推进{field}：{old} -> {new}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({"pid":71,"world_id":u64::MAX,"actual_world":91,"entity":37,"sentinel":0xF123_4567_89AB_CDEF_u64,
            "startup":1,"tick":30,"local":30,"position":1.5,"real_ms":1100.,"virtual_ms":1100.})
    }

    #[test]
    fn continuity_accepts_paused_state_and_monotonic_resumed_state() {
        let old = sample();
        assert!(continuity(&old, &old, false).is_ok());
        let mut resumed = old.clone();
        for field in ["tick", "local"] { resumed[field] = json!(31); }
        resumed["position"] = json!(1.6); resumed["real_ms"] = json!(1200.); resumed["virtual_ms"] = json!(1200.);
        assert!(continuity(&old, &resumed, true).is_ok());
    }

    #[test]
    fn continuity_rejects_restarted_world_resource_local_entity_and_time() {
        let old = sample();
        for (field, value) in [("pid", json!(72)), ("world_id", json!(3)), ("actual_world", json!(92)),
            ("entity", json!(38)), ("sentinel", json!(0)), ("startup", json!(2)), ("local", json!(0)),
            ("position", json!(0.)), ("real_ms", json!(1.)), ("virtual_ms", json!(1.))] {
            let mut reset = old.clone(); reset[field] = value;
            assert!(continuity(&old, &reset, false).is_err(), "must reject {field}");
        }
        assert!(continuity(&old, &old, true).is_err(), "resumed state must really advance");
    }
}

fn capture(world: &mut World, plan: &mut Plan, check: &mut Check, playing: bool, step: f64, ticks: u64) -> Result<Option<Value>, String> {
    let status = world.resource::<EmbeddedStatus>();
    if status.playing != playing || status.patch_epoch != check.epoch { return Ok(None); }
    if let Some(proof) = &check.captured { return Ok(Some(proof.clone())); }
    if let Some(task) = check.task.as_mut() {
        if let Some(result) = check_ready(task) {
            check.task = None; check.retry = Instant::now() + Duration::from_millis(150);
            let proof = result?;
            check.captured = proof.clone();
            return Ok(proof);
        }
        return Ok(None);
    }
    if Instant::now() < check.retry { return Ok(None); }
    let session = (status.pid, status.world_id, status.applied_revision, check.epoch);
    let geometry = world.resource::<crate::viewport_mode::ViewportGeometry>().clone();
    let author = serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?;
    let directory = plan.directory.clone(); let minimum_frame = check.minimum_frame;
    check.task = Some(IoTaskPool::get().spawn(async move {
        read(&directory, session, playing, minimum_frame, ticks, step, &author, &geometry)
    }));
    Ok(None)
}

fn read(directory: &Path, session: (u32, u64, u64, u64), playing: bool, minimum_frame: u64, ticks: u64,
    step: f64, author: &Value, geometry: &crate::viewport_mode::ViewportGeometry) -> Result<Option<Value>, String>
{
    let Some(mut proof) = embedded::code_iteration_capture_mode(&directory.join("game"), session.0, session.2,
        playing, minimum_frame, author, geometry)? else { return Ok(None); };
    if proof["game"]["world_id"] != session.1 { return Err("补丁GPU证据来自不同真实World".into()); }
    if proof["game"]["patch_epoch"] != session.3 { return Ok(None); }
    let mode = if playing { "playing" } else { "editing" };
    let path = directory.join("runtime").join(format!("hotpatch-{}-{}-{}-{mode}.json", session.0, session.1, session.3));
    let Some(runtime) = read_json(&path)? else { return Ok(None); };
    if runtime["source"] != SOURCE || runtime["pid"] != session.0 || runtime["world_id"] != session.1
        || runtime["epoch"] != session.3 || runtime["mode"] != mode || runtime["startup"] != 1
        || runtime["actual_world"].as_u64().unwrap_or(0) == 0 || runtime["entity"].as_u64().unwrap_or(0) == 0
        || runtime["sentinel"].as_u64() != Some(0xF123_4567_89AB_CDEF) {
        return Err(format!("函数体运行样本的实际实例／World／代次／资源身份不符：{runtime}"));
    }
    if runtime["observed_step"].as_f64() != Some(step) || runtime["tick"].as_u64().unwrap_or(0) < ticks
        || (playing && runtime["advance_step"].as_f64() != Some(step)) { return Ok(None); }
    let expected = match step as u32 { 1 => [1., 0., 0., 1.], 2 => [0., 1., 0., 1.], _ => [0., 0., 1., 1.] };
    if runtime["material_srgba"].as_array().is_none_or(|values| values.len() != 4
        || values.iter().zip(expected).any(|(value, expected)| value.as_f64() != Some(expected))) {
        return Err("实际探针材质没有采用新函数对应的颜色".into());
    }
    let screenshot = proof["game"]["screenshot"].as_str().ok_or("新代次GPU缺少实际图片路径")?;
    if Path::new(screenshot).components().count() != 1 || screenshot.contains(['/', '\\', ':']) { return Err("GPU证据图片路径越界".into()); }
    let pixels = inspect_pixels(&directory.join("game").join(screenshot), &proof["game"], step as usize)?;
    proof["runtime"] = runtime; proof["runtime_file"] = json!(path); proof["pixels"] = pixels;
    Ok(Some(proof))
}

fn read_json(path: &Path) -> Result<Option<Value>, String> {
    let file = match fs::File::open(path) { Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(error) => return Err(error.to_string()) };
    let mut bytes = Vec::new(); file.take(16 * 1024 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > 16 * 1024 { return Err("函数体运行证据超过16KiB预算".into()); }
    Ok(serde_json::from_slice(&bytes).ok())
}

fn inspect_pixels(path: &Path, capture: &Value, color: usize) -> Result<Value, String> {
    let mut bytes = Vec::new(); fs::File::open(path).map_err(|error| error.to_string())?
        .take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() < 24 || bytes.len() > 64 * 1024 * 1024 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" { return Err("热补丁真实GPU图片缺失或超过预算".into()); }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16 * 1024 * 1024 { return Err("热补丁GPU图像尺寸越界".into()); }
    let image = Image::from_buffer(&bytes, ImageType::Extension("png"), CompressedImageFormats::NONE, true,
        ImageSampler::nearest(), RenderAssetUsages::default()).map_err(|error| error.to_string())?
        .try_into_dynamic().map_err(|error| error.to_string())?.to_rgba8();
    let hash: String = Sha256::digest(image.as_raw()).iter().map(|byte| format!("{byte:02x}")).collect();
    if capture["capture"]["width"] != width || capture["capture"]["height"] != height
        || capture["author_tools"]["rgba_sha256"] != hash { return Err("热补丁PNG与同次GPU像素摘要不符".into()); }
    let mut counts = [0_u64; 3];
    for (_, _, pixel) in image.enumerate_pixels() {
        for channel in 0..3 {
            let value = u16::from(pixel.0[channel]);
            if value > 100 && value > u16::from(pixel.0[(channel + 1) % 3]) * 2
                && value > u16::from(pixel.0[(channel + 2) % 3]) * 2 { counts[channel] += 1; }
        }
    }
    if !(1..=3).contains(&color) || counts[color - 1] < 20 { return Err(format!("新代次GPU未出现预期颜色{color}的实际像素：{counts:?}")); }
    Ok(json!({"png":path,"rgba_sha256":hash,"dominant_rgb_pixels":counts,"expected_color":color}))
}

/// 夹具源码直接加入正式工程入口crate；函数外的实体／系统／资源代码在两次补丁中逐字保持。
pub(super) const PROBE_SOURCE: &str = r#"

// 本轮独占夹具：只有无参标量函数体会改，World、实体、资源和Local完全由实际运行实例持有。
#[inline(never)]
fn live_step() -> f32 { 1.0 }

#[derive(Component)]
struct LiveProbeEntity { position: f64 }
#[derive(Resource)]
struct LiveProbeState {
    actual_world: u64, sentinel: u64, startup: u64, tick: u64, local: u64,
    observed_step: f32, advance_step: f32,
}
#[derive(Default)]
struct LiveProbeWriter {
    task: Option<bevy::tasks::Task<Result<(), String>>>,
    key: Option<(u64, bool, u64)>,
}

fn actual_live_world(world: &World) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    world.id().hash(&mut hash); hash.finish().max(1)
}

fn initialize_live_probe(world: &mut World) {
    let startup = world.get_resource::<LiveProbeState>().map_or(1, |state| state.startup + 1);
    world.insert_resource(LiveProbeState { actual_world: actual_live_world(world), sentinel: 0xF123_4567_89AB_CDEF,
        startup, tick: 0, local: 0, observed_step: 1.0, advance_step: 1.0 });
    let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::new(0.35, 0.35, 0.35));
    let material = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.0, 0.0), unlit: true, ..default()
    });
    // 单个无碰撞、无地图持久化标记的可见探针；不会加入作者摆放或资产保存链。
    world.spawn((Name::new("function-hotpatch-state-probe"), LiveProbeEntity { position: 0.0 },
        Mesh3d(mesh), MeshMaterial3d(material), Transform::default()));
}

fn verify_live_probe_world(world: &mut World) {
    let state = world.resource::<LiveProbeState>();
    assert_eq!(state.actual_world, actual_live_world(world), "函数体热更新不得替换实际World");
    assert_eq!(state.sentinel, 0xF123_4567_89AB_CDEF, "函数体热更新不得重置资源哨兵");
}

fn advance_live_probe(time: Res<Time>, mut state: ResMut<LiveProbeState>, mut local: Local<u64>,
    mut probes: Query<&mut LiveProbeEntity>) {
    let Ok(mut probe) = probes.single_mut() else { return; };
    let step = bevy::app::hotpatch::call(live_step);
    *local += 1; state.local = *local; state.tick += 1; state.advance_step = step;
    probe.position += f64::from(step) * f64::from(time.delta_secs().min(0.05));
}

fn present_live_probe(mut state: ResMut<LiveProbeState>, mut materials: ResMut<Assets<StandardMaterial>>,
    cameras: Query<&Transform, (With<PrimarySceneView>, Without<LiveProbeEntity>)>,
    mut probes: Query<(&LiveProbeEntity, &mut Transform, &MeshMaterial3d<StandardMaterial>), Without<PrimarySceneView>>) {
    let (Ok(camera), Ok((probe, mut transform, handle))) = (cameras.single(), probes.single_mut()) else { return; };
    let step = bevy::app::hotpatch::call(live_step); state.observed_step = step;
    // 相机前固定小范围可见，累计距离仍在同一个实际组件里；切换编辑／预览只借用视角，不重建探针。
    let local = Vec3::new(0.5 + (probe.position * 0.01) as f32, -0.25, -2.4);
    *transform = Transform::from_translation(camera.transform_point(local)).with_rotation(camera.rotation);
    if let Some(mut material) = materials.get_mut(&handle.0) {
        let color = match step as u32 { 1 => Color::srgb(1.0, 0.0, 0.0), 2 => Color::srgb(0.0, 1.0, 0.0), _ => Color::srgb(0.0, 0.0, 1.0) };
        if material.base_color != color { material.base_color = color; }
    }
}

fn report_live_probe(state: Res<LiveProbeState>, project: Option<Res<EmbeddedProjectState>>,
    hotpatch: Res<feathrise_runtime::project::embedded::FunctionHotpatchState>,
    probes: Query<(Entity, &LiveProbeEntity, &Transform, &MeshMaterial3d<StandardMaterial>)>,
    materials: Res<Assets<StandardMaterial>>, real: Res<Time<bevy::time::Real>>,
    virtual_time: Res<Time<bevy::time::Virtual>>, mut writer: Local<LiveProbeWriter>) {
    use bevy::tasks::futures::check_ready;
    if let Some(task) = writer.task.as_mut() {
        if let Some(result) = check_ready(task) {
            writer.task = None;
            if let Err(error) = result { eprintln!("函数体运行取证失败：{error}"); writer.key = None; }
        } else { return; }
    }
    let (Some(project), Ok((entity, probe, transform, handle))) = (project, probes.single()) else { return; };
    let Some(material) = materials.get(&handle.0) else { return; };
    let color = material.base_color.to_srgba();
    let playing = format!("{:?}", project.mode) == "Playing";
    let key = (hotpatch.epoch(), playing, state.tick / 15);
    if writer.key == Some(key) { return; } writer.key = Some(key);
    let mode = if playing { "playing" } else { "editing" };
    let Some(directory) = std::env::var_os("FEATHRISE_CODE_ITERATION_RUNTIME_DIR") else { return; };
    let path = std::path::PathBuf::from(directory).join(format!("hotpatch-{}-{}-{}-{mode}.json", std::process::id(), project.world_id, key.0));
    let proof = format!(concat!("{{\"source\":\"actual-function-hotpatch-World-Entity-Resource-Local-Time-and-material\",",
        "\"pid\":{},\"world_id\":{},\"actual_world\":{},\"entity\":{},\"sentinel\":{},\"startup\":{},",
        "\"epoch\":{},\"mode\":\"{}\",\"tick\":{},\"local\":{},\"position\":{},",
        "\"observed_step\":{},\"advance_step\":{},\"real_ms\":{},\"virtual_ms\":{},\"visual_position\":[{},{},{}],",
        "\"material_srgba\":[{},{},{},{}]}}"),
        std::process::id(), project.world_id, state.actual_world, entity.to_bits(), state.sentinel, state.startup,
        key.0, mode, state.tick, state.local, probe.position, state.observed_step, state.advance_step,
        real.elapsed_secs_f64() * 1000.0, virtual_time.elapsed_secs_f64() * 1000.0,
        transform.translation.x, transform.translation.y, transform.translation.z, color.red, color.green, color.blue, color.alpha);
    writer.task = Some(bevy::tasks::IoTaskPool::get().spawn(async move {
        let pending = path.with_extension("pending.json");
        std::fs::write(&pending, proof.as_bytes()).map_err(|error| error.to_string())?;
        std::fs::rename(pending, path).map_err(|error| error.to_string())
    }));
}
"#;
