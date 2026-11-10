//! 无工程工作台复用原设置按钮与原倍率应用系统；同时核对设置值和真正的UiScale，不写任何配置。
use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::{
    components::{SceneEditorSettingsButton, SceneEditorSettingsScaleText},
    resources::{
        SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard, SceneEditorSettingsState,
        SceneEditorState,
    },
    types::EditorSettingsAction,
    ui::vegetation_preset_editor::VegetationPresetEditorState,
};
use bevy::{input::mouse::MouseWheel, input_focus::InputFocus, window::PrimaryWindow};
use feathrise_editor_core::{EditorSettings, EditorSettingsPlugin};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, EditorButtonClick, NumericInputCommitted,
    NumericInputSetValue,
};

pub(super) fn app(preinstalled: bool) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(SceneEditorState {
            open: true,
            ..default()
        })
        .init_resource::<SceneEditorSettingsState>()
        .init_resource::<SceneEditorLayoutState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<ActiveNumericInputScrub>()
        .init_resource::<InputFocus>()
        .init_resource::<UiScale>()
        .add_message::<MouseWheel>()
        .add_message::<NumericInputCommitted>()
        .add_message::<NumericInputSetValue>()
        .insert_resource(EditorSettings {
            editor_ui_scale: 1.25,
            ..default()
        });
    if preinstalled {
        app.add_plugins(EditorSettingsPlugin);
    }
    // 实际安装生产无工程分支；不在夹具中补倍率系统，以免掩盖漏注册。
    app.add_plugins(WorkbenchHostPagesPlugin);
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: bevy::window::WindowResolution::new(1920, 1080),
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    app.world_mut()
        .spawn((SceneEditorSettingsScaleText, Text::new("100%")));
    for action in [
        EditorSettingsAction::Open,
        EditorSettingsAction::SetScale(75),
        EditorSettingsAction::SetScale(100),
        EditorSettingsAction::SetScale(125),
        EditorSettingsAction::SetScale(150),
        EditorSettingsAction::SetAutoScale,
        EditorSettingsAction::DecreaseScale,
        EditorSettingsAction::IncreaseScale,
    ] {
        app.world_mut().spawn((
            SceneEditorSettingsButton(action),
            EditorButtonClick::default(),
            UiInteraction::None,
            BackgroundColor::default(),
            BorderColor::default(),
        ));
    }
    (app, window)
}

fn click(app: &mut App, action: EditorSettingsAction) {
    let world = app.world_mut();
    let mut buttons = world.query::<(&SceneEditorSettingsButton, &mut EditorButtonClick)>();
    for (button, mut click) in buttons.iter_mut(world) {
        *click = if button.0 == action {
            EditorButtonClick::triggered()
        } else {
            default()
        };
    }
    app.update();
    let world = app.world_mut();
    let mut clicks = world.query::<&mut EditorButtonClick>();
    for mut click in clicks.iter_mut(world) {
        *click = default(); // 原点击脉冲只消费一次，下一帧继续由正式应用系统推进。
    }
    app.update();
}

fn assert_scale(app: &mut App, expected: f32, label: &str) {
    assert!(
        (app.world().resource::<UiScale>().0 - expected).abs() < 0.0001,
        "设置文本改变后，真正UiScale也必须采用倍率"
    );
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Text, With<SceneEditorSettingsScaleText>>()
            .single(world)
            .unwrap()
            .as_str(),
        label
    );
}

#[test]
fn host_settings_percentage_buttons_apply_to_actual_ui_scale_without_scene_plugin() {
    let (mut app, _) = app(false);
    assert!(!app.is_plugin_added::<crate::SceneEditorPlugin>());
    assert_eq!(app.get_added_plugins::<EditorSettingsPlugin>().len(), 1);
    click(&mut app, EditorSettingsAction::Open);
    for (percent, expected) in [(75, 0.75), (100, 1.0), (125, 1.25), (150, 1.5)] {
        click(&mut app, EditorSettingsAction::SetScale(percent));
        let settings = app.world().resource::<EditorSettings>();
        assert_eq!(settings.editor_ui_scale, expected);
        assert!(!settings.editor_ui_scale_auto);
        assert_scale(&mut app, expected, &format!("{percent}%"));
    }
}

#[test]
fn host_auto_scale_and_adjustment_follow_actual_window_dimensions() {
    let (mut app, window) = app(false);
    click(&mut app, EditorSettingsAction::Open);
    click(&mut app, EditorSettingsAction::SetAutoScale);
    assert_scale(&mut app, 1.0, "自动 100%");
    click(&mut app, EditorSettingsAction::IncreaseScale);
    assert_scale(&mut app, 1.05, "自动 105%");
    {
        let settings = app.world().resource::<EditorSettings>();
        assert!(settings.editor_ui_scale_auto);
        assert_eq!(settings.editor_ui_scale, 1.25, "自动微调保留此前手动倍率");
        assert_eq!(settings.editor_ui_scale_auto_offset, 0.05);
    }
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set(1440.0, 810.0);
    app.update();
    assert_scale(&mut app, 0.8, "自动 80%");
    click(&mut app, EditorSettingsAction::DecreaseScale);
    assert_scale(&mut app, 0.75, "自动 75%");
    assert_eq!(
        app.world()
            .resource::<EditorSettings>()
            .editor_ui_scale_auto_offset,
        0.0
    );
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set(800.0, 600.0);
    app.update();
    assert_scale(&mut app, 0.6, "自动 60%");
    click(&mut app, EditorSettingsAction::DecreaseScale);
    assert_scale(&mut app, 0.6, "自动 60%");
    click(&mut app, EditorSettingsAction::IncreaseScale);
    assert_scale(&mut app, 0.65, "自动 65%");
}

#[test]
fn host_settings_reuses_preinstalled_settings_plugin_and_loaded_preferences() {
    let (mut app, _) = app(true);
    assert_eq!(
        app.get_added_plugins::<EditorSettingsPlugin>().len(),
        1,
        "宿主已安装同一设置插件时不得再装第二份"
    );
    click(&mut app, EditorSettingsAction::Open);
    assert_scale(&mut app, 1.25, "125%");
    click(&mut app, EditorSettingsAction::SetScale(150));
    assert_scale(&mut app, 1.5, "150%");
    assert_eq!(app.get_added_plugins::<EditorSettingsPlugin>().len(), 1);
}
