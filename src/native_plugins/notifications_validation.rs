//! 使用真实失败发现、官方鼠标及实际GPU截图验证右下角通知，不手写加载成功或错误回执。
use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::native_plugins::{
    notifications::{self, NotificationAction, NotificationState, PluginErrorNotification},
    resources::NativeTools,
    systems as native_systems,
};
use feathrise_editor_core::{AssetPreviewFocus, WorkspaceDomainState};
use feathrise_editor_scene::game_authoring::scene_api::{
    resources::SceneEditorState,
    ui::{sidebars::SceneEditorSidebarState, vegetation_preset_editor::VegetationPresetEditorState},
};

pub(super) fn install(app: &mut App) {
    notifications::install(app);
    app.init_resource::<SceneEditorState>()
        .init_resource::<WorkspaceDomainState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<bevy::clipboard::Clipboard>()
        .init_resource::<SceneEditorSidebarState>()
        .init_resource::<AssetPreviewFocus>();
}

fn discover_failure(app: &mut App, root: &Path, global: &Path) -> Vec<String> {
    let project = crate::project::open(root).unwrap();
    app.insert_resource(NativeTools::start_with_roots(
        &project,
        PluginRoots {
            project: Some(root.to_owned()),
            global: global.to_owned(),
        },
    ));
    let started = Instant::now();
    loop {
        app.world_mut()
            .run_system_cached(native_systems::poll)
            .unwrap();
        frame(app);
        let native = app.world().resource::<NativeTools>();
        if !native.discovery_pending && native.loading == 0 {
            assert!(
                native.tools.is_empty(),
                "通知夹具阶段不能加载有效DLL并锁住后续文件替换"
            );
            assert!(
                !native.errors.is_empty(),
                "本轮坏文件必须经过生产发现并产生实际错误"
            );
            let errors = native.errors.clone();
            settle(app);
            return errors;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "通知的真实发现失败未返回"
        );
    }
}

fn below(world: &World, mut child: Entity, root: Entity) -> bool {
    for _ in 0..16 {
        if child == root {
            return true;
        }
        let Some(parent) = world.get::<ChildOf>(child) else {
            return false;
        };
        child = parent.parent();
    }
    false
}

fn click_notification(app: &mut App, wanted: NotificationAction) {
    settle(app);
    let world = app.world_mut();
    let target = world
        .query::<(
            Entity,
            &NotificationAction,
            &ComputedNode,
            &InheritedVisibility,
        )>()
        .iter(world)
        .find_map(|(entity, action, node, visible)| {
            (std::mem::discriminant(action) == std::mem::discriminant(&wanted)
                && !node.is_empty()
                && visible.get())
            .then_some(entity)
        })
        .expect("右下角通知必须存在实际可见的原操作按钮");
    let point = bounds(app, target).center();
    app.world_mut().resource_mut::<CursorFixture>().position = Some(point);
    settle(app);
    assert_eq!(
        *app.world().get::<UiInteraction>(target).unwrap(),
        UiInteraction::Hovered
    );
    let window = app.world().resource::<CursorFixture>().window;
    window_input(app.world_mut(), MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    frame(app);
    window_input(app.world_mut(), MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    frame(app);
    app.world_mut().resource_mut::<CursorFixture>().position = None;
    wait_for_idle(app);
}

fn capture_notification(app: &mut App, target: &Handle<Image>, output: &Path, name: &str) -> Value {
    settle(app);
    assert!(app.world().resource::<NotificationState>().visible);
    let world = app.world_mut();
    let card = world
        .query_filtered::<Entity, With<PluginErrorNotification>>()
        .single(world)
        .unwrap();
    let rect = bounds(app, card);
    assert!(
        rect.center().x > 640.
            && rect.center().y > 360.
            && rect.max.x <= 1281.
            && rect.max.y <= 721.,
        "错误通知必须在右下角且保持在实际窗口内：{rect:?}"
    );
    let world = app.world_mut();
    let texts: Vec<_> = world
        .query::<(
            Entity,
            &Text,
            &ComputedNode,
            &InheritedVisibility,
            &bevy::text::TextLayoutInfo,
        )>()
        .iter(world)
        .filter(|(entity, _, node, visible, _)| {
            !node.is_empty() && visible.get() && below(world, *entity, card)
        })
        .map(|(_, text, _, _, layout)| {
            assert!(!layout.glyphs.is_empty(), "通知必须有真实文字glyph");
            text.0.clone()
        })
        .collect();
    assert!(
        texts.iter().any(|text| text.contains("插件")),
        "通知必须显示明确的插件错误文本"
    );
    assert!(
        texts.iter().any(|text| text.contains("查看插件")),
        "通知必须有查看插件入口"
    );
    let pixels = screenshot(app, target, &output.join(name));
    assert!(
        pixels.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]),
        "通知截图不能是空帧"
    );
    json!({"image":name,"card":[rect.min.x,rect.min.y,rect.max.x,rect.max.y],"visible_text":texts,
        "source":"actual RenderApp screenshot after production NativeTools failed discovery"})
}

pub(super) fn verify(
    app: &mut App,
    target: &Handle<Image>,
    root: &Path,
    global: &Path,
    output: &Path,
) -> Value {
    // 当前夹具只有真实坏二进制，尚未安装后续协议探针，因此不会在本进程锁定可替换DLL。
    let errors = discover_failure(app, root, global);
    click(app, |action| matches!(action, ManagerAction::Close));
    let first = capture_notification(app, target, output, "plugin-error-notification.png");
    click_notification(app, NotificationAction::Dismiss);
    assert!(!app.world().resource::<NotificationState>().visible);
    app.world_mut().resource_mut::<NativeTools>().ui_revision += 1;
    settle(app);
    assert!(
        !app.world().resource::<NotificationState>().visible,
        "普通快照修订不能重新弹出已经关闭的相同错误"
    );

    write_new(
        &root.join("plugins/notification-new-error.dll"),
        b"second-actual-invalid-native-library",
    );
    let new_errors = discover_failure(app, root, global);
    assert!(
        new_errors.len() > errors.len(),
        "新坏包必须产生一个新的真实发现错误"
    );
    assert!(
        app.world().resource::<NotificationState>().visible,
        "新错误必须重新提示"
    );
    let second = capture_notification(app, target, output, "plugin-error-new-notification.png");
    click_notification(app, NotificationAction::Inspect);
    assert!(
        app.world().resource::<PluginManagerState>().open,
        "查看插件必须打开原管理器"
    );
    let manager = app.world().resource::<PluginManagerState>();
    let selected = manager
        .selected
        .as_ref()
        .and_then(|id| manager.rows.iter().find(|row| &row.id == id));
    assert!(
        selected.is_some_and(|row| row.error.is_some()),
        "查看插件必须定位一个实际错误包，不能打开空详情"
    );
    click(app, |action| matches!(action, ManagerAction::Close));
    if app.world().resource::<NotificationState>().visible {
        click_notification(app, NotificationAction::Dismiss);
    }
    app.world_mut().remove_resource::<NativeTools>();
    super::super::open_from_workspace(app.world_mut());
    wait_for_idle(app);
    json!({"images":[first,second],"actual_initial_errors":errors,"actual_new_errors":new_errors,
        "dismissed":true,"same_error_did_not_reopen":true,"new_error_reopened":true,"inspect_opened_existing_manager_and_selected_error":true,
        "physical_os_input_verified":false})
}

/// 保留真实发现时的完整诊断和来源，用于证明归档仅撤销指定旧包的失败。
pub(super) struct LegacyFailure {
    id: String,
    original_errors: Vec<String>,
    retained_errors: Vec<String>,
    origin: PluginOrigin,
    diagnostics: (u64, u64),
    installed_adapter: bool,
}

pub(super) fn begin_legacy_failure(
    app: &mut App,
    root: &Path,
    global: &Path,
    id: &str,
) -> LegacyFailure {
    let errors = discover_failure(app, root, global);
    let prefix = format!("{id}：");
    let legacy_errors = errors
        .iter()
        .filter(|error| error.starts_with(&prefix))
        .count();
    assert_eq!(
        legacy_errors, 1,
        "必须取得唯一旧包的真实失败，不注入错误Vec"
    );
    let retained_errors: Vec<_> = errors
        .iter()
        .filter(|error| !error.starts_with(&prefix))
        .cloned()
        .collect();
    assert_eq!(
        retained_errors.len(),
        2,
        "两份真实坏DLL应同时保留在发现结果中"
    );
    let native = app.world().resource::<NativeTools>();
    assert!(
        native
            .load_states
            .get(id)
            .is_some_and(|state| state.starts_with("加载失败："))
    );
    let origin = native
        .load_origins
        .get(id)
        .expect("旧包失败必须绑定实际目录和清单摘要")
        .clone();
    assert_eq!(origin.scope, PluginScope::Project);
    let diagnostics = (native.diagnostics.instance, native.diagnostics.revision);
    assert_eq!(app.world().resource::<NotificationState>().count, 3);
    let installed_adapter = !app
        .world()
        .contains_resource::<feathrise_editor_scene::standalone::ProjectAuthorAdapters>();
    if installed_adapter {
        crate::native_plugins::adapter::install(app);
    }
    let blocker = feathrise_editor_scene::standalone::saved_play_blocker(app.world())
        .expect("真实发现失败应进入原F12检查门");
    assert!(
        errors
            .iter()
            .any(|error| blocker == format!("{error}，完成后请重新启动工程；草稿和历史已保留")),
        "阻断必须由原聚合诊断及生产F12保留草稿提示组成"
    );
    LegacyFailure {
        id: id.into(),
        original_errors: errors,
        retained_errors,
        origin,
        diagnostics,
        installed_adapter,
    }
}

pub(super) fn finish_legacy_failure(
    app: &mut App,
    target: &Handle<Image>,
    output: &Path,
    before: LegacyFailure,
) -> Value {
    settle(app);
    let native = app.world().resource::<NativeTools>();
    assert!(native.tools.is_empty(), "归档不能借机卸载或改写已加载工具");
    assert!(
        !native.load_states.contains_key(&before.id),
        "归档成功后不得保留这个never-loaded旧包的加载失败"
    );
    assert!(
        !native.load_origins.contains_key(&before.id),
        "归档成功后必须撤销同一精确来源"
    );
    assert_eq!(
        native.errors, before.retained_errors,
        "只能撤销该旧包错误，其他坏DLL诊断应逐项保持原字节和顺序"
    );
    assert_ne!(
        (native.diagnostics.instance, native.diagnostics.revision),
        before.diagnostics,
        "真实诊断集合改变必须推进通知门控身份"
    );
    let remaining = native.errors.clone();
    let notice = app.world().resource::<NotificationState>();
    assert_eq!(notice.count, 2, "通知不能保留归档前的三个失败计数");
    assert!(
        !notice.summary.contains(&before.id)
            && notice.selected_error.as_deref() != Some(before.id.as_str())
    );
    let blocker = feathrise_editor_scene::standalone::saved_play_blocker(app.world())
        .expect("另外两份坏DLL仍应正确阻止F12");
    assert!(
        remaining
            .iter()
            .any(|error| blocker == format!("{error}，完成后请重新启动工程；草稿和历史已保留"))
            && !blocker.starts_with(&format!("{}：", before.id)),
        "归档后F12不得因已解决的旧包继续阻断，也不能假装其他坏包健康"
    );
    click(app, |action| matches!(action, ManagerAction::Close));
    let image = capture_notification(app, target, output, "legacy-error-cleared-notification.png");
    assert!(
        image["visible_text"]
            .as_array()
            .unwrap()
            .iter()
            .all(|text| !text.as_str().unwrap_or_default().contains(&before.id))
    );
    click_notification(app, NotificationAction::Dismiss);
    app.world_mut().remove_resource::<NativeTools>();
    if before.installed_adapter {
        app.world_mut()
            .remove_resource::<feathrise_editor_scene::standalone::ProjectAuthorAdapters>();
    }
    super::super::open_from_workspace(app.world_mut());
    wait_for_idle(app);
    json!({"id":before.id,"original_origin":format!("{:?}",before.origin),"before_errors":before.original_errors,
        "remaining_errors":remaining,"notification_count_before":3,"notification_count_after":2,
        "legacy_state_and_origin_removed":true,"other_errors_preserved":true,"actual_f12_blocker_after":blocker,
        "notification":image,"source":"actual NativeTools discovery/poll, production archive transaction, original saved_play_blocker"})
}
