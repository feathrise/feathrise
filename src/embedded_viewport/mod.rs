//! 常驻工程视口的宿主协调：作者World留存文档，真实游戏Job拥有中央三维与玩法。
//! 协议、传输、构建和业务采用分层；帧系统只处理固定消息和共享快照句柄。

mod preview_sources;
mod previews;
mod resources;
mod systems;
mod transport;
mod tools;
mod code_apply;
mod compilation;
mod hotpatch;
#[cfg(all(feature = "validation", windows))]
pub(crate) mod validation;
#[cfg(windows)]
mod worker;

use bevy::prelude::*;
pub(crate) use resources::{ConnectionState, EmbeddedStatus};
pub(crate) use code_apply::{cancel_pending_code_apply, code_apply_status, request_code_apply};
pub(crate) use compilation::{can_stop_compilation, stop_compilation};
pub(crate) use hotpatch::{patch_context, request_patch};
use std::path::PathBuf;

pub(crate) struct EmbeddedViewportPlugin {
    project: crate::project::Project,
    target: PathBuf,
}
impl EmbeddedViewportPlugin {
    pub(crate) fn new(project: crate::project::Project, target: PathBuf) -> Self {
        Self { project, target }
    }
}
impl Plugin for EmbeddedViewportPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(resources::Settings {
            project: self.project.clone(),
            target: self.target.clone(),
        })
        .init_resource::<EmbeddedStatus>()
        .init_non_send::<resources::Runtime>()
        .add_plugins(feathrise_editor_map::standalone::embedded_snapshot::SnapshotCapturePlugin)
        .add_systems(
            Update,
            systems::drive
                .after(crate::viewport_mode::ViewportModeInput)
                .after(feathrise_editor_map::standalone::SavedPlayDispatch)
                .before(feathrise_editor_core::MapEditorSystemSet::Presentation)
                .before(crate::native_play::request_exit),
        );
        feathrise_editor_map::standalone::embedded_overlay::install(app);
        preview_sources::install(app);
    }
}

pub(crate) fn owns_play(world: &World) -> bool {
    world
        .get_resource::<crate::viewport_mode::ViewportModeState>()
        .is_some_and(|mode| mode.selected == crate::viewport_mode::ViewportMode::EmbeddedWorld)
        || world
            .get_resource::<EmbeddedStatus>()
            .is_some_and(|status| status.active)
}

/// 仅投影已有生命周期与快照回执，不依靠底栏提示文字或按钮选中态判断连接成功。
pub(crate) fn connection_state(world: &World) -> ConnectionState {
    use feathrise_editor_map::standalone::embedded_snapshot::EmbeddedSnapshotState;
    let Some(status) = world.get_resource::<EmbeddedStatus>() else {
        return ConnectionState::Disconnected;
    };
    let runtime = world.get_non_send::<resources::Runtime>();
    if runtime.is_some_and(|runtime| runtime.cleanup_failure.is_some()) {
        return ConnectionState::Failed;
    }
    if runtime.is_some_and(|runtime| runtime.compilation_stopped && runtime.active.is_some()) {
        return ConnectionState::Stopping;
    }
    if world.get_resource::<feathrise_editor_core::ProjectLaunchSettings>()
        .is_some_and(|launch| launch.switching_profile) {
        return ConnectionState::Restarting;
    }
    if runtime.is_some_and(|runtime| runtime.code_apply.is_some()) {
        return ConnectionState::Restarting;
    }
    let capture = world.get_resource::<EmbeddedSnapshotState>();
    if status.error.is_some() || (!status.playing && capture.is_some_and(|capture| capture.error.is_some())) {
        return ConnectionState::Failed;
    }
    if !status.active { return ConnectionState::Disconnected; }
    if runtime.is_some_and(|runtime| runtime.active.as_ref().is_some_and(|active| active.control.is_cancelled())) {
        return ConnectionState::Stopping;
    }
    if !status.ready { return ConnectionState::Starting; }
    // 稳定预览运行的是进入前已采用的草稿；后台作者新修订等待返回再同步，不能误报断开或同步失败。
    if status.playing && runtime.is_none_or(|runtime| runtime.waiting_mode.is_none()) {
        return ConnectionState::Connected;
    }
    // Ready只证明握手完成；有地图时还须收到当前完整快照的采用回执，空工程不等待不存在的地图。
    if runtime.is_some_and(|runtime| runtime.snapshot_in_flight.is_some() || runtime.waiting_mode.is_some())
        || capture.is_some_and(|capture| capture.map_id.is_some() && (
            capture.pending || status.applied_revision == 0
                || capture.ready.as_ref().is_none_or(|ready| ready.author_revision != capture.revision
                    || status.applied_revision < ready.author_revision)
                || runtime.is_some_and(|runtime| runtime.map_id != capture.map_id)
        ))
    {
        return ConnectionState::Synchronizing;
    }
    ConnectionState::Connected
}

/// 更换构建配置只允许在无交接的编辑态进行；未确认的收口失败永远阻止第二份Job。
pub(crate) fn build_profile_blocker(world: &World) -> Option<String> {
    let runtime = world.get_non_send::<resources::Runtime>()?;
    if let Some(error) = &runtime.cleanup_failure { return Some(error.clone()); }
    if runtime.code_apply.is_some() {
        return Some("正在应用新代码，请等待草稿和原预览模式恢复后再切换Debug/Release".into());
    }
    if runtime.hotpatch.flight.is_some() {
        return Some("正在等待函数体热更新的实际结果，请确认结束后再切换Debug/Release".into());
    }
    if runtime.exit_requested || runtime.close_return_requested {
        return Some("请先完成或取消关闭工程，再切换Debug/Release".into());
    }
    if runtime.pending_play || runtime.pending_edit || runtime.waiting_mode.is_some()
        || world.get_resource::<EmbeddedStatus>().is_some_and(|status| status.playing)
    {
        return Some("请先返回编辑并完成模式交接，再切换Debug/Release".into());
    }
    if runtime.active.as_ref().is_some_and(|active| active.control.is_cancelled()) {
        return Some("正在确认当前运行进程和通信线程退出，请稍后切换Debug/Release".into());
    }
    None
}

/// 正常关闭先由原关闭事务授权才收口游戏；返回编辑超时的故障收口另由独立预算负责。
pub(crate) fn ready_to_exit(world: &mut World) -> Result<bool, String> {
    let Some(mut runtime) = world.get_non_send_mut::<resources::Runtime>() else {
        return Ok(true);
    };
    runtime.exit_requested = true;
    if let Some(active) = runtime.active.as_ref() {
        active.control.cancel();
        return Ok(false);
    }
    if let Some(error) = &runtime.cleanup_failure {
        return Err(error.clone());
    }
    Ok(true)
}

/// 关闭事务取消或保存失败后，解除退出意图；已经停止的会话可由F12明确重试。
pub(crate) fn cancel_exit(world: &mut World) {
    if let Some(mut runtime) = world.get_non_send_mut::<resources::Runtime>() {
        runtime.exit_requested = false;
        runtime.close_return_requested = false;
        runtime.close_return_started = None;
    }
}

/// 关闭确认层属于原作者壳体。先可靠返回编辑，再检查未保存内容；不能把确认框
/// 挂在仍隐藏的预览界面下面；健康游戏在取消关闭后继续保留，失去控制响应则有界收口。
pub(crate) fn return_to_edit_before_close(world: &mut World) -> bool {
    let playing = world
        .get_resource::<EmbeddedStatus>()
        .is_some_and(|status| status.playing);
    let Some(mut runtime) = world.get_non_send_mut::<resources::Runtime>() else {
        return true;
    };
    runtime.pending_play = false;
    let changing = runtime.waiting_mode.is_some();
    runtime.close_return_requested = playing || changing;
    if runtime.close_return_requested {
        runtime
            .close_return_started
            .get_or_insert_with(std::time::Instant::now);
    } else {
        runtime.close_return_started = None;
    }
    if playing
        || runtime
            .waiting_mode
            .is_some_and(|(_, mode)| mode == feathrise_editor_protocol::Mode::Playing)
    {
        runtime.pending_edit = true;
    }
    !runtime.close_return_requested
}

/// 仅自有验收驱动调用：后台用持有的Job句柄异常终止，禁止按全局进程名定位目标。
#[cfg(feature = "validation")]
pub(crate) fn terminate_for_validation(world: &World) -> Result<(), String> {
    let status = world.resource::<EmbeddedStatus>();
    if !status.ready || status.pid == 0 {
        return Err("没有已握手的本轮常驻工程实例".into());
    }
    let active = world
        .non_send::<resources::Runtime>()
        .active
        .as_ref()
        .ok_or("常驻工作槽不存在")?;
    active.control.request_crash();
    Ok(())
}

#[cfg(test)]
mod close_tests {
    use super::*;
    use feathrise_editor_protocol::Mode;

    #[test]
    fn stable_preview_keeps_connected_while_new_author_drafts_wait_for_editing() {
        let mut world = World::new();
        world.insert_resource(EmbeddedStatus { active: true, ready: true, playing: true, ..default() });
        world.init_resource::<feathrise_editor_map::standalone::embedded_snapshot::EmbeddedSnapshotState>();
        let mut capture = world.resource_mut::<feathrise_editor_map::standalone::embedded_snapshot::EmbeddedSnapshotState>();
        capture.map_id = Some("next-author-map".into());
        capture.pending = true;
        capture.error = Some("尚未完成的新作者草稿".into());
        assert_eq!(connection_state(&world), ConnectionState::Connected);
        world.resource_mut::<EmbeddedStatus>().playing = false;
        assert_eq!(connection_state(&world), ConnectionState::Failed);
        world.resource_mut::<feathrise_editor_map::standalone::embedded_snapshot::EmbeddedSnapshotState>().error = None;
        assert_eq!(connection_state(&world), ConnectionState::Synchronizing);
    }

    #[test]
    fn close_waits_for_real_editing_and_cancels_a_pending_play_intent() {
        let mut world = World::new();
        world.insert_resource(EmbeddedStatus {
            active: true,
            playing: true,
            ..default()
        });
        let mut runtime = resources::Runtime::default();
        runtime.pending_play = true;
        world.insert_non_send(runtime);
        assert!(!return_to_edit_before_close(&mut world));
        let runtime = world.non_send::<resources::Runtime>();
        assert!(runtime.pending_edit && runtime.close_return_requested);
        assert!(!runtime.pending_play && !runtime.exit_requested);
        assert!(
            world.resource::<EmbeddedStatus>().playing,
            "请求返回不能伪造模式回执"
        );
        world.non_send_mut::<resources::Runtime>().waiting_mode = Some((1, Mode::Editing));
        world.resource_mut::<EmbeddedStatus>().playing = false;
        assert!(
            !return_to_edit_before_close(&mut world),
            "仍在交接时确认层不可提前打开"
        );
        world.non_send_mut::<resources::Runtime>().waiting_mode = None;
        assert!(return_to_edit_before_close(&mut world));
        assert!(
            !world
                .non_send::<resources::Runtime>()
                .close_return_requested
        );

        world.non_send_mut::<resources::Runtime>().waiting_mode = Some((2, Mode::Playing));
        world.non_send_mut::<resources::Runtime>().pending_edit = false;
        assert!(!return_to_edit_before_close(&mut world));
        assert!(
            world.non_send::<resources::Runtime>().pending_edit,
            "正在进入预览时关闭也必须可靠转回编辑"
        );
    }
}
