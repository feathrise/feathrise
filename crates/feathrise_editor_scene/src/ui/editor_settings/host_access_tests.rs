//! 无工程偏好交接输入门回归：旧点击/数值提交不重放，关闭保留，原逻辑禁用保持。
use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::*;
use crate::workbench::{WorkbenchHostPagesPlugin, WorkbenchSettingsAccess};
use bevy::{input_focus::InputFocus, ui::InteractionDisabled, window::PrimaryWindow};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputCommitOutcome,
    NumericInputCommitSource, NumericInputCommitted, NumericInputDisabled, NumericInputSetValue,
    NumericInputValue, NumericValue,
};

fn app() -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(SceneEditorState {
            open: true,
            ..default()
        })
        .insert_resource(SceneEditorSettingsState {
            open: true,
            ..default()
        })
        .init_resource::<SceneEditorLayoutState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<ActiveNumericInputScrub>()
        .init_resource::<InputFocus>()
        .init_resource::<UiScale>()
        .add_message::<MouseWheel>()
        .add_message::<NumericInputCommitted>()
        .add_message::<NumericInputSetValue>()
        .add_plugins(WorkbenchHostPagesPlugin);
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    let label = app.world_mut().spawn(TextColor(colors::TEXT_PRIMARY)).id();
    let preset = app
        .world_mut()
        .spawn((
            InteractionButton,
            SceneEditorSettingsButton(EditorSettingsAction::SetScale(150)),
            EditorButtonClick::default(),
            BackgroundColor::default(),
            BorderColor::default(),
        ))
        .id();
    app.world_mut().entity_mut(preset).add_child(label);
    let original_disabled = app
        .world_mut()
        .spawn((
            InteractionButton,
            SceneEditorSettingsButton(EditorSettingsAction::SetScale(125)),
            EditorButtonClick::default(),
            InteractionDisabled,
        ))
        .id();
    let close = app
        .world_mut()
        .spawn((
            InteractionButton,
            SceneEditorSettingsButton(EditorSettingsAction::Close),
            EditorButtonClick::default(),
        ))
        .id();
    app.update();
    (app, preset, original_disabled, close)
}

#[test]
fn host_settings_access_discards_blocked_and_resume_clicks_and_preserves_other_disables() {
    let (mut app, preset, original_disabled, _) = app();
    assert!(app.world().resource::<WorkbenchSettingsAccess>().editable);
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = false;
    app.world_mut()
        .entity_mut(preset)
        .insert((EditorButtonClick::triggered(), UiInteraction::Pressed));
    app.update();
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_ui_scale,
        1.0
    );
    assert!(app.world().get::<InteractionDisabled>(preset).is_some());
    assert!(
        !app.world()
            .get::<EditorButtonClick>(preset)
            .unwrap()
            .is_clicked()
    );
    assert_eq!(
        app.world().get::<UiInteraction>(preset),
        Some(&UiInteraction::None)
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(preset).unwrap().0,
        feathrise_editor_ui::colors::DIALOG_BG
    );

    // 恢复首帧即使有旧按下残留，也必须先取消，不能借新偏好所有权重放旧动作。
    app.world_mut()
        .entity_mut(preset)
        .insert(EditorButtonClick::triggered());
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = true;
    app.update();
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_ui_scale,
        1.0
    );
    assert!(app.world().get::<InteractionDisabled>(preset).is_none());
    assert!(
        app.world()
            .get::<InteractionDisabled>(original_disabled)
            .is_some()
    );
    app.world_mut()
        .entity_mut(preset)
        .insert(EditorButtonClick::triggered());
    app.update();
    app.world_mut()
        .entity_mut(preset)
        .insert(EditorButtonClick::default());
    app.update();
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_ui_scale,
        1.5
    );
    assert_eq!(app.world().resource::<UiScale>().0, 1.5);
}

#[test]
fn host_settings_access_keeps_close_and_escape_available() {
    let (mut app, _, _, close) = app();
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = false;
    app.world_mut()
        .entity_mut(close)
        .insert(EditorButtonClick::triggered());
    app.update();
    assert!(!app.world().resource::<SceneEditorSettingsState>().open);
    assert!(app.world().get::<InteractionDisabled>(close).is_none());
    app.world_mut()
        .entity_mut(close)
        .insert(EditorButtonClick::default());
    app.world_mut()
        .resource_mut::<SceneEditorSettingsState>()
        .open = true;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<SceneEditorSettingsState>().open);
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_ui_scale,
        1.0
    );
}

#[test]
fn host_settings_access_cancels_shortcut_capture_without_changing_bindings() {
    let (mut app, _, _, _) = app();
    let before = app
        .world()
        .resource::<EditorSettings>()
        .editor_keybindings
        .clone();
    {
        let mut modal = app.world_mut().resource_mut::<SceneEditorSettingsState>();
        modal.shortcuts = true;
        modal.listening = Some(feathrise_editor_core::EditorShortcutAction::FrameScene);
    }
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = false;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyK);
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorSettingsState>()
            .listening
            .is_none()
    );
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_keybindings,
        before
    );
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = true;
    app.update();
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_keybindings,
        before
    );
}

#[test]
fn host_settings_access_drains_snap_and_lod_commits_before_restoring_controls() {
    let (mut app, _, _, _) = app();
    let snap = app
        .world_mut()
        .spawn((
            snapping::SnapField::Translation,
            NumericInputValue(NumericValue::F32(1.0)),
        ))
        .id();
    let lod_default = app
        .world()
        .resource::<EditorSettings>()
        .render
        .mesh_lod
        .max_error_pixels;
    let lod = app
        .world_mut()
        .spawn((
            mesh_lod::MeshLodField::ScreenError,
            NumericInputValue(NumericValue::F32(lod_default)),
        ))
        .id();
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = false;
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(snap);
    for (entity, value) in [(snap, 0.125), (lod, 3.0)] {
        app.world_mut().write_message(NumericInputCommitted {
            entity,
            value: NumericValue::F32(value),
            outcome: NumericInputCommitOutcome::Accepted,
            source: NumericInputCommitSource::Text,
            changed: true,
            edited: true,
        });
    }
    app.update();
    assert_eq!(
        app.world()
            .resource::<EditorSettings>()
            .editor_snapping
            .translation_meters,
        1.0
    );
    assert_eq!(
        app.world()
            .resource::<EditorSettings>()
            .render
            .mesh_lod
            .max_error_pixels,
        lod_default
    );
    assert!(app.world().get::<NumericInputDisabled>(snap).is_some());
    assert!(app.world().get::<NumericInputDisabled>(lod).is_some());
    let values = app.world().resource::<Messages<NumericInputSetValue>>();
    assert!(
        values
            .get_cursor()
            .read(values)
            .any(|event| event.entity == snap && event.cancel_edit),
        "交接应向原数值控件发送取消，不能仅把输入画成灰色"
    );
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = true;
    app.update(); // 消息仍在Bevy保留窗口内；正式消费者已读过，不能在恢复后采用。
    assert_eq!(
        app.world()
            .resource::<EditorSettings>()
            .editor_snapping
            .translation_meters,
        1.0
    );
    assert_eq!(
        app.world()
            .resource::<EditorSettings>()
            .render
            .mesh_lod
            .max_error_pixels,
        lod_default
    );
    assert!(app.world().get::<NumericInputDisabled>(snap).is_none());
    assert!(app.world().get::<NumericInputDisabled>(lod).is_none());

    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = false;
    app.update();
    app.world_mut()
        .resource_mut::<SceneEditorSettingsState>()
        .shortcuts = true;
    app.world_mut()
        .resource_mut::<WorkbenchSettingsAccess>()
        .editable = true;
    app.update();
    assert!(
        app.world().get::<NumericInputDisabled>(snap).is_some(),
        "原隐藏页逻辑仍应禁用"
    );
    assert!(app.world().get::<NumericInputDisabled>(lod).is_some());
}
