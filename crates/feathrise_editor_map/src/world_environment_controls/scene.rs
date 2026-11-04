//! F12 世界环境时间、月份和天气控件生成。

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use feathrise_atmosphere::WorldClock;
use feathrise_editor_core::EditorSettings;
use crate::ui::theme as colors;
use feathrise_editor_ui::{
    SliderTick, SliderVisual, TextButtonStyle, menu_button_style, spawn_custom_text_button,
    spawn_slider_with_visual,
};

use super::components::*;
use super::helpers::{
    GRASS_WIND_MAX_CENTIMETERS, WORLD_SLIDER_THUMB_WIDTH, WORLD_SLIDER_TRACK_WIDTH,
    WORLD_SLIDER_TRAVEL, format_grass_wind_strength, format_hour, grass_wind_thumb_left,
    hour_thumb_left, month_thumb_left, weather_mode_thumb_left, weather_preview_thumb_left,
};
use crate::ui::scene::FONT_PATH;

const TIME_TICKS: [SliderTick; 5] = [
    SliderTick {
        px: 0.0,
        label: "0",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.25,
        label: "6",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.5,
        label: "12",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.75,
        label: "18",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL,
        label: "24",
    },
];

const MONTH_TICKS: [SliderTick; 12] = [
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.0 / 11.0,
        label: "1",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 1.0 / 11.0,
        label: "2",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 2.0 / 11.0,
        label: "3",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 3.0 / 11.0,
        label: "4",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 4.0 / 11.0,
        label: "5",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 5.0 / 11.0,
        label: "6",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 6.0 / 11.0,
        label: "7",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 7.0 / 11.0,
        label: "8",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 8.0 / 11.0,
        label: "9",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 9.0 / 11.0,
        label: "10",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 10.0 / 11.0,
        label: "11",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL,
        label: "12",
    },
];

const WEATHER_MODE_TICKS: [SliderTick; 6] = [
    SliderTick {
        px: 0.0,
        label: "自动",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.2,
        label: "晴",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.4,
        label: "阴",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.6,
        label: "小雨",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.8,
        label: "大雨",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL,
        label: "薄雾",
    },
];

const WEATHER_PREVIEW_TICKS: [SliderTick; 5] = [
    SliderTick {
        px: 0.0,
        label: "0",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.25,
        label: "25",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.5,
        label: "50",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 0.75,
        label: "75",
    },
    SliderTick {
        px: WORLD_SLIDER_TRAVEL,
        label: "100",
    },
];

const GRASS_WIND_TICKS: [SliderTick; 5] = [
    SliderTick {
        px: 0.0,    // 无风摆位于轨道左端。
        label: "0", // 零厘米。
    }, // 无风摆位于轨道左端。
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 10.0 / GRASS_WIND_MAX_CENTIMETERS, // 按安全上限定位十厘米刻度。
        label: "10",                                                 // 轻风摆。
    }, // 十厘米轻风摆。
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 20.0 / GRASS_WIND_MAX_CENTIMETERS, // 按安全上限定位二十厘米刻度。
        label: "20",                                                 // 中等摆幅。
    }, // 二十厘米中档。
    SliderTick {
        px: WORLD_SLIDER_TRAVEL * 30.0 / GRASS_WIND_MAX_CENTIMETERS, // 按安全上限定位三十厘米刻度。
        label: "30",                                                 // 强风摆。
    }, // 三十厘米强风摆。
    SliderTick {
        px: WORLD_SLIDER_TRAVEL, // 手柄右端完整可达。
        label: "35",             // GPU允许的最大厘米摆幅。
    }, // 与GPU安全上限一致。
]; // 沿用原F1厘米刻度，但宽度与F12天气栏一致。

fn slider_visual() -> SliderVisual {
    SliderVisual {
        track_width: WORLD_SLIDER_TRACK_WIDTH,
        thumb_width: WORLD_SLIDER_THUMB_WIDTH,
        tick_width: 20.0,
        tick_font_size: 8.0,
        ..default()
    }
}

/// 暂停填满整行，天气动作严格等宽；只调整公共按钮外壳，不改变原调试动作。
fn world_button_style(full_width: bool) -> TextButtonStyle {
    let mut style = menu_button_style(126.0, 26.0, 10.5);
    style.node.width = if full_width { percent(100) } else { Val::Auto };
    style.node.min_width = px(0);
    style.node.flex_shrink = 0.0;
    style.node.flex_grow = if full_width { 0.0 } else { 1.0 };
    style.node.flex_basis = if full_width { Val::Auto } else { px(0) };
    style.node.border_radius = BorderRadius::all(px(3));
    style
}

fn spawn_value_row<M: Component>(
    parent: &mut ChildSpawnerCommands,
    marker: M,
    font: Handle<Font>,
    title: &'static str,
    value: String,
) {
    parent
        .spawn(Node {
            width: percent(100),
            min_width: px(0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(4),
            margin: UiRect::bottom(px(3)),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(title),
                TextFont {
                    font: font.clone().into(),
                    font_size: feathrise_editor_ui::metrics::FONT_CONTROL.into(),
                    ..default()
                },
                TextColor(colors::TEXT_MUTED),
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            row.spawn((
                marker,
                Text::new(value),
                TextFont {
                    font: font.into(),
                    font_size: feathrise_editor_ui::metrics::FONT_CONTROL.into(),
                    ..default()
                },
                TextColor(colors::TEXT_VALUE),
                TextLayout {
                    linebreak: bevy::text::LineBreak::AnyCharacter,
                    ..default()
                },
                Node {
                    min_width: px(0),
                    max_width: percent(72),
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });
}

fn spawn_slider_block<Label: Component, Track: Component, Thumb: Component>(
    parent: &mut ChildSpawnerCommands,
    label_marker: Label,
    track_marker: Track,
    thumb_marker: Thumb,
    font: Handle<Font>,
    title: &'static str,
    value: String,
    thumb_left: f32,
    ticks: &[SliderTick],
) {
    parent
        .spawn(Node {
            width: percent(100),
            min_width: px(0),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            margin: UiRect::bottom(px(19)),
            ..default()
        })
        .with_children(|block| {
            spawn_value_row(block, label_marker, font.clone(), title, value);
            spawn_slider_with_visual(
                block,
                track_marker,
                thumb_marker,
                font,
                thumb_left,
                ticks,
                slider_visual(),
            );
        });
}

/// 环境页固定槽位出现时一次生成完整世界环境调试组。
pub(crate) fn setup_world_environment_controls(
    mut commands: Commands,
    slots: Query<Entity, Added<WorldEnvironmentControlsSlot>>,
    asset_server: Res<AssetServer>,
    clock: Res<WorldClock>,
    settings: Res<EditorSettings>,
) {
    for slot in &slots {
        let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH);
        commands.entity(slot).with_children(|parent| {
            let pause_label = if settings.environment.world_time_paused {
                "时间暂停：开"
            } else {
                "时间暂停：关"
            };
            spawn_custom_text_button(
                parent,
                WorldTimePauseToggle,
                font.clone(),
                pause_label,
                world_button_style(true),
            );
            spawn_slider_block(
                parent,
                WorldTimeSliderLabel,
                WorldTimeSlider,
                WorldTimeSliderThumb,
                font.clone(),
                "当前时间",
                format_hour(clock.hour_for_display()),
                hour_thumb_left(clock.hour_for_display()),
                &TIME_TICKS,
            );
            spawn_slider_block(
                parent,
                WorldMonthSliderLabel,
                WorldMonthSlider,
                WorldMonthSliderThumb,
                font.clone(),
                "当前月份",
                format!(
                    "{}月{}日 · {}",
                    clock.month(),
                    clock.day(),
                    clock.season_name()
                ),
                month_thumb_left(clock.month()),
                &MONTH_TICKS,
            );
            spawn_value_row(
                parent,
                WeatherRegionLabel,
                font.clone(),
                "天气区域",
                "当前天气 · 同步中".to_owned(),
            );
            spawn_slider_block(
                parent,
                WeatherModeSliderLabel,
                WeatherModeSlider,
                WeatherModeSliderThumb,
                font.clone(),
                "天气模式",
                "自动".to_owned(),
                weather_mode_thumb_left(0),
                &WEATHER_MODE_TICKS,
            );
            spawn_slider_block(
                parent,
                WeatherPreviewSliderLabel,
                WeatherPreviewSlider,
                WeatherPreviewSliderThumb,
                font.clone(),
                "过渡进度",
                "同步中".to_owned(),
                weather_preview_thumb_left(1.0),
                &WEATHER_PREVIEW_TICKS,
            );
            parent
                .spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    column_gap: px(4),
                    margin: UiRect::bottom(px(4)),
                    ..default()
                })
                .with_children(|row| {
                    spawn_custom_text_button(
                        row,
                        WeatherApplyImmediateButton,
                        font.clone(),
                        "立即应用",
                        world_button_style(false),
                    );
                    spawn_custom_text_button(
                        row,
                        WeatherRerollButton,
                        font.clone(), // 天气按钮之后仍需使用同一字体生成草地风摆滑块。
                        "重新随机天气",
                        world_button_style(false),
                    );
                });
            // 【功能设计】原F1摆幅成为晴天基准，实际风向、摆幅和速度均由同帧天气桥统一派生。
            spawn_slider_block(
                parent,               // 放在天气操作下方，避免与天空和天体作者参数混排。
                GrassWindSliderLabel, // 当前值仍从settings.json加载。
                GrassWindSlider,      // 拖动轨道沿用F12统一天气控件。
                GrassWindSliderThumb, // 手柄同帧跟随安全钳制的厘米档位。
                font.clone(),         // 复用环境页中文字体。
                "草尖摆幅调试",       // 说明此值不是天气风速，也不影响雨与云。
                format_grass_wind_strength(settings.environment.grass_wind_tip_strength), // 显示原持久值。
                grass_wind_thumb_left(settings.environment.grass_wind_tip_strength), // 保持打开时位置准确。
                &GRASS_WIND_TICKS, // 零至三十五厘米的原安全刻度。
            ); // 此控件只影响植被晴天基准摆幅，不改天气输出。
            parent.spawn((
                Text::new("晴天基准值 · 实际摆幅随天气变化"), // 区分持久调试标尺与当前天气派生值。
                TextFont {
                    font: font.into(),     // 保留环境页中文字体。
                    font_size: feathrise_editor_ui::metrics::FONT_CONTROL.into(), // 小号说明不抢天气状态主信息。
                    ..default()            // 其余字体值沿用公共默认。
                }, // 与右栏提示文案保持紧凑。
                TextColor(colors::TEXT_MUTED),                // 提示文字使用既有暗墨铜金色板。
                TextLayout {
                    linebreak: bevy::text::LineBreak::AnyCharacter,
                    ..default()
                },
                Node {
                    width: percent(100), // 随右侧天气栏宽度伸缩。
                    min_width: px(0),
                    flex_shrink: 0.0,
                    margin: UiRect::bottom(px(4)), // 与下方段落留出少量间距。
                    ..default()                    // 布局其余字段使用公共默认。
                }, // 给下方内容保留少量间距。
            )); // 完成草地风摆作用域说明。
        });
    }
}

type WorldSliderTrackFilter = Or<(
    With<WorldTimeSlider>,
    With<WorldMonthSlider>,
    With<WeatherModeSlider>,
    With<WeatherPreviewSlider>,
    With<GrassWindSlider>,
)>;

/// 五条原轨道与其固定刻度只在创建帧同比例转为百分比；保持260/14的原鼠标映射比例。
pub(crate) fn setup_world_environment_slider_layout(
    mut commands: Commands,
    tracks: Query<
        (Entity, &Node, &Children),
        (Added<RelativeCursorPosition>, WorldSliderTrackFilter),
    >,
    nodes: Query<&Node>,
) {
    for (entity, node, children) in &tracks {
        let mut fitted = node.clone();
        fitted.width = percent(100);
        fitted.min_width = px(0);
        fitted.max_width = percent(100);
        fitted.flex_shrink = 0.0;
        commands.entity(entity).insert(fitted);
        for child in children.iter() {
            let Ok(node) = nodes.get(child) else { continue };
            let mut fitted = node.clone();
            if let Val::Px(left) = fitted.left {
                fitted.left = percent(left / WORLD_SLIDER_TRACK_WIDTH * 100.0);
            }
            if let Val::Px(width) = fitted.width {
                fitted.width = percent(width / WORLD_SLIDER_TRACK_WIDTH * 100.0);
            }
            commands.entity(child).insert(fitted);
        }
    }
}
