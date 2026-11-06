//! 独立World首版工程运行：唯一后台工作槽构建并运行普通工程实例，编辑器World始终驻留。

#[cfg(test)]
#[path = "native_play_lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "native_play_resources.rs"]
mod resources;
#[path = "native_play_worker.rs"]
mod worker;
#[path = "native_play_code_apply.rs"]
mod code_apply;

pub(crate) use worker::{build_feature_identity, prepare as prepare_game, prepare_export, PreparedGame};
pub(crate) use worker::{prepare_function_patch, prepare_with_backend as prepare_game_with_backend};
#[cfg(test)]
pub(crate) use worker::prepared_for_test;
pub(crate) use code_apply::{cancel_pending_code_apply, code_apply_blocker, code_apply_status, request_code_apply};
pub(crate) use resources::{CodeApplyState, CodeApplyStatus, ProjectBuildBackend};

use bevy::{
    ecs::message::MessageCursor,
    prelude::*,
    window::{PrimaryWindow, WindowCloseRequested},
};
pub(crate) use resources::NativePlayStatus;
use resources::{NativePlayRuntime, NativePlaySettings};
use feathrise_editor_map::standalone::{RunSavedProject, SavedPlayFeedback, StandaloneEditorMode};
use std::{path::PathBuf, sync::mpsc, thread};

pub(crate) struct NativePlayPlugin {
    project: crate::project::Project,
    target: PathBuf,
}

impl NativePlayPlugin {
    pub(crate) fn new(project: crate::project::Project, target: PathBuf) -> Self {
        Self { project, target }
    }
}

impl Plugin for NativePlayPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NativePlaySettings {
            project: self.project.clone(),
            target: self.target.clone(),
        })
        .init_non_send::<NativePlayRuntime>()
        .init_resource::<NativePlayStatus>()
        .init_resource::<crate::native_project_close::NativeProjectCloseState>()
        .add_message::<RunSavedProject>()
        .add_message::<SavedPlayFeedback>()
        .add_systems(
            Update,
            (poll_play, code_apply::advance, preflight_play, request_play, request_exit)
                .chain()
                .after(feathrise_editor_map::standalone::SavedPlayDispatch),
        );
    }
}

/// 原作者Tasks与F12发布之后做一次提交门检查；被拒请求直接消费，完成保存后由作者重新按F12。
fn preflight_play(world: &mut World, mut cursor: Local<MessageCursor<RunSavedProject>>) {
    let requested_start = world
        .get_resource::<Messages<RunSavedProject>>()
        .is_some_and(|messages| {
            cursor
                .read(messages)
                .last()
                .is_some_and(|request| !request.stop_only)
        });
    if !requested_start || world.non_send::<NativePlayRuntime>().active.is_some()
        || crate::embedded_viewport::owns_play(world) {
        return; // 运行中的第二次F12必须留给取消，不得被另一笔作者保存挡住。
    }
    let reason = if world
        .get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|close| close.active())
    {
        let operation = world.resource::<crate::native_project_close::NativeProjectCloseState>().target().operation();
        Some(format!("正在确认{operation}，请先取消本次操作再启动工程；作者现场保留"))
    } else if world
        .get_resource::<crate::native_project_menu::NativeProjectMenuState>()
        .is_some_and(|menu| menu.saving())
    {
        Some("工程启动设置正在保存，完成后请重新启动工程".to_owned())
    } else {
        feathrise_editor_map::standalone::saved_play_blocker(world)
    };
    if let Some(reason) = reason {
        world.resource_mut::<Messages<RunSavedProject>>().clear();
        world.write_message(SavedPlayFeedback(reason));
    }
}

fn request_play(
    mut requests: MessageReader<RunSavedProject>,
    settings: Res<NativePlaySettings>,
    mut runtime: NonSendMut<NativePlayRuntime>,
    mut status: ResMut<NativePlayStatus>,
    mut feedback: MessageWriter<SavedPlayFeedback>,
    mut mode: Option<ResMut<StandaloneEditorMode>>,
    viewport: Option<Res<crate::viewport_mode::ViewportModeState>>,
    backend: Option<Res<ProjectBuildBackend>>,
    embedded: Option<Res<crate::embedded_viewport::EmbeddedStatus>>,
    logs: Option<Res<crate::project_log::ProjectLogChannel>>,
) {
    let Some(request) = requests.read().last().cloned() else {
        return;
    };
    if viewport.is_some_and(|state| state.selected == crate::viewport_mode::ViewportMode::EmbeddedWorld)
        || embedded.is_some_and(|state| state.active) {
        return; // 同一消息只有所选方式消费；嵌入会话的F12不能启动第二份游戏World。
    }
    if runtime.cleanup_failed || runtime.exit_requested {
        status.preparing = false;
        feedback.write(SavedPlayFeedback(
            "正在确认退出或上次子进程未确认收口，暂不启动另一份工程运行".into(),
        ));
        return;
    }
    if runtime.code_apply_pending.take().is_some() {
        status.preparing = false;
        code_apply::fail(&mut runtime, "已取消应用新版本；作者现场保留");
        if let Some(active) = runtime.active.as_ref() { active.control.cancel(); }
        feedback.write(SavedPlayFeedback("已取消应用新版本；作者草稿和历史保留".into()));
        return;
    }
    if let Some(active) = runtime.active.as_ref() {
        // 重复启动是明确停止当前自己拥有的构建/游戏，不积压第二份World或并行Cargo。
        active.control.cancel();
        status.preparing = false;
        feedback.write(SavedPlayFeedback("正在停止本次工程运行，作者现场保留".into()));
        return;
    }
    if request.stop_only {
        status.preparing = false;
        // 发布与轮询之间游戏可能刚好结束；显式停止意图绝不因为工作槽已空而重开工程。
        feedback.write(SavedPlayFeedback(
            "本次工程运行已结束，作者草稿和历史保留".into(),
        ));
        return;
    }
    let control = logs.as_ref()
        .map(|logs| crate::build_process::BuildControl::with_log(logs.begin_task(crate::project_log::LogTaskKind::SavedPlay)))
        .unwrap_or_default();
    let (progress_tx, progress_rx) = mpsc::sync_channel(4);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let project = settings.project.clone();
    let target = settings.target.clone();
    // 受理时冻结与顶栏相同的构建工具，后台任务不能回落到默认DX或回读之后的选择。
    let build_backend = backend.map_or(ProjectBuildBackend::Dx, |backend| *backend);
    runtime.active_request = Some(request.clone());
    runtime.code_apply_active_token = None;
    runtime.code_apply_started = None;
    runtime.launch_started = Some(started_rx);
    let job_control = control.clone();
    let handle = thread::spawn(move || {
        let result = worker::run(project, target, request, build_backend, &job_control, &progress_tx, Some(started_tx));
        let _ = result_tx.send(result);
    });
    runtime.active = Some(resources::ActivePlay {
        control,
        progress_rx,
        result_rx,
        handle,
    });
    status.started = status.started.saturating_add(1);
    status.active = true;
    status.preparing = true;
    status.last_result = None;
    status.last_exit_code = None;
    if let Some(mode) = mode.as_deref_mut() {
        mode.set_external_play_active(true);
    }
    feedback.write(SavedPlayFeedback(
        "正在后台检查并构建已保存的工程".into(),
    ));
}

/// 顶栏不能借更换构建选项绕过未确认的子进程收口或已受理的退出。
pub(crate) fn build_profile_blocker(world: &World) -> Option<String> {
    let runtime = world.get_non_send::<NativePlayRuntime>()?;
    if runtime.cleanup_failed || runtime.exit_requested {
        Some("退出或子进程收口尚未确认，暂不能切换构建选项；作者现场保留".into())
    } else if runtime.active.is_some() || runtime.code_apply_pending.is_some() {
        Some("请先结束当前工程运行或构建，再切换构建选项".into())
    } else {
        None
    }
}

/// 普通启动与代码采用分别消费各自的真实创建回执；任何一次检查都只做非阻塞读取。
fn receive_process_created(runtime: &mut NativePlayRuntime, status: &mut NativePlayStatus) {
    if runtime.launch_started.as_ref().is_some_and(|started| started.try_recv().is_ok()) {
        runtime.launch_started = None;
        status.preparing = false;
    }
    // 只有实际创建并加入专属Job的游戏才确认应用；提前排队或构造命令不算生效。
    if runtime.code_apply_started.as_ref().is_some_and(|started| started.try_recv().is_ok()) {
        runtime.code_apply_started = None;
        status.preparing = false;
        let active_token = runtime.code_apply_active_token;
        if let Some(apply) = runtime.code_apply_status.as_mut()
            && Some(apply.token) == active_token
        {
            apply.state = CodeApplyState::Applied;
            apply.error = None;
        }
    }
}

/// 完成线程已经回收后再次排空回执，覆盖“首次 try_recv 为空、随后创建并快速退出”的竞态。
fn complete_code_apply(runtime: &mut NativePlayRuntime, status: &mut NativePlayStatus) {
    receive_process_created(runtime, status);
    if let Some(token) = runtime.code_apply_active_token.take()
        && runtime.code_apply_status.as_ref().is_some_and(|apply| apply.token == token)
    {
        let failure = status.last_result.as_ref().and_then(|result| result.as_ref().err()).cloned()
            .or_else(|| runtime.code_apply_started.is_some().then(|| "新版本没有返回实际进程启动回执".into()));
        if let Some(error) = failure {
            code_apply::fail(runtime, &error);
        }
    }
}

fn poll_play(
    mut runtime: NonSendMut<NativePlayRuntime>,
    mut status: ResMut<NativePlayStatus>,
    mut feedback: MessageWriter<SavedPlayFeedback>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut mode: Option<ResMut<StandaloneEditorMode>>,
) {
    if runtime.exit_requested || runtime.active.as_ref().is_some_and(|active| active.control.is_cancelled()) {
        status.preparing = false;
    }
    receive_process_created(&mut runtime, &mut status);
    let Some(active) = runtime.active.as_ref() else {
        status.preparing = false;
        runtime.launch_started = None;
        return;
    };
    for message in active.progress_rx.try_iter().take(4) {
        feedback.write(SavedPlayFeedback(message));
    }
    // 不在帧内等待线程、进程或管道；真正完成后join仅回收线程状态。
    if !active.handle.is_finished() {
        return;
    }
    let active = runtime.active.take().expect("上方已经检查唯一工程运行槽");
    let result = active
        .result_rx
        .try_recv()
        .unwrap_or_else(|_| Err("工程运行工作线程没有返回结果".into()));
    let joined = active.handle.join().is_ok();
    let log = active.control.log_sink();
    if let Some(log) = &log {
        let message = if !joined { "任务线程异常".into() } else {
            match &result {
                Ok(code) => format!("任务结束，退出码{code}"),
                Err(error) => format!("任务未完成：{error}"),
            }
        };
        log.task_message(&message);
    }
    runtime.cleanup_failed |= active.control.cleanup_failed() || !joined;
    if let Some(mode) = mode.as_deref_mut() {
        mode.set_external_play_active(false);
    }
    status.completed = status.completed.saturating_add(1);
    status.active = false;
    status.preparing = false;
    status.last_exit_code = result.as_ref().ok().copied();
    status.last_result = Some(if joined {
        match &result {
            Ok(0) => Ok(()),
            Ok(code) => Err(format!("运行进程退出码{code}")),
            Err(error) => Err(error.clone()),
        }
    } else {
        Err("工程运行工作线程异常".into())
    });
    complete_code_apply(&mut runtime, &mut status);
    runtime.active_request = None;
    runtime.launch_started = None;
    runtime.code_apply_started = None;
    let message: String = if !joined {
        "工程运行工作线程异常；作者草稿和历史保留".into()
    } else if active.control.is_cancelled() && !active.control.cleanup_failed() {
        "本次工程运行已结束，已返回原编辑器；作者草稿和历史保留".into()
    } else {
        match result {
            Ok(0) => "工程运行已结束，已返回原编辑器；作者草稿和历史保留".into(),
            Ok(code) => format!("运行进程退出码{code}；作者草稿和历史保留，修复后可重新启动工程"),
            Err(error) => format!("工程运行未完成：{error}；作者草稿和历史保留"),
        }
    };
    // Winit按focused字段变更请求真实窗口焦点；不重建编辑器、不切本地GameScene。
    if let Ok(mut window) = windows.single_mut() {
        window.focused = true;
    }
    let message = log.map_or(message.clone(), |log| format!("{}：{message}", log.task_label()));
    feedback.write(SavedPlayFeedback(message));
}

/// 先让作者确认未保存内容，再收口精确Job；取消关闭不能提前终止仍在进行的工程运行。
pub(crate) fn request_exit(
    world: &mut World,
    mut cursor: Local<MessageCursor<WindowCloseRequested>>,
) {
    let requested = world
        .get_resource::<Messages<WindowCloseRequested>>()
        .is_some_and(|messages| cursor.read(messages).next().is_some());
    let pending = world.non_send::<NativePlayRuntime>().exit_requested
        || world
            .resource::<crate::native_project_close::NativeProjectCloseState>()
            .active();
    if !requested && !pending {
        return;
    }
    if !crate::embedded_viewport::return_to_edit_before_close(world) {
        world.non_send_mut::<NativePlayRuntime>().exit_requested = true;
        return;
    }
    if !crate::native_project_close::authorize_exit(world, requested || pending) {
        world.non_send_mut::<NativePlayRuntime>().exit_requested = false;
        crate::embedded_viewport::cancel_exit(world);
        crate::code_iteration::cancel_exit(world);
        return;
    }
    match crate::embedded_viewport::ready_to_exit(world) {
        Ok(true) => (),
        Ok(false) => {
            world.non_send_mut::<NativePlayRuntime>().exit_requested = true;
            return;
        }
        Err(error) => {
            world.resource_mut::<crate::native_project_close::NativeProjectCloseState>().hold(error.clone());
            world.write_message(SavedPlayFeedback(error));
            return;
        }
    }
    if world
        .get_resource::<crate::native_project_menu::NativeProjectMenuState>()
        .is_some_and(|menu| menu.saving())
    {
        world
            .resource_mut::<crate::native_project_close::NativeProjectCloseState>()
            .wait_for_startup_save();
        world.non_send_mut::<NativePlayRuntime>().exit_requested = true;
        return; // 已受理的启动设置写入也必须真正结束，不能随关闭丢弃任务。
    }
    let startup_waited = world
        .resource_mut::<crate::native_project_close::NativeProjectCloseState>()
        .take_startup_save_wait();
    if startup_waited {
        if let Some(error) = world
            .get_resource::<crate::native_project_menu::NativeProjectMenuState>()
            .and_then(|menu| menu.save_failed())
            .map(str::to_owned)
        {
            let reason = format!(
                "关闭等待中的工程启动设置保存失败：{error}；请取消关闭，返回工程菜单重试。作者现场保留"
            );
            world
                .resource_mut::<crate::native_project_close::NativeProjectCloseState>()
                .hold(reason.clone());
            world.non_send_mut::<NativePlayRuntime>().exit_requested = false;
            world.write_message(SavedPlayFeedback(reason));
            return;
        }
    }
    enum ExitAction {
        Blocked,
        Wait,
        Check,
    }
    let action = {
        let mut runtime = world.non_send_mut::<NativePlayRuntime>();
        if runtime.cleanup_failed {
            runtime.exit_requested = false;
            ExitAction::Blocked
        } else if let Some(active) = runtime.active.as_ref() {
            active.control.cancel();
            runtime.exit_requested = true;
            ExitAction::Wait
        } else {
            runtime.exit_requested = false;
            ExitAction::Check
        }
    };
    match action {
        ExitAction::Blocked => {
            let reason =
                "后台子进程未确认退出，保留编辑器和作者现场；请查看本次收口诊断".to_owned();
            world
                .resource_mut::<crate::native_project_close::NativeProjectCloseState>()
                .hold(reason.clone());
            world.write_message(SavedPlayFeedback(reason));
            return;
        }
        ExitAction::Wait => {
            if let Some(mut status) = world.get_resource_mut::<NativePlayStatus>() { status.preparing = false; }
            if requested {
                world.write_message(SavedPlayFeedback(
                    format!("已确认作者内容，正在停止本次运行实例与构建；确认全部结束后{}",
                        world.resource::<crate::native_project_close::NativeProjectCloseState>().target().operation()),
                ));
            }
            return; // 下一帧try_recv/is_finished继续，不在UI线程等待。
        }
        ExitAction::Check => (),
    }
    match crate::native_editor::preferences_ready_to_exit(world, requested) {
        Ok(true) => (),
        Ok(false) => {
            world.non_send_mut::<NativePlayRuntime>().exit_requested = true;
            return; // 作者偏好仍在单槽后台发布；保持窗口和事件循环，下一帧继续非阻塞检查。
        }
        Err(error) => {
            world
                .resource_mut::<crate::native_project_close::NativeProjectCloseState>()
                .hold(error.clone());
            world.non_send_mut::<NativePlayRuntime>().exit_requested = false;
            world.write_message(SavedPlayFeedback(error));
            return;
        }
    }
    world.write_message(AppExit::Success);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exit_app() -> App {
        let mut app = App::new();
        app.init_non_send::<NativePlayRuntime>()
            .init_resource::<crate::native_project_close::NativeProjectCloseState>()
            .add_message::<WindowCloseRequested>()
            .add_message::<SavedPlayFeedback>()
            .add_message::<AppExit>()
            .add_systems(Update, request_exit);
        app
    }

    #[test]
    fn close_during_owned_worker_cancels_without_exiting_early() {
        let mut app = exit_app();
        app.insert_resource(NativePlayStatus { active: true, preparing: true, ..default() });
        let control = crate::build_process::BuildControl::default();
        let (_, progress_rx) = mpsc::sync_channel(4);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            wait.recv().unwrap();
            let _ = result_tx.send(Ok(0));
        });
        app.world_mut().non_send_mut::<NativePlayRuntime>().active = Some(resources::ActivePlay {
            control: control.clone(),
            progress_rx,
            result_rx,
            handle,
        });
        app.world_mut().write_message(WindowCloseRequested {
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(!control.is_cancelled());
        assert!(app.world().resource::<NativePlayStatus>().preparing);
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        crate::native_project_close::tests::choose_discard(app.world_mut());
        app.update();
        assert!(control.is_cancelled());
        assert!(!app.world().resource::<NativePlayStatus>().preparing);
        assert!(app.world().non_send::<NativePlayRuntime>().exit_requested);
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        release.send(()).unwrap();
        let active = app
            .world_mut()
            .non_send_mut::<NativePlayRuntime>()
            .active
            .take()
            .unwrap();
        active.handle.join().unwrap(); // 测试夹具显式放行后再回收，不把线程Drop当收口确认。
    }

    #[test]
    fn clean_project_close_without_a_choice_keeps_the_owned_worker_and_window() {
        let mut app = exit_app();
        let control = crate::build_process::BuildControl::default();
        let (_, progress_rx) = mpsc::sync_channel(4);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel::<()>(1);
        let handle = thread::spawn(move || {
            let _ = wait.recv();
            let _ = result_tx.send(Ok(0));
        });
        app.world_mut().non_send_mut::<NativePlayRuntime>().active = Some(resources::ActivePlay {
            control: control.clone(),
            progress_rx,
            result_rx,
            handle,
        });
        app.world_mut().write_message(WindowCloseRequested {
            window: Entity::PLACEHOLDER,
        });
        // 无脏文档也必须等明确选择；反复推进真实退出系统不能自动收口后台或退出窗口。
        for _ in 0..4 {
            app.update();
            assert!(!control.is_cancelled());
            let runtime = app.world().non_send::<NativePlayRuntime>();
            assert!(runtime.active.is_some());
            assert!(!runtime.exit_requested);
            assert!(app.world().resource::<Messages<AppExit>>().is_empty());
            assert!(
                app.world()
                    .resource::<crate::native_project_close::NativeProjectCloseState>()
                    .active()
            );
        }
        release.send(()).unwrap();
        app.world_mut()
            .non_send_mut::<NativePlayRuntime>()
            .active
            .take()
            .unwrap()
            .handle
            .join()
            .unwrap();
    }

    #[test]
    fn asking_about_unsaved_author_content_does_not_stop_the_running_game() {
        let mut app = exit_app();
        app.insert_resource(crate::native_authoring_validation::dirty_surface_fixture());
        let control = crate::build_process::BuildControl::default();
        let (_, progress_rx) = mpsc::sync_channel(4);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel::<()>(1);
        let handle = thread::spawn(move || {
            let _ = wait.recv();
            let _ = result_tx.send(Ok(0));
        });
        app.world_mut().non_send_mut::<NativePlayRuntime>().active = Some(resources::ActivePlay {
            control: control.clone(),
            progress_rx,
            result_rx,
            handle,
        });
        app.world_mut().write_message(WindowCloseRequested {
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(
            !control.is_cancelled(),
            "尚未选择保存/放弃时不能提前终止工程运行"
        );
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        assert!(
            app.world()
                .resource::<feathrise_editor_surface_model::SurfacePresetEditorState>()
                .tabs[0]
                .draft.dirty()
        );
        release.send(()).unwrap();
        app.world_mut()
            .non_send_mut::<NativePlayRuntime>()
            .active
            .take()
            .unwrap()
            .handle
            .join()
            .unwrap();
    }

    #[test]
    fn unconfirmed_cleanup_preserves_window_instead_of_claiming_clean_exit() {
        let mut app = exit_app();
        app.world_mut()
            .non_send_mut::<NativePlayRuntime>()
            .cleanup_failed = true;
        app.world_mut().write_message(WindowCloseRequested {
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        crate::native_project_close::tests::choose_discard(app.world_mut());
        app.update();
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        assert!(!app.world().non_send::<NativePlayRuntime>().exit_requested);
        assert!(
            !app.world()
                .resource::<Messages<SavedPlayFeedback>>()
                .is_empty()
        );
    }

    #[test]
    fn startup_save_failed_while_close_was_waiting_keeps_the_window_and_diagnostic() {
        let mut app = exit_app();
        app.init_resource::<crate::native_project_menu::NativeProjectMenuState>();
        assert!(!crate::native_project_close::authorize_exit(
            app.world_mut(),
            true
        ));
        crate::native_project_close::tests::choose_discard(app.world_mut());
        assert!(crate::native_project_close::authorize_exit(
            app.world_mut(),
            false
        ));
        app.world_mut()
            .resource_mut::<crate::native_project_close::NativeProjectCloseState>()
            .wait_for_startup_save();
        app.world_mut()
            .resource_mut::<crate::native_project_menu::NativeProjectMenuState>()
            .report_save_failure("本次在途写入失败".into());
        app.update();
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        assert!(
            app.world()
                .resource::<crate::native_project_close::NativeProjectCloseState>()
                .active()
        );
        assert!(!app.world().non_send::<NativePlayRuntime>().exit_requested);
        let mut feedback = MessageCursor::<SavedPlayFeedback>::default();
        assert!(
            feedback
                .read(app.world().resource::<Messages<SavedPlayFeedback>>())
                .any(|item| item.0.contains("本次在途写入失败"))
        );
    }

    #[test]
    fn an_already_reported_old_startup_failure_does_not_permanently_lock_a_later_close() {
        let mut app = exit_app();
        app.init_resource::<crate::native_project_menu::NativeProjectMenuState>();
        app.world_mut()
            .resource_mut::<crate::native_project_menu::NativeProjectMenuState>()
            .report_save_failure("先前已回显的失败".into());
        app.world_mut().write_message(WindowCloseRequested {
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        crate::native_project_close::tests::choose_discard(app.world_mut());
        app.update();
        assert_eq!(app.world().resource::<Messages<AppExit>>().len(), 1);
    }

    #[test]
    fn finished_play_stop_intent_does_not_create_another_worker() {
        // 模拟publisher发取消后poll刚收口：消息身份必须跨越这个空槽边界，不探测磁盘或启动Cargo。
        let root = PathBuf::from("unused-stop-only-fixture");
        let mut app = App::new();
        app.init_non_send::<NativePlayRuntime>()
            .insert_resource(NativePlaySettings {
                project: crate::project::Project {
                    id: "stop-only-fixture".into(),
                    name: "停止意图回归".into(),
                    manifest: root.join("Cargo.toml"),
                    root: root.clone(),
                    package: "unused-stop-only-fixture".into(),
                    context: crate::project::ProjectContext {
                        root: root.clone(),
                        id: "stop-only-fixture".into(),
                        name: "停止意图回归".into(),
                        format: 1,
                        asset_roots: vec![root.join("assets")],
                        editor_data: root.join(".editor"),
                        storage_key: "stop-only-fixture".into(),
                        startup_scene: None,
                    },
                },
                target: root.join("unused-target"),
            })
            .insert_resource(NativePlayStatus {
                started: 1,
                completed: 1,
                last_result: Some(Ok(())),
                last_exit_code: Some(0),
                ..default()
            })
            .init_resource::<StandaloneEditorMode>()
            .add_message::<RunSavedProject>()
            .add_message::<SavedPlayFeedback>()
            .add_systems(Update, (preflight_play, request_play).chain());
        app.world_mut().write_message(RunSavedProject {
            profile: default(),
            map_id: None,
            has_unsaved_changes: true,
            stop_only: true,
        });
        app.update();
        assert!(app.world().non_send::<NativePlayRuntime>().active.is_none());
        let status = app.world().resource::<NativePlayStatus>();
        assert_eq!((status.started, status.completed), (1, 1));
        assert!(!status.active);
        assert_eq!(status.last_result, Some(Ok(())));
        let mut feedback = MessageCursor::<SavedPlayFeedback>::default();
        assert!(
            feedback
                .read(app.world().resource::<Messages<SavedPlayFeedback>>())
                .any(|message| message.0.contains("工程运行已结束"))
        );
    }
}
