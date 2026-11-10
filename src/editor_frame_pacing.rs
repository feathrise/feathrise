//! Windows 编辑器的日常帧率与编译优化共用一个 Winit 调度器，编译结束采用最新个人设置。

use bevy::{
    diagnostic::FrameCount,
    ecs::system::SystemParam,
    prelude::*,
    window::WindowEvent,
    winit::{UpdateMode, WinitSettings},
};
use feathrise_editor_core::EditorSettings;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// 15 Hz 让进度和输入仍能逐帧处理，并为并行编译让出编辑器的 CPU 时间。
const BUILD_FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 15);
const IDLE_FRAME_INTERVAL: Duration = Duration::from_millis(200);
const IDLE_AFTER: Duration = Duration::from_millis(750);

/// 作者只向所属工作台传递当前偏好和真实编译状态，独占的偏好写者保持不变。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FramePacingRequest {
    pub(crate) max_fps: u32,
    pub(crate) limit_build_fps: bool,
    pub(crate) building: bool,
}

impl FramePacingRequest {
    pub(crate) fn new(settings: &EditorSettings, building: bool) -> Self {
        Self {
            max_fps: settings.render.max_fps,
            limit_build_fps: settings.limit_build_fps,
            building,
        }
    }

    pub(crate) fn limiting_build(self) -> bool {
        self.building && self.limit_build_fps
    }
}

struct ActiveBuild {
    started: Instant,
    last_input: Instant,
    first_frame: u32,
}

#[derive(Default)]
pub(crate) struct EditorFramePacing {
    original: Option<WinitSettings>,
    build: Option<ActiveBuild>,
}

impl EditorFramePacing {
    pub(crate) fn transition(
        &mut self, request: FramePacingRequest, interacted: bool, settings: &WinitSettings,
        frame: u32, window: &'static str, now: Instant,
    ) -> Option<WinitSettings> {
        let original = self.original.get_or_insert_with(|| settings.clone());
        let normal = if request.max_fps == 0 {
            original.clone()
        } else {
            let nanos = 1_000_000_000_u64.div_ceil(u64::from(request.max_fps));
            with_limit(original, Duration::from_nanos(nanos))
        };
        if !request.limiting_build() {
            if let Some(build) = self.build.take() {
                bevy::log::debug!(target: "feathrise_editor_frame_pacing", window,
                    frames = frame.wrapping_sub(build.first_frame), seconds = now.duration_since(build.started).as_secs_f64(),
                    focused = ?normal.focused_mode, unfocused = ?normal.unfocused_mode, "Build frame pacing restored");
            }
            return modes_differ(&normal, settings).then_some(normal);
        }

        let build = self.build.get_or_insert_with(|| {
            bevy::log::debug!(target: "feathrise_editor_frame_pacing", window, "Build frame pacing applied");
            ActiveBuild { started: now, last_input: now, first_frame: frame }
        });
        if interacted {
            build.last_input = now;
        }
        // 200 ms 心跳仍消费输入和进度，首次输入提高频率；日常若设得更低，编译不提高其上限。
        let idle = now.duration_since(build.last_input) >= IDLE_AFTER;
        let interval = if idle { IDLE_FRAME_INTERVAL } else { BUILD_FRAME_INTERVAL };
        let next = with_limit(&normal, interval);
        if !modes_differ(&next, settings) {
            return None;
        }
        bevy::log::debug!(target: "feathrise_editor_frame_pacing", window, idle, "Build frame pacing switched");
        Some(next)
    }
}

fn modes_differ(a: &WinitSettings, b: &WinitSettings) -> bool {
    a.focused_mode != b.focused_mode || a.unfocused_mode != b.unfocused_mode
}

fn with_limit(original: &WinitSettings, interval: Duration) -> WinitSettings {
    let limit_mode = |mode| {
        let wait = match mode {
            UpdateMode::Continuous => interval,
            UpdateMode::Reactive { wait, .. } => wait.max(interval),
        };
        // 让 Winit 按时钟唤醒；事件照常排队，跳过主动睡眠和窗口重绘引起的提前唤醒。
        UpdateMode::Reactive {
            wait,
            react_to_device_events: false,
            react_to_user_events: false,
            react_to_window_events: false,
        }
    };
    WinitSettings {
        focused_mode: limit_mode(original.focused_mode),
        unfocused_mode: limit_mode(original.unfocused_mode),
    }
}

#[derive(SystemParam)]
pub(crate) struct PacingInput<'w, 's> {
    events: MessageReader<'w, 's, WindowEvent>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
}

impl PacingInput<'_, '_> {
    pub(crate) fn interacted(&mut self, active: bool) -> bool {
        // 只读本窗口的交互事件，排除重绘和其他应用的原始鼠标移动。长按和拖动保持交互频率。
        let input = active && self.events.read().any(|event| matches!(event,
            WindowEvent::CursorMoved(_) | WindowEvent::CursorEntered(_) | WindowEvent::MouseButtonInput(_)
            | WindowEvent::MouseWheel(_) | WindowEvent::KeyboardInput(_) | WindowEvent::Ime(_)
            | WindowEvent::TouchInput(_) | WindowEvent::WindowResized(_)));
        self.events.clear();
        active && (input || self.keys.get_pressed().next().is_some() || self.mouse.get_pressed().next().is_some())
    }
}

pub(crate) fn pace_author(
    mut pacing: Local<EditorFramePacing>, mut settings: ResMut<WinitSettings>,
    preferences: Res<EditorSettings>, frame: Res<FrameCount>, mut input: PacingInput,
) {
    let request = FramePacingRequest::new(&preferences, crate::build_process::project_build_active());
    if let Some(next) = pacing.transition(request, input.interacted(request.limiting_build()), &settings, frame.0, "author", Instant::now()) {
        *settings = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{ecs::system::RunSystemOnce, window::{CursorMoved, RequestRedraw}};

    #[test]
    fn editor_frame_pacing_loads_old_preferences_and_round_trips_changes() {
        use feathrise_editor_core::EditorSettingsData;

        let legacy: EditorSettingsData = serde_json::from_str("{}").unwrap();
        let mut settings = legacy.into_runtime();
        assert_eq!(settings.render.max_fps, 0);
        assert!(settings.limit_build_fps);
        settings.render.max_fps = 60;
        settings.limit_build_fps = false;
        let saved = serde_json::to_vec(&EditorSettingsData::from(&settings)).unwrap();
        let restored = serde_json::from_slice::<EditorSettingsData>(&saved).unwrap().into_runtime();
        assert_eq!(FramePacingRequest::new(&settings, false), FramePacingRequest::new(&restored, false));
    }

    #[test]
    fn editor_frame_pacing_switches_when_idle_and_restores_both_modes() {
        let mut pacing = EditorFramePacing::default();
        let started = Instant::now();
        let normal = WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::reactive_low_power(Duration::from_millis(100)),
        };
        let mut request = FramePacingRequest::new(&EditorSettings::default(), false);
        assert!(pacing.transition(request, false, &normal, 0, "test", started).is_none());
        request.building = true;
        let limited = pacing.transition(request, false, &normal, 1, "test", started).unwrap();
        assert!(matches!(limited.focused_mode, UpdateMode::Reactive {
            wait, react_to_device_events: false, react_to_user_events: false, react_to_window_events: false,
        } if wait == BUILD_FRAME_INTERVAL));
        assert!(matches!(limited.unfocused_mode, UpdateMode::Reactive { wait, .. } if wait == Duration::from_millis(100)));
        assert!(pacing.transition(request, false, &limited, 2, "test", started).is_none());
        let idle_at = started + IDLE_AFTER;
        let idle = pacing.transition(request, false, &limited, 3, "test", idle_at).unwrap();
        assert!(matches!(idle.focused_mode, UpdateMode::Reactive { wait, .. } if wait == IDLE_FRAME_INTERVAL));
        assert!(pacing.transition(request, false, &idle, 4, "test", idle_at).is_none());
        let interactive = pacing.transition(request, true, &idle, 5, "test", idle_at).unwrap();
        assert_eq!(interactive.focused_mode, limited.focused_mode);
        assert_eq!(interactive.unfocused_mode, limited.unfocused_mode);
        request.building = false;
        let restored = pacing.transition(request, false, &interactive, 6, "test", idle_at).unwrap();
        assert_eq!(restored.focused_mode, normal.focused_mode);
        assert_eq!(restored.unfocused_mode, normal.unfocused_mode);
    }

    #[test]
    fn editor_frame_pacing_applies_live_preference_changes() {
        let mut pacing = EditorFramePacing::default();
        let now = Instant::now();
        let original = WinitSettings::continuous();
        let mut request = FramePacingRequest { max_fps: 60, limit_build_fps: true, building: false };
        let normal = pacing.transition(request, false, &original, 1, "test", now).unwrap();
        assert!(matches!(normal.focused_mode, UpdateMode::Reactive { wait, .. } if wait == Duration::from_nanos(16_666_667)));
        request.building = true;
        let build = pacing.transition(request, true, &normal, 2, "test", now).unwrap();
        request.max_fps = 30;
        assert!(pacing.transition(request, true, &build, 3, "test", now).is_none());
        request.limit_build_fps = false;
        let restored = pacing.transition(request, true, &build, 4, "test", now).unwrap();
        assert!(matches!(restored.focused_mode, UpdateMode::Reactive { wait, .. } if wait == Duration::from_nanos(33_333_334)));
        request.max_fps = 0;
        let unlimited = pacing.transition(request, false, &restored, 5, "test", now).unwrap();
        assert_eq!(unlimited.focused_mode, UpdateMode::Continuous);
        assert_eq!(unlimited.unfocused_mode, UpdateMode::Continuous);
    }

    #[test]
    fn editor_frame_pacing_distinguishes_input_from_redraw_and_keeps_a_drag_active() {
        let mut app = App::new();
        app.add_message::<WindowEvent>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>();
        let sample = |mut input: PacingInput| input.interacted(true);
        let world = app.world_mut();
        world.write_message(WindowEvent::RequestRedraw(RequestRedraw));
        assert!(!world.run_system_once(sample).unwrap());
        world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        assert!(world.run_system_once(sample).unwrap());
        world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        world.write_message(WindowEvent::CursorMoved(CursorMoved {
            window: Entity::PLACEHOLDER, position: Vec2::ONE, delta: Some(Vec2::ONE),
        }));
        assert!(world.run_system_once(sample).unwrap());
    }
}
