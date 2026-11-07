//! 导出请求与单飞工作槽；帧内只处理有限消息和短回执，不等待 Cargo、磁盘或线程。

use super::{messages::*, resources::*, worker};
use bevy::{ecs::message::MessageCursor, prelude::*};
use feathrise_editor_core::ProjectBuildProfile;
use std::{sync::mpsc, thread};

pub(super) fn open(world: &mut World, mut requests: Local<MessageCursor<OpenExportDialog>>) {
    if requests.read(world.resource::<Messages<OpenExportDialog>>()).last().is_none() { return; }
    // 先读完本轮请求再检查捕获，先认领的浮窗及其释放帧均不能积压成稍后的自动打开。
    if modal_blocked(world) { return; }
    let mut state = world.resource_mut::<ExportState>();
    state.open = true;
    state.changed();
}

pub(in crate::project_export) fn modal_blocked(world: &World) -> bool {
    world.get_resource::<crate::native_project_close::NativeProjectCloseState>().is_some_and(|close| close.captures_input())
        || world.get_resource::<crate::native_plugins::management::PluginManagerState>().is_some_and(|plugins|plugins.captures_input())
        || world.get_resource::<crate::workspace_ui::AboutState>().is_some_and(|about|about.captures_input())
}

pub(super) fn cancel(world: &mut World, mut requests: Local<MessageCursor<CancelExport>>) {
    if requests.read(world.resource::<Messages<CancelExport>>()).last().is_none() { return; }
    let active = world.non_send::<ExportRuntime>().active.as_ref().map(|active| active.control.clone());
    if let Some(control) = active {
        control.cancel();
        let mut state = world.resource_mut::<ExportState>();
        state.status = "正在取消导出，等待本次构建和文件事务收口…".into();
        state.changed();
    }
}

pub(super) fn start(world: &mut World, mut requests: Local<MessageCursor<RequestExport>>) {
    let Some(request) = requests.read(world.resource::<Messages<RequestExport>>()).last().cloned() else { return; };
    let runtime = world.non_send::<ExportRuntime>();
    if runtime.active.is_some() { return; }
    let reason = runtime.failure.clone().or_else(|| runtime.stopping.then(|| "工程正在关闭，暂不导出".into()))
        .or_else(|| world.get_resource::<crate::native_project_close::NativeProjectCloseState>()
            .filter(|close| close.active()).map(|_| "请先取消关闭确认，再导出工程".into()))
        .or_else(|| modal_blocked(world).then(|| "请先关闭其他工程浮窗，再导出工程".into()))
        .or_else(|| crate::code_iteration::export_blocker(world))
        .or_else(|| world.get_resource::<crate::native_project_menu::NativeProjectMenuState>()
            .filter(|menu| menu.saving()).map(|_| "启动地图仍在保存，请完成后再导出".into()))
        .or_else(|| feathrise_editor_map::standalone::saved_play_blocker(world)
            .map(|_| "作者保存或事务尚未完成，请完成后再导出".into()));
    if let Some(error) = reason { report_failure(world, error); return; }
    if request.destination.as_os_str().is_empty() {
        report_failure(world, "请选择导出目录".into()); return;
    }
    let settings = world.resource::<ExportSettings>();
    let project = settings.project.clone();
    let target = settings.target.clone();
    let destination = if request.destination.is_absolute() { request.destination } else { project.root.join(request.destination) };
    let profile = world.get_resource::<feathrise_editor_core::ProjectLaunchSettings>()
        .map_or(ProjectBuildProfile::default(), |launch| launch.profile);
    let log = world.get_resource::<crate::project_log::ProjectLogChannel>()
        .map(|channel| channel.begin_task(crate::project_log::LogTaskKind::Export));
    let control = crate::build_process::BuildControl::with_export_guards(log,
        crate::code_iteration::export_input_guard(world), crate::code_iteration::export_output_registrar(world));
    let job_control = control.clone();
    let (progress_tx, progress_rx) = mpsc::sync_channel(8);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let worker = thread::Builder::new().name("feathrise-export".into()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
            worker::run(&project, &target, &destination, profile, &job_control, &progress_tx)))
            .unwrap_or_else(|_| {
                job_control.mark_cleanup_failed();
                Err("导出工作线程异常，请保留现场并重新打开编辑器".into())
            });
        let _ = result_tx.send(result);
    });
    match worker {
        Ok(handle) => {
            world.non_send_mut::<ExportRuntime>().active = Some(ActiveExport { control,
                progress: progress_rx, result: result_rx, handle });
            let mut state = world.resource_mut::<ExportState>();
            state.working = true;
            state.failed = None;
            state.receipt = None;
            state.status = format!("正在准备{}导出…", profile.label());
            state.changed();
        }
        Err(error) => report_failure(world, format!("无法启动导出后台任务：{error}")),
    }
}

pub(super) fn poll(world: &mut World) {
    if world.get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|close| !close.active())
        && let Some(mut runtime) = world.get_non_send_mut::<ExportRuntime>()
    {
        // 原关闭事务可在收口期间取消：撤回拒收新任务的意图，不复活已取消worker或清除收口失败。
        // 没有关闭资源的独立退出路径仍保持停止意图，不能把“未知”视为用户已取消关闭。
        runtime.stopping = false;
    }
    let Some(runtime) = world.get_non_send::<ExportRuntime>() else { return; };
    let Some(active) = &runtime.active else { return; };
    let progress = active.progress.try_iter().take(8).last();
    let finished = active.handle.is_finished();
    if let Some(progress) = progress {
        let mut state = world.resource_mut::<ExportState>();
        state.status = progress;
        state.changed();
    }
    if !finished { return; }
    // 只有线程实际结束才归还槽；is_finished 后 join 不等待进程、文件 IO 或清理。
    let active = world.non_send_mut::<ExportRuntime>().active.take().expect("已确认导出槽存在");
    let joined = active.handle.join().is_ok();
    let result = active.result.try_recv().unwrap_or_else(|_| Err("导出线程未返回完成回执".into()));
    if active.control.cleanup_failed() || !joined {
        world.non_send_mut::<ExportRuntime>().failure = Some("导出构建进程收口未确认，编辑器保持打开".into());
    }
    let mut state = world.resource_mut::<ExportState>();
    state.working = false;
    match result {
        Ok(receipt) if joined => {
            state.status = format!("导出完成：{} 个文件，{} 字节{}", receipt.files, receipt.bytes,
                receipt.warning.as_ref().map(|warning| format!("；{warning}")).unwrap_or_default());
            state.failed = None;
            state.receipt = Some(receipt);
        }
        Err(_) if active.control.is_cancelled() && !active.control.cleanup_failed() => {
            state.status = "导出已取消，原输出与作者草稿保留".into();
            state.failed = None;
        }
        result => {
            let error = result.err().unwrap_or_else(|| "导出线程异常，未确认成功发布".into());
            state.status = format!("导出失败：{error}");
            state.failed = Some(error);
        }
    }
    state.changed();
    if let Some(log) = active.control.log_sink() { log.task_message(&state.status); }
}

fn report_failure(world: &mut World, error: String) {
    let mut state = world.resource_mut::<ExportState>();
    state.status = format!("导出未开始：{error}");
    state.failed = Some(error);
    state.changed();
}

#[cfg(test)]
mod modal_tests {
    use super::*;

    #[test]
    fn plugin_capture_and_release_consume_export_open_requests_without_replay() {
        let mut app = App::new();
        app.insert_resource(ExportState { open:false, destination:"unused/output".into(), status:String::new(),
            failed:None, receipt:None, revision:1, working:false })
            .insert_resource(crate::native_plugins::management::PluginManagerState::new("unused-owned-project".into()))
            .add_message::<OpenExportDialog>()
            .add_systems(Update, open);
        app.world_mut().resource_mut::<crate::native_plugins::management::PluginManagerState>().open = true;
        app.world_mut().write_message(OpenExportDialog);
        app.update();
        assert!(!app.world().resource::<ExportState>().open, "同帧插件管理已认领时不能打开第二层浮窗");
        {
            let mut plugins = app.world_mut().resource_mut::<crate::native_plugins::management::PluginManagerState>();
            plugins.open = false;
            plugins.captured = true;
        }
        app.world_mut().write_message(OpenExportDialog);
        app.update();
        assert!(!app.world().resource::<ExportState>().open, "插件关闭释放帧仍丢弃底层打开意图");
        app.world_mut().resource_mut::<crate::native_plugins::management::PluginManagerState>().captured = false;
        app.update();
        assert!(!app.world().resource::<ExportState>().open, "输入归还后不能重放先前被拒绝的打开意图");
        app.world_mut().write_message(OpenExportDialog);
        app.update();
        assert!(app.world().resource::<ExportState>().open, "仅接受作者输入归还后的明确新请求");
    }
}
