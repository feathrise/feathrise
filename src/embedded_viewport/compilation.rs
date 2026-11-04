//! 尚未就绪的工程3D启动允许显式撤销；精确取消已有控制牌，不抢先归还真实工作槽。

use super::resources::{EmbeddedStatus, Runtime};
use bevy::prelude::*;

/// 仅未握手的原启动可停止；健康预览、代码应用和关闭事务各自沿原生命周期收口。
pub(crate) fn can_stop_compilation(world: &World) -> bool {
    let Some(status) = world.get_resource::<EmbeddedStatus>() else { return false; };
    let Some(runtime) = world.get_non_send::<Runtime>() else { return false; };
    status.active && !status.ready && !status.playing
        && runtime.cleanup_failure.is_none()
        && !runtime.exit_requested && !runtime.close_return_requested
        && runtime.code_apply.is_none() && runtime.waiting_mode.is_none() && !runtime.pending_edit
        && !world.get_resource::<crate::native_project_close::NativeProjectCloseState>()
            .is_some_and(|close| close.active())
        && runtime.active.as_ref().is_some_and(|active|
            !active.control.is_cancelled() && !active.control.cleanup_failed())
}

/// 点击帧只写控制牌和固定意图，不等待Cargo、进程、管道或线程；真正排空仍由原poll确认。
pub(crate) fn stop_compilation(world: &mut World) -> bool {
    if !can_stop_compilation(world) { return false; }
    let selection = world.resource::<crate::viewport_mode::ViewportModeState>().revision;
    let profile = world.get_resource::<feathrise_editor_core::ProjectLaunchSettings>()
        .map_or(default(), |settings| settings.profile);
    {
        let mut runtime = world.non_send_mut::<Runtime>();
        runtime.active.as_ref().expect("可停止的唯一工作槽已检查").control.cancel();
        runtime.compilation_stopped = true;
        runtime.compilation_stop_input = Some((selection, profile));
        runtime.pending_play = false;
        runtime.play_wait = None;
        runtime.pending_edit = false;
        runtime.restart_requested = false;
        // 旧Ready即使已排队，也不能重新建立会话。后台仍持有自身身份与Job负责完整清理。
        runtime.identity = None;
    }
    world.resource_mut::<EmbeddedStatus>().error = None;
    super::systems::feedback(world, "正在停止工程3D编译并确认后台任务退出；作者文档和历史保留");
    true
}
