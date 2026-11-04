//! 仅自有有限验收启用的F12失败恢复：后台注入坏入口、确认失败清槽、原字节恢复，同一作者World继续。

#[path = "native_play_recovery_components.rs"]
mod components;
#[path = "native_play_recovery_resources.rs"]
mod resources;
#[path = "native_play_recovery_types.rs"]
mod types;

use crate::{
    native_authoring_validation::NativeAuthoringValidation, native_play::NativePlayStatus,
    project::Project,
};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
};
use components::WorldMarker;
use resources::Recovery;
pub(crate) use resources::RecoverySet;
use feathrise_editor_map::standalone::{RunSavedProject, validation_fixture};
use feathrise_editor_surface_model::{
    SurfacePresetEditorState, components::SurfacePresetButton, types::SurfacePresetAction,
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
pub(crate) use types::RecoveryReport;
use types::{AuthorSnapshot, Phase, SurfaceSession};

pub(crate) const OWNER_FILE: &str = ".new-editor-recovery-owner.json";
const BACKUP_FILE: &str = ".new-editor-recovery-original.json";
const DESCRIPTOR: &str = "editor-project.json";
const LIMIT: u64 = 64 * 1024;

/// 同时要求显式开关、原生验收目录和匹配本工程的普通小标记；正常会话没有资源或系统。
pub(crate) fn install(app: &mut App, project: &Project) -> Result<(), String> {
    if std::env::var("NEW_EDITOR_NATIVE_SMOKE_RECOVERY").as_deref() != Ok("1") {
        return Ok(());
    }
    if std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").is_none() {
        return Err("失败恢复注入必须同时提供原生验收目录".into());
    }
    check_owner(project)?;
    let original = read_owned(&project.root, DESCRIPTOR, LIMIT)?;
    let mut value: Value = serde_json::from_slice(&original).map_err(|error| error.to_string())?;
    if value["game_entry"]["kind"] != "saved-world" || value["game_entry"]["version"] != 1 {
        return Err("失败恢复夹具必须从有效的正式工程实例入口开始".into());
    }
    value["game_entry"] = json!({"kind":"native-recovery-invalid-entry","version":0});
    let injected = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    if project.root.join(BACKUP_FILE).exists() {
        return Err("失败恢复原字节备份已存在，拒绝重用上一轮现场".into());
    }
    app.insert_resource(Recovery {
        project: project.clone(),
        original: Arc::new(original),
        injected: Arc::new(injected),
        phase: Phase::Idle,
        started: Instant::now(),
        baseline_started: 0,
        baseline_completed: 0,
        pulse: None,
        requested_surface: None,
        snapshot: None,
        io: None,
        error: None,
        cancel_sent: false,
        report: RecoveryReport {
            schema: 2,
            scope: "ordinary_surface_session_and_existing_map_history_metadata",
            complete: false,
            failure_observed: false,
            failed_attempt_started: 0,
            failed_attempt_completed: 0,
            failed_attempt_error: None,
            descriptor_restored: false,
            same_world_marker: false,
            original_save_proof_preserved: false,
            map_history_metadata_preserved: false,
            surface_session_preserved: false,
            marker_entity: String::new(),
            owner_marker_checked: true,
        },
    })
    .add_systems(Last, drive.in_set(RecoverySet));
    Ok(())
}

pub(crate) fn enabled(world: &World) -> bool {
    world.contains_resource::<Recovery>()
}

pub(crate) fn owned_project(world: &World) -> Option<Project> {
    world
        .get_resource::<Recovery>()
        .map(|state| state.project.clone())
}

/// 地图与普通地表保存都完成后开始；实际历史留在原地图，不伪造游戏技能redo哨兵。
pub(crate) fn begin(world: &mut World) -> Result<(), String> {
    let Some(state) = world.get_resource::<Recovery>() else {
        return Err("失败恢复夹具未启用".into());
    };
    if state.phase != Phase::Idle {
        return Ok(());
    }
    let saved = validation_fixture::result(world).ok_or("失败恢复前缺少原保存proof")?;
    if !saved.complete || saved.error.is_some() {
        return Err("原地图保存尚未完成，不能注入失败".into());
    }
    let authoring = world
        .get_resource::<NativeAuthoringValidation>()
        .ok_or("缺少普通地表作者验收")?
        .report();
    if !authoring.complete || authoring.error.is_some() {
        return Err("普通地表保存尚未完成，不能注入失败".into());
    }
    if world.resource::<NativePlayStatus>().active {
        return Err("失败恢复开始前已有工程运行，拒绝注入".into());
    }
    let mut state = world.resource_mut::<Recovery>();
    state.phase = Phase::SelectSurface;
    state.started = Instant::now();
    Ok(())
}

pub(crate) fn ready(world: &World) -> bool {
    !enabled(world) || result(world).is_some_and(|result| result.is_ok())
}

pub(crate) fn result(world: &World) -> Option<Result<RecoveryReport, String>> {
    let state = world.get_resource::<Recovery>()?;
    match state.phase {
        Phase::Ready => Some(verify_snapshot(world, state).map(|_| state.report.clone())),
        Phase::Failed => Some(Err(state
            .error
            .clone()
            .unwrap_or_else(|| "失败恢复未完成".into()))),
        _ => None,
    }
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut state: Mut<Recovery>| {
        if let Some(entity) = state.pulse.take() {
            let _ = world.despawn(entity);
        }
        if matches!(state.phase, Phase::Idle | Phase::Ready | Phase::Failed) {
            return;
        }
        if state.started.elapsed() > Duration::from_secs(90) && state.error.is_none() {
            state.error = Some(format!("失败恢复验收超时，阶段{:?}", state.phase));
        }
        if state.error.is_some()
            && matches!(
                state.phase,
                Phase::SelectSurface
                    | Phase::OpenSurface
                    | Phase::WaitSurface
                    | Phase::WaitMapPreview
            )
        {
            state.phase = Phase::Failed;
            return;
        }
        let step = (|| -> Result<(), String> {
            match state.phase {
                Phase::SelectSurface => {
                    if world
                        .get_resource::<feathrise_editor_core::EditorTextInputGuard>()
                        .is_some_and(|guard| guard.block_shortcuts)
                        || world
                            .get_resource::<feathrise_editor_ui::ActiveNumericInput>()
                            .is_some_and(|input| input.entity.is_some())
                    {
                        return Ok(());
                    }
                    if feathrise_editor_map::standalone::saved_play_blocker(world).is_some() {
                        return Ok(());
                    }
                    let registry = world
                        .get_resource::<feathrise_map_content::SurfacePresetRegistry>()
                        .ok_or("普通地表目录缺失")?;
                    let id = registry
                        .presets()
                        .first()
                        .ok_or("普通地表目录为空")?
                        .id
                        .clone();
                    state.requested_surface = Some(id);
                    state.pulse = Some(
                        world
                            .spawn((
                                SurfacePresetButton(SurfacePresetAction::SelectPreset(0)),
                                feathrise_editor_ui::EditorButtonClick::triggered(),
                            ))
                            .id(),
                    );
                    state.phase = Phase::OpenSurface;
                }
                Phase::OpenSurface => {
                    state.pulse = Some(
                        world
                            .spawn((
                                SurfacePresetButton(SurfacePresetAction::EditSelected),
                                feathrise_editor_ui::EditorButtonClick::triggered(),
                            ))
                            .id(),
                    );
                    state.phase = Phase::WaitSurface;
                }
                Phase::WaitSurface => {
                    let surfaces = world
                        .get_resource::<SurfacePresetEditorState>()
                        .ok_or("普通地表会话缺失")?;
                    let requested = state
                        .requested_surface
                        .as_deref()
                        .ok_or("恢复夹具缺少地表身份")?;
                    let Some(tab) = surfaces
                        .tabs
                        .iter()
                        .find(|tab| tab.draft.descriptor.id == requested)
                    else {
                        return Ok(());
                    };
                    if tab.draft.dirty() || surfaces.busy() {
                        return Err("失败恢复前地表保存尚未完成".into());
                    }
                    let surface = surface_session(tab);
                    let map = validation_fixture::recovery_author_snapshot(world)
                        .ok_or("缺少有界真实地图历史快照")?;
                    // 原地图保存成功会清空历史；此处只证明保存后实际元数据不变。
                    // 非空笔划的Undo/Redo由前置map_tools逐笔验证，不伪造保存后历史。
                    let marker = world.spawn(WorldMarker).id();
                    let saved = serde_json::to_value(
                        validation_fixture::result(world).ok_or("原保存proof丢失")?,
                    )
                    .map_err(|error| error.to_string())?;
                    let authoring = serde_json::to_value(
                        world.resource::<NativeAuthoringValidation>().report(),
                    )
                    .map_err(|error| error.to_string())?;
                    state.snapshot = Some(AuthorSnapshot {
                        marker,
                        saved,
                        authoring,
                        surface,
                        map,
                    });
                    state.report.marker_entity = format!("{marker:?}");
                    state.baseline_started = world.resource::<NativePlayStatus>().started;
                    state.baseline_completed = world.resource::<NativePlayStatus>().completed;
                    let project = state.project.clone();
                    let original = state.original.clone();
                    let injected = state.injected.clone();
                    state.io = Some(IoTaskPool::get().spawn(async move {
                        check_owner(&project)?;
                        // 先保留原字节，异常中断仍可人工恢复这份自有验收工程。
                        let backup = project.root.join(BACKUP_FILE);
                        let mut file = fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(backup)
                            .map_err(|error| error.to_string())?;
                        use std::io::Write;
                        file.write_all(&original)
                            .map_err(|error| error.to_string())?;
                        file.sync_all().map_err(|error| error.to_string())?;
                        replace_expected(&project, &original, &injected)
                    }));
                    state.phase = Phase::Injecting;
                }
                Phase::Injecting => {
                    let Some(result) = state.io.as_mut().and_then(check_ready) else {
                        return Ok(());
                    };
                    state.io = None;
                    result?;
                    if state.error.is_some() {
                        start_restore(&mut state);
                        return Ok(());
                    }
                    verify_snapshot(world, &state)?;
                    world.write_message(RunSavedProject {
                        profile: default(),
                        map_id: Some("M1-SAMPLE".into()),
                        has_unsaved_changes: false,
                        stop_only: false,
                    });
                    state.phase = Phase::WaitFailure;
                }
                Phase::WaitFailure => {
                    let status = world.resource::<NativePlayStatus>();
                    if status.completed > state.baseline_completed && !status.active {
                        state.report.failed_attempt_started = status.started;
                        state.report.failed_attempt_completed = status.completed;
                        state.report.failed_attempt_error = status
                            .last_result
                            .as_ref()
                            .and_then(|result| result.as_ref().err())
                            .cloned();
                        state.report.failure_observed = status.started
                            == state.baseline_started + 1
                            && state
                                .report
                                .failed_attempt_error
                                .as_deref()
                                .is_some_and(|error| error.contains("工程入口"));
                        if !state.report.failure_observed {
                            state.error = Some("注入坏入口后没有观察到唯一正式工程运行失败回执".into());
                        }
                        start_restore(&mut state);
                    } else if state.error.is_some() {
                        if status.active && !state.cancel_sent {
                            world.write_message(RunSavedProject {
                                profile: default(),
                                map_id: None,
                                has_unsaved_changes: false,
                                stop_only: true,
                            });
                            state.cancel_sent = true;
                        } else if !status.active {
                            start_restore(&mut state);
                        }
                    }
                }
                Phase::Restoring => {
                    let Some(result) = state.io.as_mut().and_then(check_ready) else {
                        return Ok(());
                    };
                    state.io = None;
                    result?;
                    state.report.descriptor_restored = true;
                    verify_snapshot(world, &state)?;
                    state.report.same_world_marker = true;
                    state.report.original_save_proof_preserved = true;
                    state.report.map_history_metadata_preserved = true;
                    state.report.surface_session_preserved = true;
                    if state.error.is_some() {
                        state.phase = Phase::Failed;
                    } else {
                        // 地表标签与地图历史仍留在同一作者World；原标签动作恢复画布后才继续F12。
                        state.pulse = Some(validation_fixture::request_map_tab(world)?);
                        state.phase = Phase::WaitMapPreview;
                    }
                }
                Phase::WaitMapPreview => {
                    if feathrise_editor_map::standalone::preview_ready_for(world, "M1-SAMPLE") {
                        verify_snapshot(world, &state)?;
                        state.report.complete = true;
                        state.phase = Phase::Ready;
                    }
                }
                _ => (),
            }
            Ok(())
        })();
        if let Err(error) = step {
            state.error = Some(error);
            if state.phase == Phase::Injecting || state.phase == Phase::WaitFailure {
                start_restore(&mut state);
            } else {
                state.phase = Phase::Failed;
            }
        }
    });
}

fn start_restore(state: &mut Recovery) {
    let project = state.project.clone();
    let original = state.original.clone();
    let injected = state.injected.clone();
    state.io = Some(
        IoTaskPool::get().spawn(async move { replace_expected(&project, &injected, &original) }),
    );
    state.phase = Phase::Restoring;
}

fn surface_session(tab: &feathrise_editor_surface_model::types::SurfaceTab) -> SurfaceSession {
    SurfaceSession {
        tab: tab.id,
        descriptor: tab.draft.descriptor.clone(),
        baseline: tab.draft.baseline.clone(),
        section: tab.section,
        close_confirmation: tab.close_confirmation,
    }
}

fn verify_snapshot(world: &World, state: &Recovery) -> Result<(), String> {
    let snapshot = state.snapshot.as_ref().ok_or("失败恢复前作者快照缺失")?;
    if world.get::<WorldMarker>(snapshot.marker).is_none() {
        return Err("失败后原World的同一Entity标记丢失".into());
    }
    let saved =
        serde_json::to_value(validation_fixture::result(world).ok_or("失败后原保存proof丢失")?)
            .map_err(|error| error.to_string())?;
    if saved != snapshot.saved {
        return Err("失败后原保存proof被替换或改写".into());
    }
    let authoring = serde_json::to_value(world.resource::<NativeAuthoringValidation>().report())
        .map_err(|error| error.to_string())?;
    if authoring != snapshot.authoring {
        return Err("失败后普通地表保存证据改变".into());
    }
    let tab = world
        .resource::<SurfacePresetEditorState>()
        .tabs
        .iter()
        .find(|tab| tab.id == snapshot.surface.tab)
        .ok_or("失败后普通地表标签丢失")?;
    if surface_session(tab) != snapshot.surface {
        return Err("失败后普通地表草稿/保存基线/分区被改写".into());
    }
    if validation_fixture::recovery_author_snapshot(world).as_ref() != Some(&snapshot.map) {
        return Err("失败后原地图内容或真实作者历史被改写".into());
    }
    Ok(())
}

pub(crate) fn check_owner(project: &Project) -> Result<(), String> {
    let bytes = read_owned(&project.root, OWNER_FILE, 4096)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let root = value["project_root"]
        .as_str()
        .map(PathBuf::from)
        .ok_or("自有恢复标记缺少工程根")?;
    if value["schema"] != 1
        || value["purpose"] != "native-play-recovery"
        || value["project_id"].as_str() != Some(project.id.as_str())
        || root.canonicalize().map_err(|error| error.to_string())? != project.root
    {
        return Err("自有恢复标记不属于当前工程，拒绝故障注入".into());
    }
    Ok(())
}

pub(crate) fn read_owned(root: &Path, name: &str, maximum: u64) -> Result<Vec<u8>, String> {
    let path = root.join(name);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("恢复夹具文件不可读：{}：{error}", path.display()))?;
    #[cfg(windows)]
    let ordinary = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 == 0
    };
    #[cfg(not(windows))]
    let ordinary = !metadata.file_type().is_symlink();
    if !ordinary
        || !metadata.is_file()
        || metadata.len() > maximum
        || path.canonicalize().map_err(|error| error.to_string())? != path
    {
        return Err("恢复夹具必须使用本工程内普通、有界、非链接文件".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > maximum {
        return Err("恢复夹具文件读取期间超出预算".into());
    }
    Ok(bytes)
}

fn replace_expected(project: &Project, expected: &[u8], next: &[u8]) -> Result<(), String> {
    check_owner(project)?;
    if read_owned(&project.root, DESCRIPTOR, LIMIT)? == next {
        return Ok(());
    }
    let path = project.root.join(DESCRIPTOR);
    feathrise_map_content::authoring_transaction::commit_prepared_writes_checked(
        vec![
            feathrise_map_content::authoring_transaction::PreparedWrite::new(
                path,
                next.to_vec(),
            ),
        ],
        |_, _| {
            check_owner(project)?;
            if read_owned(&project.root, DESCRIPTOR, LIMIT)? != expected {
                return Err("恢复入口字节被外部改变，拒绝覆盖".into());
            }
            Ok(())
        },
    )
    .map_err(|error| error.message().to_owned())?;
    if read_owned(&project.root, DESCRIPTOR, LIMIT)? != next {
        return Err("恢复入口原字节未确认发布".into());
    }
    Ok(())
}
