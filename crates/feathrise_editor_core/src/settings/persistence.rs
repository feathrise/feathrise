//! 编辑器用户偏好的可序列化格式与显式转换；没有项目 settings.json 路径或游戏字段。
use super::*;
use serde::{Deserialize, Serialize};
mod scene_editor_scrub;
mod scrub_mapping;
pub use scene_editor_scrub::SceneEditorScrubSensitivitySettings;
use scrub_mapping::{
    scene_editor_scrub_sensitivities_for_save, scene_editor_scrub_sensitivities_from_data,
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EditorSettingsData {
    #[serde(default)]
    pub render: feathrise_settings::RenderSettings,
    /// 缺少本字段的旧作者偏好也沿用默认编译降帧。
    #[serde(default = "default_limit_build_fps")]
    pub limit_build_fps: bool,
    #[serde(default)]
    pub scene: feathrise_settings::SceneDisplaySettings,
    #[serde(default)]
    pub environment: feathrise_settings::EnvironmentPreviewSettings,
    #[serde(default)]
    pub scrub: SceneEditorScrubSensitivitySettings,
    /// 稳定动作名对应组合键；缺项继承默认，null表示作者主动解绑。
    #[serde(default)]
    pub shortcuts: std::collections::BTreeMap<String, Option<String>>,
    /// 缺少语言字段的旧作者偏好与全新安装共用系统档默认值。
    #[serde(default = "default_ui_locale")]
    pub ui_locale: String,
    #[serde(default = "default_editor_ui_scale")]
    pub ui_scale: f32,
    /// 旧配置缺项保持手动档；自动档仍保存原手动倍率供下次手动调整。
    #[serde(default)]
    pub ui_scale_auto: bool,
    /// 自动倍率的微调独立保存；旧配置缺项为零偏移。
    #[serde(default)]
    pub ui_scale_auto_offset: f32,
    #[serde(default = "default_editor_color_theme")]
    pub color_theme: String,
    /// 缺省开启；入口保留在编辑器设置，关闭F12仍用于玩法显示，不修改场景源材质。
    #[serde(default = "default_show_terrain_backfaces")]
    pub show_terrain_backfaces: bool,
    #[serde(default = "default_asset_browser_view_mode")]
    pub asset_browser_view_mode: String,
    #[serde(default)]
    pub snapping: EditorSnappingSettings,
    /// 两栏布局单独缺省，旧配置及只保存某个字段的配置仍恢复其余默认值。
    #[serde(default)]
    pub layout: EditorLayoutSettings,
}

/// 独立编辑器偏好；缺省保持Ctrl临时吸附，未知模式由运行时回退到世界网格。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct EditorSnappingSettings {
    pub enabled: bool,
    pub show_grid: bool,
    pub mode: String,
    pub translation_meters: f32,
    pub rotation_degrees: f32,
    pub scale_factor: f32,
}

impl Default for EditorSnappingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            show_grid: true,
            mode: "WorldGrid".into(),
            translation_meters: 1.0,
            rotation_degrees: 15.0,
            scale_factor: 0.1,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct EditorLayoutSettings {
    pub content_width: f32,
    pub inspector_width: f32,
    pub content_visible: bool,
    pub inspector_visible: bool,
}

impl Default for EditorLayoutSettings {
    fn default() -> Self {
        Self {
            content_width: EditorLayoutPreferences::DEFAULT_CONTENT_WIDTH,
            inspector_width: EditorLayoutPreferences::DEFAULT_INSPECTOR_WIDTH,
            content_visible: true,
            inspector_visible: true,
        }
    }
}

fn default_asset_browser_view_mode() -> String {
    "List".into()
}

fn default_editor_color_theme() -> String {
    "NeutralGray".into()
}

fn default_editor_ui_scale() -> f32 {
    1.0
}

fn default_show_terrain_backfaces() -> bool {
    true
}

fn default_limit_build_fps() -> bool {
    DEFAULT_LIMIT_BUILD_FPS
}

fn default_ui_locale() -> String {
    EditorLocalePreference::default().as_str().into()
}

impl Default for EditorSettingsData {
    fn default() -> Self {
        Self {
            render: Default::default(),
            limit_build_fps: default_limit_build_fps(),
            scene: Default::default(),
            environment: Default::default(),
            scrub: Default::default(),
            ui_locale: default_ui_locale(),
            ui_scale: default_editor_ui_scale(),
            ui_scale_auto: false,
            ui_scale_auto_offset: 0.0,
            color_theme: default_editor_color_theme(),
            show_terrain_backfaces: default_show_terrain_backfaces(),
            shortcuts: Default::default(),
            asset_browser_view_mode: default_asset_browser_view_mode(),
            snapping: EditorSnappingSettings::default(),
            layout: EditorLayoutSettings::default(),
        }
    }
}

/// 编辑器显示与快捷键偏好和游戏镜头、玩法按键分别往返，不污染作者数据。
impl From<&EditorSettings> for EditorSettingsData {
    fn from(settings: &EditorSettings) -> Self {
        let layout = settings.editor_layout.sanitized();
        Self {
            render: settings.render,
            limit_build_fps: settings.limit_build_fps,
            scene: settings.scene,
            environment: settings.environment,
            scrub: scene_editor_scrub_sensitivities_for_save(
                &settings.scene_editor_scrub_sensitivities,
            ),
            ui_locale: settings.ui_locale.as_str().into(),
            ui_scale: sanitize_editor_ui_scale(settings.editor_ui_scale),
            ui_scale_auto: settings.editor_ui_scale_auto,
            ui_scale_auto_offset: sanitize_editor_ui_scale_auto_offset(
                settings.editor_ui_scale_auto_offset,
            ),
            color_theme: settings.editor_color_theme.as_str().into(),
            show_terrain_backfaces: settings.editor_show_terrain_backfaces,
            asset_browser_view_mode: settings.asset_browser_view_mode.as_str().into(),
            shortcuts: settings.editor_keybindings.to_saved(),
            snapping: {
                let snap = settings.editor_snapping.sanitized();
                EditorSnappingSettings {
                    enabled: snap.enabled,
                    show_grid: snap.show_grid,
                    mode: snap.mode.as_str().into(),
                    translation_meters: snap.translation_meters,
                    rotation_degrees: snap.rotation_degrees,
                    scale_factor: snap.scale_factor,
                }
            },
            layout: EditorLayoutSettings {
                content_width: layout.content_width,
                inspector_width: layout.inspector_width,
                content_visible: layout.content_visible,
                inspector_visible: layout.inspector_visible,
            },
        }
    }
}

impl EditorSettingsData {
    pub fn into_runtime(self) -> EditorSettings {
        let data = &self;
        let mut settings = EditorSettings::default();
        settings.render = data.render;
        settings.limit_build_fps = data.limit_build_fps;
        settings.render.mesh_lod = data.render.mesh_lod.sanitized();
        settings.render.exposure_compensation =
            feathrise_settings::sanitize_exposure_compensation(
                data.render.exposure_compensation,
            );
        settings.render.graphics.sanitize_vendor_upscaling_values();
        settings.scene = data.scene;
        settings.environment = data.environment;
        settings.scene_editor_scrub_sensitivities =
            scene_editor_scrub_sensitivities_from_data(&data.scrub);
        settings.ui_locale = EditorLocalePreference::from_saved(&data.ui_locale);
        settings.editor_ui_scale = sanitize_editor_ui_scale(data.ui_scale);
        settings.editor_ui_scale_auto = data.ui_scale_auto;
        settings.editor_ui_scale_auto_offset =
            sanitize_editor_ui_scale_auto_offset(data.ui_scale_auto_offset);
        settings.editor_color_theme = EditorColorTheme::from_saved(&data.color_theme);
        settings.editor_show_terrain_backfaces = data.show_terrain_backfaces;
        settings.editor_keybindings = EditorKeyBindings::from_saved(&data.shortcuts);
        settings.editor_snapping = EditorSnapSettings {
            enabled: data.snapping.enabled,
            show_grid: data.snapping.show_grid,
            mode: EditorSnapMode::from_saved(&data.snapping.mode),
            translation_meters: data.snapping.translation_meters,
            rotation_degrees: data.snapping.rotation_degrees,
            scale_factor: data.snapping.scale_factor,
        }
        .sanitized();
        settings.asset_browser_view_mode =
            AssetBrowserViewMode::from_saved(&data.asset_browser_view_mode);
        settings.editor_layout = EditorLayoutPreferences {
            content_width: data.layout.content_width,
            inspector_width: data.layout.inspector_width,
            content_visible: data.layout.content_visible,
            inspector_visible: data.layout.inspector_visible,
        }
        .sanitized();
        settings
    }
}
