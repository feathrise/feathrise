//! 常驻游戏模式交接时在官方UI分派前消费作者键鼠；稳定原生预览继续允许作者操作。

use super::EmbeddedAuthorInput;
use bevy::{
    input::{
        InputSystems,
        keyboard::{Key, KeyboardInput},
        mouse::{
            AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButtonInput, MouseMotion,
            MouseWheel,
        },
        touch::{TouchInput, Touches},
    },
    input_focus::InputFocusSystems,
    prelude::*,
    window::Ime,
};

pub(super) fn install(app: &mut App) {
    app.init_resource::<EmbeddedAuthorInput>().add_systems(
        PreUpdate,
        consume_author_input
            .after(InputSystems)
            .before(bevy::ui::UiSystems::Focus)
            .before(bevy::picking::PickingSystems::ProcessInput)
            .before(InputFocusSystems::Dispatch),
    );
}

fn consume_author_input(
    world: &mut World,
    mut keyboard_events: Local<bevy::ecs::message::MessageCursor<KeyboardInput>>,
) {
    // 跨进程焦点交接会清空ButtonInput；OS长按重复随后可能被误判为新的just_pressed。
    // 在所有原F12消费者之前去掉这个伪边沿，真实首次按下/同帧释放与原输入门仍保持。
    let (mut repeated_f12, mut fresh_f12) = (false, false);
    if let Some(events) = world.get_resource::<Messages<KeyboardInput>>() {
        for event in keyboard_events.read(events) {
            if event.key_code == KeyCode::F12 && event.state.is_pressed() {
                repeated_f12 |= event.repeat;
                fresh_f12 |= !event.repeat;
            }
        }
    }
    if repeated_f12
        && !fresh_f12
        && let Some(mut keys) = world.get_resource_mut::<ButtonInput<KeyCode>>()
    {
        keys.clear_just_pressed(KeyCode::F12);
    }
    if !world.resource::<EmbeddedAuthorInput>().blocked {
        return;
    }
    if let Some(mut keys) = world.get_resource_mut::<ButtonInput<KeyCode>>() {
        // 只复制三个小型边沿标记；F12返回编辑必须继续通过原入口，不能伪造一次新按下。
        let pressed = keys.pressed(KeyCode::F12);
        let down = keys.just_pressed(KeyCode::F12);
        let up = keys.just_released(KeyCode::F12);
        keys.reset_all();
        if up {
            keys.press(KeyCode::F12);
            keys.release(KeyCode::F12);
        }
        if pressed {
            keys.press(KeyCode::F12);
        }
        if !down {
            keys.clear_just_pressed(KeyCode::F12);
        }
    }
    if let Some(mut keys) = world.get_resource_mut::<ButtonInput<Key>>() {
        keys.reset_all();
    }
    if let Some(mut mouse) = world.get_resource_mut::<ButtonInput<MouseButton>>() {
        mouse.reset_all();
    }
    if let Some(mut touches) = world.get_resource_mut::<Touches>() {
        touches.reset_all();
    }
    if let Some(mut motion) = world.get_resource_mut::<AccumulatedMouseMotion>() {
        motion.delta = Vec2::ZERO;
    }
    if let Some(mut scroll) = world.get_resource_mut::<AccumulatedMouseScroll>() {
        scroll.delta = Vec2::ZERO;
    }
    // Bevy 数值框和 EditableText 也消费原消息；只清 ButtonInput 会让打字/粘贴改动继续泄漏。
    // 模式请求前已核对现有组合/拖动收口，此处不改焦点或正文，解除后仍保留作者现场。
    clear::<KeyboardInput>(world);
    clear::<Ime>(world);
    clear::<MouseMotion>(world);
    clear::<MouseWheel>(world);
    clear::<MouseButtonInput>(world);
    clear::<TouchInput>(world);
    // 官方文本选择/滑条使用 picking 的事件旁路，该队列在 First 已生成，也须在派发前消费。
    clear::<bevy::picking::pointer::PointerInput>(world);
}

fn clear<T: Message>(world: &mut World) {
    if let Some(mut messages) = world.get_resource_mut::<Messages<T>>() {
        messages.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::ButtonState;

    #[derive(Component, Default)]
    struct Typed(String);

    #[test]
    fn real_f12_edges_survive_handoff_but_os_repeats_never_request_play_or_stop() {
        use crate::{
            authoring::{EditorAuthoringSession, WaterNetworkEditState},
            resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
            standalone::{RunSavedProject, StandaloneEditorMode},
        };
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
            .init_resource::<SceneEditorNumericScrubMouseGuard>()
            .init_resource::<WaterNetworkEditState>()
            .init_resource::<feathrise_scene_core::input::InputGateState>()
            .init_resource::<feathrise_scene_content::ActiveDynamicScene>()
            .init_resource::<EditorAuthoringSession>()
            .init_resource::<StandaloneEditorMode>()
            .insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Idle))
            .insert_resource(SceneEditorState {
                open: true,
                dirty: true,
                ..default()
            })
            .add_message::<RunSavedProject>()
            .add_systems(
                Update,
                (
                    crate::ui::systems::toggle_scene_editor,
                    super::super::systems::publish_saved_play,
                )
                    .chain(),
            );
        install(&mut app);
        let mut requests = bevy::ecs::message::MessageCursor::<RunSavedProject>::default();
        let key = |state, repeat| KeyboardInput {
            key_code: KeyCode::F12,
            logical_key: Key::F12,
            state,
            text: None,
            repeat,
            window: Entity::PLACEHOLDER,
        };
        app.world_mut()
            .write_message(key(ButtonState::Pressed, false));
        app.update();
        let first: Vec<_> = requests
            .read(app.world().resource::<Messages<RunSavedProject>>())
            .collect();
        assert_eq!(first.len(), 1);
        assert!(!first[0].stop_only, "旧模式真实F12仍请求运行磁盘工程");

        app.world_mut()
            .resource_mut::<StandaloneEditorMode>()
            .set_external_play_active(true);
        for blocked in [true, false] {
            // 模拟交接与归还焦点均已清空按键；重复消息不能重新形成播放/停止意图。
            app.world_mut()
                .resource_mut::<EmbeddedAuthorInput>()
                .set_blocked(blocked);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.world_mut()
                .write_message(key(ButtonState::Pressed, true));
            app.update();
            assert_eq!(
                requests
                    .read(app.world().resource::<Messages<RunSavedProject>>())
                    .count(),
                0
            );
            assert!(
                !app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .just_pressed(KeyCode::F12)
            );
        }
        app.world_mut()
            .resource_mut::<EmbeddedAuthorInput>()
            .set_blocked(true);
        app.world_mut()
            .write_message(key(ButtonState::Released, false));
        app.update();
        app.world_mut()
            .write_message(key(ButtonState::Pressed, false));
        app.world_mut()
            .write_message(key(ButtonState::Released, false));
        app.update();
        let returned: Vec<_> = requests
            .read(app.world().resource::<Messages<RunSavedProject>>())
            .collect();
        assert_eq!(returned.len(), 1);
        assert!(
            returned[0].stop_only,
            "占用作者输入时真实新F12仍经原入口请求返回"
        );
        assert!(app.world().resource::<SceneEditorState>().dirty);
    }

    fn collect_text(
        event: On<bevy::input_focus::FocusedInput<KeyboardInput>>,
        mut fields: Query<&mut Typed>,
    ) {
        if let Ok(mut field) = fields.get_mut(event.focused_entity)
            && let Some(text) = &event.input.text
        {
            field.0.push_str(text);
        }
    }

    #[test]
    fn official_focused_input_is_consumed_before_dispatch_and_cannot_replay_after_handoff() {
        let mut app = App::new();
        app.add_plugins((
            bevy::input::InputPlugin,
            bevy::input_focus::InputDispatchPlugin,
        ))
        .init_resource::<bevy::input_focus::InputFocus>()
        .add_observer(collect_text);
        install(&mut app);
        // 官方派发即使已有表单焦点，也要求唯一主窗口作为冒泡终点；只创建ECS窗口，不启动Winit。
        let window = app
            .world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow))
            .id();
        let field = app.world_mut().spawn(Typed::default()).id();
        app.world_mut()
            .resource_mut::<bevy::input_focus::InputFocus>()
            .set(field, bevy::input_focus::FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<EmbeddedAuthorInput>()
            .set_blocked(true);
        let key = KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: Key::Character("a".into()),
            state: ButtonState::Pressed,
            text: Some("a".into()),
            repeat: false,
            window,
        };
        app.world_mut().write_message(key.clone());
        app.update();
        assert!(app.world().get::<Typed>(field).unwrap().0.is_empty());
        assert_eq!(
            app.world()
                .resource::<bevy::input_focus::InputFocus>()
                .get(),
            Some(field)
        );
        app.world_mut()
            .resource_mut::<EmbeddedAuthorInput>()
            .set_blocked(false);
        app.update();
        assert!(app.world().get::<Typed>(field).unwrap().0.is_empty());
        app.world_mut().write_message(key);
        app.update();
        assert_eq!(app.world().get::<Typed>(field).unwrap().0, "a");
    }

    #[test]
    fn handoff_consumes_edit_input_without_forging_or_losing_f12_edges() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<KeyboardInput>()
            .add_message::<Ime>();
        install(&mut app);
        app.world_mut()
            .resource_mut::<EmbeddedAuthorInput>()
            .set_blocked(true);
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::Delete);
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::F12);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: Key::Character("a".into()),
            state: ButtonState::Pressed,
            text: Some("a".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.world_mut().write_message(Ime::Commit {
            window: Entity::PLACEHOLDER,
            value: "不应进入作者正文".into(),
        });
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.pressed(KeyCode::F12) && keys.just_pressed(KeyCode::F12));
        assert!(!keys.pressed(KeyCode::Delete) && !keys.pressed(KeyCode::ControlLeft));
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        assert!(app.world().resource::<Messages<KeyboardInput>>().is_empty());
        assert!(app.world().resource::<Messages<Ime>>().is_empty());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.pressed(KeyCode::F12) && !keys.just_pressed(KeyCode::F12));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::F12);
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(!keys.pressed(KeyCode::F12) && keys.just_released(KeyCode::F12));
        app.world_mut()
            .resource_mut::<EmbeddedAuthorInput>()
            .set_blocked(false);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Delete);
        app.update();
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::Delete)
        );
    }
}
