//! 真实设置输入入口的录入/冲突/静默保存/关闭帧回归，不写用户设置文件。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::components::{
    SceneEditorSettingsButton, SceneEditorSettingsPanel, SceneEditorSettingsScaleText,
    SceneEditorShortcutSettingsPanel,
};
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState};
use crate::ui::editor_settings::{handle_settings_input, sync_settings_panel};
use crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard;
use bevy::input_focus::InputFocus;
use feathrise_editor_ui::ActiveNumericInputScrub;
use feathrise_editor_core::EditorShortcutAction;

fn app() -> App {
    let mut app = App::new();
    app.insert_resource(SceneEditorState {
        open: true,
        ..default()
    })
    .init_resource::<SceneEditorSettingsState>()
    .init_resource::<crate::resources::SceneEditorLayoutState>()
    .init_resource::<EditorSettings>()
    .init_resource::<SceneEditorNumericScrubMouseGuard>()
    .init_resource::<ActiveNumericInputScrub>()
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<ButtonInput<MouseButton>>()
    .init_resource::<InputFocus>()
    .add_systems(
        Update,
        (
            handle_settings_input,
            update_scene_editor_numeric_scrub_mouse_guard,
            sync_settings_panel,
            sync_shortcut_settings_panel,
        )
            .chain(),
    );
    app.world_mut()
        .spawn((SceneEditorSettingsPanel, Node::default()));
    app.world_mut()
        .spawn((SceneEditorShortcutSettingsPanel, Node::default()));
    app.world_mut()
        .spawn((SceneEditorShortcutNoticeText, Text::default()));
    app.world_mut()
        .spawn((SceneEditorSettingsScaleText, Text::default()));
    for action in [
        EditorSettingsAction::Open,
        EditorSettingsAction::OpenShortcuts,
        EditorSettingsAction::Close,
        EditorSettingsAction::ResetShortcuts,
        EditorSettingsAction::SetScale(125),
    ] {
        button(&mut app, action);
    }
    for action in EditorShortcutAction::ALL {
        button(&mut app, EditorSettingsAction::RebindShortcut(action));
        button(&mut app, EditorSettingsAction::UnbindShortcut(action));
        app.world_mut()
            .spawn((SceneEditorShortcutBindingText(action), Text::default()));
    }
    app.update();
    app
}

fn button(app: &mut App, action: EditorSettingsAction) {
    app.world_mut().spawn((
        SceneEditorSettingsButton(action),
        feathrise_editor_ui::EditorButtonClick::default(),
        UiInteraction::None,
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
        BorderColor::all(colors::COPPER_BORDER_WEAK),
    ));
}

fn click(app: &mut App, action: EditorSettingsAction) {
    let world = app.world_mut();
    for (button, mut interaction, mut click) in world
        .query::<(&SceneEditorSettingsButton, &mut UiInteraction, &mut feathrise_editor_ui::EditorButtonClick)>()
        .iter_mut(world)
    {
        *click = if button.0 == action { feathrise_editor_ui::EditorButtonClick::triggered() } else { default() };
        *interaction = if button.0 == action {
            UiInteraction::Pressed
        } else {
            UiInteraction::None
        };
    }
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    let world = app.world_mut();
    world.resource_mut::<ButtonInput<MouseButton>>().reset_all();
    for (mut interaction, mut click) in world.query::<(&mut UiInteraction, &mut feathrise_editor_ui::EditorButtonClick)>().iter_mut(world) {
        *click = default();
        *interaction = UiInteraction::None;
    }
    app.update();
}

fn press(app: &mut App, keys: &[KeyCode]) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    input.reset_all();
    for &key in keys {
        input.press(key);
    }
    drop(input);
    app.update();
}

#[test]
fn listening_idle_cancel_conflict_and_reselect_do_not_dirty_preferences() {
    let mut app = app();
    let tick = app
        .world()
        .get_resource_ref::<EditorSettings>()
        .unwrap()
        .last_changed();
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    click(
        &mut app,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
    );
    for _ in 0..4 {
        app.update();
    }
    press(&mut app, &[KeyCode::ControlLeft]); // 修饰键本身不能成为主键。
    press(&mut app, &[KeyCode::Home]); // 与查看全图冲突，录入仍保持活动。
    assert_eq!(
        app.world().resource::<SceneEditorSettingsState>().listening,
        Some(EditorShortcutAction::FocusSelected)
    );
    assert!(
        app.world()
            .resource::<SceneEditorSettingsState>()
            .shortcut_notice
            .contains("查看全图")
    );
    press(&mut app, &[KeyCode::Escape]);
    assert!(app.world().resource::<SceneEditorSettingsState>().open);
    assert!(
        app.world()
            .resource::<SceneEditorSettingsState>()
            .listening
            .is_none()
    );
    click(
        &mut app,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
    );
    press(&mut app, &[KeyCode::KeyF]); // 重选当前绑定，不制造写盘变更。
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorSettings>()
            .unwrap()
            .last_changed(),
        tick
    );
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    assert!(!app.world().resource::<SceneEditorState>().save_requested);
}

#[test]
fn custom_chord_is_adopted_once_and_capture_never_leaks_its_edge() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    click(
        &mut app,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
    );
    press(
        &mut app,
        &[KeyCode::ControlRight, KeyCode::ShiftLeft, KeyCode::KeyG],
    );
    let settings = app.world().resource::<EditorSettings>();
    assert_eq!(
        settings
            .editor_keybindings
            .get(EditorShortcutAction::FocusSelected)
            .unwrap()
            .display(),
        "Ctrl+Shift+G"
    );
    let tick = app
        .world()
        .get_resource_ref::<EditorSettings>()
        .unwrap()
        .last_changed();
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::KeyG)
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .viewport_framing_requested
            .is_none()
    );
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorSettings>()
            .unwrap()
            .last_changed(),
        tick
    );
    let world = app.world_mut();
    let (_, text) = world
        .query::<(&SceneEditorShortcutBindingText, &Text)>()
        .iter(world)
        .find(|(action, _)| action.0 == EditorShortcutAction::FocusSelected)
        .unwrap();
    assert_eq!(text.as_str(), "Ctrl+Shift+G");
}

#[test]
fn reserved_key_is_rejected_and_escape_close_keeps_a_quiet_mouse_frame() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    click(
        &mut app,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
    );
    press(&mut app, &[KeyCode::F12]);
    assert!(
        app.world()
            .resource::<SceneEditorSettingsState>()
            .listening
            .is_some()
    );
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_keybindings,
        EditorKeyBindings::default()
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::F12)
    );
    press(&mut app, &[KeyCode::Escape]); // 先取消录入。
    press(&mut app, &[KeyCode::Escape]); // 再关闭模态。
    assert!(!app.world().resource::<SceneEditorSettingsState>().open);
    assert!(
        app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
    app.update();
    assert!(
        !app.world()
            .resource::<SceneEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

#[test]
fn unbind_reset_and_hidden_page_controls_do_not_change_other_preferences() {
    let mut app = app();
    click(
        &mut app,
        EditorSettingsAction::UnbindShortcut(EditorShortcutAction::FocusSelected),
    );
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_keybindings,
        EditorKeyBindings::default()
    );
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    click(&mut app, EditorSettingsAction::SetScale(125));
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.0);
    click(
        &mut app,
        EditorSettingsAction::UnbindShortcut(EditorShortcutAction::FocusSelected),
    );
    assert!(
        app.world()
            .resource::<EditorSettings>()
            .editor_keybindings
            .get(EditorShortcutAction::FocusSelected)
            .is_none()
    );
    click(&mut app, EditorSettingsAction::ResetShortcuts);
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_keybindings,
        EditorKeyBindings::default()
    );
    let tick = app
        .world()
        .get_resource_ref::<EditorSettings>()
        .unwrap()
        .last_changed();
    click(&mut app, EditorSettingsAction::ResetShortcuts);
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorSettings>()
            .unwrap()
            .last_changed(),
        tick
    );
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Node, With<SceneEditorSettingsPanel>>()
            .single(world)
            .unwrap()
            .display,
        Display::Flex
    );
    assert_eq!(
        world
            .query_filtered::<&Node, With<SceneEditorShortcutSettingsPanel>>()
            .single(world)
            .unwrap()
            .display,
        Display::Flex
    );
    click(&mut app, EditorSettingsAction::Close);
    click(&mut app, EditorSettingsAction::Open);
    click(
        &mut app,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
    );
    assert!(
        app.world()
            .resource::<SceneEditorSettingsState>()
            .listening
            .is_none()
    );
}

#[test]
fn switching_settings_page_cancels_capture_before_same_frame_key_and_keeps_one_modal() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::Open);
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    click(
        &mut app,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
    );
    let tick = app
        .world()
        .get_resource_ref::<EditorSettings>()
        .unwrap()
        .last_changed();
    let world = app.world_mut();
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyG);
    for (button, mut interaction, mut click) in world
        .query::<(&SceneEditorSettingsButton, &mut UiInteraction, &mut feathrise_editor_ui::EditorButtonClick)>()
        .iter_mut(world)
    {
        if button.0 == EditorSettingsAction::Open {
            *click = feathrise_editor_ui::EditorButtonClick::triggered();
            *interaction = UiInteraction::Pressed;
        }
    }
    app.update();
    let state = app.world().resource::<SceneEditorSettingsState>();
    assert!(state.open && !state.shortcuts && state.listening.is_none());
    assert_eq!(state.shortcut_notice_kind, EditorShortcutNoticeKind::Idle);
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_keybindings,
        EditorKeyBindings::default()
    );
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorSettings>()
            .unwrap()
            .last_changed(),
        tick
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::KeyG)
    );
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Node, With<SceneEditorSettingsPanel>>()
            .single(world)
            .unwrap()
            .display,
        Display::Flex
    );
    assert_eq!(
        world
            .query_filtered::<&Node, With<SceneEditorShortcutSettingsPanel>>()
            .single(world)
            .unwrap()
            .display,
        Display::None
    );
    assert!(!world.resource::<SceneEditorState>().dirty);
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    click(
        &mut app,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
    );
    let world = app.world_mut();
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    for (button, mut interaction, mut click) in world
        .query::<(&SceneEditorSettingsButton, &mut UiInteraction, &mut feathrise_editor_ui::EditorButtonClick)>()
        .iter_mut(world)
    {
        if button.0 == EditorSettingsAction::Close {
            *click = feathrise_editor_ui::EditorButtonClick::triggered();
            *interaction = UiInteraction::Pressed;
        }
    }
    app.update();
    assert!(app.world().resource::<SceneEditorSettingsState>().open);
    assert!(
        app.world()
            .resource::<SceneEditorSettingsState>()
            .listening
            .is_none()
    );
    press(&mut app, &[KeyCode::Escape]);
    assert!(!app.world().resource::<SceneEditorSettingsState>().open);
}

#[test]
fn reselecting_shortcut_page_preserves_capture_feedback_and_bindings() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    click(
        &mut app,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
    );
    press(&mut app, &[KeyCode::Home]);
    let notice = app
        .world()
        .resource::<SceneEditorSettingsState>()
        .shortcut_notice
        .clone();
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    let state = app.world().resource::<SceneEditorSettingsState>();
    assert!(state.open && state.shortcuts);
    assert_eq!(state.listening, Some(EditorShortcutAction::FocusSelected));
    assert_eq!(state.shortcut_notice, notice);
    assert_eq!(
        state.shortcut_notice_kind,
        EditorShortcutNoticeKind::Warning
    );
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_keybindings,
        EditorKeyBindings::default()
    );
}

#[test]
fn shortcut_feedback_uses_explicit_tones_and_rejected_rebinding_keeps_preferences() {
    let mut app = app();
    let (notice, binding) = {
        let world = app.world_mut();
        let notice = world
            .query_filtered::<Entity, With<SceneEditorShortcutNoticeText>>()
            .single(world)
            .unwrap();
        let binding = world
            .query::<(Entity, &SceneEditorShortcutBindingText)>()
            .iter(world)
            .find(|(_, action)| action.0 == EditorShortcutAction::FocusSelected)
            .unwrap()
            .0;
        (notice, binding)
    };
    app.world_mut()
        .entity_mut(notice)
        .insert(TextColor(colors::TEXT_MUTED));
    app.world_mut()
        .entity_mut(binding)
        .insert(TextColor(colors::TEXT_MUTED));
    let card = app
        .world_mut()
        .spawn((
            ShortcutNoticeCard,
            BackgroundColor(colors::PANEL_BG_SECTION),
            BorderColor::all(colors::COPPER_BORDER_WEAK),
        ))
        .id();
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    let tick = app
        .world()
        .get_resource_ref::<EditorSettings>()
        .unwrap()
        .last_changed();
    click(
        &mut app,
        EditorSettingsAction::RebindShortcut(EditorShortcutAction::FocusSelected),
    );
    assert_eq!(
        app.world()
            .resource::<SceneEditorSettingsState>()
            .shortcut_notice_kind,
        EditorShortcutNoticeKind::Listening
    );
    assert_eq!(
        app.world().get::<TextColor>(notice).unwrap().0,
        colors::TEXT_ACCENT
    );
    press(&mut app, &[KeyCode::Home]);
    assert_eq!(
        app.world().get::<TextColor>(notice).unwrap().0,
        colors::DANGER_TEXT
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(card).unwrap().0,
        colors::DANGER_BG
    );
    assert_eq!(
        app.world().get::<BorderColor>(card).unwrap(),
        &BorderColor::all(colors::DANGER_BORDER)
    );
    assert!(
        app.world()
            .get::<Text>(notice)
            .unwrap()
            .as_str()
            .contains("查看全图")
    );
    press(&mut app, &[KeyCode::Escape]);
    assert!(app.world().resource::<SceneEditorSettingsState>().open);
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorSettings>()
            .unwrap()
            .last_changed(),
        tick
    );
    click(
        &mut app,
        EditorSettingsAction::UnbindShortcut(EditorShortcutAction::FocusSelected),
    );
    assert_eq!(app.world().get::<Text>(binding).unwrap().as_str(), "未绑定");
    assert_eq!(
        app.world().get::<TextColor>(binding).unwrap().0,
        colors::TEXT_MUTED
    );
    assert_eq!(
        app.world()
            .resource::<SceneEditorSettingsState>()
            .shortcut_notice_kind,
        EditorShortcutNoticeKind::Success
    );
    assert_eq!(
        app.world().get::<TextColor>(notice).unwrap().0,
        colors::TEXT_PRIMARY
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(card).unwrap().0,
        colors::PANEL_BG_SECTION
    );
}

#[test]
fn shortcut_scroll_converts_dpi_clamps_and_does_not_replay_on_page_or_editor_reopen() {
    let mut app = app();
    app.add_message::<MouseWheel>().add_systems(
        Update,
        scroll_shortcut_settings.after(handle_settings_input),
    );
    let body = app
        .world_mut()
        .spawn((
            SceneEditorShortcutSettingsBody,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::splat(0.5)),
            },
            ComputedNode {
                size: Vec2::splat(100.0),
                content_size: Vec2::new(100.0, 500.0),
                inverse_scale_factor: 0.25,
                ..default()
            },
            ScrollPosition::default(),
        ))
        .id();
    let window = app.world_mut().spawn_empty().id();
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    let wheel = |amount, unit| MouseWheel {
        x: 0.0,
        y: amount,
        unit,
        window,
        phase: bevy::input::touch::TouchPhase::Moved,
    };
    app.world_mut()
        .write_message(wheel(-40.0, MouseScrollUnit::Pixel));
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 10.0);
    app.world_mut()
        .write_message(wheel(-1.0, MouseScrollUnit::Line));
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 34.0);
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(body)
        .unwrap()
        .cursor_over = false;
    app.world_mut()
        .write_message(wheel(-1.0, MouseScrollUnit::Line));
    app.world_mut()
        .write_message(wheel(f32::NAN, MouseScrollUnit::Pixel));
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 34.0);
    // 鼠标移到旁边的原生轨道仍只滚动同一正文，离开二者则消费但不应用。
    let track = app
        .world_mut()
        .spawn((
            ShortcutScrollbarTrack,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::splat(0.5)),
            },
        ))
        .id();
    app.world_mut()
        .write_message(wheel(-20.0, MouseScrollUnit::Pixel));
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 39.0);
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(track)
        .unwrap()
        .cursor_over = false;
    app.world_mut()
        .write_message(wheel(-1.0, MouseScrollUnit::Line));
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 39.0);
    app.world_mut()
        .resource_mut::<SceneEditorSettingsState>()
        .shortcut_notice = "已取消快捷键录入".into();
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 39.0);
    app.world_mut()
        .get_mut::<ComputedNode>(body)
        .unwrap()
        .content_size
        .y = 120.0;
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 5.0);
    click(&mut app, EditorSettingsAction::Open);
    app.world_mut()
        .write_message(wheel(-1.0, MouseScrollUnit::Line));
    app.update();
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 0.0);
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    app.world_mut().get_mut::<ScrollPosition>(body).unwrap().y = 4.0;
    app.world_mut()
        .write_message(wheel(-1.0, MouseScrollUnit::Line));
    app.update();
    app.world_mut().resource_mut::<SceneEditorState>().open = true;
    click(&mut app, EditorSettingsAction::OpenShortcuts);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 0.0);
}
