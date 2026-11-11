//! 新场景与旧动态草稿迁移共用的空水域文件，以及作者显式新建静水体的模板。

use feathrise_scene_content::{
    DEFAULT_SCENE_WATER_PERCEPTUAL_ROUGHNESS, SCENE_SCHEMA_VERSION, SceneId, SceneStillWaterKind,
    SceneWaterAsset, SceneWaterLayer,
}; // 直接构造运行时可热加载的正式水域文件，并明确旧默认层迁移后的静水类别。

/// 首张显式静水体的R8覆盖分辨率；世界尺寸只改变每纹素覆盖米数。
pub(crate) const DEFAULT_WATER_RESOLUTION: [u32; 2] = [512, 512];

/// 为一张动态场景建立合法空水域；所有水体都由作者随后显式创建。
pub(crate) fn default_water_asset(scene_id: &SceneId) -> SceneWaterAsset {
    SceneWaterAsset {
        schema_version: SCENE_SCHEMA_VERSION,
        scene_id: scene_id.clone(),
        layers: Vec::new(),     // 默认零静水体，作者可从列表新建第一张独立R8画布。
        rivers: Vec::new(),     // 河流样条同样只由作者显式创建。
        waterfalls: Vec::new(), // 新场景没有未经作者确认的高低水体连接。
    }
}

/// 构造作者点击“新建静水体”时使用的规格模板；调用方必须换成新的稳定ID和私有路径。
pub(crate) fn default_water_layer(folder: &str) -> SceneWaterLayer {
    let folder = folder.trim_end_matches('/');
    SceneWaterLayer {
        id: "lake".to_owned(),
        label: "湖泊".to_owned(),
        mask_source_path: format!("{folder}/water/lake.r8"),
        mask_texture_path: format!("{folder}/water/lake.ktx2"),
        resolution: DEFAULT_WATER_RESOLUTION,
        canvas_bounds: None, // 旧网格水沿用所属清单；独立根水在新建事务中冻结自己的世界范围。
        water_level: 0.0,
        still_kind: SceneStillWaterKind::Lake, // 首张R8画布仍采用水平湖泊语义，河流由独立样条表达。
        preset_id: String::new(), // 空目录也允许显式建水；未选预设时使用内联外观，不能猜测原游戏ID。
        flow_direction: [1.0, 0.0],
        flow_speed: 1.0,
        base_color_srgba: [0.055, 0.24, 0.29, 0.72],
        perceptual_roughness: DEFAULT_SCENE_WATER_PERCEPTUAL_ROUGHNESS, // 默认主要读取天空GGX链的清晰Mip0/1。
    }
}

/// 为依赖合法静水层的单元测试提供显式夹具，避免测试继续把生产默认值误认为一层。
#[cfg(test)]
pub(crate) fn water_asset_with_layer_for_tests(scene_id: &SceneId, folder: &str) -> SceneWaterAsset {
    let mut water = default_water_asset(scene_id);
    water.layers.push(default_water_layer(folder));
    water
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_water_asset_is_valid_and_starts_without_any_water_objects() {
        let scene_id = SceneId::parse("MAP-test").unwrap();
        let water = default_water_asset(&scene_id);
        assert!(water.validate(&scene_id).is_ok());
        assert!(water.layers.is_empty());
        assert!(water.rivers.is_empty());
        assert!(water.waterfalls.is_empty());
    }

    #[test]
    fn explicit_water_layer_uses_scene_private_r8_and_ktx2() {
        let layer = default_water_layer("scenes/MAP-test");
        assert_eq!(layer.mask_source_path, "scenes/MAP-test/water/lake.r8");
        assert_eq!(layer.mask_texture_path, "scenes/MAP-test/water/lake.ktx2");
        assert_eq!(layer.resolution, DEFAULT_WATER_RESOLUTION);
        assert!(layer.preset_id.is_empty());
        assert_eq!(
            layer.perceptual_roughness,
            DEFAULT_SCENE_WATER_PERCEPTUAL_ROUGHNESS,
        ); // 新建静水体必须直接获得清晰天空倒影基准。
    }
}
