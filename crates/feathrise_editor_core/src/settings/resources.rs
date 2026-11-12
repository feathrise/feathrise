//! 编辑器用户偏好、数值拖动手感及界面倍率；与游戏配置文件分离。
use super::*;
use bevy::prelude::*;

/// F12场景编辑器各个可拖动数值框的独立用户灵敏度倍率。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneEditorScrubSensitivities {
    pub brush_spacing: f32,                   // 点位最小间距独立倍率。
    pub brush_radius: f32,                    // 笔刷大小的相对指数拖动倍率。
    pub brush_strength: f32,                  // 笔刷强度的线性拖动倍率。
    pub terrain_speed: f32,                   // 雕刻速度的相对指数拖动倍率。
    pub water_level: f32,                     // 静止水位的米制线性拖动倍率。
    pub mesh_water_flow_speed: f32,           // Blender整网格水体流速的线性拖动倍率。
    pub mesh_water_height_offset: f32,        // Blender整网格水体世界高度偏移的米制拖动倍率。
    pub placement_translation: f32,           // 显式资产三轴世界平移共用的米制拖动倍率。
    pub placement_rotation: f32,              // 显式资产Y轴旋转角的拖动倍率。
    pub placement_scale: f32,                 // 显式资产三轴缩放共用的拖动倍率。
    pub water_control_height: f32,            // 水网控制点世界高度的米制拖动倍率。
    pub water_control_width: f32,             // 水网控制点水面宽度的拖动倍率。
    pub water_control_depth: f32,             // 水网控制点最低稳定水深的米制拖动倍率。
    pub water_control_flow_speed: f32,        // 水网控制点流速的米每秒拖动倍率。
    pub vegetation_priority: f32,             // 植被层级整数优先级的拖动倍率。
    pub vegetation_weight: f32,               // 植被层组内权重的拖动倍率。
    pub vegetation_opacity: f32,              // 植被层不透明度的百分比拖动倍率。
    pub vegetation_group_capacity: f32,       // 植被组权重容量的百分比拖动倍率。
    pub vegetation_claim_threshold: f32,      // 植被组优先占用阈值的百分比拖动倍率。
    pub boundary_thickness: f32,              // 场景玩法边界厚度的米制拖动倍率。
    pub skybox_brightness: f32,               // 场景天空盒可见亮度的拖动倍率。
    pub environment_diffuse_intensity: f32,   // 场景天空漫反射补光强度的拖动倍率。
    pub environment_specular_intensity: f32,  // 场景天空镜面反射强度的拖动倍率。
    pub skybox_yaw: f32,                      // 场景天空盒水平旋转角的拖动倍率。
    pub weather_preview_progress: f32,        // 天气转换预览进度的百分比拖动倍率。
    pub new_scene_width: f32,                   // 新场景世界宽度的米制拖动倍率。
    pub new_scene_length: f32,                  // 新场景世界长度的米制拖动倍率。
    pub new_scene_terrain_cell: f32,            // 新场景地形GLB网格间距的米制拖动倍率。
    pub new_scene_player_collision_radius: f32, // 分块碰撞玩家预载米制半径的独立拖动倍率。
    pub new_scene_role_collision_radius: f32,   // 分块碰撞角色粗级预载半径的独立拖动倍率。
}

/// 场景编辑器数值框的稳定强类型键，业务系统不需要自行匹配配置字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] // ECS系统可以低成本复制并用作缓存键。
pub enum SceneEditorScrubField {
    // 每个变体精确对应一个独立持久化灵敏度槽位。
    BrushSpacing,                 // 圆刷最小间距。
    BrushRadius,                  // 笔刷大小。
    BrushStrength,                // 笔刷强度。
    TerrainSpeed,                 // 地形雕刻速度。
    WaterLevel,                   // 静止水位。
    MeshWaterFlowSpeed,           // 整网格水体流速。
    MeshWaterHeightOffset,        // 整网格水体世界高度偏移。
    PlacementTranslation,         // 显式资产世界平移。
    PlacementRotation,            // 显式资产Y轴旋转。
    PlacementScale,               // 显式资产三轴缩放。
    WaterControlHeight,           // 水网控制点高度。
    WaterControlWidth,            // 水网控制点宽度。
    WaterControlDepth,            // 水网控制点深度。
    WaterControlFlowSpeed,        // 水网控制点流速。
    VegetationPriority,           // 植被层级优先级。
    VegetationWeight,             // 植被层组内权重。
    VegetationOpacity,            // 植被层不透明度。
    VegetationGroupCapacity,      // 植被组权重容量。
    VegetationClaimThreshold,     // 植被组优先占用阈值。
    BoundaryThickness,            // 场景玩法边界厚度。
    SkyboxBrightness,             // 场景天空盒可见亮度。
    EnvironmentDiffuseIntensity,  // 场景天空漫反射补光强度。
    EnvironmentSpecularIntensity, // 场景天空镜面反射强度。
    SkyboxYaw,                    // 场景天空盒水平旋转角。
    WeatherPreviewProgress,       // 天气转换预览进度。
    NewSceneWidth,                  // 新场景世界宽度。
    NewSceneLength,                 // 新场景世界长度。
    NewSceneTerrainCell,            // 新场景地形GLB网格间距。
    NewScenePlayerCollisionRadius,  // 分块碰撞玩家预载半径。
    NewSceneRoleCollisionRadius,    // 分块碰撞角色粗级预载半径。
} // 结束全部三十个可拖动数值字段键。

impl SceneEditorScrubField {
    // 集中维护字段全集，方便默认值和往返测试覆盖新增变体。
    pub const ALL: [Self; 30] = [
        // 声明顺序与编辑器功能分组一致。
        Self::BrushSpacing,
        Self::BrushRadius,                  // 笔刷大小键。
        Self::BrushStrength,                // 笔刷强度键。
        Self::TerrainSpeed,                 // 地形速度键。
        Self::WaterLevel,                   // 静止水位键。
        Self::MeshWaterFlowSpeed,           // 整网格水体流速键。
        Self::MeshWaterHeightOffset,        // 整网格水体高度键。
        Self::PlacementTranslation,         // 显式资产平移键。
        Self::PlacementRotation,            // 显式资产旋转键。
        Self::PlacementScale,               // 显式资产缩放键。
        Self::WaterControlHeight,           // 水网高度键。
        Self::WaterControlWidth,            // 水网宽度键。
        Self::WaterControlDepth,            // 水网深度键。
        Self::WaterControlFlowSpeed,        // 水网流速键。
        Self::VegetationPriority,           // 植被优先级键。
        Self::VegetationWeight,             // 植被权重键。
        Self::VegetationOpacity,            // 植被不透明度键。
        Self::VegetationGroupCapacity,      // 植被组容量键。
        Self::VegetationClaimThreshold,     // 植被占用阈值键。
        Self::BoundaryThickness,            // 边界厚度键。
        Self::SkyboxBrightness,             // 天空亮度键。
        Self::EnvironmentDiffuseIntensity,  // 天空漫反射补光键。
        Self::EnvironmentSpecularIntensity, // 天空镜面反射键。
        Self::SkyboxYaw,                    // 天空方位角键。
        Self::WeatherPreviewProgress,       // 天气预览进度键。
        Self::NewSceneWidth,                  // 新场景宽度键。
        Self::NewSceneLength,                 // 新场景长度键。
        Self::NewSceneTerrainCell,            // 新场景地形GLB网格间距键。
        Self::NewScenePlayerCollisionRadius,  // 分块碰撞玩家预载半径键。
        Self::NewSceneRoleCollisionRadius,    // 分块碰撞角色粗级预载半径键。
    ]; // 结束稳定字段全集。
} // 结束字段键辅助实现。

impl SceneEditorScrubSensitivities {
    // 用统一入口集中管理强类型键与存储字段的映射。
    pub fn get(&self, field: SceneEditorScrubField) -> f32 {
        // 读取指定数值框的当前灵敏度。
        match field {
            // 所有变体必须显式对应独立槽位，新增字段时由编译器提醒补齐。
            SceneEditorScrubField::BrushSpacing => self.brush_spacing,
            SceneEditorScrubField::BrushRadius => self.brush_radius, // 读取笔刷大小倍率。
            SceneEditorScrubField::BrushStrength => self.brush_strength, // 读取笔刷强度倍率。
            SceneEditorScrubField::TerrainSpeed => self.terrain_speed, // 读取地形速度倍率。
            SceneEditorScrubField::WaterLevel => self.water_level,   // 读取静止水位倍率。
            SceneEditorScrubField::MeshWaterFlowSpeed => self.mesh_water_flow_speed, // 读取整网格水体流速倍率。
            SceneEditorScrubField::MeshWaterHeightOffset => self.mesh_water_height_offset, // 读取整网格水体高度倍率。
            SceneEditorScrubField::PlacementTranslation => self.placement_translation, // 读取显式资产平移倍率。
            SceneEditorScrubField::PlacementRotation => self.placement_rotation, // 读取显式资产旋转倍率。
            SceneEditorScrubField::PlacementScale => self.placement_scale, // 读取显式资产缩放倍率。
            SceneEditorScrubField::WaterControlHeight => self.water_control_height, // 读取水网高度倍率。
            SceneEditorScrubField::WaterControlWidth => self.water_control_width, // 读取水网宽度倍率。
            SceneEditorScrubField::WaterControlDepth => self.water_control_depth, // 读取水网深度倍率。
            SceneEditorScrubField::WaterControlFlowSpeed => self.water_control_flow_speed, // 读取水网流速倍率。
            SceneEditorScrubField::VegetationPriority => self.vegetation_priority, // 读取植被优先级倍率。
            SceneEditorScrubField::VegetationWeight => self.vegetation_weight, // 读取植被权重倍率。
            SceneEditorScrubField::VegetationOpacity => self.vegetation_opacity, // 读取植被不透明度倍率。
            SceneEditorScrubField::VegetationGroupCapacity => self.vegetation_group_capacity, // 读取植被组容量倍率。
            SceneEditorScrubField::VegetationClaimThreshold => self.vegetation_claim_threshold, // 读取植被占用阈值倍率。
            SceneEditorScrubField::BoundaryThickness => self.boundary_thickness, // 读取边界厚度倍率。
            SceneEditorScrubField::SkyboxBrightness => self.skybox_brightness,   // 读取天空亮度倍率。
            SceneEditorScrubField::EnvironmentDiffuseIntensity => self.environment_diffuse_intensity, // 读取天空漫反射补光倍率。
            SceneEditorScrubField::EnvironmentSpecularIntensity => {
                self.environment_specular_intensity
            } // 读取天空镜面反射倍率。
            SceneEditorScrubField::SkyboxYaw => self.skybox_yaw, // 读取天空方位角倍率。
            SceneEditorScrubField::WeatherPreviewProgress => self.weather_preview_progress, // 读取天气预览进度倍率。
            SceneEditorScrubField::NewSceneWidth => self.new_scene_width, // 读取新场景宽度倍率。
            SceneEditorScrubField::NewSceneLength => self.new_scene_length, // 读取新场景长度倍率。
            SceneEditorScrubField::NewSceneTerrainCell => self.new_scene_terrain_cell, // 读取地形GLB网格间距倍率。
            SceneEditorScrubField::NewScenePlayerCollisionRadius => {
                self.new_scene_player_collision_radius
            } // 读取独立预载半径倍率。
            SceneEditorScrubField::NewSceneRoleCollisionRadius => self.new_scene_role_collision_radius, // 读取角色粗级预载倍率。
        } // 结束完备字段映射。
    } // 结束强类型读取。

    pub fn set(&mut self, field: SceneEditorScrubField, sensitivity: f32) {
        // 只修改指定数值框的灵敏度槽位。
        match field {
            // 完备匹配保证新字段不会静默共用旧槽位。
            SceneEditorScrubField::BrushSpacing => self.brush_spacing = sensitivity,
            SceneEditorScrubField::BrushRadius => self.brush_radius = sensitivity, // 写入笔刷大小倍率。
            SceneEditorScrubField::BrushStrength => self.brush_strength = sensitivity, // 写入笔刷强度倍率。
            SceneEditorScrubField::TerrainSpeed => self.terrain_speed = sensitivity, // 写入地形速度倍率。
            SceneEditorScrubField::WaterLevel => self.water_level = sensitivity, // 写入静止水位倍率。
            SceneEditorScrubField::MeshWaterFlowSpeed => self.mesh_water_flow_speed = sensitivity, // 写入整网格水体流速倍率。
            SceneEditorScrubField::MeshWaterHeightOffset => {
                self.mesh_water_height_offset = sensitivity
            } // 写入整网格水体高度倍率。
            SceneEditorScrubField::PlacementTranslation => self.placement_translation = sensitivity, // 写入显式资产平移倍率。
            SceneEditorScrubField::PlacementRotation => self.placement_rotation = sensitivity, // 写入显式资产旋转倍率。
            SceneEditorScrubField::PlacementScale => self.placement_scale = sensitivity, // 写入显式资产缩放倍率。
            SceneEditorScrubField::WaterControlHeight => self.water_control_height = sensitivity, // 写入水网高度倍率。
            SceneEditorScrubField::WaterControlWidth => self.water_control_width = sensitivity, // 写入水网宽度倍率。
            SceneEditorScrubField::WaterControlDepth => self.water_control_depth = sensitivity, // 写入水网深度倍率。
            SceneEditorScrubField::WaterControlFlowSpeed => {
                self.water_control_flow_speed = sensitivity
            } // 写入水网流速倍率。
            SceneEditorScrubField::VegetationPriority => self.vegetation_priority = sensitivity, // 写入植被优先级倍率。
            SceneEditorScrubField::VegetationWeight => self.vegetation_weight = sensitivity, // 写入植被权重倍率。
            SceneEditorScrubField::VegetationOpacity => self.vegetation_opacity = sensitivity, // 写入植被不透明度倍率。
            SceneEditorScrubField::VegetationGroupCapacity => {
                self.vegetation_group_capacity = sensitivity
            } // 写入植被组容量倍率。
            SceneEditorScrubField::VegetationClaimThreshold => {
                self.vegetation_claim_threshold = sensitivity
            } // 写入植被占用阈值倍率。
            SceneEditorScrubField::BoundaryThickness => self.boundary_thickness = sensitivity, // 写入边界厚度倍率。
            SceneEditorScrubField::SkyboxBrightness => self.skybox_brightness = sensitivity, // 写入天空亮度倍率。
            SceneEditorScrubField::EnvironmentDiffuseIntensity => {
                self.environment_diffuse_intensity = sensitivity
            } // 写入天空漫反射补光倍率。
            SceneEditorScrubField::EnvironmentSpecularIntensity => {
                self.environment_specular_intensity = sensitivity
            } // 写入天空镜面反射倍率。
            SceneEditorScrubField::SkyboxYaw => self.skybox_yaw = sensitivity, // 写入天空方位角倍率。
            SceneEditorScrubField::WeatherPreviewProgress => {
                self.weather_preview_progress = sensitivity
            } // 写入天气预览进度倍率。
            SceneEditorScrubField::NewSceneWidth => self.new_scene_width = sensitivity, // 写入新场景宽度倍率。
            SceneEditorScrubField::NewSceneLength => self.new_scene_length = sensitivity, // 写入新场景长度倍率。
            SceneEditorScrubField::NewSceneTerrainCell => self.new_scene_terrain_cell = sensitivity, // 写入地形GLB网格间距倍率。
            SceneEditorScrubField::NewScenePlayerCollisionRadius => {
                self.new_scene_player_collision_radius = sensitivity
            } // 写入独立预载半径倍率。
            SceneEditorScrubField::NewSceneRoleCollisionRadius => {
                self.new_scene_role_collision_radius = sensitivity
            } // 写入角色粗级独立倍率。
        } // 结束完备字段写入。
    } // 结束强类型写入。
} // 结束数值框灵敏度访问器实现。

impl Default for SceneEditorScrubSensitivities {
    fn default() -> Self {
        Self {
            brush_spacing: 1.0,                   // 默认标准手感。
            brush_radius: 1.0,                    // 一倍使用笔刷大小字段声明的标准指数手感。
            brush_strength: 1.0,                  // 一倍使用每像素一个百分点的标准手感。
            terrain_speed: 1.0,                   // 一倍使用雕刻速度字段声明的标准指数手感。
            water_level: 1.0,                     // 一倍使用每像素零点一米的标准手感。
            mesh_water_flow_speed: 1.0,           // 一倍使用整网格水体流速的标准线性手感。
            mesh_water_height_offset: 1.0,        // 一倍使用整网格水体高度的标准米制手感。
            placement_translation: 1.0,           // 一倍使用显式资产世界平移的标准米制手感。
            placement_rotation: 1.0,              // 一倍使用显式资产Y轴角度的标准手感。
            placement_scale: 1.0,                 // 一倍使用显式资产三轴缩放的标准手感。
            water_control_height: 1.0,            // 一倍使用水网控制点高度的标准米制手感。
            water_control_width: 1.0,             // 一倍使用水网宽度的标准拖动手感。
            water_control_depth: 1.0,             // 一倍使用水网深度的标准米制手感。
            water_control_flow_speed: 1.0,        // 一倍使用水网流速的标准米每秒手感。
            vegetation_priority: 1.0,             // 一倍使用植被整数优先级的标准步长。
            vegetation_weight: 1.0,               // 一倍使用植被权重的标准拖动手感。
            vegetation_opacity: 1.0,              // 一倍使用植被不透明度的标准百分比手感。
            vegetation_group_capacity: 1.0,       // 一倍使用植被组容量的标准百分比手感。
            vegetation_claim_threshold: 1.0,      // 一倍使用植被占用阈值的标准百分比手感。
            boundary_thickness: 1.0,              // 一倍使用场景边界厚度的标准米制手感。
            skybox_brightness: 1.0,               // 一倍使用天空亮度的标准线性手感。
            environment_diffuse_intensity: 1.0,   // 一倍使用天空漫反射补光的标准线性手感。
            environment_specular_intensity: 1.0,  // 一倍使用天空镜面反射的标准线性手感。
            skybox_yaw: 1.0,                      // 一倍使用天空方位角的标准角度手感。
            weather_preview_progress: 1.0,        // 一倍使用天气转换预览的标准百分比手感。
            new_scene_width: 1.0,                   // 一倍使用新场景宽度的标准米制手感。
            new_scene_length: 1.0,                  // 一倍使用新场景长度的标准米制手感。
            new_scene_terrain_cell: 1.0,            // 一倍使用地形GLB网格间距的标准米制手感。
            new_scene_player_collision_radius: 1.0, // 一倍使用玩家预载半径的标准米制手感。
            new_scene_role_collision_radius: 1.0,   // 一倍使用角色粗级预载半径的标准米制手感。
        }
    }
}

/// 编辑器倍率按桌面逻辑像素应用，系统 DPI 仍由 Bevy 处理，不叠加游戏分辨率倍率。
pub const DEFAULT_EDITOR_UI_SCALE: f32 = 1.0;
pub const MIN_EDITOR_UI_SCALE: f32 = 0.6;
pub const MAX_EDITOR_UI_SCALE: f32 = 2.0;
/// 手动和自动共用60%–200%范围，后续调整不能让两种模式的边界再次分离。
pub const MIN_EDITOR_AUTO_UI_SCALE: f32 = MIN_EDITOR_UI_SCALE;
pub const MAX_EDITOR_AUTO_UI_SCALE: f32 = MAX_EDITOR_UI_SCALE;

pub fn sanitize_editor_ui_scale(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(MIN_EDITOR_UI_SCALE, MAX_EDITOR_UI_SCALE)
    } else {
        DEFAULT_EDITOR_UI_SCALE
    }
}

pub fn sanitize_editor_auto_ui_scale(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(MIN_EDITOR_AUTO_UI_SCALE, MAX_EDITOR_AUTO_UI_SCALE)
    } else {
        DEFAULT_EDITOR_UI_SCALE
    }
}

/// 自动档偏移使用倍率差，最多覆盖整个合法区间；坏配置回到不微调的默认值。
pub fn sanitize_editor_ui_scale_auto_offset(value: f32) -> f32 {
    let maximum = MAX_EDITOR_AUTO_UI_SCALE - MIN_EDITOR_AUTO_UI_SCALE;
    if value.is_finite() {
        value.clamp(-maximum, maximum)
    } else {
        0.0
    }
}

/// 自动档按窗口逻辑尺寸适配 1920×1080，并叠加作者微调；Bevy 另行处理系统 DPI。
pub fn effective_editor_ui_scale(
    manual: f32,
    automatic: bool,
    offset: f32,
    width: f32,
    height: f32,
) -> f32 {
    if !automatic {
        return sanitize_editor_ui_scale(manual);
    }
    let base = if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
        // 先取窗口适配倍率，再施加微调；夹取之后加偏移，让上下限处也能立即反向调整。
        let fit = (width / 1920.0).min(height / 1080.0);
        sanitize_editor_auto_ui_scale((fit * 100.0).floor() / 100.0)
    } else {
        sanitize_editor_ui_scale(manual)
    };
    sanitize_editor_auto_ui_scale(base + sanitize_editor_ui_scale_auto_offset(offset))
}

/// F12两侧栏的持久偏好使用逻辑像素；窗口适配和专注模式只属于当前会话。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorLayoutPreferences {
    pub content_width: f32,
    pub inspector_width: f32,
    pub content_visible: bool,
    pub inspector_visible: bool,
}

impl EditorLayoutPreferences {
    pub const DEFAULT_CONTENT_WIDTH: f32 = 232.0;
    pub const DEFAULT_INSPECTOR_WIDTH: f32 = 278.0;
    pub const CONTENT_MIN_WIDTH: f32 = 200.0;
    pub const CONTENT_MAX_WIDTH: f32 = 420.0;
    pub const INSPECTOR_MIN_WIDTH: f32 = 260.0;
    pub const INSPECTOR_MAX_WIDTH: f32 = 560.0;

    /// 读写都应用同一边界，避免损坏配置把侧栏撑出屏幕或传播NaN。
    pub fn sanitized(self) -> Self {
        Self {
            content_width: if self.content_width.is_finite() {
                self.content_width
                    .clamp(Self::CONTENT_MIN_WIDTH, Self::CONTENT_MAX_WIDTH)
            } else {
                Self::DEFAULT_CONTENT_WIDTH
            },
            inspector_width: if self.inspector_width.is_finite() {
                self.inspector_width
                    .clamp(Self::INSPECTOR_MIN_WIDTH, Self::INSPECTOR_MAX_WIDTH)
            } else {
                Self::DEFAULT_INSPECTOR_WIDTH
            },
            ..self
        }
    }
}

impl Default for EditorLayoutPreferences {
    fn default() -> Self {
        Self {
            content_width: Self::DEFAULT_CONTENT_WIDTH,
            inspector_width: Self::DEFAULT_INSPECTOR_WIDTH,
            content_visible: true,
            inspector_visible: true,
        }
    }
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct EditorSettings {
    pub render: feathrise_settings::RenderSettings,
    /// 独立编辑器的临时编译降帧开关。日常上限仍由 render.max_fps 保存。
    pub limit_build_fps: bool,
    /// 标题栏显示当前编辑器窗口的实测帧率，独立于游戏 HUD 及日常帧率上限。
    pub show_editor_fps: bool,
    pub scene: feathrise_settings::SceneDisplaySettings,
    pub environment: feathrise_settings::EnvironmentPreviewSettings,
    pub scene_editor_scrub_sensitivities: SceneEditorScrubSensitivities,
    pub ui_locale: EditorLocalePreference,
    pub editor_ui_scale: f32,
    pub editor_ui_scale_auto: bool,
    pub editor_ui_scale_auto_offset: f32,
    pub editor_color_theme: EditorColorTheme,
    pub editor_show_terrain_backfaces: bool,
    pub editor_layout: EditorLayoutPreferences,
    pub editor_keybindings: EditorKeyBindings,
    pub editor_snapping: EditorSnapSettings,
    pub asset_browser_view_mode: AssetBrowserViewMode,
}
impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            render: Default::default(),
            limit_build_fps: DEFAULT_LIMIT_BUILD_FPS,
            show_editor_fps: false,
            scene: Default::default(),
            environment: Default::default(),
            scene_editor_scrub_sensitivities: Default::default(),
            ui_locale: EditorLocalePreference::default(),
            editor_ui_scale: DEFAULT_EDITOR_UI_SCALE,
            editor_ui_scale_auto: false,
            editor_ui_scale_auto_offset: 0.0,
            editor_color_theme: Default::default(),
            editor_show_terrain_backfaces: true,
            editor_layout: Default::default(),
            editor_keybindings: Default::default(),
            editor_snapping: Default::default(),
            asset_browser_view_mode: Default::default(),
        }
    }
}
