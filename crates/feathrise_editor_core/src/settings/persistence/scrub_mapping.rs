// 设置持久化 — 场景编辑器公共数值框灵敏度转换。
// 保存与加载共用这一组对称映射，确保三十个逻辑字段不会漏存或串槽。

use super::{SceneEditorScrubSensitivities, SceneEditorScrubSensitivitySettings}; // 复用父模块已限定到测试面板构建的运行时与DTO类型。

/// 把运行时三十个独立灵敏度转为稳定的settings.json DTO。
pub(super) fn scene_editor_scrub_sensitivities_for_save(
    // 集中映射避免保存系统继续堆叠分支。
    source: &SceneEditorScrubSensitivities, // 当前运行时灵敏度集合。
) -> SceneEditorScrubSensitivitySettings {
    // 返回可由serde持久化的数据对象。
    SceneEditorScrubSensitivitySettings {
        // 每个逻辑字段保持自己的持久化槽位。
        brush_spacing: source.brush_spacing, // 公共间距倍率往返。
        brush_radius: source.brush_radius,   // 保存笔刷大小倍率。
        brush_strength: source.brush_strength, // 保存笔刷强度倍率。
        terrain_speed: source.terrain_speed, // 保存地形雕刻速度倍率。
        water_level: source.water_level,     // 保存静止水位倍率。
        mesh_water_flow_speed: source.mesh_water_flow_speed, // 保存整网格水体流速倍率。
        mesh_water_height_offset: source.mesh_water_height_offset, // 保存整网格水体世界高度倍率。
        placement_translation: source.placement_translation, // 保存显式资产世界平移倍率。
        placement_rotation: source.placement_rotation, // 保存显式资产Y轴旋转倍率。
        placement_scale: source.placement_scale, // 保存显式资产三轴缩放倍率。
        water_control_height: source.water_control_height, // 保存水网控制点高度倍率。
        water_control_width: source.water_control_width, // 保存水网控制点宽度倍率。
        water_control_depth: source.water_control_depth, // 保存水网控制点深度倍率。
        water_control_flow_speed: source.water_control_flow_speed, // 保存水网控制点流速倍率。
        vegetation_priority: source.vegetation_priority, // 保存植被层级优先级倍率。
        vegetation_weight: source.vegetation_weight, // 保存植被层组内权重倍率。
        vegetation_opacity: source.vegetation_opacity, // 保存植被层不透明度倍率。
        vegetation_group_capacity: source.vegetation_group_capacity, // 保存植被组权重容量倍率。
        vegetation_claim_threshold: source.vegetation_claim_threshold, // 保存植被组占用阈值倍率。
        boundary_thickness: source.boundary_thickness, // 保存场景玩法边界厚度倍率。
        skybox_brightness: source.skybox_brightness, // 保存天空盒可见亮度倍率。
        environment_diffuse_intensity: source.environment_diffuse_intensity, // 保存天空漫反射补光倍率。
        environment_specular_intensity: source.environment_specular_intensity, // 保存天空镜面反射倍率。
        skybox_yaw: source.skybox_yaw, // 保存场景天空盒方位角倍率。
        weather_preview_progress: source.weather_preview_progress, // 保存天气转换预览进度倍率。
        new_scene_width: source.new_scene_width, // 保存新场景宽度倍率。
        new_scene_length: source.new_scene_length, // 保存新场景长度倍率。
        new_scene_terrain_cell: source.new_scene_terrain_cell, // 保存新场景地形GLB网格间距倍率。
        new_scene_player_collision_radius: source.new_scene_player_collision_radius, // 保存玩家预载半径的独立倍率。
        new_scene_role_collision_radius: source.new_scene_role_collision_radius, // 保存角色粗级半径的独立倍率。
    } // 结束完整持久化映射。
} // 结束灵敏度保存转换。

/// 把settings.json DTO恢复为运行时三十个独立灵敏度。
pub(super) fn scene_editor_scrub_sensitivities_from_data(
    // 集中映射保证保存与加载按同一字段名对称。
    source: &SceneEditorScrubSensitivitySettings, // serde已为旧JSON补齐缺失的一倍默认值。
) -> SceneEditorScrubSensitivities {
    // 返回业务系统持有的强类型灵敏度集合。
    SceneEditorScrubSensitivities {
        // 一次替换完整运行时值，不依赖旧资源残留状态。
        brush_spacing: source.brush_spacing, // 公共间距倍率往返。
        brush_radius: source.brush_radius,   // 恢复笔刷大小倍率。
        brush_strength: source.brush_strength, // 恢复笔刷强度倍率。
        terrain_speed: source.terrain_speed, // 恢复地形雕刻速度倍率。
        water_level: source.water_level,     // 恢复静止水位倍率。
        mesh_water_flow_speed: source.mesh_water_flow_speed, // 恢复整网格水体流速倍率。
        mesh_water_height_offset: source.mesh_water_height_offset, // 恢复整网格水体世界高度倍率。
        placement_translation: source.placement_translation, // 恢复显式资产世界平移倍率。
        placement_rotation: source.placement_rotation, // 恢复显式资产Y轴旋转倍率。
        placement_scale: source.placement_scale, // 恢复显式资产三轴缩放倍率。
        water_control_height: source.water_control_height, // 恢复水网控制点高度倍率。
        water_control_width: source.water_control_width, // 恢复水网控制点宽度倍率。
        water_control_depth: source.water_control_depth, // 恢复水网控制点深度倍率。
        water_control_flow_speed: source.water_control_flow_speed, // 恢复水网控制点流速倍率。
        vegetation_priority: source.vegetation_priority, // 恢复植被层级优先级倍率。
        vegetation_weight: source.vegetation_weight, // 恢复植被层组内权重倍率。
        vegetation_opacity: source.vegetation_opacity, // 恢复植被层不透明度倍率。
        vegetation_group_capacity: source.vegetation_group_capacity, // 恢复植被组权重容量倍率。
        vegetation_claim_threshold: source.vegetation_claim_threshold, // 恢复植被组占用阈值倍率。
        boundary_thickness: source.boundary_thickness, // 恢复场景玩法边界厚度倍率。
        skybox_brightness: source.skybox_brightness, // 恢复天空盒可见亮度倍率。
        environment_diffuse_intensity: source.environment_diffuse_intensity, // 恢复天空漫反射补光倍率。
        environment_specular_intensity: source.environment_specular_intensity, // 恢复天空镜面反射倍率。
        skybox_yaw: source.skybox_yaw, // 恢复场景天空盒方位角倍率。
        weather_preview_progress: source.weather_preview_progress, // 恢复天气转换预览进度倍率。
        new_scene_width: source.new_scene_width, // 恢复新场景宽度倍率。
        new_scene_length: source.new_scene_length, // 恢复新场景长度倍率。
        new_scene_terrain_cell: source.new_scene_terrain_cell, // 恢复新场景地形GLB网格间距倍率。
        new_scene_player_collision_radius: source.new_scene_player_collision_radius, // 恢复玩家预载半径的独立倍率。
        new_scene_role_collision_radius: source.new_scene_role_collision_radius, // 恢复角色粗级半径的独立倍率。
    } // 结束完整运行时映射。
} // 结束灵敏度加载转换。
