//! 无工程退出门的原事务回归：确认前不退出、不取消准备，明确同意后仍等待拥有槽收口。

use feathrise_editor_ui::InteractionButton;
use super::*;
use bevy::{asset::AssetApp, input::mouse::MouseWheel, input_focus::InputFocus};
use crate::{build_process::BuildControl, user_preferences::Preferences};
use feathrise_editor_map::workbench::WorkbenchHeader;
use std::sync::mpsc;

fn fixture() -> (App, Host) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin {
        file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
        watch_for_changes_override: Some(false), ..default()
    }, bevy::scene::ScenePlugin))
        .init_asset::<Font>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputFocus>()
        .register_required_components::<InteractionButton, feathrise_editor_ui::EditorButtonClick>()
        .add_message::<MouseWheel>()
        .add_message::<AppExit>()
        .add_plugins(NativeProjectClosePlugin);
    app.world_mut().spawn(WorkbenchHeader);
    app.update();
    let home = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
        .join("target/host-empty-exit-unit");
    let host = Host {
        target_override: Some(home.join("target")), preferences: Preferences::default_for_home(&home).unwrap(),
        preferences_job: None, job: None, prepared: None, picker: None, current: None, candidate: None, retiring: None,
        want_close: false, close_sent: false, exit_app: false, notice: String::new(), changed: true,
        initial: Some(home.join("pending-project")),
    };
    (app, host)
}

fn choose(app: &mut App, label: &str) {
    let world = app.world_mut();
    let button = world.query_filtered::<(Entity, &Children), With<InteractionButton>>().iter(world)
        .find_map(|(entity, children)| children.iter().any(|child|
            world.get::<Text>(child).is_some_and(|text| text.as_str() == label)).then_some(entity))
        .expect("原确认按钮必须存在");
    assert_ne!(world.get::<Node>(button).unwrap().display, Display::None);
    assert!(world.get::<bevy::ui::InteractionDisabled>(button).is_none());
    world.entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::default());
}

/// 仅等待测试自身的释放信号，不启动工具、工程或文件写入；Drop也会释放，失败不留后台工作。
struct OwnedPreparation {
    release: Option<mpsc::SyncSender<()>>,
    control: BuildControl,
}

impl OwnedPreparation {
    fn install(host: &mut Host) -> Self {
        let (release, wait) = mpsc::sync_channel(1);
        let (_, result) = mpsc::channel();
        let control = BuildControl::default();
        host.job = Some(OpenJob {
            worker: std::thread::spawn(move || { let _ = wait.recv(); }), result, control: control.clone(),
        });
        Self { release: Some(release), control }
    }

    fn finish(&mut self, host: &mut Host) {
        self.release.take().unwrap().send(()).unwrap();
        host.job.take().unwrap().worker.join().unwrap();
    }
}

impl Drop for OwnedPreparation {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() { let _ = release.send(()); }
    }
}

#[test]
fn window_and_quit_menu_wait_for_the_same_explicit_empty_workspace_choice() {
    for menu in [false, true] {
        let (mut app, mut host) = fixture();
        let initial = host.initial.clone();
        if menu { handle(&mut host, app.world_mut(), 0, None, WorkspaceCommand::QuitApp); }
        else { request_app_exit(&mut host, app.world_mut()); }
        for _ in 0..6 {
            app.update();
            poll_empty_exit(&mut host, app.world_mut());
            finish_app_exit(&mut host, app.world_mut());
            assert!(app.world().resource::<NativeProjectCloseState>().active());
            assert_eq!(app.world().resource::<NativeProjectCloseState>().target(), CloseTarget::Editor);
            assert!(!host.exit_app);
            assert!(app.world().resource::<Messages<AppExit>>().is_empty());
            assert!(empty_exit_blocks_work(&host, app.world()));
            assert!(take_initial(&mut host, app.world()).is_none());
            assert_eq!(host.initial, initial);
        }
        // 重复X/菜单不产生默认同意；旧队列的打开操作也不能在模态之后启动准备。
        request_app_exit(&mut host, app.world_mut());
        handle(&mut host, app.world_mut(), 0, None, WorkspaceCommand::Open(initial.clone().unwrap()));
        assert!(host.job.is_none());
        assert!(!host.exit_app);
    }
}

#[test]
fn escape_and_cancel_preserve_pending_work_and_keep_the_release_frame_blocked() {
    for escape in [false, true] {
        let (mut app, mut host) = fixture();
        let mut preparation = OwnedPreparation::install(&mut host);
        request_app_exit(&mut host, app.world_mut());
        app.update();
        assert!(!preparation.control.is_cancelled());
        if escape {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::Escape);
            keys.clear();
        } else { choose(&mut app, "取消 (Esc)"); }
        poll_empty_exit(&mut host, app.world_mut());
        finish_app_exit(&mut host, app.world_mut());
        assert!(!app.world().resource::<NativeProjectCloseState>().active());
        assert!(!host.exit_app && host.job.is_some());
        assert!(!preparation.control.is_cancelled(), "取消退出不能提前取消原准备工作");
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        assert!(empty_exit_blocks_work(&host, app.world()), "取消当帧仍属于原模态输入门");
        assert!(take_initial(&mut host, app.world()).is_none());
        app.update();
        assert!(empty_exit_blocks_work(&host, app.world()), "鼠标/键盘释放帧不能交给初始工程或候选采用");
        app.update();
        assert!(!empty_exit_blocks_work(&host, app.world()));
        assert!(take_initial(&mut host, app.world()).is_some());
        preparation.finish(&mut host);
    }
}

#[test]
fn explicit_empty_exit_cancels_owned_preparation_then_writes_app_exit_after_drain() {
    let (mut app, mut host) = fixture();
    let mut preparation = OwnedPreparation::install(&mut host);
    request_app_exit(&mut host, app.world_mut());
    app.update();
    choose(&mut app, "退出编辑器");
    poll_empty_exit(&mut host, app.world_mut());
    assert!(host.exit_app && preparation.control.is_cancelled());
    assert!(empty_exit_blocks_work(&host, app.world()), "准备完成/候选就绪也不能在退出中采用");
    finish_app_exit(&mut host, app.world_mut());
    assert!(app.world().resource::<Messages<AppExit>>().is_empty(), "原准备槽未收口不能发布退出");
    preparation.finish(&mut host);
    finish_app_exit(&mut host, app.world_mut());
    assert!(!app.world().resource::<Messages<AppExit>>().is_empty(), "真实退出只由最终生产门发布");
}

#[test]
fn empty_workspace_plugin_command_opens_global_manager_without_starting_a_project() {
    use crate::native_plugins::management::{PluginManagerState, types::PluginScope};
    let (mut app, mut host) = fixture();
    let initial = host.initial.clone();
    app.insert_resource(PluginManagerState::with_roots(None, host.target_override.as_ref().unwrap().join("owned-global")));
    handle(&mut host, app.world_mut(), 0, None, WorkspaceCommand::OpenPlugins);
    let manager = app.world().resource::<PluginManagerState>();
    assert!(manager.open && !manager.has_project());
    assert_eq!(manager.scope, PluginScope::Global);
    assert_eq!(host.initial, initial);
    assert!(host.current.is_none() && host.job.is_none() && !host.exit_app);
}

#[test]
fn authorized_empty_exit_still_waits_for_the_global_plugin_writer() {
    use crate::native_plugins::management::PluginManagerState;
    let (mut app, mut host) = fixture();
    let mut manager = PluginManagerState::with_roots(None, host.target_override.as_ref().unwrap().join("owned-global"));
    manager.busy = true;
    app.insert_resource(manager);
    host.exit_app = true;
    finish_app_exit(&mut host, app.world_mut());
    assert!(app.world().resource::<Messages<AppExit>>().is_empty());
    app.world_mut().resource_mut::<PluginManagerState>().busy = false;
    finish_app_exit(&mut host, app.world_mut());
    assert!(!app.world().resource::<Messages<AppExit>>().is_empty());
}
