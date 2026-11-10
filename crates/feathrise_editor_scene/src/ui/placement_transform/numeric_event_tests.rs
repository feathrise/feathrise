//! 实体鼠标与Esc经过共享数值控件后，由原资产数值业务负责草稿、可视根和历史收口。

use feathrise_editor_ui::UiInteraction;
use super::*;
use bevy::{camera::RenderTarget, ecs::message::MessageUpdateSystems,
    input::{InputPlugin, ButtonState, keyboard::{Key, KeyboardInput}, mouse::MouseButtonInput},
    picking::input::mouse_pick_events, ui::{UiStack, RelativeCursorPosition},
    window::{CursorMoved, PrimaryWindow, WindowEvent, WindowRef}};
use feathrise_editor_ui::{ActiveNumericInputScrub, EditorUiPlugin, NumericInputRoot, NumericInputValue,
    NumericInputSpec, NumericInputScrubConfig, NumericScrubFieldOptions};

fn fixture() -> (App, Entity, Entity, Entity) {
    let (mut app, old_input, object) = app(PlacementTransformField::TranslationX);
    app.world_mut().despawn(old_input);
    let initial_transform = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        let record = &draft.placements.placements[0];
        let bounds = crate::authoring::placement_record_bounds(draft, record).unwrap();
        record.validated_transform(&draft.scene_id, bounds).unwrap()
    };
    // 原移动夹具只初始化位置；这里测试完整恢复，起点必须与权威记录的旋转及非等比缩放一致。
    app.world_mut().entity_mut(object).insert(initial_transform);
    // 数值字段位于右面板，不能同时让旧移动夹具视口声称鼠标在中央手柄上。
    let nodes: Vec<_> = app.world_mut().query_filtered::<Entity, With<crate::components::SceneEditorViewport>>()
        .iter(app.world()).collect();
    for node in nodes { app.world_mut().entity_mut(node).insert(RelativeCursorPosition::default()); }
    let value = record(&app).translation[0];
    app.add_plugins((InputPlugin, EditorUiPlugin))
        .add_message::<WindowEvent>()
        .add_systems(First, mouse_pick_events.after(MessageUpdateSystems))
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn((Node::default(), InheritedVisibility::VISIBLE)).with_children(|parent| {
                feathrise_editor_ui::spawn_numeric_scrub_field(parent, PlacementTransformField::TranslationX,
                    Handle::default(), NumericScrubFieldOptions::new("X", "m", NumericValue::F32(value),
                        NumericInputSpec::f32(3), NumericInputScrubConfig::linear(0.1, 0.001, -100., 100., 1.)));
            });
        });
    app.update();
    let input = app.world_mut().query_filtered::<Entity, With<NumericInputRoot>>().single(app.world()).unwrap();
    let window = app.world_mut().spawn((Window::default(), PrimaryWindow)).id();
    let camera = app.world_mut().spawn((Camera::default(), RenderTarget::Window(WindowRef::Entity(window)))).id();
    app.world_mut().entity_mut(input).insert((InheritedVisibility::VISIBLE,
        ComputedNode { size: Vec2::new(120., 26.), ..default() }, UiGlobalTransform::from_xy(100., 100.),
        UiTargetCamera(camera), UiInteraction::None, RelativeCursorPosition::default()));
    app.insert_resource(UiStack { partition: vec![0..1], uinodes: vec![input] });
    (app, input, object, window)
}

fn cursor(app: &mut App, window: Entity, x: f32) {
    app.world_mut().write_message(WindowEvent::CursorMoved(CursorMoved {
        window, position: Vec2::new(x, 100.), delta: None,
    }));
}

fn button(app: &mut App, window: Entity, state: ButtonState) {
    let event = MouseButtonInput { window, button: MouseButton::Left, state };
    app.world_mut().write_message(event);
    app.world_mut().write_message(WindowEvent::MouseButtonInput(event));
}

#[test]
fn numeric_scrub_original_placement_same_frame_mouse_drag_is_one_history_entry() {
    let (mut app, input, object, window) = fixture();
    let before = record(&app).clone();
    cursor(&mut app, window, 100.);
    button(&mut app, window, ButtonState::Pressed);
    cursor(&mut app, window, 120.);
    button(&mut app, window, ButtonState::Released);
    app.update();
    assert_eq!(record(&app).translation[0], before.translation[0] + 2.);
    assert_eq!(app.world().get::<NumericInputValue>(input).unwrap().0, NumericValue::F32(before.translation[0] + 2.));
    assert_eq!(app.world().get::<Transform>(object).unwrap().translation.x, before.translation[0] + 2.);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    app.update();
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn numeric_scrub_original_placement_escape_restores_value_dirty_and_history_once() {
    for dirty_before in [false, true] {
        let (mut app, input, object, window) = fixture();
        app.world_mut().resource_mut::<SceneEditorState>().dirty = dirty_before;
        let before = record(&app).clone();
        let transform_before = *app.world().get::<Transform>(object).unwrap();
        let undo_before = app.world().resource::<EditorAuthoringSession>().undo.len();
        let redo_before = app.world().resource::<EditorAuthoringSession>().redo.len();
        let mut messages = app.world().resource::<Messages<NumericInputCommitted>>().get_cursor_current();
        cursor(&mut app, window, 100.);
        button(&mut app, window, ButtonState::Pressed);
        app.update();
        cursor(&mut app, window, 120.);
        app.update();
        assert_eq!(record(&app).translation[0], before.translation[0] + 2.);
        assert!(app.world().resource::<SceneEditorState>().dirty);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), undo_before);
        app.world_mut().write_message(KeyboardInput { window, key_code: KeyCode::Escape,
            logical_key: Key::Escape, state: ButtonState::Pressed, text: None, repeat: false });
        app.update();
        let terminals: Vec<_> = messages.read(app.world().resource::<Messages<NumericInputCommitted>>()).copied().collect();
        assert_eq!(terminals.len(), 1);
        assert_eq!(terminals[0].source, NumericInputCommitSource::Scrub);
        assert_eq!(terminals[0].outcome, NumericInputCommitOutcome::Canceled);
        assert_eq!(record(&app), &before);
        assert_eq!(*app.world().get::<Transform>(object).unwrap(), transform_before);
        assert_eq!(app.world().get::<NumericInputValue>(input).unwrap().0, NumericValue::F32(before.translation[0]));
        assert_eq!(app.world().resource::<SceneEditorState>().dirty, dirty_before);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), undo_before);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().redo.len(), redo_before);
        cursor(&mut app, window, 140.);
        button(&mut app, window, ButtonState::Released);
        app.update();
        assert_eq!(messages.read(app.world().resource::<Messages<NumericInputCommitted>>()).count(), 0);
        assert_eq!(record(&app), &before);
        assert_eq!(app.world().resource::<SceneEditorState>().dirty, dirty_before);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), undo_before);
    }
}

#[test]
fn numeric_scrub_original_placement_same_frame_escape_owns_mouse_until_release_quiet_frame() {
    use crate::resources::SceneEditorNumericScrubMouseGuard;
    use crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard;
    let (mut app, input, _, window) = fixture();
    let before = record(&app).clone();
    let mut messages = app.world().resource::<Messages<NumericInputCommitted>>().get_cursor_current();
    cursor(&mut app, window, 100.);
    button(&mut app, window, ButtonState::Pressed);
    app.world_mut().write_message(KeyboardInput { window, key_code: KeyCode::Escape,
        logical_key: Key::Escape, state: ButtonState::Pressed, text: None, repeat: false });
    app.update();
    // 正式应用在NumericInput后推进此原系统；最小移动夹具未安装整套Input集合，显式按同序执行。
    app.world_mut().run_system_once(update_scene_editor_numeric_scrub_mouse_guard).unwrap();
    assert!(app.world().resource::<ActiveNumericInputScrub>().entity.is_none());
    assert!(app.world().resource::<ActiveNumericInputScrub>().owns_mouse());
    assert!(app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
    let terminals: Vec<_> = messages.read(app.world().resource::<Messages<NumericInputCommitted>>()).copied().collect();
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].outcome, NumericInputCommitOutcome::Canceled);
    cursor(&mut app, window, 250.); // 原键仍按住并已移出字段，不能被其他画笔视作新按压。
    app.update();
    app.world_mut().run_system_once(update_scene_editor_numeric_scrub_mouse_guard).unwrap();
    assert!(app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
    assert_eq!(record(&app), &before);
    assert_eq!(app.world().get::<NumericInputValue>(input).unwrap().0, NumericValue::F32(before.translation[0]));
    button(&mut app, window, ButtonState::Released);
    app.update();
    app.world_mut().run_system_once(update_scene_editor_numeric_scrub_mouse_guard).unwrap();
    assert!(app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
    assert_eq!(messages.read(app.world().resource::<Messages<NumericInputCommitted>>()).count(), 0);
    app.update();
    app.world_mut().run_system_once(update_scene_editor_numeric_scrub_mouse_guard).unwrap();
    assert!(!app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
    assert!(!app.world().resource::<ActiveNumericInputScrub>().owns_mouse());
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}
