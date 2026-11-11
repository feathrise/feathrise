//! 复用正式昼夜曲线解析环境预览计算值；不负责加载、光照写入或保存。

use feathrise_atmosphere::{
    WorldClock, WorldEnvironmentModifiers, dynamic_day_night_factor,
    time::constants::NIGHT_SKY_BRIGHTNESS_FACTOR,
};
use feathrise_scene_content::SceneEnvironmentDescriptor;

use super::types::CurrentIblValues;
/// 使用正式运行时同一系数解析场景 IBL 日间基础值，禁止 F12 形成第二套预览算法。
pub(super) fn current_ibl_values(
    environment: &SceneEnvironmentDescriptor, // 两个独立 IBL 日间基础值来自当前作者草稿。
    clock: &WorldClock,                     // 当前世界时刻决定两个环境通道的共同自然进度。
    modifiers: &WorldEnvironmentModifiers,  // 当前天气在自然昼夜结果之后共同压暗或增强两通道。
) -> CurrentIblValues {
    let day_night_factor = dynamic_day_night_factor(clock); // 复用公开运行时纯函数，任何曲线修改都会自动同步到F12。
    let natural_sky_factor =
        NIGHT_SKY_BRIGHTNESS_FACTOR + (1.0 - NIGHT_SKY_BRIGHTNESS_FACTOR) * day_night_factor; // 午夜保留百分之二基础值，正午恢复完整基础值。
    let current_factor = natural_sky_factor * modifiers.sky_brightness_factor.max(0.0); // 与正式 EnvironmentMapLight 使用相同天气钳制。
    CurrentIblValues {
        day_night_factor, // 单独回显时间系数，避免作者把天气压暗误解成曲线变化。
        diffuse: environment.environment_diffuse_intensity * current_factor, // 解析漫反射天空补光当前值。
        specular: environment.environment_specular_intensity * current_factor, // 解析镜面天空反射当前值。
        skybox: environment.skybox_brightness * current_factor, // 可见 Cube 与两张 IBL 使用同一昼夜及天气倍率。
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 直接验证F12摘要的纯值解析，不建立完整Bevy UI世界。
    use bevy::prelude::default;
    use feathrise_atmosphere::WorldTimeSnapshot; // 构造用户重点关注的十九点世界时刻。

    #[test] // F12环境显示值必须与动态场景运行时的统一环境昼夜系数和天气倍率完全一致。
    fn current_ibl_summary_uses_runtime_day_night_and_weather_factors() {
        let environment = SceneEnvironmentDescriptor {
            skybox_brightness: 531.0, // 天空盒独立作者值不应改变 IBL 当前值。
            environment_diffuse_intensity: 380.0, // 复用问题截图中的天空补光日间基础值。
            environment_specular_intensity: 320.0, // 复用问题截图中的天空反射日间基础值。
            ..default()               // 其他场景环境字段不参与当前值解析。
        };
        let clock = WorldClock::from_snapshot(WorldTimeSnapshot {
            month: 6,                               // 月份不改变统一时刻系数。
            day: 15,                                // 日期不改变统一时刻系数。
            seconds_since_midnight: 19.0 * 3_600.0, // 精确采样用户指出过暗的十九点。
        });
        let modifiers = WorldEnvironmentModifiers {
            sky_brightness_factor: 0.5, // 用非中性天气倍率证明当前值确实包含天气层。
            ..default()                 // 其他天气修正不属于环境 IBL 两项亮度。
        };
        let current = current_ibl_values(&environment, &clock, &modifiers); // 解析 F12 将要显示的实际值。
        let expected_time_factor = 0.370_590_45; // 十九点唯一余弦昼夜系数的固定验收值。
        let expected_current_factor = (NIGHT_SKY_BRIGHTNESS_FACTOR
            + (1.0 - NIGHT_SKY_BRIGHTNESS_FACTOR) * expected_time_factor)
            * 0.5; // 午夜安全端点、十九点进度和天气五成倍率共同组成实际倍率。
        assert!((current.day_night_factor - expected_time_factor).abs() < 0.000_001); // 摘要百分比必须来自正式公开函数。
        assert!((current.diffuse - 380.0 * expected_current_factor).abs() < 0.001); // 补光当前值必须正确。
        assert!((current.specular - 320.0 * expected_current_factor).abs() < 0.001); // 反射当前值必须正确。
        assert!((current.skybox - 531.0 * expected_current_factor).abs() < 0.001); // 天空盒模式可见亮度使用完全相同的统一倍率。
    }
}
