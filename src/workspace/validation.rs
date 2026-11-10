//! 显式启用的同窗口工程链有限验收；只投递真实工程命令，不注入桌面键鼠或复制作者算法。
//!
//! 本轮只拥有自建Projects及隔离偏好，复用原作者GPU/保存/F12报告；失败先留证再请求正常退出。
//! 当前作者若因未保存确认取消关闭，保留窗口与草稿，绝不强杀当前实例。

#[path = "validation_io.rs"]
mod io;
#[path = "validation_resources.rs"]
mod resources;

use super::resources::Host;
use crate::{
    workspace_ui::{Page, WorkspaceCommand, WorkspaceModel, WorkspaceProbe},
    workspace_window,
};
use bevy::{
    prelude::*,
    window::{PrimaryWindow, RawHandleWrapper},
};
use resources::*;
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) fn install(app: &mut App) -> Result<(), String> {
    let Some(directory) = std::env::var_os("FEATHRISE_WORKSPACE_SMOKE_DIR").map(PathBuf::from)
    else {
        return Ok(());
    };
    if !directory.is_absolute() {
        return Err("同窗口验收目录必须是外部独占创建的绝对目录".into());
    }
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|error| format!("同窗口验收目录必须在启动前创建：{error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("同窗口验收目录必须是普通目录，不能跟随链接".into());
    }
    let directory = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let host = app.world().non_send::<Host>();
    if host.initial.is_some() || host.current.is_some() || host.candidate.is_some() {
        return Err("同窗口验收必须从无工程工作台开始，不能使用现有作者工程".into());
    }
    if std::env::var_os("FEATHRISE_PREFERENCES_FILE").is_none()
        || !inside(host.preferences.file(), &directory)
    {
        return Err("同窗口验收必须把FEATHRISE_PREFERENCES_FILE隔离到本次输出目录".into());
    }
    if directory.join("unified-workspace.json").exists() || directory.join("Projects").exists() {
        return Err("同窗口验收目录已有报告或工程，拒绝重用".into());
    }
    let mut owner = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(".feathrise-workspace-smoke-owner.json"))
        .map_err(|error| format!("无法独占标记同窗口验收目录：{error}"))?;
    let owner_bytes = serde_json::to_vec(&json!({"pid":std::process::id(),"directory":directory}))
        .map_err(|error| error.to_string())?;
    owner
        .write_all(&owner_bytes)
        .map_err(|error| error.to_string())?;
    let initial_default = host.preferences.default_project_directory.clone();
    let preferences_file = host.preferences.file().to_path_buf();
    let home = host.preferences.home().to_path_buf();
    let prepare_directory = directory.clone();
    let job = IoJob::spawn(move |_| io::prepare(prepare_directory))?;
    app.insert_non_send(Plan {
        directory,
        started: Instant::now(),
        stage_started: Instant::now(),
        stage: Stage::Preparing,
        frames: 0,
        issued: 0,
        parent: None,
        window_unchanged: true,
        native_title: String::new(),
        title_wait_started: None,
        paths: None,
        job: Some(job),
        initial_default,
        preferences_file,
        home,
        a: None,
        b: None,
        candidates: Vec::new(),
        reports: Vec::new(),
        proofs: Vec::new(),
        failed: None,
        cancellation_sent: false,
        default_disk: None,
        completion: Completion::new(),
    })
    .add_systems(Last, drive.after(super::host::advance));
    Ok(())
}

pub(super) fn completion(world: &World) -> Option<Completion> {
    world
        .get_non_send::<Plan>()
        .map(|plan| plan.completion.clone())
}

/// 只由工程创建后台调用；重开工程不走此入口，也不在帧系统内准备资产。
pub(super) fn prepare_created(root: &Path, label: &str) -> Result<(), String> {
    let Some(directory) = std::env::var_os("FEATHRISE_WORKSPACE_SMOKE_DIR") else {
        return Ok(());
    };
    io::prepare_created(Path::new(&directory), root, label)
}

fn drive(world: &mut World) {
    let Some(mut plan) = world.remove_non_send::<Plan>() else {
        return;
    };
    plan.frames = plan.frames.saturating_add(1);
    if plan.stage != Stage::Stopped {
        if let Err(error) = tick(&mut plan, world) {
            error!("同窗口有限验收失败：{error}");
            if matches!(plan.stage, Stage::Writing | Stage::Quit) {
                complete(&plan, Err(error.clone()));
                world.non_send_mut::<Host>().notice(error);
                plan.stage(Stage::Stopped);
            } else if plan.failed.is_none() {
                plan.failed = Some(error);
                if let Some(job) = &plan.job {
                    job.cancel();
                }
                plan.stage(Stage::FailedDrain);
            }
        }
    }
    world.insert_non_send(plan);
}

fn tick(plan: &mut Plan, world: &mut World) -> Result<(), String> {
    if plan.stage == Stage::Quit {
        if world.non_send::<Host>().current.is_none()
            && world.resource::<crate::native_project_close::NativeProjectCloseState>().active()
        {
            // 报告已发布且当前仍为本轮无工程工作台；新退出确认也必须提供明确输入，不能等任务结束自动退出。
            use bevy::ecs::system::RunSystemOnce;
            world.run_system_once(crate::native_project_close::confirm_workspace_validation_close)
                .map_err(|error| format!("无法确认本轮无工程验收退出：{error}"))?;
        }
        let host = world.non_send::<Host>();
        if host.exit_app && host.current.is_none() && !host.busy() {
            complete(plan, plan.failed.clone().map_or(Ok(()), Err));
            plan.stage(Stage::Stopped);
        } else if plan.stage_started.elapsed() > Duration::from_secs(30) {
            world.write_message(WorkspaceCommand::CancelOperation);
            let error = plan
                .failed
                .clone()
                .unwrap_or_else(|| "报告已发布，但正式关闭仍未完成；保留当前作者等待处理".into());
            complete(plan, Err(error.clone()));
            world.non_send_mut::<Host>().notice(error);
            plan.stage(Stage::Stopped);
        }
        return Ok(());
    }
    // 身份错误本身也必须能落失败报告；失败收口不能再次被同一个窗口校验挡在前面。
    if plan.stage == Stage::FailedDrain {
        if !plan.cancellation_sent {
            world.write_message(WorkspaceCommand::CancelOperation);
            plan.cancellation_sent = true;
        }
        if let Some(job) = &plan.job {
            job.cancel();
        }
        let _ = plan.poll();
        if plan.job.is_none()
            && (!world.non_send::<Host>().busy()
                || plan.stage_started.elapsed() > Duration::from_secs(30))
        {
            publish(plan, world)?;
        }
        return Ok(());
    }
    if plan.stage == Stage::Writing {
        if let Some(result) = plan.poll() {
            let IoResult::Written(path) = result? else {
                return Err("报告发布返回了错误阶段结果".into());
            };
            info!("同窗口有限验收报告已发布：{}", path.display());
            if current_is_owned(plan, world) {
                world.write_message(WorkspaceCommand::QuitApp);
                plan.stage(Stage::Quit);
            } else {
                complete(
                    plan,
                    Err("发现非自有当前工程，报告已保存但拒绝自动关闭".into()),
                );
                plan.stage(Stage::Stopped);
            }
        }
        return Ok(());
    }
    if plan.started.elapsed() > Duration::from_secs(900)
        && !matches!(plan.stage, Stage::FailedDrain | Stage::Writing)
    {
        return Err(format!("同窗口有限验收超过900秒：{:?}", plan.stage));
    }
    let parent = {
        let mut query = world.query_filtered::<(&RawHandleWrapper, &Window), With<PrimaryWindow>>();
        query
            .single(world)
            .ok()
            .and_then(|(raw, _)| workspace_window::primary_hwnd(raw))
    };
    let Some(parent) = parent else {
        if plan.parent.is_some() {
            plan.window_unchanged = false;
            return Err("验收过程中主窗口RawHandle丢失".into());
        }
        return Ok(());
    };
    if plan.parent.is_some_and(|original| original != parent)
        || !workspace_window::parent_is_valid(parent, std::process::id())
    {
        plan.window_unchanged = false;
        return Err("验收过程中主窗口HWND或所属PID发生变化".into());
    }
    plan.parent = Some(parent);
    observe_identities(plan, world, parent)?;

    // 直接读取持久主HWND的系统标题，不能仅凭Bevy字段或作者子窗口标题证明任务栏已更新。
    let expected_title = world.non_send::<Host>().current.as_ref().map_or_else(
        || "Feathrise（羽立）".to_owned(),
        |current| format!("{} · Feathrise（羽立）", current.project.name),
    );
    let mut title = [0u16; 1024];
    let length = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW(parent as _, title.as_mut_ptr(), title.len() as i32) };
    plan.native_title = String::from_utf16_lossy(&title[..length.max(0) as usize]);
    if plan.native_title != expected_title {
        let started = plan.title_wait_started.get_or_insert_with(Instant::now);
        if started.elapsed() > Duration::from_secs(2) {
            return Err(format!("系统任务栏标题未同步：期望{expected_title:?}，实际{:?}", plan.native_title));
        }
        return Ok(()); // 原业务采用后，允许下一帧Update/Last完成真实标题写回。
    }
    plan.title_wait_started = None;

    match plan.stage {
        Stage::Preparing => {
            if let Some(result) = plan.poll() {
                let IoResult::Prepared(paths) = result? else {
                    return Err("验收准备返回了错误阶段结果".into());
                };
                plan.paths = Some(paths);
                plan.stage(Stage::Start);
            }
        }
        Stage::Start if idle(world) && initial_ui_ready(world) => {
            if !same_path(&plan.initial_default, &plan.home.join("feathrise")) {
                return Err("初始默认工程目录不是用户主目录下的feathrise".into());
            }
            record(plan, world, "无工程原工作台及中央开始页已真实布局");
            let paths = plan.paths.as_ref().unwrap();
            world.write_message(WorkspaceCommand::Create {
                parent: paths.parent.clone(),
                name: "中文 工程A".into(),
                game_view: Default::default(),
            });
            plan.stage(Stage::CreateA);
        }
        Stage::CreateA => {
            let expected = plan.paths.as_ref().unwrap().a.clone();
            if let Some(identity) = author_stage(plan, world, parent, 1, expected)? {
                plan.a = Some(identity);
                record(plan, world, "A创建、原作者保存及F12返回的GPU报告通过");
                world.write_message(WorkspaceCommand::Open(
                    plan.paths.as_ref().unwrap().missing.clone(),
                ));
                plan.stage(Stage::BadOpen);
            }
        }
        Stage::BadOpen if idle(world) && plan.frames >= plan.issued + 3 => {
            let host = world.non_send::<Host>();
            let identity = host.current.as_ref().map(identity);
            if identity.as_ref() != plan.a.as_ref() || host.candidate.is_some() {
                return Err("错误打开请求替换了原A作者实例或产生了候选".into());
            }
            if ["无法", "无效", "不存在", "找不到", "拒绝"]
                .iter()
                .any(|word| host.notice.contains(word))
            {
                record(
                    plan,
                    world,
                    "错误打开显示原因并保留A的同PID、同子HWND和工程身份",
                );
                let paths = plan.paths.as_ref().unwrap();
                world.write_message(WorkspaceCommand::Create {
                    parent: paths.parent.clone(),
                    name: "中文 工程B".into(),
                    game_view: feathrise_runtime::GameView::FixedTopDown,
                });
                plan.stage(Stage::CreateB);
            }
        }
        Stage::CreateB => {
            let expected = plan.paths.as_ref().unwrap().b.clone();
            if let Some(identity) = author_stage(plan, world, parent, 2, expected)? {
                if plan
                    .a
                    .as_ref()
                    .is_some_and(|a| a.project_id == identity.project_id || a.pid == identity.pid)
                {
                    return Err("B工程没有独立工程身份或作者进程".into());
                }
                if workspace_window::find_child(parent, plan.a.as_ref().unwrap().pid).is_some() {
                    return Err("切换到B之后旧A子窗口仍挂在主窗口内".into());
                }
                plan.b = Some(identity);
                record(plan, world, "B同窗口采用、原作者GPU报告及旧A窗口退役通过");
                world.write_message(WorkspaceCommand::Open(
                    plan.paths.as_ref().unwrap().a.clone(),
                ));
                plan.stage(Stage::ReopenA);
            }
        }
        Stage::ReopenA => {
            let expected = plan.paths.as_ref().unwrap().a.clone();
            if let Some(identity) = author_stage(plan, world, parent, 3, expected)? {
                if identity.project_id != plan.a.as_ref().unwrap().project_id {
                    return Err("A重开采用了不同的工程身份".into());
                }
                if workspace_window::find_child(parent, plan.b.as_ref().unwrap().pid).is_some() {
                    return Err("重开A之后旧B子窗口尚未退役".into());
                }
                record(
                    plan,
                    world,
                    "A重开恢复首轮已保存场景与对象布局，B窗口已退役",
                );
                world.write_message(WorkspaceCommand::CloseProject);
                plan.stage(Stage::Close);
            }
        }
        Stage::Close if idle(world) && world.non_send::<Host>().current.is_none() => {
            let probe = world.resource::<WorkspaceProbe>();
            if probe.page != Page::Start || probe.project.is_some() {
                return Ok(());
            }
            record(plan, world, "关闭工程回到同一主窗口中央开始页");
            world.write_message(WorkspaceCommand::SaveDefaultDirectory(
                plan.paths
                    .as_ref()
                    .unwrap()
                    .default
                    .to_string_lossy()
                    .into_owned(),
            ));
            plan.stage(Stage::SaveDefault);
        }
        Stage::SaveDefault if idle(world) => {
            let expected = &plan.paths.as_ref().unwrap().default;
            if !same_path(
                &world
                    .non_send::<Host>()
                    .preferences
                    .default_project_directory,
                expected,
            ) {
                return Err("默认工程目录保存没有被正式采用".into());
            }
            let file = plan.preferences_file.clone();
            let home = plan.home.clone();
            let paths = plan.paths.as_ref().unwrap().clone();
            plan.job = Some(IoJob::spawn(move |_| io::preferences(file, home, paths))?);
            crate::workspace_ui::show_page(world, Page::New);
            plan.stage(Stage::DefaultUi);
        }
        Stage::DefaultUi => {
            if let Some(result) = plan.poll() {
                let IoResult::Preferences(proof) = result? else {
                    return Err("设置重读返回了错误阶段结果".into());
                };
                plan.default_disk = Some(proof);
            }
            let probe = world.resource::<WorkspaceProbe>();
            let expected = &plan.paths.as_ref().unwrap().default;
            let applied = probe.page == Page::New
                && probe.fields.iter().any(|field| {
                    field.field == "Parent"
                        && same_path(Path::new(&field.value), expected)
                        && visible_rect(field.rect)
                });
            if plan.default_disk.is_some() && applied {
                record(
                    plan,
                    world,
                    "默认工程目录已落盘，原新建表单真实字段自动回显",
                );
                crate::workspace_ui::show_page(world, Page::Start);
                publish(plan, world)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn idle(world: &World) -> bool {
    !world.non_send::<Host>().busy()
}

fn current_is_owned(plan: &Plan, world: &World) -> bool {
    world
        .non_send::<Host>()
        .current
        .as_ref()
        .is_none_or(|current| {
            plan.paths.as_ref().is_some_and(|paths| {
                same_path(&current.project.root, &paths.a)
                    || same_path(&current.project.root, &paths.b)
            })
        })
}

fn identity(instance: &super::process::AuthorProcess) -> Identity {
    Identity {
        pid: instance.child.id(),
        project_id: instance.project.id.clone(),
        project_root: instance.project.root.clone(),
        hwnd: instance.hwnd,
    }
}

fn observe_identities(plan: &mut Plan, world: &World, parent: usize) -> Result<(), String> {
    let Some(paths) = &plan.paths else {
        return Ok(());
    };
    let host = world.non_send::<Host>();
    for instance in [host.current.as_ref(), host.candidate.as_ref()]
        .into_iter()
        .flatten()
    {
        if !same_path(&instance.project.root, &paths.a)
            && !same_path(&instance.project.root, &paths.b)
        {
            return Err("验收发现非本轮自建工程，停止自动操作并保留现场".into());
        }
        if let Some(hwnd) = instance.hwnd {
            if workspace_window::find_child(parent, instance.child.id()) != Some(hwnd) {
                // 原Winit窗口先销毁，GPU/World析构完成后进程才退出；已获关闭授权的旧实例
                // 在这段收口期没有HWND是预期行为，不能把它误报为运行中的父关系丢失。
                let closing_current = host.close_sent
                    && host
                        .current
                        .as_ref()
                        .is_some_and(|current| current.child.id() == instance.child.id());
                let destroyed = unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::IsWindow(hwnd as _) == 0
                };
                if !(closing_current && destroyed) {
                    return Err(format!(
                        "作者窗口身份不符：pid={},hwnd={hwnd},closing_current={closing_current},destroyed={destroyed}",
                        instance.child.id()
                    ));
                }
            }
        }
    }
    if let Some(candidate) = &host.candidate {
        let current = identity(candidate);
        if let Some(previous) = plan
            .candidates
            .iter_mut()
            .find(|previous| previous.pid == current.pid)
        {
            *previous = current;
        } else if plan.candidates.len() < 4 {
            plan.candidates.push(current);
        } else {
            return Err("有限验收产生超过预期的作者候选数量".into());
        }
    }
    Ok(())
}

fn author_stage(
    plan: &mut Plan,
    world: &World,
    parent: usize,
    index: usize,
    expected: PathBuf,
) -> Result<Option<Identity>, String> {
    if !idle(world) {
        return Ok(None);
    }
    let host = world.non_send::<Host>();
    let matches = host
        .current
        .as_ref()
        .is_some_and(|current| same_path(&current.project.root, &expected));
    if !matches
        && plan.frames >= plan.issued + 3
        && ["未能", "超时", "无效", "无法", "异常", "失败"]
            .iter()
            .any(|word| host.notice.contains(word))
    {
        return Err(format!(
            "工程采用失败：{}",
            host.notice.chars().take(2048).collect::<String>()
        ));
    }
    let Some(current) = &host.current else {
        return Ok(None);
    };
    if !matches {
        return Ok(None);
    }
    let current = identity(current);
    if current.hwnd.is_none() || workspace_window::find_child(parent, current.pid) != current.hwnd {
        return Ok(None);
    }
    if world
        .resource::<WorkspaceModel>()
        .project
        .as_ref()
        .is_none_or(|project| !same_path(&project.root, &expected))
    {
        return Ok(None);
    }
    if plan.job.is_none() {
        let first = (index == 3)
            .then(|| {
                plan.reports
                    .iter()
                    .find(|report| report.index == 1)
                    .map(|report| Arc::clone(&report.source))
            })
            .flatten();
        let directory = plan.directory.clone();
        let deadline = plan.started + Duration::from_secs(900);
        plan.job = Some(IoJob::spawn(move |cancelled| {
            io::native_report(directory, index, expected, first, deadline, cancelled)
        })?);
        return Ok(None);
    }
    if let Some(result) = plan.poll() {
        let IoResult::Native(report) = result? else {
            return Err("原作者报告返回了错误阶段结果".into());
        };
        plan.reports.push(report);
        return Ok(Some(current));
    }
    Ok(None)
}

fn initial_ui_ready(world: &mut World) -> bool {
    let font_ready = world.resource::<Assets<Font>>()
        .contains(bevy::asset::AssetId::<Font>::default());
    let window_ready = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .is_ok_and(|window| window.width() > 300. && window.height() > 200.);
    let header = world
        .query_filtered::<&ComputedNode, With<feathrise_editor_scene::workbench::WorkbenchHeader>>()
        .iter(world)
        .any(|node| node.size().x > 100. && node.size().y > 10.);
    let probe = world.resource::<WorkspaceProbe>();
    font_ready
        && window_ready
        && header
        && probe.page == Page::Start
        && probe.project.is_none()
        && ["Show(New)", "Show(Open)"].iter().all(|action| {
            probe.buttons.iter().any(|button| {
                &button.action == action && button.enabled && visible_rect(button.rect)
            })
        })
}

fn visible_rect(rect: [f32; 4]) -> bool {
    rect.iter().all(|value| value.is_finite()) && rect[2] > 1. && rect[3] > 1.
}

fn record(plan: &mut Plan, world: &World, label: &str) {
    let host = world.non_send::<Host>();
    plan.proofs.push(json!({"stage":format!("{:?}",plan.stage),"description":label,"parent_hwnd":plan.parent,"native_window_title":plan.native_title,"current":host.current.as_ref().map(identity),"elapsed_ms":plan.started.elapsed().as_secs_f64()*1000.,"ui_page":world.resource::<WorkspaceProbe>().page,"host_idle":!host.busy()}));
}

fn publish(plan: &mut Plan, world: &World) -> Result<(), String> {
    let host = world.non_send::<Host>();
    let report = json!({
        "schema":1,"scope":"single_main_window_workspace_commands_with_original_author_gpu_reports",
        "passed":plan.failed.is_none(),"error":plan.failed,"parent_hwnd":plan.parent,
        "main_hwnd_unchanged":plan.window_unchanged,"root_pid":std::process::id(),"elapsed_ms":plan.started.elapsed().as_secs_f64()*1000.,
        "initial_default_directory":plan.initial_default,"initial_default_is_home_feathrise":same_path(&plan.initial_default,&plan.home.join("feathrise")),
        "preferences_file":plan.preferences_file,"preferences_proof":plan.default_disk,
        "candidate_identities":plan.candidates,"steps":plan.proofs,
        "native_reports":plan.reports.iter().map(|report| &report.proof).collect::<Vec<_>>(),
        "process_cleanup_observed_before_quit":host.current.is_none()&&host.candidate.is_none()&&host.retiring.is_none()&&!host.busy(),
        "remaining_current":host.current.as_ref().map(identity),
        "quit_policy":"publish report in background, then dispatch normal QuitApp; external runner confirms app exit",
        "input":"real WorkspaceCommand messages; first child uses explicit ECS input ownership for original author save and F12 message fixture; self-owned close confirmations select original Save All; no native activation or keyboard injection",
        "not_verified":["native foreground and focus recovery","physical keyboard/mouse and desktop hit testing","IME candidate composition","manual brush/gizmo feel","manual dirty-document switch confirmation","same-World or unsaved instant play","all professional tools or standalone game regression"]
    });
    let directory = plan.directory.clone();
    plan.job = Some(IoJob::spawn(move |_| io::publish(directory, report))?);
    plan.stage(Stage::Writing);
    Ok(())
}

fn complete(plan: &Plan, result: Result<(), String>) {
    if let Ok(mut shared) = plan.completion.shared.try_lock() {
        shared.1 = Some(result);
    }
}
