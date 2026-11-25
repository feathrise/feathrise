//! 作者灯光的 SDK 能力边界；旧公开 SDK 仍可独立编译并显示其支持的创建菜单。

use feathrise_scene_content::ScenePlacementAssetTarget;

// 按 KHR_lights_punctual 的单位传递线性 RGB、坎德拉、可选范围和聚光半角。
pub(crate) type Parameters = ([f32; 3], f32, Option<f32>, Option<[f32; 2]>);

#[allow(dead_code)]
trait LegacyLighting {
    const SUPPORTS_LOCAL_LIGHTS: bool = false;
    fn from_local_light(_: [f32; 3], _: f32, _: Option<f32>, _: Option<[f32; 2]>) -> Result<Self, String> where Self: Sized {
        Err("当前 SDK 尚不支持场景局部灯光".into())
    }
    fn local_light_parameters(&self) -> Option<Parameters> { None }
}
impl LegacyLighting for ScenePlacementAssetTarget {}

pub(crate) const AVAILABLE: bool = ScenePlacementAssetTarget::SUPPORTS_LOCAL_LIGHTS;

pub(crate) fn parameters(target: &ScenePlacementAssetTarget) -> Option<Parameters> { target.local_light_parameters() }

pub(crate) fn is_builtin_mesh(target: &ScenePlacementAssetTarget) -> bool {
    target.is_builtin() && parameters(target).is_none()
}

pub(crate) fn target((color, intensity, range, cone): Parameters) -> Result<ScenePlacementAssetTarget, String> {
    ScenePlacementAssetTarget::from_local_light(color, intensity, range, cone)
}

pub(crate) fn new(spot: bool) -> Result<ScenePlacementAssetTarget, String> {
    target(([1.0; 3], 1000.0, Some(12.0), spot.then_some([0.0, std::f32::consts::FRAC_PI_4])))
}

pub(crate) fn label(target: &ScenePlacementAssetTarget) -> Option<&'static str> {
    parameters(target).map(|(_, _, _, cone)| if cone.is_some() { "聚光灯" } else { "点光源" })
}
