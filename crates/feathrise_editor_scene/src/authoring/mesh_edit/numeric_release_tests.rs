//! 正式专业数字拖动与建模输入同帧运行；数字松键后仍需经过公共释放静默门。

use feathrise_editor_ui::UiInteraction;
use super::*;
use bevy::{ecs::world::CommandQueue, ui::RelativeCursorPosition};
use crate::resources::SceneEditorNumericScrubMouseGuard;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, EditorUiPlugin, EditorUiSystemSet,
    NumericInputScrubConfig, NumericInputSpec, NumericInputValue, NumericScrubFieldOptions,
    NumericValue, spawn_numeric_scrub_field,
};

fn numeric_app() -> (App, Entity, Entity) {
    let (mut app, _, node) = app();
    app.add_plugins(EditorUiPlugin)
        .configure_sets(Update, EditorUiSystemSet::NumericInput.before(super::super::MeshEditInput));
    let parent = app.world_mut().spawn((Node::default(), InheritedVisibility::VISIBLE)).id();
    let mut queue = CommandQueue::default();
    let mut commands = Commands::new(&mut queue, app.world());
    let mut field = Entity::PLACEHOLDER;
    commands.entity(parent).with_children(|parent| {
        field = spawn_numeric_scrub_field(
            parent, Name::new("与对象选择并列的专业数字字段"), Handle::default(),
            NumericScrubFieldOptions::new("数值", "米", NumericValue::F32(1.0),
                NumericInputSpec::f32(3),
                NumericInputScrubConfig::linear(0.1, 0.001, -100.0, 100.0, 1.0)),
        );
    });
    queue.apply(app.world_mut());
    app.world_mut().entity_mut(field).insert((
        InheritedVisibility::VISIBLE,
        ComputedNode { size: Vec2::new(100.0, 26.0), inverse_scale_factor: 1.0, ..default() },
        UiInteraction::Hovered,
        RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::ZERO) },
    )); // 无窗口夹具只提供正式命中与布局结果；拖动、松键及公共所有权均由生产系统推进。
    app.world_mut().get_mut::<RelativeCursorPosition>(node).unwrap().cursor_over = false;
    (app, field, node)
}

#[test]
fn real_numeric_scrub_release_rejects_mesh_tab_until_the_complete_quiet_frame() {
    for button in [MouseButton::Left, MouseButton::Right] {
        let (mut app, field, node) = numeric_app();
        let before = current(&app).clone();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(button);
        fixture::step(&mut app, node, Vec2::ZERO);
        assert_eq!(app.world().resource::<ActiveNumericInputScrub>().entity, Some(field));
        assert!(app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
        app.world_mut().get_mut::<RelativeCursorPosition>(field).unwrap().normalized = Some(Vec2::new(0.48, 0.0));
        fixture::step(&mut app, node, Vec2::ZERO);
        if button == MouseButton::Left {
            assert_eq!(app.world().get::<NumericInputValue>(field).unwrap().0, NumericValue::F32(5.8));
        } else {
            assert_eq!(app.world().get::<NumericInputScrubConfig>(field).unwrap().sensitivity, 2.0);
        }

        // 真实数字系统会在本帧清空ActiveNumericInputScrub；Tab仍必须让位给同次物理松键。
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(button);
        press(&mut app, KeyCode::Tab);
        fixture::step(&mut app, node, Vec2::ZERO);
        assert!(app.world().resource::<ActiveNumericInputScrub>().entity.is_none());
        assert!(app.world().resource::<ActiveNumericInput>().entity.is_none());
        assert!(app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
        assert!(!app.world().resource::<MeshEditState>().active(), "{button:?}数字松键帧的Tab不能抢占建模输入");
        assert_eq!(current(&app), &before);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(!app.world().resource::<SceneEditorState>().dirty);

        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Tab);
        fixture::step(&mut app, node, Vec2::ZERO);
        assert!(!app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked(), "真实完整静默帧归还输入");
        press(&mut app, KeyCode::Tab);
        fixture::step(&mut app, node, Vec2::ZERO);
        assert!(app.world().resource::<MeshEditState>().active(), "静默帧后的新Tab正常进入网格编辑");
        assert_eq!(current(&app), &before);
    }
}
