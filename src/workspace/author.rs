//! 原作者实例接入同窗口工程页；进程内仍只绑定一个工程，所有专业作者系统保持原装配。
use super::types::{AuthorEvent, HostEvent};
use crate::{workspace_transport::Transport, workspace_ui::*};
use crate::native_project_close::{CloseTarget, NativeProjectCloseState};
use bevy::{
    ecs::message::MessageCursor,
    prelude::*,
    window::{PrimaryWindow, WindowCloseRequested},
};

struct Bridge {
    transport: Transport<HostEvent, AuthorEvent>,
    frames: u32,
    ready: bool,
    closing: bool,
    saw_confirmation: bool,
    candidate: bool,
}

pub(crate) fn install(app: &mut App, project: &crate::project::Project) -> Result<(), String> {
    if crate::workspace_window::EmbedTarget::from_env()?.is_none() {
        return Ok(());
    }
    // 内嵌作者默认只关闭当前工程；只有主窗口明确的退出请求才升级成退出编辑器。
    app.world_mut().resource_mut::<NativeProjectCloseState>().set_target(CloseTarget::Project);
    let token = std::env::var("FEATHRISE_WORKSPACE_TOKEN").map_err(|_| "内嵌作者缺少会话身份")?;
    let transport = Transport::new(std::io::stdin(), std::io::stdout(), token)?;
    let (preferences, error) = super::preferences()?;
    let mut model = super::model(&preferences, Some(project));
    if let Some(error) = error {
        model.status = error;
    }
    app.insert_resource(model)
        .add_plugins(WorkspaceUiPlugin)
        .add_plugins(crate::workspace_window::EmbeddedAuthorPlugin)
        .insert_non_send(Bridge {
            transport,
            frames: 0,
            ready: false,
            closing: false,
            saw_confirmation: false,
            candidate: true,
        })
        .add_systems(Update, receive.before(crate::native_play::request_exit))
        .add_systems(Last, send);
    #[cfg(feature = "validation")]
    if let Some(directory) = std::env::var_os("FEATHRISE_WORKSPACE_SMOKE_DIR") {
        let owned = std::path::PathBuf::from(directory)
            .canonicalize()
            .map_err(|error| format!("无法验证同窗口验收目录身份：{error}"))?;
        if !project.root.starts_with(owned.join("Projects")) {
            return Err("同窗口验收不得替用户已有工程确认关闭".into());
        }
        app.add_systems(
            Update,
            crate::native_project_close::confirm_workspace_validation_close
                .after(crate::native_project_close::ProjectCloseInput)
                .before(crate::native_play::request_exit),
        );
    }
    Ok(())
}

fn receive(world: &mut World) {
    for _ in 0..4 {
        let event = world.non_send::<Bridge>().transport.try_recv();
        let event = match event {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(error) => {
                world.resource_mut::<WorkspaceModel>().status =
                    format!("主窗口通信已断开：{error}");
                break;
            }
        };
        match event {
            HostEvent::Model(mut model) => {
                // 父方只能更新本实例的管理状态，绝不重定向不可变工程路径。
                let current = world
                    .resource::<WorkspaceModel>()
                    .project
                    .as_ref()
                    .map(|p| p.root.clone());
                if model.project.as_ref().map(|p| p.root.clone()) != current {
                    continue;
                }
                model.revision = world.resource::<WorkspaceModel>().revision.wrapping_add(1);
                world.insert_resource(model);
            }
            HostEvent::FillPath { field, value } => fill_path(world, field, value),
            event @ (HostEvent::RequestClose | HostEvent::RequestExitEditor) => {
                request_close(world, event.close_target().expect("关闭事件必须携带明确目标"));
            }
            HostEvent::CancelClose => crate::native_project_close::cancel_workspace_close(world),
            HostEvent::AbortCandidate => {
                if world.non_send::<Bridge>().candidate {
                    // 候选从未呈现/接收作者输入，无用户草稿；失败退役不触发旧工程保存。
                    world.write_message(AppExit::Success);
                }
            }
            HostEvent::Activate => {
                world.non_send_mut::<Bridge>().candidate = false;
                show_page(world, Page::Editor);
            }
        }
    }
}

fn request_close(world: &mut World, target: CloseTarget) {
    // 先撤回另一目标的旧choice/intent，再走原WindowCloseRequested和保存检查；不能借旧工程授权退出应用。
    world.resource_mut::<NativeProjectCloseState>().set_target(target);
    let mut bridge = world.non_send_mut::<Bridge>();
    bridge.closing = true;
    bridge.saw_confirmation = false;
    let mut windows = world.query_filtered::<Entity, With<PrimaryWindow>>();
    if let Ok(window) = windows.single(world) {
        world.write_message(WindowCloseRequested { window });
    }
}

fn send(world: &mut World, mut cursor: Local<MessageCursor<WorkspaceCommand>>) {
    let commands: Vec<_> = cursor
        .read(world.resource::<Messages<WorkspaceCommand>>())
        .take(4)
        .cloned()
        .collect();
    for command in commands {
        if matches!(command, WorkspaceCommand::SetStartupMap) {
            crate::native_project_menu::save_startup_from_workspace(world);
            continue;
        }
        if matches!(command, WorkspaceCommand::ExportProject) {
            world.write_message(crate::project_export::messages::OpenExportDialog);
            continue;
        }
        if matches!(command, WorkspaceCommand::OpenPlugins) {
            // 插件作者会话属于当前工程实例；菜单意图在作者侧消费，不让父宿主另开一套管理器。
            crate::native_plugins::management::open_from_workspace(world);
            continue;
        }
        if let Err(error) = world
            .non_send::<Bridge>()
            .transport
            .try_send(AuthorEvent::Command(command))
        {
            world.resource_mut::<WorkspaceModel>().status = error;
        }
    }
    let status = world.get_resource::<crate::workspace_window::EmbeddedAuthorStatus>();
    let ready = status.is_some_and(|status| status.ready);
    #[cfg(windows)]
    let hwnd = status.and_then(|status| status.hwnd).unwrap_or_default();
    let error = status.and_then(|status| status.error.clone());
    let close = world.resource::<NativeProjectCloseState>();
    let confirmation = close.active();
    let close_target = close.target();
    let mut bridge = world.non_send_mut::<Bridge>();
    bridge.frames = bridge.frames.saturating_add(1);
    if let Some(error) = error {
        let _ = bridge.transport.try_send(AuthorEvent::Failed(error));
    } else if ready && bridge.frames > 3 && !bridge.ready {
        #[cfg(windows)]
        let event = AuthorEvent::Ready { hwnd };
        #[cfg(unix)]
        let event = AuthorEvent::ReadySurface { session: std::env::var("FEATHRISE_WORKSPACE_TOKEN").unwrap_or_default() };
        if bridge
            .transport
            .try_send(event)
            .is_ok()
        {
            bridge.ready = true;
        }
    }
    if bridge.closing {
        bridge.saw_confirmation |= confirmation;
        if bridge.saw_confirmation && !confirmation {
            if bridge
                .transport
                .try_send(AuthorEvent::CloseCancelledFor { target: close_target })
                .is_ok()
            {
                bridge.closing = false;
                bridge.saw_confirmation = false;
            }
        }
    }
}

#[cfg(test)]
mod close_target_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn host_target_is_adopted_before_original_window_close_and_upgrade_restarts_confirmation() {
        let mut world = World::new();
        world.init_resource::<NativeProjectCloseState>();
        world.init_resource::<Messages<WindowCloseRequested>>();
        world.init_resource::<Messages<WorkspaceCommand>>();
        world.init_resource::<Messages<feathrise_editor_map::standalone::SavedPlayFeedback>>();
        let window = world.spawn((Window::default(), PrimaryWindow)).id();
        world.insert_non_send(Bridge {
            transport: Transport::new(std::io::empty(), std::io::sink(), "close-target-fixture".into()).unwrap(),
            frames: 0, ready: false, closing: false, saw_confirmation: false, candidate: false,
        });

        request_close(&mut world, CloseTarget::Project);
        assert_eq!(world.resource::<NativeProjectCloseState>().target(), CloseTarget::Project);
        assert!(!crate::native_project_close::authorize_exit(&mut world, true));
        world.run_system_once(send).unwrap();
        assert!(world.non_send::<Bridge>().saw_confirmation,
            "确认观察来自原关闭状态，不直接写已确认标志");

        request_close(&mut world, CloseTarget::Editor);
        assert_eq!(world.resource::<NativeProjectCloseState>().target(), CloseTarget::Editor);
        assert!(world.non_send::<Bridge>().closing);
        assert!(!world.non_send::<Bridge>().saw_confirmation,
            "升级目标必须等待新确认，不沿用旧Project的观察回执");
        assert!(!crate::native_project_close::authorize_exit(&mut world, false));
        let mut cursor = MessageCursor::<WindowCloseRequested>::default();
        let requests = cursor.read(world.resource::<Messages<WindowCloseRequested>>()).map(|request| request.window).collect::<Vec<_>>();
        assert_eq!(requests, [window, window], "两种目标都必须经过原窗口关闭事务");
    }
}
