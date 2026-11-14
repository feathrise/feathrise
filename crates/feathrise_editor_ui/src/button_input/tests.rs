//! 共享按钮协议回归：按住、合法释放、移出/换代/失焦取消和同帧快点。

use crate::UiInteraction;
use crate::InteractionButton;
use super::*;
use bevy::{prelude::*, ui::InteractionDisabled, window::PrimaryWindow};

struct Fixture {
    app: App,
    button: Entity,
    other: Entity,
    window: Entity,
}

fn fixture() -> Fixture {
    let mut app = App::new();
    register(&mut app);
    app.init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>();
    // 这里隔离官方命中输出测试协议，真实布局/命中链另由workspace_tabs回归验证。
    let spawn = |world: &mut World| {
        world
            .spawn((
                InteractionButton,
                UiInteraction::None,
                InheritedVisibility::VISIBLE,
                bevy::ui::RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::splat(0.5)),
                },
                ComputedNode {
                    size: Vec2::new(80.0, 26.0),
                    ..default()
                },
            ))
            .id()
    };
    let button = spawn(app.world_mut());
    let other = spawn(app.world_mut());
    let window = app
        .world_mut()
        .spawn((PrimaryWindow, Window::default()))
        .id();
    Fixture {
        app,
        button,
        other,
        window,
    }
}

impl Fixture {
    fn frame(&mut self, interaction: UiInteraction, press: bool, release: bool) {
        *self
            .app
            .world_mut()
            .get_mut::<UiInteraction>(self.button)
            .unwrap() = interaction;
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        if press {
            self.app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
        }
        if release {
            self.app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left);
        }
        self.app.update();
    }
    fn clicked(&self, entity: Entity) -> bool {
        self.app
            .world()
            .get::<EditorButtonClick>(entity)
            .unwrap()
            .is_clicked()
    }
}

#[test]
fn only_original_button_release_emits_one_frame_click() {
    let mut test = fixture();
    test.frame(UiInteraction::Pressed, true, false);
    assert!(!test.clicked(test.button));
    for _ in 0..3 {
        test.frame(UiInteraction::Pressed, false, false);
        assert!(!test.clicked(test.button));
    }
    test.frame(UiInteraction::Hovered, false, true);
    assert!(test.clicked(test.button));
    test.frame(UiInteraction::Hovered, false, false);
    assert!(!test.clicked(test.button));
}

#[test]
fn outside_release_and_press_elsewhere_do_not_activate_hovered_button() {
    let mut test = fixture();
    test.frame(UiInteraction::Pressed, true, false);
    *test
        .app
        .world_mut()
        .get_mut::<UiInteraction>(test.other)
        .unwrap() = UiInteraction::Hovered;
    test.frame(UiInteraction::None, false, true);
    assert!(!test.clicked(test.button) && !test.clicked(test.other));
    test.frame(UiInteraction::None, true, false);
    test.frame(UiInteraction::Hovered, false, true);
    assert!(!test.clicked(test.button));
}

#[test]
fn same_frame_quick_click_is_not_lost_or_repeated() {
    let mut test = fixture();
    test.frame(UiInteraction::Pressed, true, true);
    assert!(test.clicked(test.button));
    test.frame(UiInteraction::None, false, false);
    assert!(!test.clicked(test.button));
}

#[test]
fn same_frame_release_then_press_waits_for_the_new_release() {
    let mut test = fixture();
    test.frame(UiInteraction::Pressed, true, false);
    *test
        .app
        .world_mut()
        .get_mut::<UiInteraction>(test.button)
        .unwrap() = UiInteraction::Pressed;
    let mut mouse = test
        .app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>();
    mouse.clear();
    mouse.release(MouseButton::Left);
    mouse.press(MouseButton::Left);
    test.app.update();
    assert!(
        !test.clicked(test.button),
        "先释放再按下的终态仍按住，不能当作快点"
    );
    test.frame(UiInteraction::Hovered, false, true);
    assert!(test.clicked(test.button));
}

#[test]
fn cursor_controls_outside_release_and_modern_none_can_return_to_the_original_button() {
    let mut test = fixture();
    test.frame(UiInteraction::Pressed, true, false);
    test.app
        .world_mut()
        .get_mut::<bevy::ui::RelativeCursorPosition>(test.button)
        .unwrap()
        .cursor_over = false;
    test.frame(UiInteraction::None, false, false); // 现代拾取移出会清掉三态Pressed，原捕获仍须归同一按钮。
    assert!(!test.clicked(test.button));
    test.frame(UiInteraction::Pressed, false, true);
    assert!(
        !test.clicked(test.button),
        "官方Pressed残留不能让移出松手执行"
    );
    test.app
        .world_mut()
        .get_mut::<bevy::ui::RelativeCursorPosition>(test.button)
        .unwrap()
        .cursor_over = true;
    test.frame(UiInteraction::Pressed, true, false);
    test.app
        .world_mut()
        .get_mut::<bevy::ui::RelativeCursorPosition>(test.button)
        .unwrap()
        .cursor_over = false;
    test.frame(UiInteraction::None, false, false); // 仍持键移出后再移回，不能丢弃原按钮的手势所有权。
    test.app
        .world_mut()
        .get_mut::<bevy::ui::RelativeCursorPosition>(test.button)
        .unwrap()
        .cursor_over = true;
    test.frame(UiInteraction::Hovered, false, true);
    assert!(
        test.clicked(test.button),
        "原按钮按下后移回原按钮松开仍是合法点击"
    );
}

#[test]
fn disabled_hidden_rebound_and_reparented_buttons_cancel_old_capture() {
    for reason in 0..4 {
        let mut test = fixture();
        test.frame(UiInteraction::Pressed, true, false);
        match reason {
            0 => {
                test.app
                    .world_mut()
                    .entity_mut(test.button)
                    .insert(InteractionDisabled);
            }
            1 => {
                test.app
                    .world_mut()
                    .get_mut::<Node>(test.button)
                    .unwrap()
                    .display = Display::None;
            }
            2 => test
                .app
                .world_mut()
                .get_mut::<EditorButtonClick>(test.button)
                .unwrap()
                .invalidate(),
            _ => {
                let parent = test.app.world_mut().spawn_empty().id();
                test.app
                    .world_mut()
                    .entity_mut(test.button)
                    .insert(ChildOf(parent));
            }
        }
        test.frame(UiInteraction::Hovered, false, true);
        assert!(!test.clicked(test.button), "取消原因{reason}不能补发旧点击");
    }
}

#[test]
fn loss_of_focus_escape_or_business_interaction_reset_cancels_capture() {
    for reason in 0..3 {
        let mut test = fixture();
        test.frame(UiInteraction::Pressed, true, false);
        match reason {
            0 => {
                test.app
                    .world_mut()
                    .get_mut::<Window>(test.window)
                    .unwrap()
                    .focused = false
            }
            1 => test
                .app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape),
            _ => (),
        }
        test.frame(
            if reason == 2 {
                UiInteraction::None
            } else {
                UiInteraction::Pressed
            },
            false,
            false,
        );
        test.app
            .world_mut()
            .get_mut::<Window>(test.window)
            .unwrap()
            .focused = true;
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        test.frame(UiInteraction::Hovered, false, true);
        assert!(
            !test.clicked(test.button),
            "取消原因{reason}不能在恢复后补发"
        );
    }
}
