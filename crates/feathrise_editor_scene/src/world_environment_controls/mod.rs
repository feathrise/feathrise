//! F12 世界时间、月份与天气调试覆盖控件。
//!
//! 【功能设计】这里迁移原 F1“世界环境”整组；时钟和月份直接写 `WorldClock`，暂停继续
//! 写 `settings.json`，天气模式及过渡比例只写运行时 `WeatherDebugOverride`，不污染场景。

mod components;
mod helpers;
mod scene;
mod systems;

pub(crate) use components::WorldEnvironmentControlsSlot;
pub(crate) use scene::{setup_world_environment_controls, setup_world_environment_slider_layout};
pub(crate) use systems::{
    handle_grass_wind_slider, handle_world_environment_controls, sync_world_environment_controls,
}; // 滑块输入独立于时间天气大系统。
