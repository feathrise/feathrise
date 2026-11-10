//! 全局LOD卡片固定控件身份；字段不会随所选场景或网格对象改变目标。

use bevy::prelude::*;
use feathrise_settings::MeshLodSettings;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MeshLodField {
    #[default]
    ScreenError,
    Hysteresis,
    SafeNear,
    CollisionError,
}

impl MeshLodField {
    pub(crate) fn value(self, value: MeshLodSettings) -> f32 {
        match self {
            Self::ScreenError => value.max_error_pixels,
            Self::Hysteresis => value.hysteresis_ratio,
            Self::SafeNear => value.collision_safe_near_meters,
            Self::CollisionError => value.collision_max_error_meters,
        }
    }

    pub(super) fn set(self, settings: &mut MeshLodSettings, value: f32) {
        match self {
            Self::ScreenError => settings.max_error_pixels = value,
            Self::Hysteresis => settings.hysteresis_ratio = value,
            Self::SafeNear => settings.collision_safe_near_meters = value,
            Self::CollisionError => settings.collision_max_error_meters = value,
        }
    }

    pub(super) fn config(self) -> (&'static str, &'static str, f32, f32, f32) {
        match self {
            Self::ScreenError => ("屏幕误差", "px", 0.1, 16.0, 0.1),
            Self::Hysteresis => ("切换迟滞", "比例", 0.0, 0.5, 0.01),
            Self::SafeNear => ("安全近区", "m", 32.0, 4096.0, 1.0),
            Self::CollisionError => ("碰撞误差", "m", 0.0001, 0.25, 0.001),
        }
    }
}

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct Slot(pub(super) MeshLodField);
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Action {
    #[default]
    Display,
    Collision,
    Reset,
}
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ActionButton(pub(super) Action);
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct ActionLabel(pub(super) Action);
