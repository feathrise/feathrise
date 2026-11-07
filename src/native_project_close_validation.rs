//! 在本轮自建工程真实App中验证关闭取消、保存失败及重试；原App真正返回后才落退出证据。

#[path = "native_project_close_validation_resources.rs"]
mod resources;
use super::{
    components::CloseAction,
    resources::{CloseStage, NativeProjectCloseState},
};
use crate::native_validation::{CLOSE_MAP_MANIFEST_PATH, MAP_BEFORE_CLOSE_FILENAME};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, futures::check_ready},
    window::{PrimaryWindow, WindowCloseRequested},
};
pub(crate) use resources::ValidationReport;
use resources::*;
use feathrise_editor_map::standalone::{ProjectSaveProgress, project_save_progress, validation_fixture};
use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) fn install(app: &mut App, directory: PathBuf) -> bool {
    if std::env::var("NEW_EDITOR_NATIVE_SMOKE_CLOSE").as_deref() != Ok("1") {
        return false;
    }
    // 恢复夹具安装时已经核对规范工程路径、UUID和独占标记；关闭验收仅共用这份自有租约。
    if !crate::native_play_recovery_validation::enabled(app.world()) {
        return false;
    }
    let Some(project) = crate::native_play_recovery_validation::owned_project(app.world()) else {
        return false;
    };
    let Ok((asset_root, original)) = (|| -> Result<_, String> {
        let asset_root = project
            .context
            .asset_roots
            .first()
            .ok_or("验收工程缺少资产根")?
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let original = crate::native_play_recovery_validation::read_owned(
            &asset_root,
            validation_fixture::CLOSE_DOCUMENT_PATH,
            4096,
        )?;
        Ok((asset_root, original))
    })() else {
        return false;
    };
    app.insert_resource(Fixture {
        phase: Phase::Idle,
        report: ValidationReport {
            directory,
            proof: Arc::new(Mutex::new(None)),
        },
        started: Instant::now(),
        project,
        asset_root,
        original: Arc::new(original),
        draft: None,
        map_save_before: None,
        marker: None,
        io: None,
        pulse: None,
        capture: Arc::new(Mutex::new(None)),
        cancel_verified: false,
        failure_verified: false,
        clean_prompt_started: None,
        clean_cancel_verified: false,
    });
    true
}

pub(crate) fn completion(world: &World) -> Option<ValidationReport> {
    world
        .get_resource::<Fixture>()
        .map(|fixture| fixture.report.clone())
}

impl ValidationReport {
    /// 此处已经离开Bevy帧与Winit循环，允许同步写这份固定小报告；失败不能伪装正常退出。
    pub(crate) fn finish(self, exit: &AppExit) -> Result<(), String> {
        if *exit != AppExit::Success {
            return Err(format!("工程关闭验收未正常退出：{exit:?}"));
        }
        let mut proof = self
            .proof
            .lock()
            .map_err(|_| "关闭验收证据锁损坏")?
            .take()
            .ok_or("工程App已返回，但缺少关闭完成证据")?;
        proof["app_run_returned"] = true.into();
        std::fs::write(
            self.directory.join("project-close.json"),
            serde_json::to_vec_pretty(&proof).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())
    }
}

/// 只有原App截图与其他作者验收已经写好，才请求真正关闭本工程。
pub(crate) fn begin(world: &mut World) {
    if let Err(error) = start(world) {
        error!("工程关闭验收不能启动：{error}");
        world.write_message(AppExit::error());
    }
}

fn start(world: &mut World) -> Result<(), String> {
    if world.resource::<Fixture>().phase != Phase::Idle {
        return Ok(());
    }
    validation_fixture::request_close_document(world)?;
    let mut fixture = world.resource_mut::<Fixture>();
    fixture.phase = Phase::LoadingDocument;
    fixture.started = Instant::now();
    Ok(())
}

fn request_window_close(world: &mut World) -> Result<(), String> {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .map_err(|error| error.to_string())?;
    world.write_message(WindowCloseRequested { window });
    Ok(())
}

fn choose(world: &mut World, action: CloseAction) -> Result<Entity, String> {
    let button = world
        .query::<(Entity, &CloseAction, Has<bevy::ui::InteractionDisabled>)>()
        .iter(world)
        .find_map(|(entity, value, disabled)| (*value == action && !disabled).then_some(entity))
        .ok_or("原工程关闭确认按钮尚未可用")?;
    world.entity_mut(button).insert((
        Interaction::Pressed,
        feathrise_editor_ui::EditorButtonClick::triggered(),
    ));
    Ok(button)
}

fn capture_prompt(world: &mut World, fixture: &Fixture, filename: &'static str) {
    let directory = fixture.report.directory.clone();
    let capture = fixture.capture.clone();
    world.spawn(Screenshot::primary_window()).observe(move |image: On<ScreenshotCaptured>| {
        let image = image.image.clone();
        let directory = directory.clone(); let capture = capture.clone();
        IoTaskPool::get().spawn(async move {
            let result = image.try_into_dynamic().map_err(|error| error.to_string())
                .and_then(|image| image.to_rgb8().save(directory.join(filename)).map_err(|error| error.to_string()));
            if let Ok(mut completion) = capture.lock() { *completion = Some(result); }
        }).detach();
    });
}

fn retained(world: &World, fixture: &Fixture) -> bool {
    fixture
        .marker
        .is_some_and(|marker| world.get_entity(marker).is_ok())
        && fixture.draft.as_ref().is_some_and(|expected| {
            validation_fixture::close_document_snapshot(world).as_ref() == Some(expected)
                && expected["dirty"] == true
                && expected["can_undo"] == true
                && expected["can_redo"] == true
        })
}

fn change_owned_file(fixture: &mut Fixture, restore: bool) {
    let project = fixture.project.clone();
    let root = fixture.asset_root.clone();
    let original = fixture.original.clone();
    let manifest_before_close = (!restore)
        .then(|| fixture.report.directory.join(MAP_BEFORE_CLOSE_FILENAME));
    fixture.io = Some(IoTaskPool::get().spawn(async move {
        use crate::native_play_recovery_validation::{check_owner, read_owned};
        check_owner(&project)?;
        if let Some(output) = manifest_before_close {
            // 保存并退出尚未写盘。保留本工程当时的字节，以便回验上一轮 F12 实例的来源。
            let bytes = read_owned(&root, CLOSE_MAP_MANIFEST_PATH, 64 * 1024)?;
            std::fs::File::create_new(output)
                .and_then(|mut file| file.write_all(&bytes))
                .map_err(|error| format!("关闭前地图清单快照无法写入：{error}"))?;
        }
        let conflict = "关闭验收的外部冲突版本\n".as_bytes();
        let (expected, next) = if restore {
            (conflict, original.as_slice())
        } else {
            (original.as_slice(), conflict)
        };
        // 仅改变本轮自有文本，复用生产提交前身份检查；正常作者目录不能进入此夹具。
        feathrise_map_content::authoring_transaction::commit_prepared_writes_checked(
            vec![
                feathrise_map_content::authoring_transaction::PreparedWrite::new(
                    root.join(validation_fixture::CLOSE_DOCUMENT_PATH),
                    next.to_vec(),
                ),
            ],
            |_, _| {
                check_owner(&project)?;
                if read_owned(&root, validation_fixture::CLOSE_DOCUMENT_PATH, 4096)? != expected {
                    return Err("关闭验收文件被其他写者改变，拒绝覆盖".into());
                }
                Ok(())
            },
        )
        .map_err(|error| error.message().to_owned())?;
        Ok(())
    }));
}

/// 必须放在有限smoke的Last末尾；普通编辑器没有Fixture时立即返回。
pub(crate) fn drive(world: &mut World) {
    if !world.contains_resource::<Fixture>() {
        return;
    }
    let result = world.resource_scope(|world, mut fixture: Mut<Fixture>| -> Result<(), String> {
        if let Some(button) = fixture.pulse.take() {
            if let Ok(mut entity) = world.get_entity_mut(button) {
                entity.insert((Interaction::None, feathrise_editor_ui::EditorButtonClick::default()));
            }
        }
        if matches!(fixture.phase, Phase::Idle | Phase::Complete) { return Ok(()); }
        if fixture.started.elapsed() > Duration::from_secs(90) {
            return Err(format!("工程关闭验收超时：{:?}，{}", fixture.phase, world.resource::<NativeProjectCloseState>().status));
        }
        let stage = world.resource::<NativeProjectCloseState>().stage;
        match fixture.phase {
            Phase::LoadingDocument => {
                if validation_fixture::close_document_ready(world)? {
                    if !feathrise_editor_map::standalone::project_close_snapshot(world).unsaved.is_empty() {
                        return Err("清洁工程关闭验收开始前仍有未保存内容".into());
                    }
                    fixture.marker = Some(world.spawn_empty().id());
                    fixture.clean_prompt_started = Some(Instant::now());
                    fixture.phase = Phase::CleanPrompt;
                    request_window_close(world)?;
                }
            }
            Phase::CleanPrompt if stage == CloseStage::Confirm => {
                if !world.resource::<NativeProjectCloseState>().documents.is_empty() {
                    return Err("清洁工程确认层误报未保存内容".into());
                }
                // 连续真实帧不提供选择，必须一直等待；旧实现会直接发退出，不可能走到截图。
                if fixture.clean_prompt_started.is_some_and(|started| started.elapsed() >= Duration::from_millis(600)) {
                    capture_prompt(world, &fixture, "project-close-clean.png");
                    fixture.phase = Phase::CleanCapturing;
                }
            }
            Phase::CleanCapturing => {
                if stage != CloseStage::Confirm { return Err("清洁工程在未选择时自行关闭".into()); }
                let captured = fixture.capture.try_lock().ok().and_then(|mut result| result.take());
                if let Some(result) = captured {
                    result?;
                    fixture.pulse = Some(choose(world, CloseAction::Cancel)?);
                    fixture.phase = Phase::CleanCancelled;
                }
            }
            Phase::CleanCancelled if stage == CloseStage::Idle => {
                if !fixture.marker.is_some_and(|marker| world.get_entity(marker).is_ok()) {
                    return Err("清洁工程取消后丢失原World".into());
                }
                fixture.clean_cancel_verified = true;
                fixture.draft = Some(validation_fixture::edit_close_document(world)?);
                fixture.map_save_before = Some(validation_fixture::edit_close_map_with_stale_dirty_marker(world)?);
                fixture.phase = Phase::CancelPrompt;
                request_window_close(world)?;
            }
            Phase::CancelPrompt if stage == CloseStage::Confirm => {
                if !retained(world, &fixture) || !world.resource::<NativeProjectCloseState>().documents.iter().any(|name| name.contains(validation_fixture::CLOSE_DOCUMENT_PATH)) {
                    return Err("关闭确认没有列出普通文本未保存草稿及实际历史".into());
                }
                if !world.resource::<NativeProjectCloseState>().documents.iter().any(|name| name.contains("[M1-SAMPLE]")) {
                    return Err("关闭确认漏报界面标记已清零但仍有待写数据的地图".into());
                }
                capture_prompt(world, &fixture, "project-close.png");
                fixture.phase = Phase::Capturing;
            }
            Phase::Capturing => {
                let captured = fixture.capture.try_lock().ok().and_then(|mut result| result.take());
                if let Some(result) = captured {
                    result?;
                    fixture.pulse = Some(choose(world, CloseAction::Cancel)?);
                    fixture.phase = Phase::Cancelled;
                }
            }
            Phase::Cancelled if stage == CloseStage::Idle => {
                if !retained(world, &fixture) { return Err("取消工程关闭后丢失原草稿或World实体".into()); }
                fixture.cancel_verified = true;
                change_owned_file(&mut fixture, false);
                fixture.phase = Phase::InjectingConflict;
            }
            Phase::InjectingConflict => {
                if let Some(result) = fixture.io.as_mut().and_then(check_ready) {
                    fixture.io = None; result?;
                    request_window_close(world)?;
                    fixture.phase = Phase::FailurePrompt;
                }
            }
            Phase::FailurePrompt if stage == CloseStage::Confirm => {
                fixture.pulse = Some(choose(world, CloseAction::Save)?);
                fixture.phase = Phase::FailedSave;
            }
            Phase::FailedSave if stage == CloseStage::Confirm && matches!(project_save_progress(world), ProjectSaveProgress::Failed { .. }) => {
                if !retained(world, &fixture) || world.resource::<NativeProjectCloseState>().failed.as_ref().is_none_or(|error| !error.contains("外部修改")) {
                    return Err("保存失败没有保留同一World草稿及明确失败原因".into());
                }
                fixture.failure_verified = true;
                change_owned_file(&mut fixture, true);
                fixture.phase = Phase::RestoringConflict;
            }
            Phase::RestoringConflict => {
                if let Some(result) = fixture.io.as_mut().and_then(check_ready) {
                    fixture.io = None; result?;
                    if validation_fixture::close_map_save_sequence(world, true) != fixture.map_save_before {
                        return Err("点击保存并退出前，地图的界面标记与待写数据不再分歧".into());
                    }
                    fixture.pulse = Some(choose(world, CloseAction::Save)?);
                    fixture.phase = Phase::FinalSave;
                }
            }
            Phase::FinalSave if !world.resource::<Messages<AppExit>>().is_empty() => {
                if !matches!(project_save_progress(world), ProjectSaveProgress::Succeeded)
                    || !validation_fixture::close_document_snapshot(world).is_some_and(|snapshot| snapshot["dirty"] == false
                        && fixture.draft.as_ref().is_some_and(|before| snapshot["source"] == before["source"]
                            && snapshot["generation"] == before["generation"] && snapshot["can_undo"] == true && snapshot["can_redo"] == true))
                    || !fixture.marker.is_some_and(|marker| world.get_entity(marker).is_ok())
                { return Err("工程退出先于真实保存成功，或保存重试丢失原World".into()); }
                let map_save_after = validation_fixture::close_map_save_sequence(world, false)
                    .filter(|after| fixture.map_save_before.is_some_and(|before| *after == before.wrapping_add(1)))
                    .ok_or("退出前缺少待写地图的真实保存回执")?;
                let proof = serde_json::json!({
                    "passed":true,"cancel_kept_same_world_and_dirty_draft":fixture.cancel_verified,
                    "clean_prompt_waited_without_choice_and_cancel_kept_same_world":fixture.clean_cancel_verified,
                    "injected_save_failure_kept_draft":fixture.failure_verified,
                    "retry_used_original_document_writer":true,"save_completed_before_app_exit":true,
                    "map_save_sequence_before":fixture.map_save_before,"map_save_sequence_after":map_save_after,
                    "input":"existing close button action pulses and real WindowCloseRequested",
                    "schema":3,"scope":"ordinary_document_close_cancel_conflict_retry",
                    "dirty_fixture":"owned ordinary text document edit/edit/undo with actual undo and redo; production batch save",
                    "failure_fixture":"owned text file external-version conflict rejected by original document writer, exact original bytes restored before retry",
                    "not_verified":["physical system-X click","OS-level write permission failure"]
                });
                // 不清AppExit：Winit/RenderApp可能已接纳WindowClosing，不能撤销退出后再等另一帧。
                *fixture.report.proof.try_lock().map_err(|_| "关闭验收证据槽不可用")? = Some(proof);
                fixture.phase = Phase::Complete;
            }
            Phase::FinalSave if stage == CloseStage::Confirm => {
                if let ProjectSaveProgress::Failed { detail } = project_save_progress(world) {
                    return Err(format!("地图退出保存重试失败：{detail}"));
                }
            }
            _ => {}
        }
        Ok(())
    });
    if let Err(error) = result {
        world.resource_mut::<Fixture>().phase = Phase::Complete;
        error!("工程关闭验收失败：{error}");
        world.write_message(AppExit::error());
    }
}
