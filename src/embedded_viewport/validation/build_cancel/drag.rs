//! 在本轮自有实窗中分帧投递窗口输入，读取原分隔线布局和真实 Cargo 帧率档位。

use bevy::{
    input::{ButtonState, mouse::MouseButtonInput},
    prelude::*,
    window::{CursorMoved, PrimaryWindow, WindowEvent},
    winit::{UpdateMode, WinitSettings},
};
use feathrise_editor_map::standalone::embedded_validation::content_splitter_probe;
use serde_json::{Value, json};

#[derive(Debug, Default)]
pub(super) struct CargoDrag {
    step: Step,
    origin: Vec2,
    before_width: f32,
}

#[derive(Debug, Default)]
enum Step {
    #[default]
    Position,
    Press,
    Move,
    Release,
    Verify,
}

impl CargoDrag {
    pub(super) fn advance(&mut self, world: &mut World) -> Result<Option<Value>, String> {
        let Some((center, width, hovered, dragging)) = content_splitter_probe(world) else { return Ok(None); };
        let Ok(window) = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world) else { return Ok(None); };
        match self.step {
            Step::Position => {
                self.origin = center;
                self.before_width = width;
                move_cursor(world, window, center);
                self.step = Step::Press;
            }
            // 让预定位输入过期。第一下按键本身必须从实际 200 ms 的空闲档唤回。
            Step::Press if hovered && pacing_is(world, 200) => {
                mouse_button(world, window, ButtonState::Pressed);
                self.step = Step::Move;
            }
            Step::Move if dragging && pacing_is(world, 66) => {
                move_cursor(world, window, self.origin + Vec2::new(40., 0.));
                self.step = Step::Release;
            }
            Step::Move if world.resource::<ButtonInput<MouseButton>>().just_pressed(MouseButton::Left) && !dragging => {
                let cursor = world.get::<Window>(window).and_then(Window::physical_cursor_position);
                return Err(format!("分隔线未捕获首次按下，分隔线中心={center:?}，光标={cursor:?}，悬停={hovered}"));
            }
            Step::Release if width > self.before_width + 12. && center.x > self.origin.x + 12. => {
                mouse_button(world, window, ButtonState::Released);
                self.step = Step::Verify;
            }
            Step::Verify if !dragging && !world.resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left) => {
                return Ok(Some(json!({"input":"Bevy WindowEvent across frames", "idle_wait_ms":200,
                    "active_wait_ms":66, "origin":self.origin.to_array(),
                    "final_splitter_center":center.to_array(), "width_before":self.before_width, "width_after":width})));
            }
            _ => {}
        }
        Ok(None)
    }
}

fn pacing_is(world: &World, millis: u128) -> bool {
    let settings = world.resource::<WinitSettings>();
    [&settings.focused_mode, &settings.unfocused_mode].into_iter().all(|mode| matches!(mode,
        UpdateMode::Reactive { wait, react_to_device_events: false, react_to_user_events: false,
            react_to_window_events: false } if wait.as_millis() == millis))
}

fn move_cursor(world: &mut World, window: Entity, center: Vec2) {
    let mut target = world.get_mut::<Window>(window).unwrap();
    target.set_physical_cursor_position(Some(center.as_dvec2()));
    let position = center / target.scale_factor();
    window_input(world, CursorMoved { window, position, delta: None });
}

fn mouse_button(world: &mut World, window: Entity, state: ButtonState) {
    window_input(world, MouseButtonInput { button: MouseButton::Left, state, window });
}

/// 与窗口后端一样同时投递聚合和类型化消息，拾取和原鼠标系统照常读取各自通道。
fn window_input(world: &mut World, input: impl bevy::ecs::message::Message + Clone + Into<WindowEvent>) {
    let event = input.clone().into();
    world.write_message(input);
    world.write_message(event);
}
