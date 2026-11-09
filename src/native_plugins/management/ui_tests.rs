//! 插件状态和原按钮回归：拒载不能伪装主动停用，坏包仍能进入更新与移除流程。
use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::native_plugins::management::types::PendingChange;

fn entry() -> PackageEntry {
    PackageEntry {
        id: "fixture.broken".into(),
        tool: "夹具插件".into(),
        version: None,
        directory: "unused-ui-fixture/plugins/broken".into(),
        path: "unused-ui-fixture/plugins/fixture.broken.dll".into(),
        scope: PluginScope::Project,
        inherited: false,
        effective: true,
        overridden_by_project: false,
        global_available: false,
        legacy: false,
        origin: None,
        enabled: false,
        error: Some("动态库缺失".into()),
        pending: None,
        runtime_dependencies: Vec::new(),
        runtime_missing: Vec::new(),
    }
}

#[test]
fn unavailable_and_staged_packages_do_not_claim_the_user_disabled_them() {
    let mut package = entry();
    let mut state = PluginManagerState::new("unused-ui-fixture".into());
    assert_eq!(state_line(&package, &state, EditorLocale::Chinese), "不可加载 · 包检查失败");
    package.error = None;
    assert_eq!(state_line(&package, &state, EditorLocale::Chinese), "已停用");
    package.pending = Some(PendingChange {
        kind: PendingKind::Install,
        version: Some("1.0.0".into()),
    });
    assert_eq!(
        state_line(&package, &state, EditorLocale::Chinese),
        "尚未安装 · 待安装（重开工程后生效）"
    );
    // 已加载旧会话的真实回执优先于当前磁盘目录的安全检查投影。
    package.pending = None;
    package.error = Some("动态库缺失".into());
    state
        .load_states
        .insert(package.id.clone(), "已加载".into());
    assert_eq!(state_line(&package, &state, EditorLocale::Chinese), "已加载 · 包检查失败");
}

#[test]
fn no_project_reports_committed_settings_but_never_hides_unfinished_installation() {
    let mut package = entry();
    package.scope = PluginScope::Global;
    package.error = None;
    package.enabled = true;
    let state = PluginManagerState::with_roots(None, std::env::temp_dir().join("unused-global-ui-fixture"));
    assert_eq!(state_line(&package, &state, EditorLocale::Chinese), "已生效");
    package.enabled = false;
    assert_eq!(state_line(&package, &state, EditorLocale::Chinese), "已停用");
    package.pending = Some(PendingChange { kind: PendingKind::Install, version: Some("1.0.0".into()) });
    assert_eq!(state_line(&package, &state, EditorLocale::Chinese), "尚未安装 · 待安装（尚未完成）");
    assert!(!detail_text(Some(&package), DetailKind::Pending, &state, EditorLocale::Chinese).contains("重开"));
}

fn feedback_app(package: PackageEntry) -> (App, Vec<(Entity, Entity)>) {
    let mut app = App::new();
    let mut state = PluginManagerState::new("unused-ui-fixture".into());
    state.selected = Some(package.id.clone());
    state.open = true;
    state.rows.push(package);
    app.insert_resource(state).add_systems(Update, present);
    let mut controls = Vec::new();
    for (control, action) in [
        (ManagerControl::Toggle, ManagerAction::SetEnabled { id: String::new(), enabled: true }),
        (ManagerControl::Update, ManagerAction::Update(String::new())),
        (ManagerControl::Remove, ManagerAction::RequestRemove(String::new())),
        (ManagerControl::CancelPending, ManagerAction::CancelPending(String::new())),
    ] {
        let label = app.world_mut().spawn((Text::new("操作"), TextColor(c::TEXT_PRIMARY))).id();
        let button = app.world_mut().spawn((
            control, action, ManagerBinding(0), EditorButtonClick::default(), UiInteraction::None,
            RelativeCursorPosition::default(), feathrise_editor_ui::dialog_button_style(false).colors,
            BackgroundColor::default(), BorderColor::all(c::DIALOG_BORDER),
        )).add_child(label).id();
        controls.push((button, label));
    }
    app.update(); // 首次换绑允许撤销旧交互；之后才输入真实状态，不能借初始化假装悬停通过。
    (app, controls)
}

#[test]
fn operational_buttons_show_hover_press_and_cancel_press_after_cursor_leaves() {
    let mut package = entry();
    package.error = None;
    package.enabled = true;
    let (mut app, controls) = feedback_app(package);
    let colors = feathrise_editor_ui::dialog_button_style(false).colors;
    for (button, _) in controls.iter().take(3) {
        assert!(app.world().get::<InteractionDisabled>(*button).is_none());
        *app.world_mut().get_mut::<RelativeCursorPosition>(*button).unwrap() =
            RelativeCursorPosition { cursor_over: true, ..default() };
        *app.world_mut().get_mut::<UiInteraction>(*button).unwrap() = UiInteraction::Hovered;
        app.update();
        assert_eq!(app.world().get::<BackgroundColor>(*button).unwrap().0, colors.hovered);
        *app.world_mut().get_mut::<UiInteraction>(*button).unwrap() = UiInteraction::Pressed;
        app.update();
        assert_eq!(app.world().get::<BackgroundColor>(*button).unwrap().0, colors.pressed);
        app.world_mut().get_mut::<RelativeCursorPosition>(*button).unwrap().cursor_over = false;
        app.update();
        assert_eq!(app.world().get::<BackgroundColor>(*button).unwrap().0, colors.normal);
    }
}

#[test]
fn no_pending_cancel_is_visibly_disabled_and_only_a_real_pending_plan_enables_it() {
    let mut package = entry();
    package.error = None;
    let (mut app, controls) = feedback_app(package);
    let (button, label) = controls[3];
    assert!(app.world().get::<InteractionDisabled>(button).is_some());
    assert_eq!(app.world().get::<TextColor>(label).unwrap().0, c::TEXT_MUTED.with_alpha(0.45));
    *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    app.world_mut().get_mut::<RelativeCursorPosition>(button).unwrap().cursor_over = true;
    app.update();
    assert_eq!(app.world().get::<BackgroundColor>(button).unwrap().0, c::DIALOG_BG);
    {
        let mut state = app.world_mut().resource_mut::<PluginManagerState>();
        state.rows[0].pending = Some(PendingChange { kind: PendingKind::Enable, version: None });
        state.revision += 1;
    }
    app.update();
    assert!(app.world().get::<InteractionDisabled>(button).is_none());
    assert_eq!(app.world().get::<TextColor>(label).unwrap().0, c::TEXT_PRIMARY);
    assert!(app.world().get::<InteractionDisabled>(controls[0].0).is_some());
    assert!(app.world().get::<InteractionDisabled>(controls[1].0).is_some());
    assert!(app.world().get::<InteractionDisabled>(controls[2].0).is_some());
}

#[test]
fn legacy_package_allows_archival_removal_but_not_enabling_updating_or_fake_cancellation() {
    let mut package = entry();
    package.legacy = true;
    let (mut app, controls) = feedback_app(package);
    assert!(app.world().get::<InteractionDisabled>(controls[0].0).is_some());
    assert!(app.world().get::<InteractionDisabled>(controls[1].0).is_some());
    assert!(app.world().get::<InteractionDisabled>(controls[2].0).is_none());
    assert!(app.world().get::<InteractionDisabled>(controls[3].0).is_some());
    app.world_mut().resource_mut::<PluginManagerState>().confirm_remove = Some("fixture.broken".into());
    let text = app.world_mut().spawn((ManagerText::Confirmation, Text::default(), TextColor::default())).id();
    app.update();
    assert!(app.world().get::<Text>(text).unwrap().contains("归档移除"));
    assert!(app.world().get::<Text>(text).unwrap().contains("原文件保留"));
}

#[test]
fn directory_button_updates_display_and_click_identity_when_switching_scope() {
    let root = std::env::temp_dir().join("unused-directory-ui-fixture");
    let mut app = App::new();
    let mut state = PluginManagerState::with_roots(Some(root.join("project")), root.join("global"));
    state.open = true;
    state.scope = PluginScope::Global;
    app.insert_resource(state).add_systems(Update, present);
    let label = app.world_mut().spawn((ManagerText::Directory, Text::default(), TextColor::default())).id();
    let button = app.world_mut().spawn((
        ManagerControl::OpenDirectory, ManagerAction::OpenDirectory, ManagerBinding(0),
        EditorButtonClick::default(), UiInteraction::None,
        feathrise_editor_ui::dialog_button_style(false).colors, BackgroundColor::default(),
    )).add_child(label).id();
    app.update();
    assert!(app.world().get::<Text>(label).unwrap().contains(&crate::project::display_path(&root.join("global"))));
    *app.world_mut().get_mut::<EditorButtonClick>(button).unwrap() = EditorButtonClick::triggered();
    {
        let mut state = app.world_mut().resource_mut::<PluginManagerState>();
        state.scope = PluginScope::Project;
        state.revision += 1;
    }
    app.update();
    assert!(app.world().get::<Text>(label).unwrap().contains(&crate::project::display_path(&root.join("project").join("plugins"))));
    assert!(!app.world().get::<EditorButtonClick>(button).unwrap().is_clicked());
    assert!(app.world().get::<InteractionDisabled>(button).is_none());
}

#[test]
fn production_buttons_block_enabling_invalid_package_and_keep_repair_actions_available() {
    let mut app = App::new();
    let mut state = PluginManagerState::new("unused-ui-fixture".into());
    let package = entry();
    state.selected = Some(package.id.clone());
    state.open = true;
    state.rows.push(package);
    app.insert_resource(state).add_systems(Update, present);
    let mut controls = Vec::new();
    for (control, action) in [
        (
            ManagerControl::Toggle,
            ManagerAction::SetEnabled {
                id: String::new(),
                enabled: true,
            },
        ),
        (ManagerControl::Update, ManagerAction::Update(String::new())),
        (
            ManagerControl::Remove,
            ManagerAction::RequestRemove(String::new()),
        ),
    ] {
        controls.push(
            app.world_mut()
                .spawn((
                    control,
                    action,
                    ManagerBinding(0),
                    EditorButtonClick::default(),
                    UiInteraction::None,
                    feathrise_editor_ui::dialog_button_style(false).colors,
                    BackgroundColor::default(),
                ))
                .id(),
        );
    }
    app.update();
    assert!(
        app.world()
            .get::<InteractionDisabled>(controls[0])
            .is_some()
    );
    assert!(
        app.world()
            .get::<InteractionDisabled>(controls[1])
            .is_none()
    );
    assert!(
        app.world()
            .get::<InteractionDisabled>(controls[2])
            .is_none()
    );
    {
        let mut state = app.world_mut().resource_mut::<PluginManagerState>();
        state.rows[0].error = None;
        state.revision += 1;
    }
    app.update();
    assert!(
        app.world()
            .get::<InteractionDisabled>(controls[0])
            .is_none(),
        "包修复后原启用按钮恢复可用"
    );
}
