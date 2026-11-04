//! F12 世界环境控件的局部 ECS 标记。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct WorldEnvironmentControlsSlot;
#[derive(Component, Default)]
pub(crate) struct WorldTimePauseToggle;
#[derive(Component, Default)]
pub(crate) struct WorldTimeSlider;
#[derive(Component, Default)]
pub(crate) struct WorldTimeSliderThumb;
#[derive(Component, Default)]
pub(crate) struct WorldTimeSliderLabel;
#[derive(Component, Default)]
pub(crate) struct WorldMonthSlider;
#[derive(Component, Default)]
pub(crate) struct WorldMonthSliderThumb;
#[derive(Component, Default)]
pub(crate) struct WorldMonthSliderLabel;
#[derive(Component, Default)]
pub(crate) struct WeatherModeSlider;
#[derive(Component, Default)]
pub(crate) struct WeatherModeSliderThumb;
#[derive(Component, Default)]
pub(crate) struct WeatherModeSliderLabel;
#[derive(Component, Default)]
pub(crate) struct WeatherPreviewSlider;
#[derive(Component, Default)]
pub(crate) struct WeatherPreviewSliderThumb;
#[derive(Component, Default)]
pub(crate) struct WeatherPreviewSliderLabel;
#[derive(Component, Default)]
pub(crate) struct WeatherRegionLabel;
#[derive(Component, Default)]
pub(crate) struct WeatherApplyImmediateButton;
#[derive(Component, Default)]
pub(crate) struct WeatherRerollButton;
#[derive(Component, Default)] // F12天气栏只调全局晴天草尖基准摆幅。
pub(crate) struct GrassWindSlider;
#[derive(Component, Default)] // 手柄只显示原设置的厘米档位。
pub(crate) struct GrassWindSliderThumb;
#[derive(Component, Default)] // 标签回显持久化全局设置而非天气风速。
pub(crate) struct GrassWindSliderLabel;
