//! 仅在作者显式新建且目录为空时提供中性草稿，不向空工程自动安装任何预设。

use feathrise_map_content::{
    DEFAULT_MAP_WATER_PERCEPTUAL_ROUGHNESS, WaterMacroWave, WaterMicroNormalLayer,
    WaterPresetDescriptor,
};

pub(super) fn new_descriptor() -> WaterPresetDescriptor {
    WaterPresetDescriptor {
        id: String::new(), // 新建入口统一分配工程内唯一 ID；默认参数不是隐藏的目录项。
        aliases: Vec::new(),
        label: "新水体预设".into(),
        base_color_srgba: [0.055, 0.24, 0.29, 0.72],
        perceptual_roughness: DEFAULT_MAP_WATER_PERCEPTUAL_ROUGHNESS,
        reflectance: 0.5,
        trough_darkening: 0.1,
        shore_wave_attenuation: 1.0,
        // 初始平静水面不依赖游戏风浪参数；作者可在原四波表单逐项开启。
        macro_waves: [WaterMacroWave {
            direction: [1.0, 0.0],
            amplitude_meters: 0.0,
            wavelength_meters: 4.0,
            speed_meters_per_second: 0.0,
            steepness: 0.0,
        }; 4],
        micro_normals: std::array::from_fn(|_| WaterMicroNormalLayer {
            texture_path: None,
            tile_meters: 1.0,
            velocity_meters_per_second: [0.0, 0.0],
            strength: 0.0,
        }),
    }
}
