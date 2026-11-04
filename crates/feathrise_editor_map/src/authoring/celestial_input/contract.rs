//! 天体光照编辑字段、继承属性、输入规格与跨帧事务普通类型。

use bevy::prelude::*;
use feathrise_map_content::{
    CelestialLightingProfile, MAX_EDITABLE_LIGHT_SOURCE_ANGULAR_DIAMETER_DEGREES,
    MIN_EDITABLE_LIGHT_SOURCE_ANGULAR_DIAMETER_DEGREES, MapCelestialLightingOverrides,
    MapEnvironmentDescriptor, MapId, resolve,
};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::MapEditorScrubField;
use feathrise_editor_ui::{
    NumericInputBoundaryMode, NumericInputScrubConfig, NumericInputScrubMapping, NumericInputSpec,
    NumericValue,
};

const SCRUB_SENSITIVITY_MIN: f32 = 0.01;
const SCRUB_SENSITIVITY_MAX: f32 = 100.0;
const ILLUMINANCE_SCRUB_MAX: f32 = 1_000_000.0; // 精确输入仍只要求有限非负；该值仅限制一次指针拖动。

/// 数值框修改全局基础配置或当前地图覆盖。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CelestialNumericScope {
    #[default]
    Global,
    Map,
}

/// 地图继承开关的六个原子属性；颜色三个通道共同继承或共同覆盖。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CelestialOverrideProperty {
    #[default]
    DayIlluminance,
    DayColor,
    NightIlluminance,
    NightColor,
    Azimuth,
    AngularDiameter,
}

impl CelestialOverrideProperty {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::DayIlluminance => "日间基准照度",
            Self::DayColor => "日间基准颜色",
            Self::NightIlluminance => "月亮午夜峰值照度",
            Self::NightColor => "夜间基准颜色",
            Self::Azimuth => "天体方位角偏移",
            Self::AngularDiameter => "光源角直径",
        }
    }

    pub(crate) fn is_overridden(self, overrides: &MapCelestialLightingOverrides) -> bool {
        match self {
            Self::DayIlluminance => overrides.day_base_illuminance_lux.is_some(),
            Self::DayColor => overrides.day_base_color_srgb.is_some(),
            Self::NightIlluminance => overrides.night_peak_illuminance_lux.is_some(),
            Self::NightColor => overrides.night_base_color_srgb.is_some(),
            Self::Azimuth => overrides.azimuth_offset_degrees.is_some(),
            Self::AngularDiameter => overrides.light_source_angular_diameter_degrees.is_some(),
        }
    }

    /// 启用时复制当前解析值，关闭时删除对应字段。
    pub(crate) fn set_overridden(
        self,
        overrides: &mut MapCelestialLightingOverrides,
        effective: &CelestialLightingProfile,
        enabled: bool,
    ) {
        match self {
            Self::DayIlluminance => {
                overrides.day_base_illuminance_lux =
                    enabled.then_some(effective.day_base_illuminance_lux)
            }
            Self::DayColor => {
                overrides.day_base_color_srgb = enabled.then_some(effective.day_base_color_srgb)
            }
            Self::NightIlluminance => {
                overrides.night_peak_illuminance_lux =
                    enabled.then_some(effective.night_peak_illuminance_lux)
            }
            Self::NightColor => {
                overrides.night_base_color_srgb = enabled.then_some(effective.night_base_color_srgb)
            }
            Self::Azimuth => {
                overrides.azimuth_offset_degrees =
                    enabled.then_some(effective.azimuth_offset_degrees)
            }
            Self::AngularDiameter => {
                overrides.light_source_angular_diameter_degrees =
                    enabled.then_some(effective.light_source_angular_diameter_degrees)
            }
        }
    }
}

/// 十个可精确输入的标量；两个颜色各由三个通道组成。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CelestialValueField {
    #[default]
    DayIlluminance,
    DayRed,
    DayGreen,
    DayBlue,
    NightIlluminance,
    NightRed,
    NightGreen,
    NightBlue,
    Azimuth,
    AngularDiameter,
}

impl CelestialValueField {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::DayIlluminance => "日间照度",
            Self::DayRed | Self::NightRed => "R",
            Self::DayGreen | Self::NightGreen => "G",
            Self::DayBlue | Self::NightBlue => "B",
            Self::NightIlluminance => "月光峰值",
            Self::Azimuth => "方位角",
            Self::AngularDiameter => "角直径",
        }
    }

    pub(crate) const fn unit(self) -> &'static str {
        match self {
            Self::DayIlluminance | Self::NightIlluminance => "lux",
            Self::Azimuth | Self::AngularDiameter => "°",
            _ => "",
        }
    }

    pub(crate) const fn property(self) -> CelestialOverrideProperty {
        match self {
            Self::DayIlluminance => CelestialOverrideProperty::DayIlluminance,
            Self::DayRed | Self::DayGreen | Self::DayBlue => CelestialOverrideProperty::DayColor,
            Self::NightIlluminance => CelestialOverrideProperty::NightIlluminance,
            Self::NightRed | Self::NightGreen | Self::NightBlue => {
                CelestialOverrideProperty::NightColor
            }
            Self::Azimuth => CelestialOverrideProperty::Azimuth,
            Self::AngularDiameter => CelestialOverrideProperty::AngularDiameter,
        }
    }

    pub(crate) const fn spec(self) -> NumericInputSpec {
        match self {
            Self::DayIlluminance | Self::NightIlluminance => NumericInputSpec::f32(0)
                .with_min(NumericValue::F32(0.0))
                .with_step(NumericValue::F32(1.0))
                .with_step_buttons(false)
                .with_live_update(false)
                .with_replace_on_first_key(true),
            Self::DayRed
            | Self::DayGreen
            | Self::DayBlue
            | Self::NightRed
            | Self::NightGreen
            | Self::NightBlue => NumericInputSpec::f32(3)
                .with_min(NumericValue::F32(0.0))
                .with_max(NumericValue::F32(1.0))
                .with_step(NumericValue::F32(0.001))
                .with_step_buttons(false)
                .with_live_update(false)
                .with_replace_on_first_key(true),
            Self::Azimuth => NumericInputSpec::f32(1)
                .with_min(NumericValue::F32(0.0))
                .with_max(NumericValue::F32(360.0))
                .with_step(NumericValue::F32(0.1))
                .with_step_buttons(false)
                .with_live_update(false)
                .with_replace_on_first_key(true)
                .with_boundary_mode(NumericInputBoundaryMode::Wrap),
            Self::AngularDiameter => NumericInputSpec::f32(3)
                .with_min(NumericValue::F32(
                    MIN_EDITABLE_LIGHT_SOURCE_ANGULAR_DIAMETER_DEGREES,
                ))
                .with_max(NumericValue::F32(
                    MAX_EDITABLE_LIGHT_SOURCE_ANGULAR_DIAMETER_DEGREES,
                ))
                .with_step(NumericValue::F32(0.001))
                .with_step_buttons(false)
                .with_live_update(false)
                .with_replace_on_first_key(true),
        }
    }

    const fn sensitivity_field(self) -> MapEditorScrubField {
        match self {
            Self::DayIlluminance | Self::NightIlluminance => MapEditorScrubField::SkyboxBrightness,
            Self::DayRed
            | Self::DayGreen
            | Self::DayBlue
            | Self::NightRed
            | Self::NightGreen
            | Self::NightBlue => MapEditorScrubField::EnvironmentDiffuseIntensity,
            Self::Azimuth | Self::AngularDiameter => MapEditorScrubField::SkyboxYaw,
        }
    }

    pub(crate) fn sensitivity(self, settings: &EditorSettings) -> f32 {
        let value = settings
            .map_editor_scrub_sensitivities
            .get(self.sensitivity_field());
        if value.is_finite() {
            value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX)
        } else {
            1.0
        }
    }

    pub(crate) fn set_sensitivity(self, settings: &mut EditorSettings, value: f32) {
        settings.map_editor_scrub_sensitivities.set(
            self.sensitivity_field(),
            value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX),
        );
    }

    pub(crate) fn scrub_config(self, settings: &EditorSettings) -> NumericInputScrubConfig {
        let (units_per_pixel, step, minimum, maximum) = match self {
            Self::DayIlluminance | Self::NightIlluminance => {
                (10.0, 1.0, 0.0, ILLUMINANCE_SCRUB_MAX)
            }
            Self::DayRed
            | Self::DayGreen
            | Self::DayBlue
            | Self::NightRed
            | Self::NightGreen
            | Self::NightBlue => (0.005, 0.001, 0.0, 1.0),
            Self::Azimuth => (0.25, 0.1, 0.0, 360.0),
            Self::AngularDiameter => (
                0.005,
                0.001,
                MIN_EDITABLE_LIGHT_SOURCE_ANGULAR_DIAMETER_DEGREES,
                MAX_EDITABLE_LIGHT_SOURCE_ANGULAR_DIAMETER_DEGREES,
            ),
        };
        NumericInputScrubConfig {
            mapping: NumericInputScrubMapping::Linear { units_per_pixel },
            drag_step: step,
            minimum,
            maximum,
            sensitivity: self.sensitivity(settings),
            minimum_sensitivity: SCRUB_SENSITIVITY_MIN,
            maximum_sensitivity: SCRUB_SENSITIVITY_MAX,
            drag_threshold_pixels: 4.0,
            sensitivity_pixels_per_octave: 48.0,
        }
    }

    pub(crate) const fn read(self, profile: &CelestialLightingProfile) -> f32 {
        match self {
            Self::DayIlluminance => profile.day_base_illuminance_lux,
            Self::DayRed => profile.day_base_color_srgb[0],
            Self::DayGreen => profile.day_base_color_srgb[1],
            Self::DayBlue => profile.day_base_color_srgb[2],
            Self::NightIlluminance => profile.night_peak_illuminance_lux,
            Self::NightRed => profile.night_base_color_srgb[0],
            Self::NightGreen => profile.night_base_color_srgb[1],
            Self::NightBlue => profile.night_base_color_srgb[2],
            Self::Azimuth => profile.azimuth_offset_degrees,
            Self::AngularDiameter => profile.light_source_angular_diameter_degrees,
        }
    }

    pub(crate) fn write(
        self,
        profile: &mut CelestialLightingProfile,
        value: f32,
    ) -> Result<(), String> {
        if !value.is_finite() {
            return Err(format!("{}必须是有限数字", self.label()));
        }
        match self {
            Self::DayIlluminance if value >= 0.0 => profile.day_base_illuminance_lux = value,
            Self::NightIlluminance if value >= 0.0 => profile.night_peak_illuminance_lux = value,
            Self::DayRed | Self::DayGreen | Self::DayBlue if (0.0..=1.0).contains(&value) => {
                let index = match self {
                    Self::DayRed => 0,
                    Self::DayGreen => 1,
                    _ => 2,
                };
                profile.day_base_color_srgb[index] = value;
            }
            Self::NightRed | Self::NightGreen | Self::NightBlue if (0.0..=1.0).contains(&value) => {
                let index = match self {
                    Self::NightRed => 0,
                    Self::NightGreen => 1,
                    _ => 2,
                };
                profile.night_base_color_srgb[index] = value;
            }
            Self::Azimuth => profile.azimuth_offset_degrees = value.rem_euclid(360.0),
            Self::AngularDiameter
                if (MIN_EDITABLE_LIGHT_SOURCE_ANGULAR_DIAMETER_DEGREES
                    ..=MAX_EDITABLE_LIGHT_SOURCE_ANGULAR_DIAMETER_DEGREES)
                    .contains(&value) =>
            {
                profile.light_source_angular_diameter_degrees = value;
            }
            Self::DayIlluminance | Self::NightIlluminance => {
                return Err(format!("{}必须是非负数", self.label()));
            }
            Self::AngularDiameter => {
                return Err(format!(
                    "光源角直径必须在 {:.2}°～{:.1}° 之间",
                    MIN_EDITABLE_LIGHT_SOURCE_ANGULAR_DIAMETER_DEGREES,
                    MAX_EDITABLE_LIGHT_SOURCE_ANGULAR_DIAMETER_DEGREES,
                ));
            }
            _ => return Err(format!("{}必须在 0～1 之间", self.label())),
        }
        profile.validate()
    }
}

/// 一个数值根的完整作用域与字段身份。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CelestialNumericKey {
    pub(crate) scope: CelestialNumericScope,
    pub(crate) field: CelestialValueField,
}

impl CelestialNumericKey {
    pub(crate) const fn new(scope: CelestialNumericScope, field: CelestialValueField) -> Self {
        Self { scope, field }
    }

    pub(crate) fn displayed_value(
        self,
        global: &CelestialLightingProfile,
        environment: Option<&MapEnvironmentDescriptor>,
    ) -> Option<f32> {
        let profile = match self.scope {
            CelestialNumericScope::Global => global.clone(),
            CelestialNumericScope::Map => resolve(global, &environment?.celestial_overrides),
        };
        Some(self.field.read(&profile))
    }

    pub(crate) fn map_editable(self, environment: &MapEnvironmentDescriptor) -> bool {
        self.scope == CelestialNumericScope::Map
            && self
                .field
                .property()
                .is_overridden(&environment.celestial_overrides)
    }
}

/// BSN 固定数值挂载槽。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CelestialNumericInputSlot(pub(crate) CelestialNumericKey);

/// 公共 NumericInput 根业务标记。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct CelestialNumericInput(pub(crate) CelestialNumericKey);

/// 地图覆盖切换按钮。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CelestialOverrideToggle(pub(crate) CelestialOverrideProperty);

/// 地图覆盖切换按钮的动态状态文字。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CelestialOverrideToggleLabel(pub(crate) CelestialOverrideProperty);

/// 全局或地图折叠标题按钮。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CelestialSectionToggle(pub(crate) CelestialNumericScope);

/// 折叠标题的动态文本。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CelestialSectionToggleLabel(pub(crate) CelestialNumericScope);

/// 折叠正文根。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CelestialSectionBody(pub(crate) CelestialNumericScope);

/// 日间或夜间颜色预览；字段用任一颜色通道表达目标颜色组。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CelestialColorSwatch {
    pub(crate) scope: CelestialNumericScope,
    pub(crate) day: bool,
}

/// 两个纵向折叠区的会话显隐状态。
#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct CelestialSectionState {
    pub(crate) global_expanded: bool,
    pub(crate) map_expanded: bool,
}

impl Default for CelestialSectionState {
    fn default() -> Self {
        Self {
            global_expanded: true,
            map_expanded: true,
        }
    }
}

impl CelestialSectionState {
    pub(crate) const fn expanded(self, scope: CelestialNumericScope) -> bool {
        match scope {
            CelestialNumericScope::Global => self.global_expanded,
            CelestialNumericScope::Map => self.map_expanded,
        }
    }

    pub(crate) fn toggle(&mut self, scope: CelestialNumericScope) {
        match scope {
            CelestialNumericScope::Global => self.global_expanded = !self.global_expanded,
            CelestialNumericScope::Map => self.map_expanded = !self.map_expanded,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CelestialContextStamp {
    pub(super) global_revision: u64,
    pub(super) map_id: Option<MapId>,
    pub(super) map_revision: Option<u64>,
}

#[derive(Clone, Debug)]
pub(super) struct CelestialNumericFocus {
    pub(super) entity: Entity,
    pub(super) key: CelestialNumericKey,
    pub(super) stamp: CelestialContextStamp,
}

#[derive(Clone, Debug)]
pub(super) enum CelestialScrubBefore {
    Global {
        profile: CelestialLightingProfile,
        dirty: bool,
    },
    Map {
        map_id: MapId,
        environment: MapEnvironmentDescriptor,
    },
}

#[derive(Clone, Debug)]
pub(super) struct CelestialNumericScrub {
    pub(super) entity: Entity,
    pub(super) key: CelestialNumericKey,
    pub(super) stamp: CelestialContextStamp,
    pub(super) before: CelestialScrubBefore,
    pub(super) initial_value: f32,
    pub(super) latest_value: f32,
    pub(super) aborted: bool,
}

/// 二十个字段共享一个字符焦点与一个左拖事务，防止跨字段串写。
#[derive(Resource, Default)]
pub(crate) struct CelestialNumericInputState {
    pub(super) focus: Option<CelestialNumericFocus>,
    pub(super) scrub: Option<CelestialNumericScrub>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabling_color_override_copies_the_whole_effective_color() {
        let global = CelestialLightingProfile::default();
        let mut overrides = MapCelestialLightingOverrides::default();
        CelestialOverrideProperty::DayColor.set_overridden(&mut overrides, &global, true);
        assert_eq!(
            overrides.day_base_color_srgb,
            Some(global.day_base_color_srgb)
        );
        CelestialOverrideProperty::DayColor.set_overridden(&mut overrides, &global, false);
        assert_eq!(overrides.day_base_color_srgb, None);
    }
}
