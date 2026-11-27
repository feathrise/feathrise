//! 环境方案模拟参数的持久编辑、单笔拖动事务及回显。

use feathrise_editor_ui::UiInteraction;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use feathrise_atmosphere::{WeatherDebugOverride, WorldClock};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_ui::EditorButtonClick;
use feathrise_editor_ui::localization::EditorLocale;

use super::components::*;
use super::authoring::{editing_target, simulation_from_runtime};
use super::types::{SimulationDrag, SimulationInputState};
use crate::authoring::{EditorAuthoringSession, GlobalEnvironmentAuthoringState, GlobalEnvironmentSaveTask};
use crate::ui::{environment::EnvironmentPageState, sidebars::SceneEditorSidebarState};
use super::helpers::{
    centered_cursor_x_to_ratio, format_grass_wind_strength, format_hour,
    grass_wind_strength_from_ratio, grass_wind_thumb_left, hour_from_ratio, hour_thumb_left,
    month_from_ratio, month_thumb_left, weather_mode_from_ratio, weather_mode_thumb_left,
    weather_preview_thumb_left, WORLD_SLIDER_TRACK_WIDTH,
};
use crate::resources::SceneEditorState;

/// 输入只写明确选中的方案或独立场景，原运行时资源统一消费解析后的持久配置。
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_world_environment_controls(
    mut editor: ResMut<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    page: Res<EnvironmentPageState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut global: ResMut<GlobalEnvironmentAuthoringState>,
    save: Res<GlobalEnvironmentSaveTask>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    runtime: (Res<WorldClock>, Res<WeatherDebugOverride>, Res<EditorSettings>),
    buttons: Query<(&EditorButtonClick, Has<WorldTimePauseToggle>),
        (Changed<EditorButtonClick>, Or<(With<WorldTimePauseToggle>, With<WeatherApplyImmediateButton>)>)>,
    tracks: Query<(&UiInteraction, &RelativeCursorPosition, Has<WorldTimeSlider>, Has<WorldMonthSlider>,
        Has<WeatherModeSlider>, Has<WeatherPreviewSlider>),
        Or<(With<WorldTimeSlider>, With<WorldMonthSlider>, With<WeatherModeSlider>, With<WeatherPreviewSlider>, With<GrassWindSlider>)>>,
    mut state: Local<SimulationInputState>,
) {
    let target = editing_target(&editor, &sidebar, &page, &session, &global, &save);
    let cancel = keyboard.just_pressed(KeyCode::Escape);
    let identity_changed = state.drag.as_ref().is_some_and(|drag| {
        target.as_ref() != Some(&drag.target) || drag.target.version(&session, &global) != drag.version
    });
    if state.drag.is_some() && (cancel || identity_changed || !mouse.pressed(MouseButton::Left)) {
        let drag = state.drag.take().expect("已确认存在模拟参数拖动");
        drag.finish(cancel || identity_changed, &mut session, &mut global, &mut editor);
        state.blocked = (cancel || identity_changed) && mouse.pressed(MouseButton::Left);
    }
    if !mouse.pressed(MouseButton::Left) { state.blocked = false; }
    let Some(target) = target.filter(|_| !state.blocked && !cancel) else { return };
    let before = target.value(&session, &global);
    let mut value = before.unwrap_or_else(|| simulation_from_runtime(&runtime.0, &runtime.1, &runtime.2));
    let mut discrete = false;
    for (click, pause) in &buttons {
        if !click.is_clicked() { continue; }
        if pause { value.paused = !value.paused; } else { value.weather_transition = 1.0; }
        discrete = true;
    }
    let mut touched = discrete;
    for (interaction, cursor, hour, month, mode, transition) in &tracks {
        if *interaction != UiInteraction::Pressed || !mouse.pressed(MouseButton::Left) { continue; }
        let Some(position) = cursor.normalized else { continue };
        touched = true;
        let ratio = centered_cursor_x_to_ratio(position.x);
        if hour { value.hour = hour_from_ratio(ratio); }
        else if month { value.month = month_from_ratio(ratio); value.day = 15; }
        else if mode { value.weather_mode_index = weather_mode_from_ratio(ratio) as u8; }
        else if transition { value.weather_transition = ratio; }
        else { value.grass_wind_tip_strength = grass_wind_strength_from_ratio(ratio); }
    }
    if !touched || before == Some(value) { return; }
    if let Err(error) = value.validate() { editor.status = error; return; }
    if state.drag.is_none() {
        state.drag = Some(SimulationDrag {
            version: target.version(&session, &global), dirty_before: target.dirty(&session, &global),
            target: target.clone(), before, latest: value,
        });
    }
    target.write(Some(value), &mut session, &mut global);
    editor.dirty = true;
    state.drag.as_mut().expect("模拟拖动已冻结").latest = value;
    if discrete {
        state.drag.take().expect("离散操作立即提交").finish(false, &mut session, &mut global, &mut editor);
    }
}

type WorldEnvironmentNodeFilter = Or<(
    With<WorldTimeSliderThumb>,
    With<WorldMonthSliderThumb>,
    With<WeatherModeSliderThumb>,
    With<WeatherPreviewSliderThumb>,
    With<GrassWindSliderThumb>, // 草尖摆幅与天气滑块同帧回显。
)>;
type WorldEnvironmentTextFilter = Or<(
    With<WorldTimePauseLabel>,
    With<WorldTimeSliderLabel>,
    With<WorldMonthSliderLabel>,
    With<WeatherModeSliderLabel>,
    With<WeatherPreviewSliderLabel>,
    With<WeatherRegionLabel>,
    With<GrassWindSliderLabel>, // 迁移后标签不依赖F1可见状态。
)>;

/// 显示被编辑方案的持久值，浏览未引用方案也能准确回显自己的配置。
pub(crate) fn sync_world_environment_controls(
    editor: Res<SceneEditorState>,
    locale: Option<Res<EditorLocale>>,
    page: Res<EnvironmentPageState>,
    session: Res<EditorAuthoringSession>,
    global: Res<GlobalEnvironmentAuthoringState>,
    runtime: (Res<WorldClock>, Res<WeatherDebugOverride>, Res<EditorSettings>),
    mut nodes: Query<
        (
            &mut Node,
            Has<WorldTimeSliderThumb>,
            Has<WorldMonthSliderThumb>,
            Has<WeatherModeSliderThumb>,
            Has<WeatherPreviewSliderThumb>,
            Has<GrassWindSliderThumb>, // 只修改本控件手柄，不扫描整个编辑器UI。
        ),
        WorldEnvironmentNodeFilter,
    >,
    mut texts: Query<
        (
            &mut Text,
            Has<WorldTimePauseLabel>,
            Has<WorldTimeSliderLabel>,
            Has<WorldMonthSliderLabel>,
            Has<WeatherModeSliderLabel>,
            Has<WeatherPreviewSliderLabel>,
            Has<WeatherRegionLabel>,
            Has<GrassWindSliderLabel>, // 只修改天气栏风摆标签。
        ),
        WorldEnvironmentTextFilter,
    >,
) {
    if !editor.open {
        return;
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    let configuration = match page.page {
        crate::ui::environment::EnvironmentPage::Library => global.selected_environment().map(|profile| profile.appearance.simulation),
        crate::ui::environment::EnvironmentPage::Scene => session.draft.as_ref().and_then(|draft| {
            feathrise_scene_content::resolve_scene_environment(&draft.manifest.environment,
                &global.draft.celestial_lighting, &global.draft.library).environment.simulation
        }),
    }.unwrap_or_else(|| simulation_from_runtime(&runtime.0, &runtime.1, &runtime.2));
    let mut clock = WorldClock::default();
    clock.set_month_midpoint(configuration.month);
    let mode_text = if configuration.weather_mode_index == 0 {
        locale.text("label.auto")
    } else {
        feathrise_atmosphere::WeatherKind::ALL.get(configuration.weather_mode_index as usize - 1)
            .map_or(locale.text("label.auto"), |kind| locale.editor_label(kind.display_name()))
    };
    let status_text = format!("{} · {:.0}%", mode_text, configuration.weather_transition * 100.0);

    for (mut node, time, month, mode, preview, grass) in &mut nodes {
        let left = if time {
            Some(hour_thumb_left(configuration.hour))
        } else if month {
            Some(month_thumb_left(configuration.month))
        } else if mode {
            Some(weather_mode_thumb_left(configuration.weather_mode_index as usize))
        } else if preview {
            Some(weather_preview_thumb_left(configuration.weather_transition))
        } else if grass {
            Some(grass_wind_thumb_left(configuration.grass_wind_tip_strength))
        } else {
            None
        };
        if let Some(left) = left {
            let desired = percent(left / WORLD_SLIDER_TRACK_WIDTH * 100.0);
            if node.left != desired {
                node.left = desired;
            }
        }
    }
    for (mut text, pause, time, month, mode, preview, region, grass) in &mut texts {
        let desired = if pause {
            locale.text(if configuration.paused { "world.pause.on" } else { "world.pause.off" }).to_owned()
        } else if time {
            format_hour(configuration.hour)
        } else if month {
            let month = configuration.month;
            let month_name = if locale == EditorLocale::English {
                // 世界时钟的月份只用于呈现，不受操作系统地区格式影响。
                ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
                    .get(month.saturating_sub(1) as usize).copied().unwrap_or("—").to_owned()
            } else {
                month.to_string()
            };
            locale.format("world.calendar.date", &[
                ("month", month_name),
                ("day", configuration.day.to_string()),
                ("season", locale.editor_label(clock.season_name()).to_owned()),
            ])
        } else if mode {
            mode_text.to_owned()
        } else if preview {
            status_text.clone()
        } else if region {
            locale.format("world.weather.current", &[("weather", mode_text.to_owned())])
        } else if grass {
            format_grass_wind_strength(configuration.grass_wind_tip_strength)
        } else {
            continue;
        };
        if text.as_str() != desired {
            **text = desired;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::{EnvironmentSimulation, SceneEnvironmentSource, WorldEnvironmentLibrary};
    use crate::ui::{environment::EnvironmentPage, sidebars::EditorRightSidebarTab};

    fn editor() -> App {
        let mut app = App::new();
        let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
        app.insert_resource(SceneEditorState { open: true, selected_scene_id: Some(draft.scene_id.clone()), ..default() })
            .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
            .init_resource::<GlobalEnvironmentAuthoringState>()
            .init_resource::<GlobalEnvironmentSaveTask>()
            .init_resource::<SceneEditorSidebarState>()
            .init_resource::<EnvironmentPageState>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<WorldClock>()
            .init_resource::<WeatherDebugOverride>()
            .init_resource::<EditorSettings>()
            .add_systems(Update, handle_world_environment_controls);
        app.world_mut().resource_mut::<SceneEditorSidebarState>().right = EditorRightSidebarTab::Environment;
        app.world_mut().resource_mut::<EnvironmentPageState>().page = EnvironmentPage::Library;
        let mut global = app.world_mut().resource_mut::<GlobalEnvironmentAuthoringState>();
        global.draft.library = WorldEnvironmentLibrary::default().with_legacy_default(&global.draft.celestial_lighting);
        global.maintain_environment_selection();
        app
    }

    #[test]
    fn grass_wind_edits_the_selected_profile_with_one_undo_per_drag() {
        let mut app = editor();
        let settings_before = app.world().resource::<EditorSettings>().environment.grass_wind_tip_strength;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        let track = app.world_mut().spawn((GrassWindSlider, UiInteraction::Pressed,
            RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::new(0.0, 0.0)) })).id();
        app.update();
        app.world_mut().get_mut::<RelativeCursorPosition>(track).unwrap().normalized = Some(Vec2::new(0.5, 0.0));
        app.update();
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert_eq!(app.world().resource::<GlobalEnvironmentAuthoringState>().selected_environment().unwrap().appearance.simulation.grass_wind_tip_strength, 0.35);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.undo.len(), 1);
        assert!(session.undo[0].is_global_environment());
        assert_eq!(app.world().resource::<EditorSettings>().environment.grass_wind_tip_strength, settings_before);
    }

    #[test]
    fn a_cancelled_simulation_drag_restores_values_without_rebinding_until_release() {
        let mut app = editor();
        let before = app.world().resource::<GlobalEnvironmentAuthoringState>().selected_environment().unwrap().appearance.simulation;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.world_mut().spawn((GrassWindSlider, UiInteraction::Pressed,
            RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::new(0.5, 0.0)) }));
        app.update();
        app.world_mut().resource_mut::<EnvironmentPageState>().page = EnvironmentPage::Scene;
        app.update();
        assert_eq!(app.world().resource::<GlobalEnvironmentAuthoringState>().selected_environment().unwrap().appearance.simulation, before);
        assert!(!app.world().resource::<GlobalEnvironmentAuthoringState>().dirty);
        assert!(!app.world().resource::<SceneEditorState>().dirty);
        app.world_mut().resource_mut::<EnvironmentPageState>().page = EnvironmentPage::Library;
        app.update();
        assert_eq!(app.world().resource::<GlobalEnvironmentAuthoringState>().selected_environment().unwrap().appearance.simulation, before);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }

    #[test]
    fn returning_a_simulation_drag_to_its_start_leaves_no_dirty_state_or_undo() {
        let mut app = editor();
        let before = app.world().resource::<GlobalEnvironmentAuthoringState>().selected_environment().unwrap().appearance.simulation;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        let track = app.world_mut().spawn((GrassWindSlider, UiInteraction::Pressed,
            RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::new(0.5, 0.0)) })).id();
        app.update();
        let cursor = (grass_wind_thumb_left(before.grass_wind_tip_strength)
            + super::super::helpers::WORLD_SLIDER_THUMB_WIDTH * 0.5) / WORLD_SLIDER_TRACK_WIDTH - 0.5;
        app.world_mut().get_mut::<RelativeCursorPosition>(track).unwrap().normalized = Some(Vec2::new(cursor, 0.0));
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<GlobalEnvironmentAuthoringState>().selected_environment().unwrap().appearance.simulation, before);
        assert!(!app.world().resource::<GlobalEnvironmentAuthoringState>().dirty);
        assert!(!app.world().resource::<SceneEditorState>().dirty);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }

    #[test]
    fn simulation_controls_read_the_selected_profile_instead_of_editor_preferences() {
        let mut app = editor();
        let id = app.world().resource::<GlobalEnvironmentAuthoringState>().selected_environment_id.clone().unwrap();
        app.world_mut().resource_mut::<GlobalEnvironmentAuthoringState>().draft.library.profiles_mut()
            .get_mut(&id).unwrap().appearance.simulation.grass_wind_tip_strength = 0.23;
        let thumb = app.world_mut().spawn((GrassWindSliderThumb, Node::default())).id();
        let label = app.world_mut().spawn((GrassWindSliderLabel, Text::new(""))).id();
        app.add_systems(Update, sync_world_environment_controls.after(handle_world_environment_controls));
        app.update();
        assert_eq!(app.world().get::<Node>(thumb).unwrap().left,
            percent(grass_wind_thumb_left(0.23) / WORLD_SLIDER_TRACK_WIDTH * 100.0));
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "23 cm");
    }

    #[test]
    fn a_legacy_local_environment_is_untouched_until_actual_input() {
        let mut app = editor();
        app.world_mut().resource_mut::<EnvironmentPageState>().page = EnvironmentPage::Scene;
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let environment = &mut session.draft.as_mut().unwrap().manifest.environment;
        environment.source = SceneEnvironmentSource::Local;
        environment.simulation = None;
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.undo.is_empty());
        assert!(session.draft.as_ref().unwrap().manifest.environment.simulation.is_none());
        assert!(!app.world().resource::<SceneEditorState>().dirty);
    }

    #[test]
    fn inherited_scene_rejects_hidden_inputs_and_local_copy_accepts_them() {
        let mut app = editor();
        app.world_mut().resource_mut::<EnvironmentPageState>().page = EnvironmentPage::Scene;
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
            .manifest.environment.source = SceneEnvironmentSource::InheritDefault;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.world_mut().spawn((WorldTimeSlider, UiInteraction::Pressed,
            RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::new(0.25, 0.0)) }));
        app.update();
        assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().manifest.environment.simulation.is_none());
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().manifest.environment.source = SceneEnvironmentSource::Local;
        session.draft.as_mut().unwrap().manifest.environment.simulation = Some(EnvironmentSimulation::default());
        app.update();
        assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().manifest.environment.simulation.unwrap().hour > 12.0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    }
}
