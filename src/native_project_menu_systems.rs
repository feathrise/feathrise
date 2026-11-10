//! 工程菜单输入、单槽任务轮询与原UI回显；F12、确认和取消不穿透到后方作者动作。

use super::{components::*, resources::*};
use bevy::{
    ecs::message::MessageCursor,
    input_focus::InputFocus,
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
    ui::{InteractionDisabled, RelativeCursorPosition},
    window::{PrimaryWindow, WindowCloseRequested},
};
use feathrise_editor_core::{EditorHostOverlay, EditorTextInputGuard};
use feathrise_editor_scene::standalone::SavedPlayFeedback;
use feathrise_scene_content::ActiveDynamicScene;

pub(super) fn handle(
    buttons: Query<
        (&ProjectAction, &feathrise_editor_ui::EditorButtonClick),
        Changed<feathrise_editor_ui::EditorButtonClick>,
    >,
    panels: Query<&RelativeCursorPosition, With<ProjectMenuPanel>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut text_guard: ResMut<EditorTextInputGuard>,
    mut overlay: ResMut<EditorHostOverlay>,
    mut focus: ResMut<InputFocus>,
    settings: Res<ProjectMenuSettings>,
    active: Res<ActiveDynamicScene>,
    mut state: ResMut<NativeProjectMenuState>,
    mut feedback: (MessageWriter<SavedPlayFeedback>, MessageWriter<crate::project_export::messages::OpenExportDialog>),
    windows: Query<Entity, With<PrimaryWindow>>,
    mut close: ResMut<Messages<WindowCloseRequested>>,
    mut close_cursor: Local<MessageCursor<WindowCloseRequested>>,
    project_close: Res<crate::native_project_close::NativeProjectCloseState>,
    overlays: (
        Option<Res<crate::project_log::ProjectLogState>>,
        Option<Res<crate::viewport_mode::BuildProfileMenuState>>,
        Option<Res<crate::window_chrome::ChromeState>>,
        Option<ResMut<crate::native_plugins::management::PluginManagerState>>,
        Option<Res<crate::project_export::ui::ExportOverlayState>>,
        Option<Res<crate::workspace_ui::AboutState>>,
    ),
) {
    let (logs, profiles, chrome, mut plugins, export, about) = overlays;
    let was_open = state.open;
    if state.pending_close
        && (mouse.get_just_pressed().next().is_some() || keys.get_just_pressed().next().is_some())
    {
        state.pending_close = false; // 作者开始另一个明确操作时撤销尚未发出的返回意图。
    }
    let action = buttons
        .iter()
        .find_map(|(action, click)| click.is_clicked().then_some(*action))
        .filter(|_| !project_close.captures_input()
            && !plugins.as_ref().is_some_and(|plugins| plugins.captures_input())
            && !export.as_ref().is_some_and(|export| export.captures_input())
            && !about.as_ref().is_some_and(|about| about.captures_input()));
    let mut captured = was_open || project_close.captures_input();
    if project_close.captures_input() {
        state.open = false;
        state.pending_close = false;
    }
    if plugins.as_ref().is_some_and(|plugins| plugins.captures_input())
        || export.as_ref().is_some_and(|export| export.captures_input())
        || about.as_ref().is_some_and(|about| about.captures_input()) { state.open = false; }
    if take_menu_close(
        was_open && !project_close.captures_input(),
        windows.single().ok(),
        &mut close_cursor,
        &mut close,
    ) {
        // 系统X先释放菜单的原输入门，再只发一次正式关闭请求；脏位仍由原退出事务判断。
        state.open = false;
        state.pending_close = true;
    } else {
        match action {
            Some(ProjectAction::Toggle) => {
                state.open = !state.open;
                captured = true;
                if state.open {
                    focus.clear();
                }
            }
            Some(ProjectAction::SetStartup) if state.open && !state.saving() => {
                if let Some(id) = active.scene_id.as_ref() {
                    let project = settings.project.clone();
                    let id = id.0.clone(); // 冻结稳定ID；后台完成后不读取作者已经切到的另一个场景。
                    state.failed = None;
                    state.status = "正在后台校验当前场景并保存启动设置…".into();
                    state.task = Some(
                        IoTaskPool::get().spawn(async move { super::store::save(&project, &id) }),
                    );
                    feedback.0.write(SavedPlayFeedback(state.status.clone()));
                } else {
                    state.status = "请先进入一个场景，再将它设为启动场景".into();
                }
            }
            Some(ProjectAction::BackToManager) if state.open && !state.saving() => {
                state.open = false;
                state.pending_close = true; // 关闭点击帧仍由菜单拥有，下一静默帧再交给原退出门。
            }
            Some(ProjectAction::Export) if state.open && !state.saving() => {
                state.open = false;
                feedback.1.write(crate::project_export::messages::OpenExportDialog);
            }
            Some(ProjectAction::Plugins) if state.open && !state.saving() && plugins.is_some() => {
                state.open = false;
                let plugins = plugins.as_mut().expect("分支已检查管理器存在");
                plugins.open = true;
                plugins.captured = true; // 菜单释放帧直接转交同一输入门，背景作者不能先收到点击。
                focus.clear();
            }
            _ => {}
        }
    }
    if was_open
        && (keys.just_pressed(KeyCode::Escape)
            || mouse.get_just_pressed().next().is_some()
                && !panels.iter().any(RelativeCursorPosition::cursor_over))
    {
        state.open = false;
    }
    captured |= state.open || profiles.is_some_and(|profiles| profiles.captures_input())
        || chrome.is_some_and(|chrome| chrome.captures_input())
        || plugins.as_ref().is_some_and(|plugins| plugins.captures_input())
        || export.is_some_and(|export| export.captures_input())
        || about.is_some_and(|about| about.captures_input());
    if overlay.active != captured {
        overlay.active = captured;
    }
    // 日志正文获焦时只认领键盘，工程子视口与浮窗外的鼠标仍保持可用。
    if captured || logs.is_some_and(|logs| logs.owns_keyboard()) {
        claim_keyboard(&mut keys, &mut text_guard);
    }
}

fn take_menu_close(
    menu_open: bool,
    primary: Option<Entity>,
    cursor: &mut MessageCursor<WindowCloseRequested>,
    messages: &mut Messages<WindowCloseRequested>,
) -> bool {
    let Some(primary) = primary.filter(|_| menu_open) else {
        // 关闭态也必须推进游标；不能在稍后打开菜单时重新接管已由退出门处理的旧X。
        cursor.clear(messages);
        return false;
    };
    let mut deferred = false;
    let mut other_windows = Vec::new();
    for request in cursor.read(messages) {
        if request.window == primary {
            deferred = true;
        } else {
            other_windows.push(request.clone());
        }
    }
    if deferred {
        // Messages没有按ID删除接口。只保留本次未接管的新请求，绝不重放缓冲区里的旧事件。
        // handle排在原退出门之前，因此其它窗口本帧的新X仍会正常交给后方读者。
        messages.clear();
        messages.write_batch(other_windows);
        cursor.clear(messages);
    }
    deferred
}

pub(super) fn emit_close(
    mut state: ResMut<NativeProjectMenuState>,
    overlay: Res<EditorHostOverlay>,
    text: Res<EditorTextInputGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut close: MessageWriter<WindowCloseRequested>,
) {
    if !state.pending_close
        || overlay.active
        || text.block_shortcuts
        || mouse.get_pressed().next().is_some()
        || mouse.get_just_pressed().next().is_some()
        || mouse.get_just_released().next().is_some()
        || keys.get_just_pressed().next().is_some()
        || keys.get_just_released().next().is_some()
    {
        return;
    }
    if let Ok(window) = windows.single() {
        state.pending_close = false;
        // 原鼠标门已在Input阶段经过静默释放；消息只发一次，脏位拒绝后不会自动挂起／重试退出。
        close.write(WindowCloseRequested { window });
    }
}

fn claim_keyboard(keys: &mut ButtonInput<KeyCode>, guard: &mut EditorTextInputGuard) {
    // 沿用原加载／帮助菜单的按键边沿消费方式，不改真实按住状态或建立第二套输入系统。
    guard.block_shortcuts = true;
    let edges: Vec<_> = keys
        .get_just_pressed()
        .chain(keys.get_just_released())
        .copied()
        .collect();
    for key in edges {
        keys.clear_just_pressed(key);
        keys.clear_just_released(key);
    }
}

pub(super) fn poll(
    mut state: ResMut<NativeProjectMenuState>,
    mut feedback: MessageWriter<SavedPlayFeedback>,
) {
    let Some(result) = state.task.as_mut().and_then(check_ready) else {
        return;
    };
    state.task = None;
    state.status = match result {
        Ok(saved) => format!(
            "{}：{}（{}）。场景草稿仍按原保存流程处理。",
            if saved.changed {
                "启动设置已保存"
            } else {
                "已是当前启动场景"
            },
            saved.name,
            saved.id
        ),
        Err(error) => {
            state.report_save_failure(error);
            state.status.clone()
        }
    };
    feedback.write(SavedPlayFeedback(state.status.clone()));
}

pub(super) fn present(
    mut commands: Commands,
    state: Res<NativeProjectMenuState>,
    active: Res<ActiveDynamicScene>,
    mut overlays: Query<&mut Node, With<ProjectMenuOverlay>>,
    mut statuses: Query<&mut Text, With<ProjectMenuStatus>>,
    buttons: Query<(Entity, &ProjectAction, Has<InteractionDisabled>)>,
) {
    if !state.is_changed() && !active.is_changed() {
        return;
    }
    for mut node in &mut overlays {
        node.display = if state.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut text in &mut statuses {
        if text.0 != state.status {
            text.0.clone_from(&state.status);
        }
    }
    for (entity, action, disabled) in &buttons {
        let next = match action {
            ProjectAction::Toggle => false,
            ProjectAction::SetStartup => state.saving() || active.scene_id.is_none(),
            ProjectAction::Export => state.saving(),
            ProjectAction::BackToManager => state.saving(),
            ProjectAction::Plugins => state.saving(),
        };
        if disabled != next {
            if next {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_runs_without_plugin_manager_and_transfers_capture_when_one_is_installed() {
        // 本例只执行业务输入，不创建工程目录、不加载插件，也不启动后台保存。
        let root = std::path::PathBuf::from("unused-menu-input-fixture");
        let mut app = App::new();
        app.insert_resource(ProjectMenuSettings {
            project: crate::project::Project {
                id: "menu-input".into(), name: "菜单输入夹具".into(), manifest: root.join("Cargo.toml"),
                package: "menu-input".into(), root: root.clone(),
                context: crate::project::ProjectContext { root: root.clone(), id: "menu-input".into(),
                    name: "菜单输入夹具".into(), format: 1, asset_roots: vec![root.join("assets")],
                    editor_data: root.join(".editor"), storage_key: "menu-input".into(), startup_scene: None },
            }, managed: false,
        }).init_resource::<ButtonInput<MouseButton>>().init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EditorHostOverlay>().init_resource::<EditorTextInputGuard>()
            .init_resource::<InputFocus>().init_resource::<ActiveDynamicScene>()
            .init_resource::<NativeProjectMenuState>()
            .init_resource::<crate::native_project_close::NativeProjectCloseState>()
            .add_message::<WindowCloseRequested>().add_message::<SavedPlayFeedback>()
            .add_message::<crate::project_export::messages::OpenExportDialog>()
            .add_systems(Update, handle);
        app.update();
        let toggle = app.world_mut().spawn((ProjectAction::Toggle, feathrise_editor_ui::EditorButtonClick::triggered())).id();
        app.update(); app.world_mut().despawn(toggle);
        assert!(app.world().resource::<NativeProjectMenuState>().open);
        assert!(app.world().resource::<EditorHostOverlay>().active);
        let unavailable = app.world_mut().spawn((ProjectAction::Plugins, feathrise_editor_ui::EditorButtonClick::triggered())).id();
        app.update(); app.world_mut().despawn(unavailable);
        assert!(app.world().resource::<NativeProjectMenuState>().open, "缺少插件管理器时不关闭原菜单或访问缺失资源");

        app.insert_resource(crate::native_plugins::management::PluginManagerState::new(root));
        app.world_mut().spawn((ProjectAction::Plugins, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.update();
        assert!(!app.world().resource::<NativeProjectMenuState>().open);
        assert!(app.world().resource::<crate::native_plugins::management::PluginManagerState>().captures_input());
        assert!(app.world().resource::<EditorHostOverlay>().active, "菜单到管理器的释放点击不能穿透背景作者");
    }

    #[test]
    fn system_close_is_deferred_only_for_open_menu_without_replaying_old_requests() {
        let mut world = World::new();
        let primary = world.spawn_empty().id();
        let other = world.spawn_empty().id();
        let mut messages = Messages::default();
        let mut menu = MessageCursor::default();
        let mut exit = MessageCursor::default();

        messages.write(WindowCloseRequested { window: primary });
        assert!(!take_menu_close(
            false,
            Some(primary),
            &mut menu,
            &mut messages
        ));
        assert_eq!(exit.read(&messages).count(), 1); // 关闭态的系统X照常透传。
        assert!(!take_menu_close(
            true,
            Some(primary),
            &mut menu,
            &mut messages
        ));

        messages.write(WindowCloseRequested { window: primary });
        messages.write(WindowCloseRequested { window: other });
        assert!(take_menu_close(
            true,
            Some(primary),
            &mut menu,
            &mut messages
        ));
        let passed: Vec<_> = exit.read(&messages).map(|request| request.window).collect();
        assert_eq!(passed, [other]); // 本次只延后主窗口X，不吞其它窗口的关闭请求。
        assert!(!take_menu_close(
            true,
            Some(primary),
            &mut menu,
            &mut messages
        ));
        assert_eq!(exit.read(&messages).count(), 0);

        messages.write(WindowCloseRequested { window: primary });
        assert!(!take_menu_close(
            false,
            Some(primary),
            &mut menu,
            &mut messages
        ));
        assert_eq!(exit.read(&messages).count(), 1);
        assert!(!take_menu_close(
            true,
            Some(primary),
            &mut menu,
            &mut messages
        ));
    }

    #[test]
    fn menu_consumes_confirm_cancel_and_play_edges_without_sticking_held_keys() {
        let mut keys = ButtonInput::default();
        for key in [
            KeyCode::Enter,
            KeyCode::NumpadEnter,
            KeyCode::Escape,
            KeyCode::F12,
        ] {
            keys.press(key);
        }
        let mut guard = EditorTextInputGuard::default();
        claim_keyboard(&mut keys, &mut guard);
        assert!(guard.block_shortcuts);
        for key in [
            KeyCode::Enter,
            KeyCode::NumpadEnter,
            KeyCode::Escape,
            KeyCode::F12,
        ] {
            assert!(!keys.just_pressed(key));
            assert!(keys.pressed(key));
            keys.release(key);
            assert!(!keys.pressed(key));
        }
    }

    #[test]
    fn close_request_waits_for_menu_release_and_is_only_sent_once() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(EditorHostOverlay { active: true })
            .insert_resource(EditorTextInputGuard {
                block_shortcuts: true,
            })
            .insert_resource(NativeProjectMenuState {
                pending_close: true,
                ..default()
            })
            .add_message::<WindowCloseRequested>()
            .add_systems(Update, emit_close);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let mut cursor = bevy::ecs::message::MessageCursor::<WindowCloseRequested>::default();
        app.update();
        assert_eq!(
            cursor
                .read(app.world().resource::<Messages<WindowCloseRequested>>())
                .count(),
            0
        );
        app.world_mut().resource_mut::<EditorHostOverlay>().active = false;
        app.world_mut()
            .resource_mut::<EditorTextInputGuard>()
            .block_shortcuts = false;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        assert_eq!(
            cursor
                .read(app.world().resource::<Messages<WindowCloseRequested>>())
                .count(),
            0
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.update();
        assert_eq!(
            cursor
                .read(app.world().resource::<Messages<WindowCloseRequested>>())
                .count(),
            1
        );
        app.update();
        assert_eq!(
            cursor
                .read(app.world().resource::<Messages<WindowCloseRequested>>())
                .count(),
            0
        );
        assert!(
            !app.world()
                .resource::<NativeProjectMenuState>()
                .pending_close
        );
    }
}
