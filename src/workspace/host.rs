//! 持续存在的Feathrise主窗口；候选通过后才请求原作者关闭，失败与取消均保留旧现场。
use super::{
    process::{self, AuthorProcess},
    resources::*,
    types::*,
};
use crate::workspace_ui::*;
use crate::native_project_close::{CloseTarget, NativeProjectClosePlugin, NativeProjectCloseState};
use crate::native_editor::preferences as author_preferences;
use bevy::{
    ecs::message::MessageCursor,
    prelude::*,
    render::{
        RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        settings::WgpuSettings,
    },
    window::{PrimaryWindow, RawHandleWrapper, WindowCloseRequested},
    winit::{UpdateMode, WinitSettings},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub(crate) fn run(initial: Option<PathBuf>, target_override: Option<PathBuf>) -> Result<(), String> {
    #[cfg(all(feature = "validation", windows))]
    let placement_validation = crate::window_chrome::placement_validation::prepare(initial.is_some())?;
    trace("准备主窗口环境");
    feathrise_runtime::prepare_vulkan_environment();
    let (preferences, error) = super::preferences()?;
    let (author_settings, author_error) = author_preferences::load(&preferences);
    let notices = [error, author_error].into_iter().flatten().collect::<Vec<_>>();
    trace("个人设置已读取，开始装配原工作台");
    let model = super::model(&preferences, None);
    #[cfg(windows)]
    let saved_window = preferences.main_window;
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<PipelinedRenderingPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
            .disable::<bevy::app::hotpatch::HotPatchPlugin>()
            .set(AssetPlugin {
                file_path: crate::project_template_root()
                    .join("assets")
                    .to_string_lossy()
                    .into_owned(),
                ..default()
            })
            .set(WindowPlugin {
                close_when_requested: false,
                primary_window: Some(Window {
                    title: "Feathrise（羽立）".into(),
                    decorations: !cfg!(target_os = "windows"), // Windows由统一自绘顶栏保留原生窗口操作。
                    #[cfg(windows)]
                    visible: saved_window.is_none(),
                    present_mode: if cfg!(windows) && !author_settings.editor.render.graphics.vsync {
                        bevy::window::PresentMode::AutoNoVsync
                    } else { default() },
                    resolution: bevy::window::WindowResolution::new(1270, 800),
                    resize_constraints: bevy::window::WindowResizeConstraints {
                        min_width: 720.,
                        min_height: 560.,
                        ..default()
                    },
                    ..default()
                }),
                ..default()
            })
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    backends: Some(crate::portable_author::backend()),
                    ..default()
                }
                .into(),
                ..default()
            }),
    )
    .add_plugins(feathrise_runtime::DefaultFontPlugin)
    .add_plugins((
        crate::branding::EditorBrandingPlugin,
        feathrise_editor_ui::EditorUiPlugin,
        bevy::input_focus::tab_navigation::TabNavigationPlugin,
        feathrise_editor_documents::DocumentsEditorPlugin,
        feathrise_editor_map::workbench::WorkbenchUiPlugin,
        NativeProjectClosePlugin,
    ))
    .insert_resource(model)
    .add_plugins(WorkspaceUiPlugin)
    .insert_resource(ClearColor(Color::srgb_u8(30, 30, 30)))
    .insert_resource(WinitSettings {
        focused_mode: if cfg!(windows) { UpdateMode::Continuous } else { UpdateMode::reactive(Duration::from_millis(16)) },
        unfocused_mode: UpdateMode::reactive_low_power(Duration::from_millis(32)),
    })
    .insert_non_send(Host {
        target_override,
        preferences,
        preferences_job: None,
        job: None,
        prepared: None,
        picker: None,
        current: None,
        candidate: None,
        retiring: None,
        want_close: false,
        close_sent: false,
        exit_app: false,
        notice: if notices.is_empty() { "未打开工程".into() } else { notices.join("；") },
        changed: true,
        initial,
    })
    .add_systems(Startup, setup)
    .add_systems(Update, sync_window_title)
    .add_systems(Last, advance);
    #[cfg(windows)]
    {
        crate::window_chrome::placement::install(&mut app, saved_window);
        app.add_systems(Last, pace_workspace.after(advance));
    }
    let preferences_completion = author_preferences::install_host(&mut app, author_settings);
    #[cfg(feature = "validation")]
    crate::locale_validation::install(&mut app, None)?;
    crate::native_plugins::management::install_host(&mut app);
    #[cfg(unix)]
    crate::portable_author::host::install(&mut app);
    feathrise_runtime::install_vulkan_surface_guard(&mut app);
    #[cfg(all(feature = "validation", windows))]
    super::validation::install(&mut app)?;
    #[cfg(all(feature = "validation", windows))]
    if let Some(plan) = placement_validation {
        app.insert_resource(plan)
            .add_systems(Last, crate::window_chrome::placement_validation::drive.after(advance));
    }
    trace("原工作台已装配，启动窗口事件循环");
    #[cfg(all(feature = "validation", windows))]
    let validation = super::validation::completion(app.world());
    let exit = app.run();
    let preferences_result = preferences_completion.finish();
    #[cfg(all(feature = "validation", windows))]
    if let Some(validation) = validation {
        validation.finish()?;
    }
    preferences_result?;
    if exit.is_success() {
        Ok(())
    } else {
        Err(format!("Feathrise工作台退出失败：{exit:?}"))
    }
}

fn trace(stage: &str) {
    if std::env::var_os("FEATHRISE_STARTUP_TRACE").is_some() {
        eprintln!("Feathrise启动：{stage}");
    }
}
fn setup(mut commands: Commands) {
    trace("主窗口Startup，建立工作台相机");
    commands.spawn(Camera2d);
}

#[cfg(windows)]
fn pace_workspace(
    host: NonSend<Host>, mut pacing: Local<crate::editor_frame_pacing::EditorFramePacing>,
    preferences: Res<feathrise_editor_core::EditorSettings>,
    mut settings: ResMut<WinitSettings>, frame: Res<bevy::diagnostic::FrameCount>,
    mut input: crate::editor_frame_pacing::PacingInput,
) {
    use crate::editor_frame_pacing::FramePacingRequest;

    let author_states = [host.current.as_ref(), host.candidate.as_ref()]
        .map(|instance| instance.and_then(|author| author.frame_pacing));
    // 当前作者掌管个人偏好；没有当前作者时使用正在进入的候选或首页的最新偏好。
    let mut request = author_states.into_iter().flatten().next()
        .unwrap_or_else(|| FramePacingRequest::new(&preferences, false));
    request.building = crate::build_process::project_build_active()
        || author_states.into_iter().flatten().any(|state| state.building);
    if let Some(next) = pacing.transition(request, input.interacted(request.limiting_build()), &settings, frame.0, "workspace", Instant::now()) {
        *settings = next;
    }
}

pub(super) fn advance(
    world: &mut World,
    mut commands: Local<MessageCursor<WorkspaceCommand>>,
    mut close: Local<MessageCursor<WindowCloseRequested>>,
) {
    let mut windows = world.query_filtered::<(&RawHandleWrapper, Entity), With<PrimaryWindow>>();
    let Ok((raw, window)) = windows.single(world) else {
        return;
    };
    #[cfg(windows)]
    let Some(parent) = crate::workspace_window::primary_hwnd(raw) else { return; };
    #[cfg(unix)]
    let parent = crate::workspace_window::primary_hwnd(raw).unwrap_or(0);
    let closing = close
        .read(world.resource::<Messages<WindowCloseRequested>>())
        .any(|request| request.window == window);
    let actions: Vec<_> = commands
        .read(world.resource::<Messages<WorkspaceCommand>>())
        .take(4)
        .cloned()
        .collect();
    let mut host = world
        .remove_non_send::<Host>()
        .expect("主窗口拥有工程生命周期");
    poll_author_preferences(&mut host, world);
    if closing {
        request_app_exit(&mut host, world);
    }
    if let Some(initial) = take_initial(&mut host, world) {
        handle(
            &mut host,
            world,
            parent,
            None,
            WorkspaceCommand::Open(initial),
        );
    }
    for command in actions {
        handle(&mut host, world, parent, None, command);
    }
    poll_empty_exit(&mut host, world);

    let mut events = Vec::new();
    for (candidate, instance) in [
        (false, host.current.as_mut()),
        (true, host.candidate.as_mut()),
    ] {
        if let Some(instance) = instance {
            for _ in 0..4 {
                match instance.transport.try_recv() {
                    #[cfg(windows)]
                    Ok(Some(AuthorEvent::FramePacing(state))) => instance.frame_pacing = Some(state),
                    Ok(Some(event)) => events.push((candidate, instance.child.id(), event)),
                    Ok(None) => break,
                    Err(_) => {
                        // 通道中断时解除父窗口降帧，进程退出仍由下方精确句柄判定。
                        #[cfg(windows)]
                        { instance.frame_pacing = None; }
                        break;
                    }
                }
            }
        }
    }
    for (candidate, pid, event) in events {
        match event {
            #[cfg(windows)]
            AuthorEvent::Ready { hwnd } if candidate => {
                if crate::workspace_window::find_child(parent, pid) == Some(hwnd) {
                    if let Some(instance) = host.candidate.as_mut() {
                        instance.hwnd = Some(hwnd);
                    }
                    host.notice("目标工程已准备，正在交接当前工程…");
                } else {
                    fail_candidate(&mut host, "候选作者窗口身份不匹配".into());
                }
            }
            #[cfg(unix)]
            AuthorEvent::ReadySurface { session } if candidate => {
                if host.candidate.as_ref().is_some_and(|instance| instance.surface.matches(&session)) {
                    if let Some(instance) = host.candidate.as_mut() { instance.surface_ready = true; }
                    host.notice("目标工程已准备，正在交接当前工程…");
                } else { fail_candidate(&mut host, "候选作者共享画面会话身份不匹配".into()); }
            }
            AuthorEvent::Command(command) if !candidate => {
                handle(&mut host, world, parent, Some(pid), command)
            }
            event @ (AuthorEvent::CloseCancelled | AuthorEvent::CloseCancelledFor { .. }) if !candidate => {
                let target = event.cancelled_target().expect("取消回执必须具有关闭目标");
                if accepts_close_cancel(host.current.as_ref().map(|current| current.child.id()), pid,
                    host.want_close && host.close_sent, host.exit_app, target) {
                    host.want_close = false;
                    host.close_sent = false;
                    host.exit_app = false;
                    retire_candidate(&mut host);
                    host.notice("已取消，当前工程、草稿和历史保留");
                }
            }
            AuthorEvent::Failed(error) if candidate => fail_candidate(&mut host, error),
            _ => {}
        }
    }
    poll_jobs(&mut host);
    poll_prepared(&mut host, world, parent);
    #[cfg(unix)]
    poll_surfaces(&mut host, world);
    poll_picker(&mut host, world);
    poll_processes(&mut host, world, parent);
    poll_author_preferences(&mut host, world);
    if !empty_exit_blocks_work(&host, world)
        && host.candidate.as_ref().is_some_and(AuthorProcess::ready) {
        if host.current.is_none() {
            adopt(&mut host, world, parent);
        } else {
            host.want_close = true;
        }
    }
    if let Some(current) = &host.current {
        queue_close_request(host.want_close, host.exit_app, &mut host.close_sent,
            |event| current.send(event));
    }
    if host.changed {
        publish(&mut host, world);
    }
    sync_models(&mut host, world);
    finish_app_exit(&mut host, world);
    world.insert_non_send(host);
}

fn handle(
    host: &mut Host,
    world: &mut World,
    parent: usize,
    origin: Option<u32>,
    command: WorkspaceCommand,
) {
    // 启动地图与导出由当前作者持有；全局插件入口仍须由无工程主宿主消费。
    if matches!(command, WorkspaceCommand::SetStartupMap | WorkspaceCommand::ExportProject) {
        return;
    }
    if matches!(command, WorkspaceCommand::QuitApp) {
        request_app_exit(host, world);
        return;
    }
    // 模态确认及取消后的静默帧都由原关闭输入门持有；队列中旧的Open/取消准备也不能穿透。
    if empty_exit_blocks_work(host, world) {
        return;
    }
    if matches!(command, WorkspaceCommand::CancelOperation) {
        cancel_preparation(host);
        if host.close_sent {
            if let Some(current) = &host.current {
                let _ = current.send(HostEvent::CancelClose);
            }
        }
        host.want_close = false;
        host.close_sent = false;
        host.exit_app = false;
        host.notice("正在取消本次准备，当前工程和已发布文件保留");
        return;
    }
    if host.busy() || host.exit_app
        || (!authors_present(host) && !author_preferences::home_ready(world)) {
        host.notice("正在处理当前工程操作，请等待或取消后重试");
        return;
    }
    // 有工程时原作者消费插件入口；没有作者的开始页由主宿主管理全局安装。
    if matches!(command, WorkspaceCommand::OpenPlugins) {
        if host.current.is_none() {
            crate::native_plugins::management::open_from_workspace(world);
        }
        return;
    }
    match command {
        command @ (WorkspaceCommand::Open(_) | WorkspaceCommand::Create { .. }) => {
            host.job = Some(process::open_job(command));
            host.notice("正在后台校验工程；当前工程保持打开…");
        }
        WorkspaceCommand::CloseProject => {
            host.want_close = host.current.is_some();
            host.notice("正在检查当前工程的未保存内容…");
        }
        WorkspaceCommand::Browse { field, initial } => {
            // 系统模态框归持续存在的顶层窗口；来源PID只用于回填身份。
            // 给它子HWND会被Windows提升到顶层owner，破坏原取消器的精确owner匹配。
            match crate::folder_picker::FolderPicker::start(
                parent,
                initial,
                !matches!(field, WorkspaceField::OpenPath),
            ) {
                Ok(picker) => {
                    host.picker = Some(Picker {
                        picker,
                        field,
                        origin,
                    });
                    host.changed = true;
                }
                Err(error) => host.notice(error),
            }
        }
        WorkspaceCommand::SaveDefaultDirectory(value) => {
            let mut preferences = host.preferences.clone();
            match preferences.set_default_project_directory(&value) {
                Ok(()) => {
                    host.preferences_job = Some(process::save_preferences(preferences));
                    host.notice("正在保存默认工程目录…");
                }
                Err(error) => host.notice(error),
            }
        }
        WorkspaceCommand::RemoveRecent(path) => {
            let mut preferences = host.preferences.clone();
            if preferences.remove_recent(&path) {
                host.preferences_job = Some(process::save_preferences(preferences));
                host.notice("正在移除最近记录，工程文件保持原样…");
            }
        }
        WorkspaceCommand::SetStartupMap
        | WorkspaceCommand::ExportProject
        | WorkspaceCommand::OpenPlugins
        | WorkspaceCommand::CancelOperation
        | WorkspaceCommand::QuitApp => {}
    }
    let _ = world;
}

pub(super) fn request_app_exit(host: &mut Host, world: &mut World) {
    if host.current.is_none() {
        if host.exit_app { return; } // 已明确同意退出，只等待现有任务槽收口，不重复弹框。
        let Some(mut state) = world.get_resource_mut::<NativeProjectCloseState>() else {
            host.notice("退出确认尚未准备完成，请稍后重试");
            return;
        };
        let start = state.target() != CloseTarget::Editor || !state.captures_input();
        if !start { return; }
        state.set_target(CloseTarget::Editor);
        let requested = !state.active();
        drop(state);
        // 没有工程仍须作者明确选择；此处不置退出标志，更不能提前取消后台准备或候选。
        let _ = crate::native_project_close::authorize_exit(world, requested);
        host.notice("请确认是否退出编辑器");
        return;
    }
    // 菜单退出与窗口关闭走同一事务；原作者决定未保存处理，宿主等待全部拥有任务收口。
    upgrade_to_editor_exit(&mut host.exit_app, &mut host.close_sent);
    cancel_preparation(host);
    host.want_close = host.current.is_some();
    host.notice("正在退出编辑器，等待当前工程和本次任务收口…");
}

pub(super) fn poll_empty_exit(host: &mut Host, world: &mut World) {
    if host.current.is_some() { return; } // 有作者时沿用带目标的子进程协议，不叠加第二个父确认层。
    let active = world.get_resource::<NativeProjectCloseState>()
        .is_some_and(|state| state.target() == CloseTarget::Editor && state.active());
    if !active { return; }
    if crate::native_project_close::authorize_exit(world, false) {
        if !host.exit_app {
            upgrade_to_editor_exit(&mut host.exit_app, &mut host.close_sent);
            cancel_preparation(host);
            host.want_close = false;
            host.notice("正在退出编辑器，等待本次任务收口…");
            // 只有这次明确选择允许重试失败写入；等待中的普通帧不能每帧重试磁盘。
            if let Err(error) = author_preferences::ready_to_exit(world, true) {
                hold_preferences_exit(host, world, error);
            }
        }
    } else if !world.resource::<NativeProjectCloseState>().active() {
        host.exit_app = false;
        #[cfg(windows)]
        crate::window_chrome::placement::cancel_exit(world);
        host.want_close = false;
        host.close_sent = false;
        host.notice("已取消退出编辑器，当前工作台和准备任务保留");
    }
}

pub(super) fn finish_app_exit(host: &mut Host, world: &mut World) {
    // 无工程必须先通过同一确认取得exit_app；后台线程、候选和设置写者继续沿原有非阻塞门收口。
    if host.exit_app && host.current.is_none() && !host.busy()
        && crate::native_plugins::management::ready_to_exit(world) {
        match author_preferences::ready_to_exit(world, false) {
            Ok(true) => {
                #[cfg(windows)]
                if let Some(placement) = crate::window_chrome::placement::capture_on_exit(world)
                    && host.preferences.main_window != Some(placement)
                {
                    // 复用主宿主的单写者与原文校验。窗口保持存活，下一次轮询收到落盘结果后才退出。
                    let mut preferences = host.preferences.clone();
                    preferences.main_window = Some(placement);
                    host.preferences_job = Some(process::save_preferences(preferences));
                    return;
                }
                world.write_message(AppExit::Success);
            }
            Ok(false) => (),
            Err(error) => hold_preferences_exit(host, world, error),
        }
    }
}

fn hold_preferences_exit(host: &mut Host, world: &mut World, error: String) {
    host.exit_app = false;
    #[cfg(windows)]
    crate::window_chrome::placement::cancel_exit(world);
    if let Some(mut state) = world.get_resource_mut::<NativeProjectCloseState>() {
        state.hold(error.clone());
    }
    host.notice(error);
}

pub(super) fn empty_exit_blocks_work(host: &Host, world: &World) -> bool {
    host.current.is_none() && (host.exit_app || world.get_resource::<NativeProjectCloseState>()
        .is_some_and(NativeProjectCloseState::captures_input))
}

fn take_initial(host: &mut Host, world: &World) -> Option<PathBuf> {
    if empty_exit_blocks_work(host, world) { None } else { host.initial.take() }
}

fn close_target(exit_app: bool) -> CloseTarget {
    if exit_app { CloseTarget::Editor } else { CloseTarget::Project }
}

fn upgrade_to_editor_exit(exit_app: &mut bool, close_sent: &mut bool) {
    // 工程确认已经显示时，第一次X/退出仍要可靠重发Editor目标；重复X不能不断重置同一确认。
    if !*exit_app {
        *exit_app = true;
        *close_sent = false;
    }
}

fn queue_close_request(
    want_close: bool, exit_app: bool, close_sent: &mut bool,
    mut send: impl FnMut(HostEvent) -> Result<(), String>,
) {
    if want_close && !*close_sent && send(HostEvent::close_request(close_target(exit_app))).is_ok() {
        *close_sent = true;
    }
}

fn accepts_close_cancel(current_pid: Option<u32>, source_pid: u32, awaiting: bool,
    exit_app: bool, cancelled: CloseTarget) -> bool {
    // 目标和进程都属于输入身份；旧工程的迟到取消不能撤回当前退出编辑器请求。
    awaiting && current_pid == Some(source_pid) && close_target(exit_app) == cancelled
}

fn poll_jobs(host: &mut Host) {
    if host
        .preferences_job
        .as_ref()
        .is_some_and(|job| job.worker.is_finished())
    {
        let job = host.preferences_job.take().unwrap();
        let joined = job.worker.join().is_ok();
        match job
            .result
            .try_recv()
            .unwrap_or_else(|_| Err("设置后台未返回结果".into()))
        {
            Ok(preferences) if joined => {
                host.preferences = preferences;
                host.notice("编辑器个人设置已保存");
            }
            result => {
                let error = result.err().unwrap_or_else(|| "设置保存线程异常".into());
                if host.exit_app {
                    warn!("退出前的编辑器个人设置未能保存：{error}");
                }
                host.notice(error);
            }
        }
    }
    if host
        .job
        .as_ref()
        .is_some_and(|job| job.worker.is_finished())
    {
        let job = host.job.take().unwrap();
        let joined = job.worker.join().is_ok();
        let result = if !joined {
            Err("工程准备线程异常".into())
        } else if job.control.is_cancelled() {
            Err("工程准备已取消；已发布文件保留，当前工程不变".into())
        } else {
            job.result
                .try_recv()
                .unwrap_or_else(|_| Err("工程后台未返回结果".into()))
        };
        match result {
            Ok(project)
                if host
                    .current
                    .as_ref()
                    .is_some_and(|current| current.project.root == project.root) =>
            {
                if let Some(current) = &host.current {
                    let _ = current.send(HostEvent::Activate);
                }
                host.notice("已返回当前工程，原文档与历史保留");
            }
            Ok(project) if !host.exit_app => {
                host.prepared = Some(project);
                host.notice("工程已校验，正在确认首页界面偏好已保存…");
            }
            Ok(_) => host.notice("工程准备已停止"),
            Err(error) => host.notice(error),
        }
    }
}

fn authors_present(host: &Host) -> bool {
    host.current.is_some() || host.candidate.is_some() || host.retiring.is_some()
}

fn sync_preferences_access(world: &mut World) {
    let editable = author_preferences::home_ready(world);
    if let Some(mut access) = world.get_resource_mut::<
        feathrise_editor_map::workbench::WorkbenchSettingsAccess>() {
        if access.editable != editable { access.editable = editable; }
    }
}

fn poll_author_preferences(host: &mut Host, world: &mut World) {
    let before = author_preferences::home_ready(world);
    let error = author_preferences::poll_host(world, authors_present(host));
    sync_preferences_access(world);
    if before != author_preferences::home_ready(world) { host.changed = true; }
    if let Some(error) = error { host.notice(error); }
}

fn poll_prepared(host: &mut Host, world: &mut World, parent: usize) {
    if host.prepared.is_none() || host.exit_app { return; }
    match author_preferences::yield_to_author(world) {
        Ok(false) => return,
        Err(error) => {
            host.prepared = None;
            host.notice(format!("工程未交接：{error}；已发布的工程文件保留"));
            return;
        }
        Ok(true) => (),
    }
    // flush和交权限在同一Last完成；候选尚未显示时首页也不能再接受会丢失的末次设置输入。
    sync_preferences_access(world);
    let project = host.prepared.take().expect("已核对唯一预检结果");
    match AuthorProcess::spawn(project, host.target_override.as_deref(), parent) {
        Ok(candidate) => {
            host.candidate = Some(candidate);
            host.notice("正在准备目标工程工作台；当前工程保持打开…");
        }
        Err(error) => host.notice(error),
    }
}

fn poll_picker(host: &mut Host, world: &mut World) {
    let Some(result) = host.picker.as_mut().and_then(|picker| picker.picker.poll()) else {
        return;
    };
    let picker = host.picker.take().unwrap();
    let origin_current = picker.origin.is_none()
        || host
            .current
            .as_ref()
            .is_some_and(|p| Some(p.child.id()) == picker.origin);
    if origin_current && !host.exit_app {
        match result {
            Ok(Some(value)) => {
                if let Some(current) = &host.current {
                    if Some(current.child.id()) == picker.origin {
                        if let Err(error) = current.send(HostEvent::FillPath {
                            field: picker.field,
                            value,
                        }) {
                            host.notice(error);
                        }
                    } else {
                        fill_path(world, picker.field, value);
                    }
                } else {
                    fill_path(world, picker.field, value);
                }
            }
            Ok(None) => {}
            Err(error) => host.notice(error),
        }
    }
    host.changed = true;
}

#[cfg(unix)]
fn poll_surfaces(host: &mut Host, world: &mut World) {
    if let Some(candidate) = host.candidate.as_mut() {
        if let Err(error) = crate::portable_author::host::poll(&mut candidate.surface, world, false) {
            fail_candidate(host, error);
        }
    }
    if let Some(current) = host.current.as_mut() {
        if let Err(error) = crate::portable_author::host::poll(&mut current.surface, world, true) {
            host.notice(error);
        }
    }
}

fn poll_processes(host: &mut Host, world: &mut World, parent: usize) {
    if host
        .retiring
        .as_ref()
        .is_some_and(|(_, since)| since.elapsed() > Duration::from_secs(15))
    {
        if let Some((candidate, _)) = host.retiring.as_mut() {
            let _ = candidate.child.kill();
        }
    }
    if host
        .retiring
        .as_mut()
        .is_some_and(|(candidate, _)| candidate.child.try_wait().ok().flatten().is_some())
    {
        host.retiring = None;
        host.changed = true;
    }
    if let Some(candidate) = host.candidate.as_mut() {
        if let Ok(Some(status)) = candidate.child.try_wait() {
            fail_candidate(
                host,
                format!("目标工程工作台未能打开：{status}；原工程保留"),
            );
        } else if candidate.started.elapsed() > Duration::from_secs(120) && !candidate.ready()
        {
            fail_candidate(host, "目标工程准备超时，原工程保留".into());
        }
    }
    let ended = host
        .current
        .as_mut()
        .map(|current| current.child.try_wait());
    if let Some(Ok(Some(status))) = ended {
        host.current = None;
        host.want_close = false;
        host.close_sent = false;
        host.changed = true;
        if host
            .candidate
            .as_ref()
            .is_some_and(AuthorProcess::ready)
            && !host.exit_app
        {
            adopt(host, world, parent);
        } else {
            show_page(world, Page::Start);
            host.notice(if status.success() {
                "工程已关闭，回到开始页".into()
            } else {
                format!("工程工作台异常退出：{status}；工程文件保留，可从最近记录重开")
            });
            // 作者子进程退役不会保证向仍在前台的父窗口补发焦点；开始页须接回原生与Bevy输入。
            if !host.exit_app {
                if let Err(error) = crate::workspace_window::restore_host_focus(world, parent) {
                    host.notice(error);
                }
            }
        }
    }
}

fn adopt(host: &mut Host, world: &mut World, parent: usize) {
    // 后台准备可以完成，但无工程退出确认未选择或刚取消时，隐藏候选不能接管工作台。
    if empty_exit_blocks_work(host, world) { return; }
    let Some(mut candidate) = host.candidate.take() else {
        return;
    };
    // 先可靠排入采用意图再显示，避免满通道使已可编辑实例仍被当作可无条件退役的隐藏候选。
    let result = candidate.send(HostEvent::Activate)
        .and_then(|()| candidate.activate(world, parent));
    if let Err(error) = result {
        let _ = candidate.send(HostEvent::AbortCandidate);
        host.retiring = Some((candidate, Instant::now()));
        host.notice(error);
        return;
    }
    candidate.model_revision = 0;
    let _ = host
        .preferences
        .record_opened(&candidate.project.name, &candidate.project.root);
    if host.preferences_job.is_none() {
        host.preferences_job = Some(process::save_preferences(host.preferences.clone()));
    }
    host.notice(format!("已打开 {}", candidate.project.name));
    host.current = Some(candidate);
    host.want_close = false;
    host.close_sent = false;
}

#[cfg(test)]
#[path = "host_empty_exit_tests.rs"]
mod empty_exit_tests;
#[cfg(test)]
#[path = "host_empty_exit_ui_tests.rs"]
mod empty_exit_ui_tests;
#[cfg(test)]
#[path = "host_preferences_tests.rs"]
mod preferences_tests;

fn retire_candidate(host: &mut Host) {
    if let Some(candidate) = host.candidate.take() {
        let _ = candidate.send(HostEvent::AbortCandidate);
        host.retiring = Some((candidate, Instant::now()));
        host.changed = true;
    }
}
fn fail_candidate(host: &mut Host, error: String) {
    if host.close_sent {
        if let Some(current) = &host.current {
            let _ = current.send(HostEvent::CancelClose);
        }
    }
    host.want_close = false;
    host.close_sent = false;
    retire_candidate(host);
    host.notice(error);
}
fn cancel_preparation(host: &mut Host) {
    host.prepared = None;
    if let Some(job) = &host.job {
        job.control.cancel();
    }
    if let Some(picker) = &host.picker {
        picker.picker.cancel();
    }
    retire_candidate(host);
}

fn publish(host: &mut Host, world: &mut World) {
    let mut model = super::model(&host.preferences, host.current.as_ref().map(|p| &p.project));
    model.status = host.notice.clone();
    model.busy = host.busy() || (!authors_present(host) && !author_preferences::home_ready(world));
    model.cancellable = host.job.is_some() || host.prepared.is_some()
        || host.candidate.is_some() || host.picker.is_some();
    model.revision = world.resource::<WorkspaceModel>().revision.wrapping_add(1);
    world.insert_resource(model);
    host.changed = false;
}

fn sync_window_title(
    model: Res<WorkspaceModel>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !model.is_changed() {
        return;
    }
    // 系统任务栏和Alt+Tab继续显示原工程名称格式，与客户区自绘标题分开维护。
    // 必须在Update设置，确保本帧Last的Winit写回先提交标题；Last之后才修改Window时，
    // 新到的窗口事件可能把尚未提交的标题复制进CachedWindow，令后端误判为已经同步。
    let title = model.project.as_ref().map_or_else(
        || "Feathrise（羽立）".into(),
        |project| format!("{} · Feathrise（羽立）", project.name),
    );
    if let Ok(mut window) = windows.single_mut() {
        if window.title != title {
            window.title = title;
        }
    }
}
fn sync_models(host: &mut Host, world: &World) {
    let model = world.resource::<WorkspaceModel>();
    for instance in [&mut host.current, &mut host.candidate]
        .into_iter()
        .flatten()
    {
        if instance.model_revision == model.revision {
            continue;
        }
        let mut local = model.clone();
        local.project = Some(WorkspaceProject {
            name: instance.project.name.clone(),
            root: instance.project.root.clone(),
        });
        if instance.send(HostEvent::Model(local)).is_ok() {
            instance.model_revision = model.revision;
        }
    }
}

#[cfg(test)]
mod close_target_tests {
    use super::*;

    #[test]
    fn project_confirmation_upgrades_to_one_retriable_editor_request() {
        let mut exit_app = false;
        let mut sent = false;
        let mut requests = Vec::new();
        queue_close_request(true, exit_app, &mut sent, |event| {
            requests.push(event.close_target().unwrap()); Ok(())
        });
        assert!(sent);
        assert_eq!(requests, [CloseTarget::Project]);

        upgrade_to_editor_exit(&mut exit_app, &mut sent);
        assert!(!sent, "Project已经发出也必须重发Editor确认");
        queue_close_request(true, exit_app, &mut sent, |_| Err("控制队列暂满".into()));
        assert!(!sent, "发送未入队时保留新目标，下一帧重试");
        queue_close_request(true, exit_app, &mut sent, |event| {
            requests.push(event.close_target().unwrap()); Ok(())
        });
        assert!(sent);
        assert_eq!(requests, [CloseTarget::Project, CloseTarget::Editor]);

        upgrade_to_editor_exit(&mut exit_app, &mut sent);
        queue_close_request(true, exit_app, &mut sent, |_| panic!("重复X不能不断打断当前Editor确认"));
    }

    #[test]
    fn cancellation_requires_current_process_pending_request_and_the_same_target() {
        assert!(accepts_close_cancel(Some(42), 42, true, false, CloseTarget::Project));
        assert!(!accepts_close_cancel(Some(42), 42, true, true, CloseTarget::Project),
            "旧Project取消不能撤回已升级的Editor请求");
        assert!(!accepts_close_cancel(Some(42), 41, true, true, CloseTarget::Editor));
        assert!(!accepts_close_cancel(None, 42, true, true, CloseTarget::Editor));
        assert!(!accepts_close_cancel(Some(42), 42, false, true, CloseTarget::Editor));
        assert!(accepts_close_cancel(Some(42), 42, true, true, CloseTarget::Editor));
    }
}
