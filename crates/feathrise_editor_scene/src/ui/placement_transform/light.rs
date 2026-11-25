//! 局部灯光参数直接复用场景属性的数值拖动、精确输入和整条摆放历史。

use crate::authoring::local_lights;
use feathrise_scene_content::ScenePlacementRecord;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LightField { Red, Green, Blue, Intensity, Range, InnerAngle, OuterAngle }

impl LightField {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Red => "R", Self::Green => "G", Self::Blue => "B", Self::Intensity => "光强",
            Self::Range => "照射范围", Self::InnerAngle => "内锥半角", Self::OuterAngle => "外锥半角",
        }
    }

    pub(super) const fn unit(self) -> &'static str {
        match self {
            Self::Intensity => "cd", Self::Range => "m",
            Self::InnerAngle | Self::OuterAngle => "°", _ => "",
        }
    }

    pub(super) const fn limits(self) -> (f32, f32) {
        match self {
            Self::Intensity => (0.0, f32::MAX / (4.0 * std::f32::consts::PI)),
            Self::Range => (f32::MIN_POSITIVE, f32::MAX),
            Self::InnerAngle | Self::OuterAngle => (0.0, 90.0),
            _ => (0.0, 1.0),
        }
    }

    pub(super) fn value(self, record: &ScenePlacementRecord) -> Option<f32> {
        let (rgb, intensity, range, cone) = local_lights::parameters(&record.asset_target)?;
        Some(match self {
            Self::Red => rgb[0], Self::Green => rgb[1], Self::Blue => rgb[2], Self::Intensity => intensity,
            Self::Range => range.unwrap_or(20.0), Self::InnerAngle => cone?[0].to_degrees(), Self::OuterAngle => cone?[1].to_degrees(),
        })
    }

    pub(super) fn set(self, record: &mut ScenePlacementRecord, value: f32) -> Result<bool, String> {
        let (mut rgb, mut intensity, mut range, mut cone) =
            local_lights::parameters(&record.asset_target).ok_or("当前选中对象不是局部灯光")?;
        match self {
            Self::Red => rgb[0] = value, Self::Green => rgb[1] = value, Self::Blue => rgb[2] = value,
            Self::Intensity => intensity = value, Self::Range => range = Some(value),
            Self::InnerAngle | Self::OuterAngle => {
                let angles = cone.as_mut().ok_or("请选择聚光灯")?;
                angles[usize::from(self == Self::OuterAngle)] = value.to_radians();
            }
        }
        let replacement = local_lights::target((rgb, intensity, range, cone))?;
        let changed = record.asset_target != replacement;
        if changed { record.asset_target = replacement; }
        Ok(changed)
    }
}
