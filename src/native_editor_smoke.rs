//! 显式环境变量启用的原生完整App启动验收；真实原UI就绪后采集GPU输出并正常退出。

#[path = "native_editor_smoke_resources.rs"]
mod resources;

use crate::{native_authoring_validation, native_play::NativePlayStatus};
use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
    window::PrimaryWindow,
};
use resources::SmokePlan;
use feathrise_editor_scene::standalone::RunSavedProject;
use feathrise_runtime::game_plugins::ScenePhase;
use feathrise_scene_core::input::EditorInputState;
use feathrise_scene_content::SceneContentRegistry;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

pub(super) fn install(
    app: &mut App,
    project_root: PathBuf,
    asset_root: PathBuf,
) -> Result<(), String> {
    let Some(directory) = std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").map(PathBuf::from) else {
        return Ok(());
    };
    if !directory.is_absolute() {
        return Err("原生编辑器验收输出目录必须是绝对路径".into());
    }
    if feathrise_editor_scene::standalone::validation_fixture::author_tools_enabled()
        && !crate::native_play_recovery_validation::enabled(app.world())
    {
        return Err("专业作者验收必须先核对本轮自有工程标记".into());
    }
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("无法创建验收输出目录：{error}"))?;
    let font = feathrise_editor_core::editor_font(app.world().resource::<AssetServer>(), feathrise_editor_core::FONT_PATH);
    let save_fixture = feathrise_editor_scene::standalone::validation_fixture::install(app);
    native_authoring_validation::install(app, project_root.clone(), asset_root.clone());
    let close_fixture = crate::native_project_close::install_validation(app, directory.clone());
    app.insert_resource(SmokePlan {
        started: Instant::now(),
        frames: 0,
        stable_frames: 0,
        scene_requested: false,
        scene_error: None,
        preview_ready: false,
        author_status: String::new(),
        save_fixture,
        save_requested: false,
        save_error: None,
        save_proof: None,
        current_author_document: None,
        font,
        directory,
        project_root,
        asset_root,
        screenshot_requested: false,
        capture_started: None,
        run_saved_play: std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_PLAY").is_some(),
        play_requested: false,
        play_completed_before: 0,
        play_result: None,
        cancel_requested: false,
        completion: Arc::new(Mutex::new(None)),
        close_fixture,
        recovery_ready: !crate::native_play_recovery_validation::enabled(app.world()),
        recovery_report: None,
        f12_release_pending: false,
    })
    .add_systems(
        Last,
        (
            open_original_scene,
            observe,
            crate::native_project_close::drive_validation,
        )
            .chain()
            .after(crate::native_play_recovery_validation::RecoverySet),
    );
    if std::env::var_os("FEATHRISE_WORKSPACE_SMOKE_DIR").is_some()
        && crate::workspace_window::EmbedTarget::from_env()?.is_some()
    {
        app.add_systems(
            Update,
            scripted_input_ownership.before(feathrise_editor_core::SceneEditorSystemSet::Input),
        );
    }
    Ok(())
}

/// 本次有限验收以ECS动作模拟作者输入，不能依赖外部桌面是否恰好把本窗口放在前台。
/// 只在自有工程夹具的一次保存/F12链中投影输入所有权；不发送原生激活或键鼠。
/// 嵌入窗口在Last前撤回该值，Last后仍按真实前台/GetFocus还原，正常会话没有此系统。
fn scripted_input_ownership(
    plan: Res<SmokePlan>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if plan.save_fixture && !plan.screenshot_requested {
        if let Ok(mut window) = windows.single_mut() {
            window.focused = true;
        }
    }
}

/// 薄入口仍走原场景选择及受检切换场景事务；只有显式有限验收才驱动，不复制作者加载算法。
fn open_original_scene(world: &mut World) {
    if world.resource::<SmokePlan>().f12_release_pending {
        emit_original_f12(world, bevy::input::ButtonState::Released);
        world.resource_mut::<SmokePlan>().f12_release_pending = false;
    }
    let plan = world.resource::<SmokePlan>();
    if !plan.scene_requested && plan.stable_frames >= 30 {
        let result = feathrise_editor_scene::standalone::request_open_scene(world, "M1-SAMPLE");
        let mut plan = world.resource_mut::<SmokePlan>();
        plan.scene_requested = true;
        plan.stable_frames = 0;
        if let Err(error) = result {
            plan.scene_error = Some(error);
        }
    }
    let mut preview_ready = feathrise_editor_scene::standalone::preview_ready_for(world, "M1-SAMPLE");
    if preview_ready && !world.resource::<SmokePlan>().save_fixture {
        match feathrise_editor_scene::standalone::validation_fixture::focus_validation_document(world) {
            Ok(ready) => preview_ready = ready,
            Err(error) => world.resource_mut::<SmokePlan>().scene_error = Some(error),
        }
    }
    let author_status = feathrise_editor_scene::standalone::author_status(world)
        .unwrap_or_default()
        .to_owned();
    let plan = world.resource::<SmokePlan>();
    if preview_ready && plan.save_fixture && !plan.save_requested && plan.stable_frames >= 30 {
        world.resource_mut::<SmokePlan>().save_requested = true;
        if let Err(error) = feathrise_editor_scene::standalone::validation_fixture::request_edit_and_save(
            world,
            "M1-SAMPLE",
        ) {
            world.resource_mut::<SmokePlan>().save_error = Some(error);
        }
    }
    let save_proof = feathrise_editor_scene::standalone::validation_fixture::result(world);
    let save_ready = !world.resource::<SmokePlan>().save_fixture
        || save_proof.as_ref().is_some_and(|proof| proof.complete);
    native_authoring_validation::drive(world, preview_ready && save_ready);
    if crate::native_play_recovery_validation::enabled(world)
        && save_proof.as_ref().is_some_and(|proof| proof.complete)
        && world
            .resource::<native_authoring_validation::NativeAuthoringValidation>()
            .report()
            .complete
        && let Err(error) = crate::native_play_recovery_validation::begin(world)
    {
        world.resource_mut::<SmokePlan>().save_error = Some(error);
    }
    let recovery_ready = crate::native_play_recovery_validation::ready(world);
    let recovery_report = crate::native_play_recovery_validation::result(world);
    let current_document = (preview_ready
        && save_ready
        && world
            .resource::<SmokePlan>()
            .current_author_document
            .is_none())
    .then(|| feathrise_editor_scene::standalone::validation_fixture::current_author_snapshot(world))
    .flatten();
    let mut plan = world.resource_mut::<SmokePlan>();
    plan.preview_ready = preview_ready;
    plan.author_status = author_status;
    plan.save_proof = save_proof;
    plan.recovery_ready = recovery_ready;
    plan.recovery_report = recovery_report;
    if current_document.is_some() {
        plan.current_author_document = current_document;
    }
}

fn observe(
    mut commands: Commands,
    mut plan: ResMut<SmokePlan>,
    fonts: Res<Assets<Font>>,
    blocks: Res<EditorInputState>,
    registry: Option<Res<SceneContentRegistry>>,
    scene: Res<State<ScenePhase>>,
    header: Query<&ComputedNode, With<feathrise_editor_scene::workbench::WorkbenchHeader>>,
    window: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &Projection, &GlobalTransform), With<Camera3d>>,
    placements: Query<(&feathrise_scene::ScenePlacementInstance, &GlobalTransform)>,
    active: Res<feathrise_scene_content::ActiveDynamicScene>,
    (play_status, code_iteration, project_logs): (
        Res<NativePlayStatus>,
        Option<Res<crate::code_iteration::CodeIterationState>>,
        Option<Res<crate::project_log::ProjectLogState>>,
    ),
    authoring: Res<native_authoring_validation::NativeAuthoringValidation>,
    mut play: MessageWriter<RunSavedProject>,
    mut exit: MessageWriter<AppExit>,
    #[cfg(all(feature = "fsr3_frame_generation", target_os = "windows"))]
    fsr: Option<Res<feathrise_fsr3_frame_generation::Fsr3FrameGenerationRuntimeStatus>>,
) {
    plan.frames = plan.frames.saturating_add(1);
    let completion = plan
        .completion
        .try_lock()
        .ok()
        .and_then(|mut result| result.take());
    if let Some(result) = completion {
        match result {
            Ok(()) => {
                if std::env::var_os("FEATHRISE_WORKSPACE_SMOKE_DIR").is_some()
                    && crate::workspace_window::EmbedTarget::from_env()
                        .is_ok_and(|target| target.is_some())
                {
                    // 同窗口有限验收需要让宿主继续真实切换/关闭此实例；GPU报告完成不代替关闭事务。
                    // 正常会话及原独立验收没有该开关，仍按原路径自行退出。
                    plan.capture_started = None;
                    return;
                }
                if plan.close_fixture {
                    commands.queue(crate::native_project_close::begin_validation);
                } else {
                    exit.write(AppExit::Success);
                }
            }
            Err(message) => {
                error!("原生完整App启动验收失败：{message}");
                exit.write(AppExit::error());
            }
        }
        return;
    }
    if plan.screenshot_requested {
        // 后台首编可能很久；截图预算从发出请求计时，不能复用整个会话的起点。
        if plan
            .capture_started
            .is_some_and(|started| started.elapsed().as_secs() > 60)
        {
            error!("原生编辑器截图或报告后台写入超时");
            exit.write(AppExit::error());
        }
        return;
    }
    let header_ready = header
        .iter()
        .any(|node| node.size().x > 100.0 && node.size().y > 10.0);
    let font_ready = fonts.contains(plan.font.id());
    let catalog_ready = registry
        .as_ref()
        .is_some_and(|registry| registry.revision > 0 && registry.last_error.is_none());
    let window_ready = window
        .single()
        .is_ok_and(|window| window.width() > 100.0 && window.height() > 100.0);
    let editor_3d_ready = plan.scene_requested && plan.scene_error.is_none() && plan.preview_ready;
    let ready = blocks.scene_editor_open
        && header_ready
        && font_ready
        && catalog_ready
        && window_ready
        && (!plan.scene_requested || editor_3d_ready);
    if ready {
        plan.stable_frames = plan.stable_frames.saturating_add(1);
    } else {
        plan.stable_frames = 0;
    }
    let save_failed = plan.save_error.is_some()
        || plan
            .save_proof
            .as_ref()
            .is_some_and(|proof| proof.error.is_some());
    let save_complete =
        !plan.save_fixture || plan.save_proof.as_ref().is_some_and(|proof| proof.complete);
    let authoring_complete = authoring.report().complete;
    let authoring_failed = authoring.report().error.is_some();
    let recovery_failed = plan.recovery_report.as_ref().is_some_and(Result::is_err);
    if plan.save_fixture
        && editor_3d_ready
        && !save_complete
        && !save_failed
        && plan.started.elapsed().as_secs() < author_budget(240)
    {
        return; // 真实作者创建、保存及交接继续在原系统推进，未得到回执就不能运行旧磁盘版本冒充通过。
    }
    if editor_3d_ready
        && !authoring_complete
        && !authoring_failed
        && !save_failed
        && !recovery_failed
        && plan.started.elapsed().as_secs() < author_budget(360)
    {
        return; // 原预设目录就绪或显式三域保存仍在推进；F12只能读取三个原保存回执全部确认后的工程。
    }
    if plan.run_saved_play
        && !plan.recovery_ready
        && !recovery_failed
        && !save_failed
        && !authoring_failed
        && plan.started.elapsed().as_secs() < author_budget(360)
    {
        return; // 自有故障注入仍在恢复原字节；不能提前运行或把“尚未恢复”记为正式工程运行成功。
    }
    if plan.run_saved_play
        && save_complete
        && authoring_complete
        && !authoring_failed
        && plan.recovery_ready
        && editor_3d_ready
        && plan.stable_frames >= 30
        && !plan.play_requested
    {
        // 给本App官方键盘消息队列投递F12，穿过原toggle_scene_editor和保存发布门，不绕到宿主意图。
        // 此项仍不声称物理键盘/系统焦点验收；正常会话没有夹具资源。
        plan.play_requested = true;
        plan.play_completed_before = play_status.completed;
        commands.queue(|world: &mut World| {
            emit_original_f12(world, bevy::input::ButtonState::Pressed);
            world.resource_mut::<SmokePlan>().f12_release_pending = true;
        });
        return;
    }
    if plan.play_requested && plan.play_result.is_none() {
        if play_status.completed > plan.play_completed_before && !play_status.active {
            plan.play_result = Some(
                play_status
                    .last_result
                    .clone()
                    .unwrap_or_else(|| Err("已退出但缺少工程运行结果".into())),
            );
            plan.stable_frames = 0;
            return; // 给原状态栏和焦点恢复完整渲染帧，再采集“返回后”画面。
        } else {
            if plan.started.elapsed().as_secs() > 1800 && !plan.cancel_requested {
                plan.cancel_requested = true;
                play.write(RunSavedProject {
                    profile: default(),
                    scene_id: None,
                    has_unsaved_changes: false,
                    stop_only: true,
                });
            }
            return; // 编译与游戏运行期间继续绘制；只有后台槽已收口才采集报告并结束App。
        }
    }
    let timed_out = plan.scene_error.is_some()
        || save_failed
        || authoring_failed
        || recovery_failed
        || (plan.started.elapsed().as_secs()
            >= if plan.scene_requested {
                author_budget(360)
            } else {
                60
            }
            && !plan.play_requested);
    if (plan.stable_frames < 30 || !editor_3d_ready) && !timed_out {
        return;
    }

    // 首轮明确注入了坏工程入口，保留其代码状态供分析；干净打开/重开不能带着代码失败仍计为通过。
    let injected_recovery = plan.recovery_report.as_ref().and_then(|report| report.as_ref().ok())
        .is_some_and(|report| report.failure_observed && report.descriptor_restored && report.owner_marker_checked);
    let code_iteration_error = match code_iteration.as_deref() {
        Some(state) if state.phase == crate::code_iteration::CodeIterationPhase::Failed && !injected_recovery => Some(state.status.as_str()),
        None => Some("原完整编辑器未安装代码迭代状态，无法核对打开后的后台服务"),
        _ => None,
    };
    let passed = ready
        && editor_3d_ready
        && save_complete
        && !save_failed
        && authoring_complete
        && !authoring_failed
        && plan.recovery_ready
        && !recovery_failed
        && code_iteration_error.is_none()
        && (!plan.run_saved_play || plan.play_result.as_ref().is_some_and(Result::is_ok))
        && !plan.cancel_requested;
    let instances = placements.iter().filter(|(instance, _)| instance.scene_id().as_str() == "M1-SAMPLE" && instance.is_active())
        .take(128).map(|(instance, transform)| json!({"id":instance.id(),"position":transform.translation().to_array(),"asset_target":instance.asset_target()})).collect::<Vec<_>>();
    let mut report = json!({
        "schema": 2,
        "scope": if plan.run_saved_play { "native_full_editor_saved_game_launcher_roundtrip" } else { "native_full_original_editor_startup_only" },
        "passed": passed,
        "project_root": plan.project_root,
        "asset_root": plan.asset_root,
        "cwd": std::env::current_dir().ok(),
        "sdk_ui_root": feathrise_editor_core::sdk_asset_root(),
        "source_ui": "feathrise_editor_scene::SceneEditorPlugin original scene and author systems",
        "excluded_scope": "player initialization and RPG author/runtime capabilities",
        "plugins": ["SceneRuntimePlugin", "FpsDisplayPlugin", "SceneEditorPlugin", "SurfaceEditorPlugin", "DocumentsEditorPlugin", "WeightPanelPlugin", "GlbEditorPlugin", "StandaloneEditorPlugin", "NativePlayPlugin"],
        "editor_open": blocks.scene_editor_open,
        "original_header_layout_ready": header_ready,
        "font_loaded": font_ready,
        "project_catalog_ready": catalog_ready,
        "catalog_revision": registry.as_ref().map(|registry| registry.revision),
        "scene_count": registry.as_ref().map(|registry| registry.scenes.len()),
        "catalog_error": registry.as_ref().and_then(|registry| registry.last_error.as_deref()),
        "state": format!("{:?}", scene.get()),
        "camera_3d_count": cameras.iter().count(),
    });
    // 报告按验收域逐项组装，保留既有扁平字段，避免单个 json! 宏的递归深度随字段增长。
    // 仅最终报告采集一次有界快照，不在每帧复制日志；Watcher/工具探测失败也保留原 status，而非只看编译诊断。
    report["code_iteration"] = code_iteration.as_deref().map(|state| json!({
        "project_id":state.project_id,"project_root":state.project_root,
        "phase":format!("{:?}",state.phase),"status":state.status,
        "diagnostic_revision":state.diagnostic_revision,"diagnostic_count":state.diagnostics.len(),
        "diagnostics":state.diagnostics.iter().take(32).collect::<Vec<_>>(),
        "diagnostics_truncated":state.diagnostics.len() > 32,
        "failure_injected_recovery_observed":injected_recovery,
    })).unwrap_or(serde_json::Value::Null);
    report["code_iteration_error"] = json!(code_iteration_error);
    report["project_log_tail"] = project_logs.as_deref().map(crate::project_log::ProjectLogState::validation_tail)
        .unwrap_or(serde_json::Value::Null);
    report["editor_views"] = json!(cameras.iter().take(8).map(|(camera, projection, transform)| json!({
            "active":camera.is_active,"position":transform.translation().to_array(),
            "projection":format!("{projection:?}"),
        })).collect::<Vec<_>>());
    report["editor_3d_ready"] = json!(editor_3d_ready);
    report["scene_request_error"] = json!(plan.scene_error);
    report["author_status"] = json!(plan.author_status);
    report["author_save_requested"] = json!(plan.save_requested);
    report["author_save_error"] = json!(plan.save_error);
    report["author_save"] = json!(plan.save_proof);
    report["current_author_document"] = json!(plan.current_author_document);
    report["preset_authoring"] = json!(authoring.report());
    report["saved_play_recovery"] = json!(plan.recovery_report);
    report["placement_instances"] = json!(instances);
    report["active_scene"] = json!(active.scene_id.as_ref().map(|id| &id.0));
    report["saved_play_requested"] = json!(plan.play_requested);
    report["saved_play_result"] = json!(plan.play_result);
    report["saved_play_started"] = json!(play_status.started);
    report["saved_play_completed"] = json!(play_status.completed);
    report["saved_play_active_after_return"] = json!(play_status.active);
    report["saved_play_exit_code"] = json!(play_status.last_exit_code);
    report["frames"] = json!(plan.frames);
    report["elapsed_ms"] = json!(plan.started.elapsed().as_secs_f64() * 1000.0);
    report["not_verified"] = json!(["all professional tools beyond the explicit author_save and preset_authoring reports", "preset directory physical hit testing and IME entry", "physical F12 keyboard and native folder dialog", "same-World and unsaved instant play"]);
    report["rpg_author_plugins_installed"] = json!(false);
    // 分发验收记录实际运行事实；DLL 加载、设备附着和真实 2x 分别报告，不以画面就绪代替插帧。
    #[cfg(all(feature = "fsr3_frame_generation", target_os = "windows"))]
    {
        report["fsr_frame_generation"] = fsr.as_deref().map(|status| json!({
            "compiled": true, "phase": format!("{:?}", status.phase),
            "requested": status.requested, "runtime_loaded": status.runtime_loaded,
            "device_attached": status.device_attached, "swapchain_ready": status.swapchain_ready,
            "context_ready": status.context_ready, "inputs_ready": status.inputs_ready,
            "frames_presented": status.frames_presented, "total_present_count": status.total_present_count,
            "effective_2x": status.is_effective_2x(),
            "blocker": status.blocker.as_ref().map(|blocker| blocker.message()), "detail": status.detail,
        })).unwrap_or_else(|| json!({"compiled": true, "status_installed": false}));
    }
    #[cfg(not(all(feature = "fsr3_frame_generation", target_os = "windows")))]
    { report["fsr_frame_generation"] = json!({"compiled": false}); }
    report["scripted_input_ownership"] =
        json!(plan.save_fixture && std::env::var_os("FEATHRISE_WORKSPACE_SMOKE_DIR").is_some());
    let directory = plan.directory.clone();
    let completion = plan.completion.clone();
    plan.screenshot_requested = true;
    plan.capture_started = Some(Instant::now());
    // 截图是该完整App自身交换链输出，包含编辑器自有3D；不读取或自动操纵桌面窗口。
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>| {
            let image = captured.image.clone();
            let directory = directory.clone();
            let completion = completion.clone();
            let report = report.clone();
            IoTaskPool::get()
                .spawn(async move {
                    let result = (|| -> Result<(), String> {
                        image
                            .try_into_dynamic()
                            .map_err(|error| format!("GPU截图转换失败：{error}"))?
                            .to_rgb8()
                            .save(directory.join("native-editor.png"))
                            .map_err(|error| error.to_string())?;
                        std::fs::write(
                            directory.join("native-editor.json"),
                            serde_json::to_vec_pretty(&report)
                                .map_err(|error| error.to_string())?,
                        )
                        .map_err(|error| error.to_string())?;
                        if report["passed"] != true {
                            // 子域失败应保留原原因，不能被统一的“界面未就绪”掩盖。
                            let reason = report["scene_request_error"]
                                .as_str()
                                .or_else(|| report["author_save_error"].as_str())
                                .or_else(|| report["author_save"]["error"].as_str())
                                .or_else(|| report["preset_authoring"]["error"].as_str())
                                .or_else(|| report["saved_play_recovery"]["Err"].as_str())
                                .or_else(|| report["saved_play_result"]["Err"].as_str())
                                .or_else(|| report["code_iteration_error"].as_str())
                                .unwrap_or("原界面、字体、工程目录或作者回执未在时限内同时就绪");
                            return Err(format!("原完整编辑器验收未通过：{reason}"));
                        }
                        Ok(())
                    })();
                    if let Ok(mut slot) = completion.lock() {
                        *slot = Some(result);
                    }
                })
                .detach();
        },
    );
}

fn emit_original_f12(world: &mut World, state: bevy::input::ButtonState) {
    if let Ok(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
    {
        world.write_message(bevy::input::keyboard::KeyboardInput {
            key_code: KeyCode::F12,
            logical_key: bevy::input::keyboard::Key::F12,
            state,
            text: None,
            repeat: false,
            window,
        });
    }
}

/// 只有显式六域作者验收扩大有限等待预算；常规启动和后台构建预算保持原合同。
fn author_budget(ordinary: u64) -> u64 {
    if feathrise_editor_scene::standalone::validation_fixture::author_tools_enabled() {
        720
    } else {
        ordinary
    }
}
