//! F12 世界时间与天气调试输入及最终状态回显。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use feathrise_atmosphere::{WeatherCommand, WeatherDebugOverride, WeatherState, WorldClock};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_ui::EditorButtonClick;
use feathrise_editor_ui::localization::EditorLocale;

use super::components::*;
use super::helpers::{
    centered_cursor_x_to_ratio, format_game_seconds, format_grass_wind_strength, format_hour,
    grass_wind_strength_from_ratio, grass_wind_thumb_left, hour_from_ratio, hour_thumb_left,
    month_from_ratio, month_thumb_left, weather_mode_from_ratio, weather_mode_thumb_left,
    weather_preview_thumb_left, WORLD_SLIDER_TRACK_WIDTH,
};
use crate::resources::MapEditorState;

type PauseButtonFilter = (
    Changed<EditorButtonClick>,
    With<InteractionButton>,
    With<WorldTimePauseToggle>,
);
type ImmediateButtonFilter = (
    Changed<EditorButtonClick>,
    With<InteractionButton>,
    With<WeatherApplyImmediateButton>,
);
type RerollButtonFilter = (
    Changed<EditorButtonClick>,
    With<InteractionButton>,
    With<WeatherRerollButton>,
);

/// F12 输入在本帧时间推进与天气解析前直接写各自公开运行时资源。
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_world_environment_controls(
    editor: Res<MapEditorState>,
    pause_buttons: Query<&EditorButtonClick, PauseButtonFilter>,
    time_tracks: Query<(&UiInteraction, &RelativeCursorPosition), With<WorldTimeSlider>>,
    month_tracks: Query<(&UiInteraction, &RelativeCursorPosition), With<WorldMonthSlider>>,
    mode_tracks: Query<(&UiInteraction, &RelativeCursorPosition), With<WeatherModeSlider>>,
    preview_tracks: Query<(&UiInteraction, &RelativeCursorPosition), With<WeatherPreviewSlider>>,
    immediate_buttons: Query<&EditorButtonClick, ImmediateButtonFilter>,
    reroll_buttons: Query<&EditorButtonClick, RerollButtonFilter>,
    mut clock: ResMut<WorldClock>,
    mut settings: ResMut<EditorSettings>,
    mut weather_override: ResMut<WeatherDebugOverride>,
    weather_state: Res<WeatherState>,
    mut weather_commands: MessageWriter<WeatherCommand>,
) {
    if !editor.open || editor.close_requested {
        return;
    }
    for interaction in &pause_buttons {
        if interaction.is_clicked() {
            settings.environment.world_time_paused = !settings.environment.world_time_paused;
        }
    }
    for (interaction, cursor) in &time_tracks {
        if *interaction == UiInteraction::Pressed
            && let Some(position) = cursor.normalized
        {
            clock.set_hour(hour_from_ratio(centered_cursor_x_to_ratio(position.x)));
        }
    }
    for (interaction, cursor) in &month_tracks {
        if *interaction == UiInteraction::Pressed
            && let Some(position) = cursor.normalized
        {
            clock.set_month_midpoint(month_from_ratio(centered_cursor_x_to_ratio(position.x)));
        }
    }
    for (interaction, cursor) in &mode_tracks {
        if *interaction == UiInteraction::Pressed
            && let Some(position) = cursor.normalized
        {
            weather_override.set_mode_index(weather_mode_from_ratio(centered_cursor_x_to_ratio(
                position.x,
            )));
        }
    }
    for (interaction, cursor) in &preview_tracks {
        if *interaction == UiInteraction::Pressed
            && let Some(position) = cursor.normalized
        {
            weather_override.set_preview(centered_cursor_x_to_ratio(position.x));
        }
    }
    if immediate_buttons
        .iter()
        .any(|interaction| interaction.is_clicked())
    {
        let kind = weather_override
            .forced()
            .unwrap_or_else(|| weather_state.target());
        weather_override.set_preview(1.0);
        weather_commands.write(WeatherCommand::ApplyImmediate { kind });
    }
    if reroll_buttons
        .iter()
        .any(|interaction| interaction.is_clicked())
        && weather_override.forced().is_none()
    {
        weather_commands.write(WeatherCommand::Reroll);
    }
}

/// 【功能设计】滑块只保存晴天基准摆幅；天气桥在本帧天气解析后唯一写入草和Mesh风场。
pub(crate) fn handle_grass_wind_slider(
    editor: Res<MapEditorState>, // F12关闭或正在关闭时不可响应隐藏控件。
    grass_tracks: Query<(&UiInteraction, &RelativeCursorPosition), With<GrassWindSlider>>, // 只查询迁移后的唯一滑块。
    mut settings: ResMut<EditorSettings>, // 沿用原settings.json自动保存字段。
) {
    if !editor.open || editor.close_requested {
        return; // 隐藏的UI不得继续调整全局风摆。
    }
    for (interaction, cursor) in &grass_tracks {
        if *interaction == UiInteraction::Pressed
            && let Some(position) = cursor.normalized
        {
            let strength = grass_wind_strength_from_ratio(centered_cursor_x_to_ratio(position.x)); // 保留原F1厘米步进。
            if (settings.environment.grass_wind_tip_strength - strength).abs() > f32::EPSILON {
                settings.environment.grass_wind_tip_strength = strength; // 数值变更才触发原有设置自动保存。
            }
        }
    }
}

type WorldEnvironmentNodeFilter = Or<(
    With<WorldTimeSliderThumb>,
    With<WorldMonthSliderThumb>,
    With<WeatherModeSliderThumb>,
    With<WeatherPreviewSliderThumb>,
    With<WeatherRerollButton>,
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

/// 世界光照和天气效果已经应用后，把最终运行时状态统一回写 F12 控件。
pub(crate) fn sync_world_environment_controls(
    editor: Res<MapEditorState>,
    locale: Option<Res<EditorLocale>>,
    clock: Res<WorldClock>,
    weather_override: Res<WeatherDebugOverride>,
    weather_state: Res<WeatherState>,
    settings: Res<EditorSettings>, // 全局风摆设置由同一持久资源回显。
    mut nodes: Query<
        (
            &mut Node,
            Has<WorldTimeSliderThumb>,
            Has<WorldMonthSliderThumb>,
            Has<WeatherModeSliderThumb>,
            Has<WeatherPreviewSliderThumb>,
            Has<WeatherRerollButton>,
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
    let automatic_progress = if weather_state.is_transitioning() {
        weather_state.transition_progress()
    } else {
        1.0
    };
    let progress =
        if weather_override.forced().is_some() || weather_override.preview_override_active() {
            weather_override.preview()
        } else {
            automatic_progress
        };
    let remaining = if weather_state.is_transitioning() {
        weather_state.transition_duration_game_seconds()
            * (1.0 - weather_state.transition_progress()).clamp(0.0, 1.0)
    } else {
        weather_state.stable_remaining_game_seconds()
    };
    let target = weather_override
        .forced()
        .unwrap_or_else(|| weather_state.target());
    let mode_text = weather_override
        .forced()
        .map_or_else(|| locale.text("label.auto"), |kind| locale.editor_label(kind.display_name()));
    let current_weather = locale.editor_label(weather_state.current().display_name());
    let status_text = format!(
        "{} → {} · {:.0}% · {}",
        current_weather,
        locale.editor_label(target.display_name()),
        progress * 100.0,
        format_game_seconds(remaining),
    );

    for (mut node, time, month, mode, preview, reroll, grass) in &mut nodes {
        let left = if time {
            Some(hour_thumb_left(clock.hour_for_display()))
        } else if month {
            Some(month_thumb_left(clock.month()))
        } else if mode {
            Some(weather_mode_thumb_left(weather_override.mode_index()))
        } else if preview {
            Some(weather_preview_thumb_left(progress))
        } else if reroll {
            let display = if weather_override.forced().is_none() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
            None
        } else if grass {
            Some(grass_wind_thumb_left(settings.environment.grass_wind_tip_strength)) // 重开F12和外部设置恢复后位置正确。
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
            locale.text(if settings.environment.world_time_paused { "world.pause.on" } else { "world.pause.off" }).to_owned()
        } else if time {
            format_hour(clock.hour_for_display())
        } else if month {
            let month = clock.month();
            let month_name = if locale == EditorLocale::English {
                // 世界时钟的月份只用于呈现，不受操作系统地区格式影响。
                ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
                    .get(month.saturating_sub(1) as usize).copied().unwrap_or("—").to_owned()
            } else {
                month.to_string()
            };
            locale.format("world.calendar.date", &[
                ("month", month_name),
                ("day", clock.day().to_string()),
                ("season", locale.editor_label(clock.season_name()).to_owned()),
            ])
        } else if mode {
            mode_text.to_owned()
        } else if preview {
            status_text.clone()
        } else if region {
            locale.format("world.weather.current", &[("weather", current_weather.to_owned())])
        } else if grass {
            format_grass_wind_strength(settings.environment.grass_wind_tip_strength) // 保留原F1的厘米标签而不改变天气风速。
        } else {
            continue;
        };
        if text.as_str() != desired {
            **text = desired;
        }
    }
}

#[cfg(test)] // F12滑块迁移使用最小ECS世界验证同帧全局风摆与关闭隔离。
mod tests {
    use super::*; // 复用正式滑块输入系统与标记。

    #[test] // 滑块只写晴天基准设置，GPU风场由天气解析后的唯一桥负责。
    fn grass_wind_slider_updates_base_strength_only_while_editor_is_open() {
        let mut app = App::new(); // 不安装F1面板即可运行编辑器输入。
        app.insert_resource(MapEditorState {
            open: true,  // 模拟F12天气栏打开。
            ..default()  // 其他会话字段保持默认值。
        }); // 模拟F12天气栏打开。
        app.insert_resource(EditorSettings::default()); // 原F1持久设置默认十六厘米。
        app.add_systems(Update, handle_grass_wind_slider); // 使用正式输入系统。
        let track = app
            .world_mut()
            .spawn((
                GrassWindSlider,      // 天气栏唯一草地风摆轨道。
                UiInteraction::Pressed, // 拖动手柄。
                RelativeCursorPosition {
                    cursor_over: true,                     // 模拟游标处于轨道内。
                    normalized: Some(Vec2::new(0.5, 0.0)), // 指向轨道右端。
                }, // 指向轨道右端。
            ))
            .id(); // 后续复用轨道验证F12关闭隔离。
        app.update(); // 右端拖动应在同一帧达到三十五厘米。
        assert_eq!(
            app.world()
                .resource::<EditorSettings>()
                .environment.grass_wind_tip_strength,
            0.35
        ); // 持久字段保留原范围。

        app.world_mut().resource_mut::<MapEditorState>().open = false; // 关闭F12但轨道实体尚未清理。
        app.world_mut()
            .entity_mut(track)
            .get_mut::<RelativeCursorPosition>()
            .unwrap()
            .normalized = Some(Vec2::new(-0.5, 0.0)); // 隐藏轨道被重置为左端。
        app.update(); // 隐藏控件不得改写持久风摆。
        assert_eq!(
            app.world()
                .resource::<EditorSettings>()
                .environment.grass_wind_tip_strength,
            0.35
        ); // 关闭隔离生效。
        app.world_mut().resource_mut::<MapEditorState>().open = true; // 重开F12使用同一设置。
        app.update(); // 左端拖动归零，模拟用户调整。
        assert_eq!(
            app.world()
                .resource::<EditorSettings>()
                .environment.grass_wind_tip_strength,
            0.0
        ); // 原一厘米步进仍有效。
    }

    #[test] // 重新打开天气页要从原全局设置恢复手柄和厘米数值。
    fn grass_wind_controls_read_saved_value_on_open() {
        let mut app = App::new(); // 最小ECS世界验证UI回显。
        app.insert_resource(MapEditorState {
            open: true,  // 模拟F12天气栏打开。
            ..default()  // 其他会话字段不干扰控件同步。
        }); // F12可见。
        app.insert_resource(WorldClock::default()); // 同一回显系统的世界时间输入。
        app.insert_resource(WeatherDebugOverride::default()); // 天气默认自然模式。
        app.insert_resource(WeatherState::default()); // 天气状态摘要输入。
        app.insert_resource(EditorSettings {
            environment: feathrise_settings::EnvironmentPreviewSettings { grass_wind_tip_strength: 0.23, ..default() }, // 模拟作者用户配置已加载二十三厘米。
            ..default()                         // 其余全局设置保持原默认值。
        }); // 模拟settings.json已加载二十三厘米。
        let thumb = app
            .world_mut()
            .spawn((GrassWindSliderThumb, Node::default()))
            .id(); // 记录新手柄实体。
        let label = app
            .world_mut()
            .spawn((GrassWindSliderLabel, Text::new("")))
            .id(); // 记录新数值标签。
        app.add_systems(Update, sync_world_environment_controls); // 使用正式回显系统。
        app.update(); // 首帧读全局设置而非默认风力。
        assert_eq!(
            app.world().entity(thumb).get::<Node>().unwrap().left,
            percent(grass_wind_thumb_left(0.23) / WORLD_SLIDER_TRACK_WIDTH * 100.0)
        ); // 恢复原厘米档位置。
        assert_eq!(app.world().entity(label).get::<Text>().unwrap().0, "23 cm"); // 恢复原显示值。
    }
}
