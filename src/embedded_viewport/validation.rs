//! 双3D模式的真实窗口回归：原作者请求、未保存历史、协议回执、同World往返及故障恢复。
//! 仅显式 validation 构建和本轮自建夹具安装；所有成功条件来自原业务或真实GPU观测。

mod empty_fixture;
mod build_backend;
mod close_evidence;
mod live_fixture;
mod live_preview;
mod tool_previews;
mod handle_hover;
mod view_controls;
mod orchestration;
mod placement_evidence;
mod wire_evidence;
mod presentation_evidence;
mod preview_toolbar;
mod resources;
mod window_evidence;
#[path = "../native_preview_validation.rs"]
pub(crate) mod native_preview_validation;
pub(crate) mod build_cancel;
pub(crate) use orchestration::{read_guest_capture, run, run_owned_project};

/// 代码迭代专项复用本轮归属与实际GPU/实例/原生窗口核验，不安装另一套常驻验收阶段。
pub(crate) fn code_iteration_owner(root: &Path) -> Result<crate::project::Project, String> {
    orchestration::owner(root)
}

pub(crate) fn code_iteration_asset_hashes(root: &Path) -> Result<std::collections::BTreeMap<PathBuf, String>, String> {
    orchestration::asset_hashes(root)
}

pub(crate) fn code_iteration_capture(
    directory: &Path,
    pid: u32,
    revision: u64,
    min_frame: u64,
    author: &serde_json::Value,
    geometry: &crate::viewport_mode::ViewportGeometry,
) -> Result<Option<serde_json::Value>, String> {
    let Some(capture) = read_guest_capture(directory, pid, revision, false, min_frame)? else { return Ok(None); };
    placement_evidence::verify(&capture, author)?;
    Ok(window_evidence::query_live(&capture, geometry)?.map(|window| serde_json::json!({"game":capture,"window":window})))
}

/// 新代码专项在后台读取当前模式的新GPU帧；不能拿Editing旧图充当Playing补丁后的画面。
pub(crate) fn code_iteration_capture_mode(
    directory: &Path, pid: u32, revision: u64, playing: bool, min_frame: u64,
    author: &serde_json::Value, geometry: &crate::viewport_mode::ViewportGeometry,
) -> Result<Option<serde_json::Value>, String> {
    let Some(capture) = read_guest_capture(directory, pid, revision, playing, min_frame)? else { return Ok(None); };
    placement_evidence::verify(&capture, author)?;
    Ok(window_evidence::query_live(&capture, geometry)?.map(|window| serde_json::json!({"game":capture,"window":window})))
}

#[derive(Resource)]
struct CodeIterationReturnClick(resources::GameMouseRelease);

#[derive(Resource)]
struct CodeIterationReturnFocusObserved;

/// 进入报告／退出收口时撤销唯一鼠标租约；原生租约Drop只释放自己成功按下的按钮。
pub(crate) fn code_iteration_cancel_return_click(world: &mut World) {
    world.remove_resource::<CodeIterationReturnClick>();
}

/// 仅使用后台已核验的本轮窗口证据点击原HUD；按下与释放跨帧，不做文件IO或伪造模式回执。
pub(crate) fn code_iteration_return_to_edit(
    world: &mut World, proof: &serde_json::Value, pid: u32,
) -> Result<bool, String> {
    if let Some(mut pulse) = world.remove_resource::<CodeIterationReturnClick>() {
        if pulse.0.pressed.elapsed() < Duration::from_millis(40)
            || !window_evidence::release_game_click(&mut pulse.0)? {
            world.insert_resource(pulse);
            return Ok(false);
        }
        if pulse.0.native.is_none() {
            println!("FEATHRISE_CODE_ITERATION_RETURN_INPUT {}", json!({"phase":"up_submitted",
                "pid":pulse.0.pid,"hwnd":pulse.0.hwnd,"point":pulse.0.point}));
        }
        return Ok(true);
    }
    if !world.resource::<EmbeddedStatus>().playing { return Ok(true); }
    // 只在所属前台根窗口内、游戏线程真实持焦后发送一次点击；失焦时等实际激活，不改写focused。
    if !window_evidence::game_return_focus_ready(proof, pid) {
        // 本轮首次等待补充只读原生事实；不逐帧打印，也不以这次复读改变原焦点等待门。
        if !world.contains_resource::<CodeIterationReturnFocusObserved>() {
            println!("FEATHRISE_CODE_ITERATION_RETURN_INPUT {}", json!({"phase":"focus_unready_readback",
                "observation":window_evidence::describe_game_return_focus(proof, pid)}));
            world.insert_resource(CodeIterationReturnFocusObserved);
        }
        return Ok(false);
    }
    let click = window_evidence::request_game_return(proof, pid)?;
    if click.native.is_none() {
        println!("FEATHRISE_CODE_ITERATION_RETURN_INPUT {}", json!({"phase":"down_submitted",
            "pid":click.pid,"hwnd":click.hwnd,"point":click.point}));
    }
    world.insert_resource(CodeIterationReturnClick(click));
    Ok(false)
}

use self::resources::{Phase, Plan};
use super::EmbeddedStatus;
use crate::viewport_mode::{RequestViewportMode, ViewportMode, ViewportModeState};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
    window::PrimaryWindow,
};
use feathrise_editor_protocol::Mode;
use feathrise_editor_map::standalone::embedded_snapshot::{self, EmbeddedSnapshotState};
use feathrise_editor_map::standalone::{self, embedded_validation as author};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const POSITION: [f32; 3] = [7.0, 1.0, -2.0];

pub(crate) fn install(app: &mut App, project_root: &Path) -> Result<(), String> {
    let Some(directory) = std::env::var_os("FEATHRISE_EMBEDDED_SMOKE_DIR").map(PathBuf::from)
    else {
        return Ok(());
    };
    let project = orchestration::owner(project_root)?;
    if !directory.is_absolute()
        || !directory.is_dir()
        || directory.join("embedded-editor.json").exists()
    {
        return Err("常驻验收输出目录必须由本轮协调器独占创建，且不得覆盖旧回执".into());
    }
    if std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref() == Ok("native-preview") {
        return native_preview_validation::install(app, &project.root, directory);
    }
    if matches!(std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref(), Ok("build-backend" | "bundled-dx")) {
        return build_backend::install(app, directory);
    }
    if std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref() == Ok("preview-chrome") {
        return crate::window_chrome::preview_validation::install(app, &project.root, directory);
    }
    author::install(app);
    let empty_fixture = orchestration::is_empty_fixture(&project.root)?;
    let live_fixture = live_fixture::selected(&project.root)?;
    println!(
        "FEATHRISE_EMBEDDED_VALIDATION_START {}",
        json!({"project_id":project.id,"host_pid":std::process::id(),"directory":directory})
    );
    app.insert_resource(Plan {
        project_root: project.root,
        project_id: project.id,
        directory,
        phase: Phase::Open,
        started: Instant::now(),
        changed: Instant::now(),
        wait_logged: false,
        empty_fixture,
        live_fixture,
        live_evidence: None,
        tool_evidence: None,
        preview_toolbar_evidence: None,
        frames: 0,
        initial: None,
        added: None,
        presentation_baseline: None,
        close_evidence: None,
        pid: 0,
        world_id: 0,
        frame_before: 0,
        ticks_before: 0,
        pause_requested_after_frame: 0,
        current_revision: 0,
        pulse: None,
        game_mouse_release: None,
        steps: Vec::new(),
        error: None,
        completion: None,
        evidence: None,
        evidence_ready: None,
        region_evidence: None,
        evidence_retry: Instant::now(),
    })
    .add_systems(
        Update,
        input_ownership.before(feathrise_editor_core::MapEditorSystemSet::Input),
    )
    .add_systems(Last, drive);
    Ok(())
}

/// 注入原ECS输入链前只投影本夹具焦点；不操作外部桌面键鼠，物理手感另列未验。
fn input_ownership(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if let Ok(mut window) = windows.single_mut() {
        window.focused = true;
    }
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut plan: Mut<Plan>| {
        if let Some(pulse) = plan.pulse.take() {
            author::release_pulse(world, pulse);
        }
        if plan.game_mouse_release.as_ref()
            .is_some_and(|release| release.pressed.elapsed() >= Duration::from_millis(100)) {
            let mut release = plan.game_mouse_release.take().unwrap();
            match window_evidence::release_game_click(&mut release) {
                Ok(false) => plan.game_mouse_release = Some(release),
                Ok(true) => {},
                Err(error) => { plan.error = Some(error); plan.phase = Phase::Failed; }
            }
        }
        plan.frames += 1;
        if matches!(plan.phase, Phase::Capture | Phase::Finished) {
            if let Some(completion) = &plan.completion {
                match completion.try_recv() {
                    Ok(result) => {
                        if let Err(error) = result {
                            log_failure(&plan, &error);
                            plan.error = Some(error);
                        }
                        world.write_message(if plan.error.is_none() {
                            AppExit::Success
                        } else {
                            AppExit::error()
                        });
                        plan.phase = Phase::Finished;
                    }
                    Err(async_channel::TryRecvError::Closed) => {
                        log_failure(&plan, "验收截图/报告后台任务未返回结果");
                        world.write_message(AppExit::error());
                        plan.phase = Phase::Finished;
                    }
                    Err(async_channel::TryRecvError::Empty) => {}
                }
            }
            return;
        }
        // 首次连接和故障重建允许Cargo预算；已连接后的有限夹具业务不能再静默等待半小时。
        let phase_budget = if matches!(plan.phase, Phase::Embedded | Phase::Recovered) {
            Duration::from_secs(1800)
        } else if plan.phase == Phase::LivePreview {
            Duration::from_secs(850) // 含独立600秒首次驱动准备；手势阶段仍逐项限时90秒。
        } else {
            Duration::from_secs(120)
        };
        if plan.phase != Phase::Failed
            && (plan.started.elapsed() > Duration::from_secs(2200)
                || plan.changed.elapsed() > phase_budget)
        {
            let error = format!("常驻验收超时：{:?}", plan.phase);
            log_failure(&plan, &error);
            plan.error = Some(error);
            plan.phase = Phase::Failed;
        }
        let result = advance(world, &mut plan);
        // 每阶段最多一条有界等待诊断；复杂夹具不能只留最后一个成功阶段让故障失去上下文。
        if !plan.wait_logged && plan.changed.elapsed() > Duration::from_secs(10) {
            plan.wait_logged = true;
            let presentation = author::presentation_probe(world);
            let capture = world.resource::<EmbeddedSnapshotState>();
            let geometry = world.resource::<crate::viewport_mode::ViewportGeometry>();
            eprintln!("FEATHRISE_EMBEDDED_VALIDATION_WAIT {}", json!({
                "phase":format!("{:?}",plan.phase),"author":author::probe(world),"presentation":presentation,
                "capture":{"revision":capture.revision,"pending":capture.pending,
                    "error":capture.error,"ready_revision":capture.ready.as_ref().map(|value|value.author_revision),
                    "current":embedded_snapshot::ready_is_current(world)},
                "game":format!("{:?}",world.resource::<EmbeddedStatus>()),
                "previews":super::previews::diagnostic(world),
                "geometry":{"revision":geometry.revision,"rect":geometry.rect,"visible":geometry.visible},
            }));
        }
        if let Err(error) = result {
            log_failure(&plan, &error);
            plan.error = Some(error);
            plan.phase = Phase::Failed;
        }
        if plan.phase == Phase::Failed {
            if let Some(active) = world
                .get_non_send::<super::resources::Runtime>()
                .and_then(|runtime| runtime.active.as_ref())
            {
                active.control.cancel();
            }
            if !world.resource::<EmbeddedStatus>().active {
                capture_report(world, &mut plan);
            }
        }
    });
}

fn advance(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    let status = world.resource::<EmbeddedStatus>();
    let (active, ready, pid, world_id, playing, observed_mode, revision, frames, ticks) = (
        status.active,
        status.ready,
        status.pid,
        status.world_id,
        status.playing,
        status.observed_mode,
        status.applied_revision,
        status.frames,
        status.ticks,
    );
    if !matches!(
        plan.phase,
        Phase::Open | Phase::Local | Phase::Crashed | Phase::Recovered | Phase::Failed
    ) && let Some(error) = &status.error
    {
        return Err(format!("常驻阶段 {:?} 失败：{error}", plan.phase));
    }
    if matches!(
        plan.phase,
        Phase::Embedded | Phase::Added | Phase::Undo | Phase::Redo
    ) && let Some(error) = &world.resource::<EmbeddedSnapshotState>().error
    {
        return Err(format!(
            "常驻阶段 {:?} 的作者快照被拒绝：{error}",
            plan.phase
        ));
    }
    match plan.phase {
        Phase::Open if plan.frames >= 30 => {
            world.write_message(RequestViewportMode(ViewportMode::SavedWorld));
            standalone::request_open_map(world, "M1-SAMPLE")?;
            plan.advance(Phase::Local);
        }
        Phase::Local
            if !active
                && standalone::preview_ready_for(world, "M1-SAMPLE")
                && world.resource::<ViewportModeState>().selected == ViewportMode::SavedWorld =>
        {
            if !presentation_evidence::mode_ready(world, false)? {
                return Ok(());
            }
            let initial = author::probe(world)?;
            plan.current_revision = initial.revision;
            plan.initial = Some(initial);
            if plan.empty_fixture && !plan.initial.as_ref().unwrap().placements.is_empty() {
                return Err("空地图专项在初始作者来源中发现已有实例".into());
            }
            record(world, plan, "original_editor_3d_ready")?;
            world.write_message(RequestViewportMode(ViewportMode::EmbeddedWorld));
            plan.advance(Phase::Embedded);
        }
        Phase::Embedded if ready && current_snapshot_applied(world, revision) && frames > 12 => {
            if !guest_capture(world, plan, pid, revision, false, 0)? {
                return Ok(());
            }
            plan.pid = pid;
            plan.world_id = world_id;
            record(world, plan, "resident_game_3d_ready")?;
            if plan.empty_fixture {
                press_preview(world, plan)?;
                plan.advance(Phase::EmptyPlaying);
            } else {
                author::request_cuboid(world, POSITION)?;
                plan.advance(Phase::Added);
            }
        }
        Phase::EmptyPlaying if playing && observed_mode == Some(Mode::Playing) && ticks > 3 => {
            same_world(plan, pid, world_id)?;
            if !guest_capture(world, plan, pid, revision, true, 0)? {
                return Ok(());
            }
            record(world, plan, "empty_map_f12_play_same_world")?;
            plan.pause_requested_after_frame = frames;
            plan.game_mouse_release = Some(window_evidence::request_game_return(
                plan.region_evidence
                    .as_ref()
                    .ok_or("缺少空地图运行窗口取证")?,
                pid,
            )?);
            plan.advance(Phase::EmptyPaused);
        }
        Phase::EmptyPaused
            if !playing
                && editing_heartbeat_after_request(
                    observed_mode,
                    frames,
                    plan.pause_requested_after_frame,
                ) =>
        {
            same_world(plan, pid, world_id)?;
            if !guest_capture(
                world,
                plan,
                pid,
                revision,
                false,
                plan.pause_requested_after_frame + 1,
            )? {
                return Ok(());
            }
            record(world, plan, "empty_map_f12_returned")?;
            author::request_cuboid(world, POSITION)?;
            plan.advance(Phase::Added);
        }
        Phase::Added => {
            same_world(plan, pid, world_id)?;
            let current = author::probe(world)?;
            let initial = plan.initial.as_ref().unwrap();
            if current.placements.len() == initial.placements.len() + 1
                && current.undo_len == initial.undo_len + 1
                && current.dirty
                && current.last_added_id.as_ref().is_some_and(|id| {
                    current
                        .placements
                        .iter()
                        .any(|p| &p.id == id && p.translation == POSITION)
                })
                && current_snapshot_applied(world, revision)
                && plan.changed.elapsed() > Duration::from_secs(2)
            {
                if !guest_capture(world, plan, pid, revision, false, 0)? {
                    return Ok(());
                }
                unchanged_save(plan, &current)?;
                plan.current_revision = current.revision;
                plan.added = Some(current);
                record(world, plan, "unsaved_cuboid_applied")?;
                if let Some(pulse) = author::pulse_history(world, false)? {
                    plan.pulse = Some(pulse);
                    plan.advance(Phase::Undo);
                }
            }
        }
        Phase::Undo => {
            same_world(plan, pid, world_id)?;
            let current = author::probe(world)?;
            let initial = plan.initial.as_ref().unwrap();
            if current.placements == initial.placements
                && current.undo_len == initial.undo_len
                && current.redo_len == 1
                && current.revision > plan.current_revision
                && current_snapshot_applied(world, revision)
                && plan.changed.elapsed() > Duration::from_secs(2)
            {
                if !guest_capture(world, plan, pid, revision, false, 0)? {
                    return Ok(());
                }
                unchanged_save(plan, &current)?;
                plan.current_revision = current.revision;
                record(world, plan, "original_undo_applied")?;
                if let Some(pulse) = author::pulse_history(world, true)? {
                    plan.pulse = Some(pulse);
                    plan.advance(Phase::Redo);
                }
            }
        }
        Phase::Redo => {
            same_world(plan, pid, world_id)?;
            let current = author::probe(world)?;
            if current.placements == plan.added.as_ref().unwrap().placements
                && current.undo_len == plan.added.as_ref().unwrap().undo_len
                && current.redo_len == 0
                && current.revision > plan.current_revision
                && current_snapshot_applied(world, revision)
                && plan.changed.elapsed() > Duration::from_secs(2)
            {
                if !guest_capture(world, plan, pid, revision, false, 0)? {
                    return Ok(());
                }
                unchanged_save(plan, &current)?;
                plan.current_revision = current.revision;
                plan.added = Some(current);
                record(world, plan, "original_redo_applied")?;
                plan.advance(Phase::Tools);
            }
        }
        Phase::Tools => {
            same_world(plan, pid, world_id)?;
            if plan.tool_evidence.is_none() {
                let Some(evidence)=tool_previews::exercise(world,&plan.directory)? else {return Ok(())};
                plan.tool_evidence=Some(evidence);
            }
            if !plan.empty_fixture && plan.tool_evidence.as_ref().is_some_and(|evidence|evidence["terrain_hover"].is_null()) {
                let Some(evidence)=handle_hover::exercise(world,&plan.directory)? else {return Ok(())};
                plan.tool_evidence.as_mut().unwrap()["terrain_hover"]=evidence;
            }
            {
                record(world, plan, "semantic_tools_rendered_and_cleared")?;
                if plan.live_fixture {
                    plan.advance(Phase::LivePreview);
                } else {
                    plan.ticks_before = ticks;
                    press_preview(world, plan)?;
                    plan.advance(Phase::Playing);
                }
            }
        }
        Phase::LivePreview => {
            same_world(plan, pid, world_id)?;
            let cube = plan.steps.iter().find(|step|step["stage"] == "unsaved_cuboid_applied")
                .and_then(|step|step["author"]["last_added_id"].as_str()).ok_or("连续预览缺少原新建方块ID")?.to_owned();
            if let Some(evidence) = live_preview::exercise(world, &plan.directory, &cube)? {
                plan.live_evidence = Some(evidence);
                plan.added = Some(author::probe(world)?);
                record(world, plan, "continuous_previews_completed")?;
                plan.ticks_before = ticks;
                press_preview(world, plan)?;
                plan.advance(Phase::Playing);
            }
        }
        Phase::Playing
            if playing && observed_mode == Some(Mode::Playing) && ticks > plan.ticks_before + 3 =>
        {
            if !guest_capture(world, plan, pid, revision, true, 0)? {
                return Ok(());
            }
            same_world(plan, pid, world_id)?;
            record(world, plan, "f12_play_same_world")?;
            if let Some(evidence) = preview_toolbar::exercise(world, plan)? {
                plan.preview_toolbar_evidence = Some(evidence);
                plan.pause_requested_after_frame = frames;
                plan.game_mouse_release = Some(window_evidence::request_game_return(
                    plan.region_evidence.as_ref().ok_or("缺少运行窗口取证")?,
                    pid,
                )?);
                plan.advance(Phase::Paused);
            }
        }
        Phase::Paused
            if !playing
                && ready
                && editing_heartbeat_after_request(
                    observed_mode,
                    frames,
                    plan.pause_requested_after_frame,
                ) =>
        {
            same_world(plan, pid, world_id)?;
            // ModeApplied 与低频 Status 是两种回执。只能用本次 Editing 心跳建立
            // 暂停基线，避免把交接前最后一条 Playing 心跳里的 ticks 当成暂停事实。
            plan.ticks_before = ticks;
            plan.frame_before = frames;
            plan.advance(Phase::PauseStable);
        }
        Phase::PauseStable => {
            same_world(plan, pid, world_id)?;
            if playing || observed_mode != Some(Mode::Editing) {
                return Err("暂停观察期间运行实例不再保持已确认的 Editing 心跳身份".into());
            }
            if ticks != plan.ticks_before {
                return Err(format!(
                    "返回编辑后运行ticks仍在推进：编辑心跳基线 frame={} ticks={}，当前 frame={frames} ticks={ticks}",
                    plan.frame_before, plan.ticks_before
                ));
            }
            // 游戏截图通常先于首条Editing心跳落盘；截图只需晚于本次F12，
            // 玩法暂停则单独用Editing心跳基线与后续心跳比较，不能混用两个时间点。
            let capture_min_frame = plan.pause_requested_after_frame.saturating_add(1);
            if frames <= plan.frame_before.saturating_add(8)
                || plan.changed.elapsed() <= Duration::from_secs(2)
                || !guest_capture(world, plan, pid, revision, false, capture_min_frame)?
            {
                return Ok(());
            }
            preserve_author(world, plan)?;
            record(world, plan, "f12_edit_same_world_and_paused_ticks")?;
            super::terminate_for_validation(world)?;
            plan.advance(Phase::Crashed);
        }
        Phase::Crashed if !active => {
            if world.resource::<EmbeddedStatus>().error.is_none() {
                return Err("故障注入未观察到真实异常退出诊断".into());
            }
            preserve_author(world, plan)?;
            record(world, plan, "crash_preserved_unsaved_author_and_history")?;
            press_preview(world, plan)?;
            plan.advance(Phase::Recovered);
        }
        Phase::Recovered
            if ready
                && playing
                && observed_mode == Some(Mode::Playing)
                && current_snapshot_applied(world, revision)
                && ticks > 3 =>
        {
            if !guest_capture(world, plan, pid, revision, true, 0)? {
                return Ok(());
            }
            if pid == plan.pid || world_id == plan.world_id {
                return Err("崩溃恢复没有新的PID/World身份".into());
            }
            preserve_author(world, plan)?;
            record(world, plan, "restart_adopted_same_unsaved_author")?;
            plan.pid = pid;
            plan.world_id = world_id;
            plan.pause_requested_after_frame = frames;
            plan.advance(Phase::ClosePlaying);
        }
        Phase::ClosePlaying => {
            // 关闭流程会先返回Editing；不能把后续取消驱动继续放在playing守卫内。
            if let Some(evidence) = close_evidence::exercise(world, &plan.directory)? {
                plan.close_evidence = Some(evidence);
                plan.advance(Phase::RecoveredPause);
            }
        }
        Phase::RecoveredPause
            if !playing
                && ready
                && editing_heartbeat_after_request(
                    observed_mode,
                    frames,
                    plan.pause_requested_after_frame,
                )
                && plan.changed.elapsed() > Duration::from_secs(2) =>
        {
            let capture_min_frame = plan.pause_requested_after_frame.saturating_add(1);
            if !guest_capture(world, plan, pid, revision, false, capture_min_frame)? {
                return Ok(());
            }
            same_world(plan, pid, world_id)?;
            preserve_author(world, plan)?;
            record(world, plan, "recovered_world_returned_to_edit")?;
            world.write_message(RequestViewportMode(ViewportMode::SavedWorld));
            plan.advance(Phase::ReturnLocal);
        }
        Phase::ReturnLocal
            if !active
                && world.resource::<ViewportModeState>().selected == ViewportMode::SavedWorld
                && standalone::preview_ready_for(world, "M1-SAMPLE") =>
        {
            if !presentation_evidence::mode_ready(world, false)? {
                return Ok(());
            }
            preserve_author(world, plan)?;
            record(world, plan, "back_to_original_editor_3d_with_same_author")?;
            if crate::viewport_mode::ready_to_exit(world, false)? {
                capture_report(world, plan);
            }
        }
        _ => {}
    }
    Ok(())
}

fn current_snapshot_applied(world: &World, applied_revision: u64) -> bool {
    use feathrise_editor_map::standalone::embedded_snapshot::{self, EmbeddedSnapshotState};
    // 捕获修订覆盖多个作者域，不能与单个 EditorAuthoringSession.revision 混用。
    embedded_snapshot::ready_is_current(world)
        && world
            .resource::<EmbeddedSnapshotState>()
            .ready
            .as_ref()
            .is_some_and(|snapshot| applied_revision >= snapshot.author_revision)
}

fn editing_heartbeat_after_request(mode: Option<Mode>, frame: u64, requested_after: u64) -> bool {
    mode == Some(Mode::Editing) && frame > requested_after
}

fn same_world(plan: &Plan, pid: u32, world_id: u64) -> Result<(), String> {
    if pid != plan.pid || world_id != plan.world_id || pid == 0 || world_id == 0 {
        Err("普通数据编辑/F12意外替换运行实例PID或World".into())
    } else {
        Ok(())
    }
}
fn unchanged_save(plan: &Plan, current: &author::AuthorProbe) -> Result<(), String> {
    if current.save_sequence != plan.initial.as_ref().unwrap().save_sequence {
        Err("未保存验收期间原保存序号发生变化".into())
    } else {
        Ok(())
    }
}
fn preserve_author(world: &World, plan: &Plan) -> Result<(), String> {
    let current = author::probe(world)?;
    let expected = plan.added.as_ref().unwrap();
    unchanged_save(plan, &current)?;
    if current.placements != expected.placements
        || current.undo_len != expected.undo_len
        || current.redo_len != expected.redo_len
        || !current.dirty
    {
        Err("运行实例切换/故障丢失作者未保存草稿或撤销历史".into())
    } else {
        Ok(())
    }
}
fn press_preview(world: &mut World, plan: &mut Plan) -> Result<(), String> {
    plan.presentation_baseline = Some(presentation_evidence::capture_baseline(world)?);
    let button = world.query_filtered::<Entity, With<feathrise_editor_core::ProjectLaunchButton>>()
        .single(world).map_err(|_| "缺少唯一真实预览按钮")?;
    if world.get::<bevy::ui::InteractionDisabled>(button).is_some() {
        return Err("预览按钮尚未可用".into());
    }
    world.entity_mut(button).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    plan.pulse = Some(button);
    Ok(())
}
fn record(world: &mut World, plan: &mut Plan, stage: &str) -> Result<(), String> {
    // 同一阶段可能等待原按钮下帧可用；只记录一次，避免观测报告无限增长。
    if plan.steps.last().is_some_and(|step| step["stage"] == stage) {
        return Ok(());
    }
    let presentation = presentation_evidence::record(world, plan.presentation_baseline.as_ref(), stage)?;
    let status = world.resource::<EmbeddedStatus>();
    let current = author::probe(world)?;
    let pause_baseline = (stage == "f12_edit_same_world_and_paused_ticks").then(|| {
        json!({
            "requested_after_frame":plan.pause_requested_after_frame,
            "editing_frame":plan.frame_before,"editing_ticks":plan.ticks_before
        })
    });
    plan.steps.push(json!({"stage":stage,"passed":true,"pid":status.pid,"world_id":status.world_id,
        "mode":if status.playing {"playing"}else{"editing"},"applied_revision":status.applied_revision,
        "observed_mode":status.observed_mode,"pause_baseline":pause_baseline,
        "host_window_region":plan.region_evidence,
        "author_presentation":presentation,
        "frame":status.frames,"ticks":status.ticks,"starts":status.starts,"completed":status.completed,
        "author":current,"elapsed_ms":plan.started.elapsed().as_secs_f64()*1000.0}));
    // 只在真实阶段首个回执后输出；等待按钮或GPU证据时不逐帧刷日志。
    println!(
        "FEATHRISE_EMBEDDED_VALIDATION_STEP {}",
        json!({"stage":stage,"pid":status.pid,"world_id":status.world_id,
            "revision":status.applied_revision,"frame":status.frames,"ticks":status.ticks,
            "elapsed_ms":plan.started.elapsed().as_secs_f64()*1000.0})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_baseline_requires_a_new_editing_heartbeat_after_the_request() {
        // ModeApplied 先于下一条心跳：无模式、旧编辑心跳、较新的预览心跳均不能当暂停基线。
        assert!(!editing_heartbeat_after_request(None, 120, 100));
        assert!(!editing_heartbeat_after_request(
            Some(Mode::Editing),
            90,
            100
        ));
        assert!(!editing_heartbeat_after_request(
            Some(Mode::Editing),
            100,
            100
        ));
        assert!(!editing_heartbeat_after_request(
            Some(Mode::Playing),
            120,
            100
        ));
        assert!(editing_heartbeat_after_request(
            Some(Mode::Editing),
            121,
            100
        ));
    }
}

fn log_failure(plan: &Plan, error: &str) {
    eprintln!(
        "FEATHRISE_EMBEDDED_VALIDATION_FAILED {}",
        json!({"phase":format!("{:?}",plan.phase),"error":error,
            "host_pid":std::process::id(),"elapsed_ms":plan.started.elapsed().as_secs_f64()*1000.0})
    );
}

/// 截图回执由游戏在 PNG 真正落盘之后发布；帧内只非阻塞轮询单个后台读取任务。
/// 同修订返回编辑会覆写末次图片，因此 min_frame 必须晚于本次交接，不能误用前次编辑截图。
fn guest_capture(
    world: &mut World,
    plan: &mut Plan,
    pid: u32,
    revision: u64,
    playing: bool,
    min_frame: u64,
) -> Result<bool, String> {
    // ModeApplied只能证明协议交接；根/顶栏实际显隐完成后才接受对应游戏截图。
    if !presentation_evidence::mode_ready(world, playing)? {
        return Ok(false);
    }
    let geometry = world.resource::<crate::viewport_mode::ViewportGeometry>();
    // 真实后台窗口按生产规则禁用游戏输入；先等本轮窗口取得前台，不能把逻辑focused注入当作原生许可。
    if playing && !geometry.focused { return Ok(false); }
    if geometry.excluded_rects.len() > feathrise_editor_protocol::MAX_VIEWPORT_EXCLUDED_RECTS {
        return Err("作者几何超出32个留孔预算，拒绝复制验收快照".into());
    }
    if geometry.excluded_shapes.len() > feathrise_editor_protocol::MAX_VIEWPORT_EXCLUDED_SHAPES {
        return Err("作者XYZ轮廓超过十个固定形状预算".into());
    }
    let identity = (pid, revision, playing, min_frame, geometry.revision);
    if plan.evidence_ready == Some(identity) {
        return Ok(true);
    }
    if let Some((pending, result)) = &plan.evidence {
        match result.try_recv() {
            Ok(result) => {
                let pending = *pending;
                plan.evidence = None;
                if let Some(proof) = result? {
                    plan.evidence_ready = Some(pending);
                    plan.region_evidence = Some(proof);
                }
                plan.evidence_retry = Instant::now() + Duration::from_millis(100);
            }
            Err(async_channel::TryRecvError::Closed) => {
                return Err("运行实例GPU证据读取任务未返回结果".into());
            }
            Err(async_channel::TryRecvError::Empty) => return Ok(false),
        }
    }
    if plan.evidence_ready == Some(identity) {
        return Ok(true);
    }
    if Instant::now() < plan.evidence_retry {
        return Ok(false);
    }
    let directory = plan.directory.join("game");
    let geometry = geometry.clone();
    // 有界夹具最多64个实例；完整快照与F12阶段同时核对最终位置，防止取消后旧来源重新覆盖。
    let author = serde_json::to_value(author::probe(world)?).map_err(|error| error.to_string())?;
    let (sender, receiver) = async_channel::bounded(1);
    plan.evidence = Some((identity, receiver));
    IoTaskPool::get()
        .spawn(async move {
            let result =
                orchestration::read_guest_capture(&directory, pid, revision, playing, min_frame)
                    .and_then(|value| match value {
                        Some(capture) => {
                            placement_evidence::verify(&capture, &author)?;
                            window_evidence::query_live(&capture, &geometry)
                        }
                        None => Ok(None),
                    });
            let _ = sender.try_send(result);
        })
        .detach();
    Ok(false)
}

fn capture_report(world: &mut World, plan: &mut Plan) {
    if plan.phase == Phase::Capture {
        return;
    }
    let report = json!({"schema":1,"scope":"resident_project_view_and_preserved_saved_world_comparison",
        "passed":plan.error.is_none(),"owner_checked":true,"project_id":plan.project_id,"project_root":plan.project_root,
        "empty_fixture":plan.empty_fixture,
        "live_fixture":plan.live_fixture,"continuous_previews":plan.live_evidence,
        "semantic_tools":plan.tool_evidence,
        "preview_toolbar_controls":plan.preview_toolbar_evidence,
        "close_while_playing":plan.close_evidence,
        "host_pid":std::process::id(),"steps":plan.steps,"error":plan.error,
        "original_preview_button_chain":true,"original_creation_undo_redo_chain":true,
        "legacy_f12_stage_names_only":true,
        "game_preview_return_input":"owned HWND mouse -> actual Bevy toolbar button -> RequestMode -> ModeApplied",
        "not_verified":["physical native focus and mouse feel","full large-map performance","all unsaved author domains"],
        "host_screenshot_scope":"original editor GPU output after guest teardown; guest screenshots recorded separately"});
    let directory = plan.directory.clone();
    let (sender, receiver) = async_channel::bounded(1);
    plan.completion = Some(receiver);
    plan.advance(Phase::Capture);
    world
        .spawn(Screenshot::primary_window())
        .observe(move |captured: On<ScreenshotCaptured>| {
            let image = captured.image.clone();
            let directory = directory.clone();
            let sender = sender.clone();
            let report = report.clone();
            IoTaskPool::get()
                .spawn(async move {
                    let result = (|| -> Result<(), String> {
                        image
                            .try_into_dynamic()
                            .map_err(|error| error.to_string())?
                            .to_rgb8()
                            .save(directory.join("editor-returned.png"))
                            .map_err(|error| error.to_string())?;
                        std::fs::write(
                            directory.join("embedded-editor.json"),
                            serde_json::to_vec_pretty(&report)
                                .map_err(|error| error.to_string())?,
                        )
                        .map_err(|error| error.to_string())?;
                        Ok(())
                    })();
                    let _ = sender.try_send(result);
                })
                .detach();
        });
}
