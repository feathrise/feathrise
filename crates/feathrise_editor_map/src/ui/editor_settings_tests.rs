//! 设置模态、倍率边界、键盘隔离与关闭帧鼠标保护回归，不写正式配置或地图。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard;
use bevy::input_focus::FocusCause;
use feathrise_editor_ui::ActiveNumericInputScrub;

#[derive(Resource, Default)]
struct ApplicationPreferences { mouse_sensitivity: f32 } // 测试私有应用输入，普通编辑器不解释其语义。

fn app() -> App {
    let mut app = App::new();
    app.insert_resource(MapEditorState {
        open: true,
        ..default()
    })
    .init_resource::<MapEditorSettingsState>()
    .init_resource::<MapEditorLayoutState>()
    .init_resource::<EditorSettings>()
    .init_resource::<ApplicationPreferences>()
    .add_systems(Startup, crate::ui::layout::restore_editor_layout_preferences)
    .init_resource::<feathrise_editor_ui::EditorPalette>()
    .add_systems(PostUpdate, crate::ui::theme::sync_editor_theme)
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<ButtonInput<MouseButton>>()
    .init_resource::<ActiveNumericInputScrub>()
    .init_resource::<InputFocus>()
    .add_systems(
        Update,
        (
            handle_settings_input,
            crate::ui::layout::remember_editor_layout_preferences,
            update_map_editor_numeric_scrub_mouse_guard,
            sync_settings_panel,
        )
            .chain(),
    );
    app.world_mut().spawn((
        MapEditorSettingsPanel,
        Node {
            display: Display::None,
            ..default()
        },
    ));
    app.world_mut()
        .spawn((MapEditorSettingsScaleText, Text::new("100%")));
    for action in [
        EditorSettingsAction::Open,
        EditorSettingsAction::Close,
        EditorSettingsAction::ResetLayout,
        EditorSettingsAction::ToggleTerrainBackfaces,
        EditorSettingsAction::DecreaseScale,
        EditorSettingsAction::IncreaseScale,
        EditorSettingsAction::SetAutoScale,
        EditorSettingsAction::SetScale(75),
        EditorSettingsAction::SetScale(100),
        EditorSettingsAction::SetScale(125),
        EditorSettingsAction::SetScale(150),
        EditorSettingsAction::SetColorTheme(EditorColorTheme::NeutralGray),
        EditorSettingsAction::SetColorTheme(EditorColorTheme::GraphiteBlue),
        EditorSettingsAction::SetColorTheme(EditorColorTheme::WarmCopper),
    ] {
        app.world_mut().spawn((
            MapEditorSettingsButton(action),
            feathrise_editor_ui::EditorButtonClick::default(),
            UiInteraction::None,
            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
            BorderColor::all(colors::COPPER_BORDER_WEAK),
        ));
    }
    app
}

fn click(app: &mut App, action: EditorSettingsAction) {
    let world = app.world_mut();
    let mut buttons = world.query::<(&MapEditorSettingsButton, &mut UiInteraction, &mut feathrise_editor_ui::EditorButtonClick)>();
    for (button, mut interaction, mut click) in buttons.iter_mut(world) {
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
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all();
    let world = app.world_mut();
    let mut interactions = world.query::<(&mut UiInteraction, &mut feathrise_editor_ui::EditorButtonClick)>();
    for (mut interaction, mut click) in interactions.iter_mut(world) {
        *click = default();
        *interaction = UiInteraction::None; // 无拾取插件的测试中，显式模拟松键并移出按钮。
    }
    app.update();
}

#[test]
fn editor_settings_terrain_backfaces_persists_across_f12_and_ignores_hidden_page() {
    let mut app = app();
    app.init_resource::<feathrise_scene::TerrainBackfacesPreview>()
        .add_systems(PostUpdate, sync_terrain_backfaces_preview);
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    assert!(app.world().resource::<feathrise_scene::TerrainBackfacesPreview>().enabled);
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    click(&mut app, EditorSettingsAction::Open);
    click(&mut app, EditorSettingsAction::ToggleTerrainBackfaces);
    assert!(!app.world().resource::<EditorSettings>().editor_show_terrain_backfaces);
    assert!(!app.world().resource::<feathrise_scene::TerrainBackfacesPreview>().enabled);
    app.world_mut().resource_mut::<MapEditorSettingsState>().shortcuts = true;
    click(&mut app, EditorSettingsAction::ToggleTerrainBackfaces);
    assert!(!app.world().resource::<EditorSettings>().editor_show_terrain_backfaces);
    app.world_mut().resource_mut::<MapEditorSettingsState>().shortcuts = false;
    click(&mut app, EditorSettingsAction::ToggleTerrainBackfaces);
    assert!(app.world().resource::<feathrise_scene::TerrainBackfacesPreview>().enabled);
    app.world_mut().resource_mut::<MapEditorState>().close_requested = true;
    app.update();
    assert!(app.world().resource::<feathrise_scene::TerrainBackfacesPreview>().enabled);
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    assert!(app.world().resource::<feathrise_scene::TerrainBackfacesPreview>().enabled);
    assert!(app.world().resource::<EditorSettings>().editor_show_terrain_backfaces);
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    app.world_mut().resource_mut::<MapEditorState>().close_requested = false;
    app.update();
    assert!(app.world().resource::<feathrise_scene::TerrainBackfacesPreview>().enabled);
    click(&mut app, EditorSettingsAction::Open); // F12关闭会收起设置模态，再次操作需重新打开面板。
    click(&mut app, EditorSettingsAction::ToggleTerrainBackfaces);
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    assert!(!app.world().resource::<feathrise_scene::TerrainBackfacesPreview>().enabled);
}

#[test]
fn editor_settings_color_themes_switch_immediately_without_rebuilding_or_dirtying_map() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::Open);
    click(&mut app, EditorSettingsAction::SetScale(125));
    let entities: Vec<_> = app.world_mut().query::<Entity>().iter(app.world()).collect();
    let status = app.world().resource::<MapEditorState>().status.clone();
    for theme in EditorColorTheme::ALL {
        let action = EditorSettingsAction::SetColorTheme(theme);
        click(&mut app, action);
        assert_eq!(app.world().resource::<EditorSettings>().editor_color_theme, theme);
        assert_eq!(app.world().resource::<feathrise_editor_ui::EditorPalette>().name(), theme.as_str());
        assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.25);
        assert_eq!(app.world().resource::<MapEditorState>().status, status);
        assert!(!app.world().resource::<MapEditorState>().save_requested);
        for entity in &entities { assert!(app.world().get_entity(*entity).is_ok()); }
        let changed = app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed();
        click(&mut app, action);
        assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
        let world = app.world_mut();
        let mut buttons = world.query::<(&MapEditorSettingsButton, &BackgroundColor)>();
        assert_eq!(buttons.iter(world).find(|(b, _)| b.0 == action).unwrap().1.0, colors::SELECT_BG);
    }
}

#[test]
fn editor_settings_color_theme_ignores_hidden_and_shortcut_page_controls() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::SetColorTheme(EditorColorTheme::WarmCopper));
    assert_eq!(app.world().resource::<EditorSettings>().editor_color_theme, EditorColorTheme::NeutralGray);
    app.world_mut().resource_mut::<MapEditorSettingsState>().open = true;
    app.world_mut().resource_mut::<MapEditorSettingsState>().shortcuts = true;
    click(&mut app, EditorSettingsAction::SetColorTheme(EditorColorTheme::GraphiteBlue));
    assert_eq!(app.world().resource::<EditorSettings>().editor_color_theme, EditorColorTheme::NeutralGray);
}

#[test]
fn editor_settings_reset_layout_restores_both_panes_without_resetting_other_preferences() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::Open);
    let preferred = EditorLayoutPreferences {
        content_width: 326.0, inspector_width: 490.0,
        content_visible: false, inspector_visible: true,
    };
    {
        let mut settings = app.world_mut().resource_mut::<EditorSettings>();
        settings.editor_layout = preferred;
        settings.editor_ui_scale = 1.25;
        settings.editor_color_theme = EditorColorTheme::GraphiteBlue;
        settings.asset_browser_view_mode = feathrise_editor_core::AssetBrowserViewMode::Thumbnails;
        settings.editor_keybindings.assign(feathrise_editor_core::EditorShortcutAction::FrameMap, None).unwrap();
    }
    {
        let mut layout = app.world_mut().resource_mut::<MapEditorLayoutState>();
        layout.apply_preferences(preferred);
        layout.toggle(crate::types::EditorLayoutAction::Focus);
    }
    let status = app.world().resource::<MapEditorState>().status.clone();
    let entities: Vec<_> = app.world_mut().query::<Entity>().iter(app.world()).collect();
    click(&mut app, EditorSettingsAction::ResetLayout);
    let layout = app.world().resource::<MapEditorLayoutState>();
    assert_eq!(layout.preferences(), EditorLayoutPreferences::default());
    assert!(layout.focus_restore.is_none() && layout.drag.is_none());
    assert!(layout.content_visible && layout.inspector_visible);
    let settings = app.world().resource::<EditorSettings>();
    assert_eq!(settings.editor_layout, EditorLayoutPreferences::default());
    assert_eq!(settings.editor_ui_scale, 1.25);
    assert_eq!(settings.editor_color_theme, EditorColorTheme::GraphiteBlue);
    assert_eq!(settings.asset_browser_view_mode, feathrise_editor_core::AssetBrowserViewMode::Thumbnails);
    assert!(settings.editor_keybindings.get(feathrise_editor_core::EditorShortcutAction::FrameMap).is_none());
    assert_eq!(app.world().resource::<MapEditorState>().status, status);
    assert!(!app.world().resource::<MapEditorState>().save_requested);
    for entity in entities { assert!(app.world().get_entity(entity).is_ok()); }
    assert!(app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
    let changed = app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed();
    click(&mut app, EditorSettingsAction::ResetLayout);
    assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
}

#[test]
fn editor_settings_reset_layout_ignores_closed_and_shortcut_page_controls() {
    let mut app = app();
    let preferred = EditorLayoutPreferences { content_width: 320.0, ..default() };
    app.world_mut().resource_mut::<EditorSettings>().editor_layout = preferred;
    click(&mut app, EditorSettingsAction::ResetLayout);
    assert_eq!(app.world().resource::<MapEditorLayoutState>().preferences(), preferred);
    app.world_mut().resource_mut::<MapEditorSettingsState>().open = true;
    app.world_mut().resource_mut::<MapEditorSettingsState>().shortcuts = true;
    click(&mut app, EditorSettingsAction::ResetLayout);
    assert_eq!(app.world().resource::<EditorSettings>().editor_layout, preferred);
    assert_eq!(app.world().resource::<MapEditorLayoutState>().preferences(), preferred);
}

#[test]
fn editor_settings_open_and_close_do_not_dirty_game_preferences_or_editor() {
    let mut app = app();
    app.update();
    let changed = app
        .world()
        .get_resource_ref::<EditorSettings>()
        .unwrap()
        .last_changed();
    let status = app.world().resource::<MapEditorState>().status.clone();
    click(&mut app, EditorSettingsAction::Open);
    assert!(app.world().resource::<MapEditorSettingsState>().open);
    click(&mut app, EditorSettingsAction::Close);
    assert!(!app.world().resource::<MapEditorSettingsState>().open);
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorSettings>()
            .unwrap()
            .last_changed(),
        changed
    );
    assert_eq!(app.world().resource::<MapEditorState>().status, status);
    assert!(!app.world().resource::<MapEditorState>().save_requested);
}

#[test]
fn editor_settings_steps_clamp_and_reset_only_editor_scale() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<ApplicationPreferences>()
        .mouse_sensitivity = 0.007;
    click(&mut app, EditorSettingsAction::Open);
    for _ in 0..30 {
        click(&mut app, EditorSettingsAction::DecreaseScale);
    }
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 0.6);
    for _ in 0..30 {
        click(&mut app, EditorSettingsAction::IncreaseScale);
    }
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 2.0);
    click(&mut app, EditorSettingsAction::SetScale(100));
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.0);
    assert_eq!(
        app.world().resource::<ApplicationPreferences>().mouse_sensitivity,
        0.007
    );
}

#[test]
fn editor_settings_reselecting_current_scale_does_not_request_a_save() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::Open);
    click(&mut app, EditorSettingsAction::SetScale(125));
    let changed = app
        .world()
        .get_resource_ref::<EditorSettings>()
        .unwrap()
        .last_changed();
    click(&mut app, EditorSettingsAction::SetScale(125));
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorSettings>()
            .unwrap()
            .last_changed(),
        changed
    );
}

#[test]
fn editor_auto_scale_settings_keeps_adjustment_when_window_changes() {
    let mut app = app();
    let window = app.world_mut().spawn(Window {
        resolution: bevy::window::WindowResolution::new(1920, 1080),
        ..default()
    }).id();
    click(&mut app, EditorSettingsAction::Open);
    click(&mut app, EditorSettingsAction::SetScale(125));
    click(&mut app, EditorSettingsAction::SetAutoScale);
    assert!(app.world().resource::<EditorSettings>().editor_ui_scale_auto);
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.25);
    let changed = app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed();
    click(&mut app, EditorSettingsAction::SetAutoScale);
    assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
    app.world_mut().get_mut::<Window>(window).unwrap().resolution =
        bevy::window::WindowResolution::new(1440, 810);
    app.update();
    assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
    let world = app.world_mut();
    assert_eq!(world.query_filtered::<&Text, With<MapEditorSettingsScaleText>>()
        .single(world).unwrap().as_str(), "自动 75%");
    let mut buttons = world.query::<(&MapEditorSettingsButton, &BackgroundColor)>();
    assert_eq!(buttons.iter(world).find(|(button, _)| button.0 == EditorSettingsAction::SetAutoScale)
        .unwrap().1.0, colors::SELECT_BG);
    assert_ne!(buttons.iter(world).find(|(button, _)| button.0 == EditorSettingsAction::SetScale(75))
        .unwrap().1.0, colors::SELECT_BG);
    click(&mut app, EditorSettingsAction::IncreaseScale);
    let settings = app.world().resource::<EditorSettings>();
    assert!(settings.editor_ui_scale_auto);
    assert_eq!(settings.editor_ui_scale, 1.25);
    assert_eq!(settings.editor_ui_scale_auto_offset, 0.05);
    let world = app.world_mut();
    assert_eq!(world.query_filtered::<&Text, With<MapEditorSettingsScaleText>>()
        .single(world).unwrap().as_str(), "自动 80%");
    let changed = world.get_resource_ref::<EditorSettings>().unwrap().last_changed();
    app.world_mut().get_mut::<Window>(window).unwrap().resolution =
        bevy::window::WindowResolution::new(1920, 1080);
    app.update();
    let world = app.world_mut();
    assert_eq!(world.query_filtered::<&Text, With<MapEditorSettingsScaleText>>()
        .single(world).unwrap().as_str(), "自动 105%");
    assert_eq!(world.get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
    click(&mut app, EditorSettingsAction::DecreaseScale);
    assert!(app.world().resource::<EditorSettings>().editor_ui_scale_auto);
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale_auto_offset, 0.0);
    click(&mut app, EditorSettingsAction::IncreaseScale);
    click(&mut app, EditorSettingsAction::SetScale(100));
    let settings = app.world().resource::<EditorSettings>();
    assert!(!settings.editor_ui_scale_auto);
    assert_eq!(settings.editor_ui_scale, 1.0);
    assert_eq!(settings.editor_ui_scale_auto_offset, 0.05);
    click(&mut app, EditorSettingsAction::SetAutoScale);
    assert!(app.world().resource::<EditorSettings>().editor_ui_scale_auto);
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale_auto_offset, 0.05);
}

#[test]
fn editor_auto_scale_steps_clamp_without_accumulating_hidden_offset() {
    let mut app = app();
    app.world_mut().spawn(Window {
        resolution: bevy::window::WindowResolution::new(1920, 1080), ..default()
    });
    click(&mut app, EditorSettingsAction::Open);
    click(&mut app, EditorSettingsAction::SetAutoScale);
    for _ in 0..50 { click(&mut app, EditorSettingsAction::IncreaseScale); }
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale_auto_offset, 1.0);
    let world = app.world_mut();
    assert_eq!(world.query_filtered::<&Text, With<MapEditorSettingsScaleText>>()
        .single(world).unwrap().as_str(), "自动 200%");
    let changed = app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed();
    click(&mut app, EditorSettingsAction::IncreaseScale);
    assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
    click(&mut app, EditorSettingsAction::DecreaseScale);
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale_auto_offset, 0.95);
    for _ in 0..60 { click(&mut app, EditorSettingsAction::DecreaseScale); }
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale_auto_offset, -0.4);
    let world = app.world_mut();
    assert_eq!(world.query_filtered::<&Text, With<MapEditorSettingsScaleText>>()
        .single(world).unwrap().as_str(), "自动 60%");
    let changed = app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed();
    click(&mut app, EditorSettingsAction::DecreaseScale);
    assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
    click(&mut app, EditorSettingsAction::IncreaseScale);
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale_auto_offset, -0.35);
    assert!(app.world().resource::<EditorSettings>().editor_ui_scale_auto);
}

#[test]
fn editor_settings_repairs_nonfinite_scale_on_reset() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::Open);
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = f32::NAN;
    click(&mut app, EditorSettingsAction::SetScale(100));
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.0);
}

#[test]
fn editor_settings_hidden_controls_cannot_change_scale() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::SetScale(75));
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.0);
    assert!(!app.world().resource::<MapEditorSettingsState>().open);
}

#[test]
fn editor_settings_swallow_shortcuts_and_escape_close_has_a_quiet_frame() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::Open);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyS);
    app.update();
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::KeyS)
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<MapEditorSettingsState>().open);
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Escape)
    );
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    app.update();
    assert!(
        !app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

#[test]
fn editor_settings_cannot_open_during_another_mouse_transaction() {
    let mut app = app();
    let numeric = app.world_mut().spawn_empty().id();
    app.world_mut()
        .resource_mut::<ActiveNumericInputScrub>()
        .entity = Some(numeric);
    app.update();
    click(&mut app, EditorSettingsAction::Open);
    assert!(!app.world().resource::<MapEditorSettingsState>().open);
}

#[test]
fn editor_settings_release_only_editor_text_focus() {
    for editor_field in [true, false] {
        let mut app = app();
        let field = if editor_field {
            app.world_mut().spawn(EditorTextInput).id()
        } else {
            app.world_mut().spawn_empty().id()
        };
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Navigated);
        click(&mut app, EditorSettingsAction::Open);
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            (!editor_field).then_some(field)
        );
    }
}

#[test]
fn editor_settings_external_close_hides_modal_and_keeps_preference() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::Open);
    click(&mut app, EditorSettingsAction::SetScale(75));
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    app.update();
    assert!(!app.world().resource::<MapEditorSettingsState>().open);
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Node, With<MapEditorSettingsPanel>>()
            .single(world)
            .unwrap()
            .display,
        Display::None
    );
    assert_eq!(world.resource::<EditorSettings>().editor_ui_scale, 0.75);
}

#[test]
fn editor_settings_preset_selection_and_percentage_stay_visible() {
    let mut app = app();
    click(&mut app, EditorSettingsAction::Open);
    click(&mut app, EditorSettingsAction::SetScale(125));
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Text, With<MapEditorSettingsScaleText>>()
            .single(world)
            .unwrap()
            .as_str(),
        "125%"
    );
    let mut buttons = world.query::<(&MapEditorSettingsButton, &BackgroundColor, &BorderColor)>();
    let (_, background, border) = buttons
        .iter(world)
        .find(|(button, _, _)| button.0 == EditorSettingsAction::SetScale(125))
        .unwrap();
    assert_eq!(background.0, colors::SELECT_BG);
    assert_eq!(
        *border,
        BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
    );
    // 默认背面显示可能已开启，先通过原动作关闭，确保下次点击验证选中态。
    if app.world().resource::<EditorSettings>().editor_show_terrain_backfaces {
        click(&mut app, EditorSettingsAction::ToggleTerrainBackfaces);
    }
    // 主题和视口开关同样保留选中填充，不可再把弱轮廓独自改成高亮框。
    for action in [
        EditorSettingsAction::SetColorTheme(EditorColorTheme::GraphiteBlue),
        EditorSettingsAction::ToggleTerrainBackfaces,
    ] {
        click(&mut app, action);
        let world = app.world_mut();
        let mut buttons = world.query::<(&MapEditorSettingsButton, &BackgroundColor, &BorderColor)>();
        let (_, background, border) = buttons
            .iter(world)
            .find(|(button, _, _)| button.0 == action)
            .unwrap();
        assert_eq!(
            background.0,
            feathrise_editor_ui::dialog_button_style(true).colors.normal
        );
        assert_eq!(
            *border,
            BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
        );
    }
}

fn scroll_app(inverse_scale: f32) -> (App, Entity) {
    let mut app = app();
    app.add_message::<MouseWheel>()
        .add_systems(Update, scroll_settings_body.after(handle_settings_input));
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .open = true;
    let body = app
        .world_mut()
        .spawn((
            SettingsBody,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::splat(0.5)),
            },
            ComputedNode {
                size: Vec2::new(100.0, 100.0),
                content_size: Vec2::new(100.0, 500.0),
                inverse_scale_factor: inverse_scale,
                ..default()
            },
            ScrollPosition::default(),
        ))
        .id();
    app.update();
    (app, body)
}

fn scroll_wheel(app: &mut App, amount: f32, unit: MouseScrollUnit) {
    let window = app.world_mut().spawn_empty().id();
    app.world_mut().write_message(MouseWheel {
        x: 0.0,
        y: amount,
        unit,
        window,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    app.update();
}

#[test]
fn editor_settings_scroll_converts_pixels_with_ui_and_dpi_and_only_uses_hovered_body() {
    let (mut app, body) = scroll_app(0.25);
    scroll_wheel(&mut app, -1.0, MouseScrollUnit::Line);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 24.0);
    scroll_wheel(&mut app, -40.0, MouseScrollUnit::Pixel);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 34.0);
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(body)
        .unwrap()
        .cursor_over = false;
    scroll_wheel(&mut app, -1.0, MouseScrollUnit::Line);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 34.0);
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(body)
        .unwrap()
        .cursor_over = true;
    scroll_wheel(&mut app, -1000.0, MouseScrollUnit::Pixel);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 100.0);
    scroll_wheel(&mut app, 1000.0, MouseScrollUnit::Line);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 0.0);
}

#[test]
fn editor_settings_scroll_preserves_preference_edits_but_clamps_when_content_shrinks() {
    let (mut app, body) = scroll_app(1.0);
    scroll_wheel(&mut app, -3.0, MouseScrollUnit::Line);
    let changed = app
        .world()
        .get_resource_ref::<EditorSettings>()
        .unwrap()
        .last_changed();
    app.update();
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorSettings>()
            .unwrap()
            .last_changed(),
        changed
    );
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_color_theme = EditorColorTheme::WarmCopper;
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.5;
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 72.0);
    app.world_mut()
        .get_mut::<ComputedNode>(body)
        .unwrap()
        .content_size
        .y = 120.0;
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 20.0);
}

#[test]
fn editor_settings_close_shortcut_page_and_reopen_never_replay_old_wheel() {
    let (mut app, body) = scroll_app(1.0);
    scroll_wheel(&mut app, -2.0, MouseScrollUnit::Line);
    assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 48.0);
    for page in 0..3 {
        match page {
            0 => {
                app.world_mut()
                    .resource_mut::<MapEditorSettingsState>()
                    .open = false
            }
            1 => {
                app.world_mut()
                    .resource_mut::<MapEditorSettingsState>()
                    .shortcuts = true
            }
            _ => app.world_mut().resource_mut::<MapEditorState>().open = false,
        }
        scroll_wheel(&mut app, -100.0, MouseScrollUnit::Line);
        assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 0.0);
        app.world_mut().resource_mut::<MapEditorState>().open = true;
        app.world_mut()
            .resource_mut::<MapEditorSettingsState>()
            .open = true;
        app.world_mut()
            .resource_mut::<MapEditorSettingsState>()
            .shortcuts = false;
        app.update();
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 0.0);
    }
}

#[test]
fn editor_settings_reset_is_not_a_selected_preset_and_close_keeps_feedback() {
    let mut app = app();
    let utility = app
        .world_mut()
        .spawn((
            SettingsUtilityButton,
            MapEditorSettingsButton(EditorSettingsAction::SetScale(100)),
            UiInteraction::None,
            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
            BorderColor::all(colors::COPPER_BORDER_WEAK),
        ))
        .id();
    let close = app
        .world_mut()
        .spawn((
            SettingsUtilityButton,
            MapEditorSettingsButton(EditorSettingsAction::Close),
            UiInteraction::Hovered,
            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
            BorderColor::all(colors::COPPER_BORDER_WEAK),
        ))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<BackgroundColor>(utility).unwrap().0,
        colors::BUTTON_DEV_SMALL_NORMAL
    );
    assert_eq!(
        app.world().get::<BorderColor>(utility).unwrap(),
        &BorderColor::all(colors::COPPER_BORDER_WEAK)
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(close).unwrap().0,
        colors::BUTTON_DEV_SMALL_HOVERED
    );
}

#[test]
fn editor_settings_root_releases_focus_and_stable_labels_are_not_rewritten() {
    let mut app = app();
    let root = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<MapEditorSettingsPanel>>()
            .single(world)
            .unwrap()
    };
    app.world_mut().entity_mut(root).insert(FocusPolicy::Pass);
    let theme = app
        .world_mut()
        .spawn((SettingsThemeText, Text::new("")))
        .id();
    click(&mut app, EditorSettingsAction::Open);
    assert_eq!(
        app.world().get::<FocusPolicy>(root),
        Some(&FocusPolicy::Block)
    );
    let changed = app
        .world()
        .entity(theme)
        .get_ref::<Text>()
        .unwrap()
        .last_changed();
    app.world_mut()
        .resource_mut::<ApplicationPreferences>()
        .mouse_sensitivity = 0.007;
    app.update();
    assert_eq!(
        app.world()
            .entity(theme)
            .get_ref::<Text>()
            .unwrap()
            .last_changed(),
        changed
    );
    click(&mut app, EditorSettingsAction::Close);
    assert_eq!(
        app.world().get::<FocusPolicy>(root),
        Some(&FocusPolicy::Pass)
    );
}
