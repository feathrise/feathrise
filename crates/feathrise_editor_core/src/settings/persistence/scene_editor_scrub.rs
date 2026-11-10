// 设置数据类型 — 场景编辑器公共数值框灵敏度 DTO。
// 独立维护二十九个持久化槽位及其旧配置迁移，父模块只负责稳定公开路径。

use serde::{Deserialize, Serialize}; // 每个字段都直接参与 settings.json 序列化与反序列化。

/// F12场景编辑器各个可拖动数值框的独立灵敏度倍率。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(from = "SceneEditorScrubSensitivitySettingsSerde")] // 旧单一环境倍率读取后复制到补光与反射，保存只写新字段。
pub struct SceneEditorScrubSensitivitySettings {
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub brush_spacing: f32, // 点位最小间距倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub brush_radius: f32, // 笔刷大小的相对指数拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub brush_strength: f32, // 笔刷强度的线性拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub terrain_speed: f32, // 雕刻速度的相对指数拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub water_level: f32, // 静止水位的米制线性拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub mesh_water_flow_speed: f32, // Blender整网格水体流速的线性拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub mesh_water_height_offset: f32, // Blender整网格水体世界高度偏移的米制拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub placement_translation: f32, // 显式资产三轴世界平移共用的米制拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub placement_rotation: f32, // 显式资产Y轴旋转角的拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub placement_scale: f32, // 显式资产三轴缩放共用的拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有水网控制点高度时补标准一倍手感。
    pub water_control_height: f32, // 水网控制点世界高度的米制拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有水网宽度时补标准一倍手感。
    pub water_control_width: f32, // 水网控制点水面宽度的拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有水网深度时补标准一倍手感。
    pub water_control_depth: f32, // 水网控制点最低稳定水深的米制拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有水网流速时补标准一倍手感。
    pub water_control_flow_speed: f32, // 水网控制点流速的米每秒拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有植被优先级时补标准一倍手感。
    pub vegetation_priority: f32, // 植被层级整数优先级的拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有植被权重时补标准一倍手感。
    pub vegetation_weight: f32, // 植被层组内权重的拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有植被不透明度时补标准一倍手感。
    pub vegetation_opacity: f32, // 植被层不透明度的百分比拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有植被组容量时补标准一倍手感。
    pub vegetation_group_capacity: f32, // 植被组权重容量的百分比拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有植被占用阈值时补标准一倍手感。
    pub vegetation_claim_threshold: f32, // 植被组优先占用阈值的百分比拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有边界厚度时补标准一倍手感。
    pub boundary_thickness: f32, // 场景玩法边界厚度的米制拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有天空亮度时补标准一倍手感。
    pub skybox_brightness: f32, // 场景天空盒可见亮度的拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有天空补光倍率时优先迁移原环境照明倍率，再补标准一倍手感。
    pub environment_diffuse_intensity: f32, // 场景天空漫反射补光强度的拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有天空反射倍率时优先迁移原环境照明倍率，再补标准一倍手感。
    pub environment_specular_intensity: f32, // 场景天空镜面反射强度的拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有天空方位角时补标准一倍手感。
    pub skybox_yaw: f32, // 场景天空盒水平旋转角的拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有天气预览进度时补标准一倍手感。
    pub weather_preview_progress: f32, // 天气转换预览进度的百分比拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有新场景宽度时补标准一倍手感。
    pub new_scene_width: f32, // 新场景世界宽度的米制拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有新场景长度时补标准一倍手感。
    pub new_scene_length: f32, // 新场景世界长度的米制拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    // 旧配置没有地形网格间距时补标准一倍手感，不改变既有场景创建输入。
    pub new_scene_terrain_cell: f32, // 新场景地形GLB网格间距的米制拖动倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub new_scene_player_collision_radius: f32, // 分块碰撞玩家预载半径的独立倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    pub new_scene_role_collision_radius: f32, // 旧设置缺字段时角色粗级半径使用标准一倍手感。
}

impl Default for SceneEditorScrubSensitivitySettings {
    fn default() -> Self {
        Self {
            brush_spacing: default_scene_editor_scrub_sensitivity(),
            brush_radius: default_scene_editor_scrub_sensitivity(), // 默认保留字段声明的标准指数手感。
            brush_strength: default_scene_editor_scrub_sensitivity(), // 默认每像素使用标准百分比增量。
            terrain_speed: default_scene_editor_scrub_sensitivity(), // 默认保留字段声明的标准指数手感。
            water_level: default_scene_editor_scrub_sensitivity(),   // 默认每像素调整标准米制增量。
            mesh_water_flow_speed: default_scene_editor_scrub_sensitivity(), // 默认使用流速字段的标准线性增量。
            mesh_water_height_offset: default_scene_editor_scrub_sensitivity(), // 默认使用世界高度字段的标准米制增量。
            placement_translation: default_scene_editor_scrub_sensitivity(), // 默认使用显式资产世界平移的标准米制增量。
            placement_rotation: default_scene_editor_scrub_sensitivity(), // 默认使用显式资产Y轴角度的标准增量。
            placement_scale: default_scene_editor_scrub_sensitivity(), // 默认使用显式资产三轴缩放的标准增量。
            water_control_height: default_scene_editor_scrub_sensitivity(), // 默认使用水网控制点高度的标准米制增量。
            water_control_width: default_scene_editor_scrub_sensitivity(), // 默认使用水网宽度的标准拖动手感。
            water_control_depth: default_scene_editor_scrub_sensitivity(), // 默认使用水网深度的标准米制增量。
            water_control_flow_speed: default_scene_editor_scrub_sensitivity(), // 默认使用水网流速的标准米每秒增量。
            vegetation_priority: default_scene_editor_scrub_sensitivity(), // 默认使用植被整数优先级的标准步长。
            vegetation_weight: default_scene_editor_scrub_sensitivity(), // 默认使用植被权重的标准拖动手感。
            vegetation_opacity: default_scene_editor_scrub_sensitivity(), // 默认使用植被不透明度的标准百分比增量。
            vegetation_group_capacity: default_scene_editor_scrub_sensitivity(), // 默认使用植被组容量的标准百分比增量。
            vegetation_claim_threshold: default_scene_editor_scrub_sensitivity(), // 默认使用植被占用阈值的标准百分比增量。
            boundary_thickness: default_scene_editor_scrub_sensitivity(), // 默认使用场景边界厚度的标准米制增量。
            skybox_brightness: default_scene_editor_scrub_sensitivity(), // 默认使用天空亮度的标准增量。
            environment_diffuse_intensity: default_scene_editor_scrub_sensitivity(), // 默认使用天空漫反射补光的标准增量。
            environment_specular_intensity: default_scene_editor_scrub_sensitivity(), // 默认使用天空镜面反射的标准增量。
            skybox_yaw: default_scene_editor_scrub_sensitivity(), // 默认使用天空方位角的标准角度增量。
            weather_preview_progress: default_scene_editor_scrub_sensitivity(), // 默认使用天气转换预览的标准百分比增量。
            new_scene_width: default_scene_editor_scrub_sensitivity(), // 默认使用新场景宽度的标准米制增量。
            new_scene_length: default_scene_editor_scrub_sensitivity(), // 默认使用新场景长度的标准米制增量。
            new_scene_terrain_cell: default_scene_editor_scrub_sensitivity(), // 默认使用地形GLB网格间距的标准米制增量。
            new_scene_player_collision_radius: default_scene_editor_scrub_sensitivity(), // 旧设置缺字段时沿用标准手感。
            new_scene_role_collision_radius: default_scene_editor_scrub_sensitivity(), // 新角色粗级输入默认独立一倍手感。
        }
    }
}

fn default_scene_editor_scrub_sensitivity() -> f32 {
    1.0 // 一倍表示完全使用每个字段自己的标准米制、百分比或指数映射。
}

/// 仅用于兼容读取的扁平设置形状；两个新环境字段保持Option以识别是否应采用旧值。
#[derive(Deserialize, Default)]
struct SceneEditorScrubSensitivitySettingsSerde {
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    brush_spacing: f32, // 旧设置缺少时补一倍。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    brush_radius: f32, // 旧配置缺少时补一倍笔刷大小手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    brush_strength: f32, // 旧配置缺少时补一倍笔刷强度手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    terrain_speed: f32, // 旧配置缺少时补一倍雕刻速度手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    water_level: f32, // 旧配置缺少时补一倍静止水位手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    mesh_water_flow_speed: f32, // 旧配置缺少时补一倍整网格水流速手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    mesh_water_height_offset: f32, // 旧配置缺少时补一倍整网格水高度手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    placement_translation: f32, // 旧配置缺少时补一倍显式资产平移手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    placement_rotation: f32, // 旧配置缺少时补一倍显式资产旋转手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    placement_scale: f32, // 旧配置缺少时补一倍显式资产缩放手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    water_control_height: f32, // 旧配置缺少时补一倍水网高度手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    water_control_width: f32, // 旧配置缺少时补一倍水网宽度手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    water_control_depth: f32, // 旧配置缺少时补一倍水网深度手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    water_control_flow_speed: f32, // 旧配置缺少时补一倍水网流速手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    vegetation_priority: f32, // 旧配置缺少时补一倍植被优先级手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    vegetation_weight: f32, // 旧配置缺少时补一倍植被权重手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    vegetation_opacity: f32, // 旧配置缺少时补一倍植被不透明度手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    vegetation_group_capacity: f32, // 旧配置缺少时补一倍植被组容量手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    vegetation_claim_threshold: f32, // 旧配置缺少时补一倍植被占用阈值手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    boundary_thickness: f32, // 旧配置缺少时补一倍边界厚度手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    skybox_brightness: f32, // 旧配置缺少时补一倍天空背景手感。
    #[serde(default)]
    environment_diffuse_intensity: Option<f32>, // 新补光倍率存在时优先采用。
    #[serde(default)]
    environment_specular_intensity: Option<f32>, // 新反射倍率存在时优先采用。
    #[serde(default)]
    environment_light_intensity: Option<f32>, // 旧单一倍率只在迁移边界读取。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    skybox_yaw: f32, // 旧配置缺少时补一倍天空方向手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    weather_preview_progress: f32, // 旧配置缺少时补一倍天气预览手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    new_scene_width: f32, // 旧配置缺少时补一倍新场景宽度手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    new_scene_length: f32, // 旧配置缺少时补一倍新场景长度手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    new_scene_terrain_cell: f32, // 旧配置缺少时补一倍地形GLB网格间距手感。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    new_scene_player_collision_radius: f32, // 旧配置缺字段时只补本字段，不串用格距倍率。
    #[serde(default = "default_scene_editor_scrub_sensitivity")]
    new_scene_role_collision_radius: f32, // 旧配置缺字段时只补角色粗级半径倍率。
}

impl From<SceneEditorScrubSensitivitySettingsSerde> for SceneEditorScrubSensitivitySettings {
    fn from(source: SceneEditorScrubSensitivitySettingsSerde) -> Self {
        let legacy = source.environment_light_intensity; // 同一旧倍率可分别兜底两个新槽位。
        Self {
            brush_spacing: source.brush_spacing,
            brush_radius: source.brush_radius, // 复制笔刷大小倍率。
            brush_strength: source.brush_strength, // 复制笔刷强度倍率。
            terrain_speed: source.terrain_speed, // 复制雕刻速度倍率。
            water_level: source.water_level,   // 复制静止水位倍率。
            mesh_water_flow_speed: source.mesh_water_flow_speed, // 复制整网格水流速倍率。
            mesh_water_height_offset: source.mesh_water_height_offset, // 复制整网格水高度倍率。
            placement_translation: source.placement_translation, // 复制显式资产平移倍率。
            placement_rotation: source.placement_rotation, // 复制显式资产旋转倍率。
            placement_scale: source.placement_scale, // 复制显式资产缩放倍率。
            water_control_height: source.water_control_height, // 复制水网高度倍率。
            water_control_width: source.water_control_width, // 复制水网宽度倍率。
            water_control_depth: source.water_control_depth, // 复制水网深度倍率。
            water_control_flow_speed: source.water_control_flow_speed, // 复制水网流速倍率。
            vegetation_priority: source.vegetation_priority, // 复制植被优先级倍率。
            vegetation_weight: source.vegetation_weight, // 复制植被权重倍率。
            vegetation_opacity: source.vegetation_opacity, // 复制植被不透明度倍率。
            vegetation_group_capacity: source.vegetation_group_capacity, // 复制植被组容量倍率。
            vegetation_claim_threshold: source.vegetation_claim_threshold, // 复制植被占用阈值倍率。
            boundary_thickness: source.boundary_thickness, // 复制边界厚度倍率。
            skybox_brightness: source.skybox_brightness, // 复制天空背景倍率。
            environment_diffuse_intensity: source
                .environment_diffuse_intensity
                .or(legacy)
                .unwrap_or_else(default_scene_editor_scrub_sensitivity), // 旧单倍率完整迁移到补光槽。
            environment_specular_intensity: source
                .environment_specular_intensity
                .or(legacy)
                .unwrap_or_else(default_scene_editor_scrub_sensitivity), // 旧单倍率完整迁移到反射槽。
            skybox_yaw: source.skybox_yaw,     // 复制天空方向倍率。
            weather_preview_progress: source.weather_preview_progress, // 复制天气预览倍率。
            new_scene_width: source.new_scene_width, // 复制新场景宽度倍率。
            new_scene_length: source.new_scene_length, // 复制新场景长度倍率。
            new_scene_terrain_cell: source.new_scene_terrain_cell, // 复制地形GLB网格间距倍率。
            new_scene_player_collision_radius: source.new_scene_player_collision_radius, // 独立恢复玩家预载半径倍率。
            new_scene_role_collision_radius: source.new_scene_role_collision_radius, // 独立恢复角色粗级预载半径倍率。
        }
    }
}
