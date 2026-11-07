//! 公共数值协议进入设置模态：提交/拒绝/取消、字段隔离及Esc归属，不写真实settings.json。

use super::*;
use crate::resources::MapEditorNumericScrubMouseGuard;
use crate::ui::editor_settings::handle_settings_input;
use crate::{components::MapEditorSettingsButton, types::EditorSettingsAction};
use bevy::ecs::system::RunSystemOnce;
use feathrise_editor_ui::{
    EditorUiPlugin, EditorUiSystemSet, NumericInputCommitSource, NumericInputEditState,
};

fn app() -> (App, [Entity; 3]) {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<EditorSettings>()
        .init_resource::<crate::resources::MapEditorLayoutState>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .insert_resource(MapEditorSettingsState {
            open: true,
            ..default()
        })
        .add_plugins(EditorUiPlugin)
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn_empty().with_children(|parent| {
                for field in [
                    SnapField::Translation,
                    SnapField::Rotation,
                    SnapField::Scale,
                ] {
                    let (label, unit, min, max, step) = field.config();
                    // 复用设置页的双语控件，覆盖实际吸附字段的数值输入协议。
                    spawn_localized_numeric_scrub_field(
                        parent,
                        field,
                        default(),
                        NumericScrubFieldOptions::new(
                            label,
                            unit,
                            NumericValue::F32(field.value(default())),
                            NumericInputSpec::f32(3)
                                .with_min(NumericValue::F32(min))
                                .with_max(NumericValue::F32(max)),
                            NumericInputScrubConfig::linear(step, step, min, max, 1.0),
                        ),
                    );
                }
            });
        })
        .add_systems(
            Update,
            (handle_settings_input, handle_snap_inputs)
                .chain()
                .after(EditorUiSystemSet::NumericInput),
        )
        .add_systems(PostUpdate, sync_snap_inputs);
    app.update();
    let world = app.world_mut();
    let mut ids = [Entity::PLACEHOLDER; 3];
    for (entity, field) in world.query::<(Entity, &SnapField)>().iter(world) {
        ids[match field {
            SnapField::Translation => 0,
            SnapField::Rotation => 1,
            SnapField::Scale => 2,
        }] = entity;
    }
    (app, ids)
}

fn commit(app: &mut App, entity: Entity, value: f32, outcome: NumericInputCommitOutcome) {
    app.world_mut().write_message(NumericInputCommitted {
        entity,
        value: NumericValue::F32(value),
        outcome,
        source: NumericInputCommitSource::Text,
        changed: true,
        edited: true,
    });
    app.update();
}

#[test]
fn snap_modal_commits_one_field_rejects_bad_events_and_reset_preserves_other_preferences() {
    let (mut app, fields) = app();
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_ui_scale = 1.25;
    commit(
        &mut app,
        fields[0],
        0.125,
        NumericInputCommitOutcome::Accepted,
    );
    assert_eq!(
        app.world()
            .resource::<EditorSettings>()
            .editor_snapping
            .translation_meters,
        0.125
    );
    for (value, outcome) in [
        (90.0, NumericInputCommitOutcome::Canceled),
        (45.0, NumericInputCommitOutcome::Rejected),
        (f32::NAN, NumericInputCommitOutcome::Accepted),
        (-1.0, NumericInputCommitOutcome::Accepted),
    ] {
        commit(&mut app, fields[1], value, outcome);
        assert_eq!(
            app.world()
                .resource::<EditorSettings>()
                .editor_snapping
                .rotation_degrees,
            15.0
        );
    }
    commit(
        &mut app,
        fields[2],
        0.25,
        NumericInputCommitOutcome::Accepted,
    );
    app.world_mut().spawn((
        MapEditorSettingsButton(EditorSettingsAction::ResetSnapping),
        feathrise_editor_ui::EditorButtonClick::triggered(),
        Interaction::Pressed,
    ));
    app.update();
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_snapping,
        EditorSnapSettings::default()
    );
    assert_eq!(app.world().resource::<EditorSettings>().editor_ui_scale, 1.25);
    assert!(!app.world().resource::<MapEditorState>().dirty);
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .shortcuts = true;
    commit(
        &mut app,
        fields[0],
        50.0,
        NumericInputCommitOutcome::Accepted,
    );
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_snapping,
        EditorSnapSettings::default()
    );
}

#[test]
fn snap_precision_escape_only_cancels_number_then_next_escape_closes_modal() {
    let (mut app, fields) = app();
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(fields[0]);
    {
        let mut edit = app
            .world_mut()
            .get_mut::<NumericInputEditState>(fields[0])
            .unwrap();
        edit.editing = true;
        edit.buffer = "25".into();
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(app.world().resource::<MapEditorSettingsState>().open);
    assert!(
        app.world()
            .resource::<ActiveNumericInput>()
            .entity
            .is_none()
    );
    assert_eq!(
        app.world().resource::<EditorSettings>().editor_snapping,
        EditorSnapSettings::default()
    );
    {
        let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keyboard.release(KeyCode::Escape);
        keyboard.clear();
        keyboard.press(KeyCode::Escape);
    }
    app.update();
    assert!(!app.world().resource::<MapEditorSettingsState>().open);
}

#[test]
fn snap_scrub_escape_requests_restore_and_keeps_modal_open() {
    let (mut app, fields) = app();
    app.world_mut()
        .resource_mut::<ActiveNumericInputScrub>()
        .entity = Some(fields[0]);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut()
        .run_system_once(handle_settings_input)
        .unwrap();
    assert!(app.world().resource::<MapEditorSettingsState>().open);
    let mut cursor = bevy::ecs::message::MessageCursor::<NumericInputSetValue>::default();
    assert!(
        cursor
            .read(app.world().resource::<Messages<NumericInputSetValue>>())
            .any(|event| event.entity == fields[0]
                && event.cancel_edit
                && event.value == NumericValue::F32(1.0))
    );
}
