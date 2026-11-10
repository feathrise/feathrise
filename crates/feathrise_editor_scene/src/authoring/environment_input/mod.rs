//! 天空盒亮度、环境补光、环境反射和天空/IBL 水平朝向的公共专业数值输入领域。
//!
//! 【功能设计】亮度与方向左拖只实时改内存天空请求，松手才写一条环境撤销并推进一次
//! `environment_revision`；运行时天气调试已经统一迁入独立 F12 世界环境控件。

mod scene; // 四个固定槽位首次挂载完整公共控件。
mod sync; // 草稿、预览、禁用态和持久倍率同步。
mod systems; // 精确提交、左拖事务与右拖倍率消息处理。
mod transaction; // 冲突和异常收口只恢复本事务拥有的单一环境字段。

use bevy::prelude::*; // Component、Entity和跨帧Resource。
use feathrise_scene_content::{SceneEnvironmentDescriptor, SceneId}; // 完整环境快照与稳定场景身份。
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::SceneEditorScrubField; // 四个逻辑字段各自持久倍率。
use feathrise_editor_ui::{
    NumericInputBoundaryMode, NumericInputScrubConfig, NumericInputScrubMapping, NumericInputSpec,
    NumericValue,
}; // 共享 UI 控件统一处理中键字符、左右拖动和循环角度。

use super::environment::SKYBOX_MAX_BRIGHTNESS; // 环境模块集中声明天空亮度硬上限。
use crate::authoring::resources::EditorAuthoringSession; // 当前环境草稿和作者修订。
use transaction::rollback_environment_scrub_if_owned; // 不用旧完整环境快照覆盖外部字段。

pub(crate) use scene::setup_environment_numeric_inputs; // Presentation首次创建四个根。
pub(crate) use sync::sync_environment_numeric_inputs; // Presentation同步当前作者与预览值。
pub(crate) use systems::handle_environment_numeric_inputs; // Input消费四类公共消息。

const SCRUB_SENSITIVITY_MIN: f32 = 0.01; // 最细允许标准手感百分之一。
const SCRUB_SENSITIVITY_MAX: f32 = 100.0; // 最快限制为标准手感一百倍。

/// 右栏四个历史环境字段的业务身份。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EnvironmentNumericKind {
    #[default]
    SkyboxBrightness, // 天空盒模式的日间可见亮度；动态模式只保留切换用作者值。
    EnvironmentDiffuseIntensity, // 场景清单中的日间PBR天空漫反射补光强度，单位cd/m²。
    EnvironmentSpecularIntensity, // 场景清单中的日间PBR天空镜面反射强度，单位cd/m²。
    SkyboxYaw,                   // 场景清单中的 IBL 水平旋转角，循环零至三百六十度。
}

impl EnvironmentNumericKind {
    /// 右栏字段短标签。
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::SkyboxBrightness => "天空盒日间亮度", // 动态模式保留值但不参与画面，作用范围由环境概览说明。
            Self::EnvironmentDiffuseIntensity => "日间基础补光", // 人物、地形和粗糙材质的补光基础值同样接受昼夜变化。
            Self::EnvironmentSpecularIntensity => "日间基础反射", // 水面与光滑材质的反射基础值同样接受昼夜变化。
            Self::SkyboxYaw => "天空/IBL 方向", // 主 Cube 与两张 IBL 保持同一水平旋转。
        }
    }

    /// 字段右侧单位。
    pub(super) const fn unit(self) -> &'static str {
        match self {
            Self::SkyboxBrightness
            | Self::EnvironmentDiffuseIntensity
            | Self::EnvironmentSpecularIntensity => "cd/m²", // 三个天空能量通道采用相同单位但分别调节。
            Self::SkyboxYaw => "°", // 保存角度制水平朝向。
        }
    }

    /// 公共精确输入规格；天空方向使用循环边界而不是端点钳制。
    pub(super) const fn spec(self) -> NumericInputSpec {
        match self {
            Self::SkyboxBrightness
            | Self::EnvironmentDiffuseIntensity
            | Self::EnvironmentSpecularIntensity => NumericInputSpec::f32(0)
                .with_min(NumericValue::F32(0.0)) // 零亮度仍是合法作者选择。
                .with_max(NumericValue::F32(SKYBOX_MAX_BRIGHTNESS)) // 防止极端Uniform破坏精度。
                .with_step(NumericValue::F32(1.0)) // 背景、补光和反射均使用整数cd/m²显示与吸附。
                .with_step_buttons(false) // 旧按钮由专业字段取代。
                .with_live_update(false) // 字符只在提交边沿写场景环境。
                .with_replace_on_first_key(true), // 中键输入首字符替换旧值。
            Self::SkyboxYaw => NumericInputSpec::f32(1)
                .with_min(NumericValue::F32(0.0)) // 循环区间从零度开始。
                .with_max(NumericValue::F32(360.0)) // 三百六十度与零度等价。
                .with_step(NumericValue::F32(0.1)) // 十分之一度足以精确对齐天空纹理。
                .with_step_buttons(false) // 旧左右十五度按钮完全移除。
                .with_live_update(false) // 字符提交形成一次环境命令。
                .with_replace_on_first_key(true) // 首字符替换当前角度。
                .with_boundary_mode(NumericInputBoundaryMode::Wrap), // -15和375分别归一到345和15。
        }
    }

    /// 当前字段对应的独立设置键。
    pub(super) const fn sensitivity_field(self) -> SceneEditorScrubField {
        match self {
            Self::SkyboxBrightness => SceneEditorScrubField::SkyboxBrightness, // 亮度自己的倍率。
            Self::EnvironmentDiffuseIntensity => SceneEditorScrubField::EnvironmentDiffuseIntensity, // 天空补光自己的倍率。
            Self::EnvironmentSpecularIntensity => SceneEditorScrubField::EnvironmentSpecularIntensity, // 天空反射自己的倍率。
            Self::SkyboxYaw => SceneEditorScrubField::SkyboxYaw, // 方向自己的倍率。
        }
    }

    /// 读取并修复本字段持久倍率。
    pub(super) fn sensitivity(self, settings: &EditorSettings) -> f32 {
        let value = settings
            .scene_editor_scrub_sensitivities
            .get(self.sensitivity_field()); // 设置按逻辑字段而不是控件实体保存。
        if value.is_finite() {
            value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX) // 手改设置仍服从公共范围。
        } else {
            1.0 // 非有限配置回退标准手感。
        }
    }

    /// 左值和右倍率拖动共享的公共配置。
    pub(super) fn scrub_config(self, settings: &EditorSettings) -> NumericInputScrubConfig {
        let (units_per_pixel, step, minimum, maximum) = match self {
            Self::SkyboxBrightness
            | Self::EnvironmentDiffuseIntensity
            | Self::EnvironmentSpecularIntensity => (10.0, 1.0, 0.0, SKYBOX_MAX_BRIGHTNESS), // 三个cd/m²字段每像素十单位线性变化，可从零开始。
            Self::SkyboxYaw => (0.25, 0.1, 0.0, 360.0), // 每四像素一度并由公共Wrap保持循环。
        };
        NumericInputScrubConfig {
            mapping: NumericInputScrubMapping::Linear { units_per_pixel }, // 四项都允许零值，统一使用线性映射。
            drag_step: step,                            // 各字段显示精度决定吸附档。
            minimum,                                    // 公共左拖下界。
            maximum,                                    // 公共左拖上界；Yaw由Spec进一步循环。
            sensitivity: self.sensitivity(settings),    // 每个字段自己的持久倍率。
            minimum_sensitivity: SCRUB_SENSITIVITY_MIN, // 右拖最细倍率。
            maximum_sensitivity: SCRUB_SENSITIVITY_MAX, // 右拖最快倍率。
            drag_threshold_pixels: 4.0,                 // 四像素内不误判拖动。
            sensitivity_pixels_per_octave: 48.0,        // 右拖四十八像素翻倍或减半。
        }
    }

    /// 从完整环境读取 UI 显示值。
    pub(super) fn displayed_value(
        self,
        environment: Option<&SceneEnvironmentDescriptor>,
    ) -> Option<f32> {
        match self {
            Self::SkyboxBrightness => environment.map(|value| value.skybox_brightness), // 场景清单原单位显示。
            Self::EnvironmentDiffuseIntensity => {
                environment.map(|value| value.environment_diffuse_intensity)
            } // PBR漫反射补光独立原单位显示。
            Self::EnvironmentSpecularIntensity => {
                environment.map(|value| value.environment_specular_intensity)
            } // PBR镜面反射独立原单位显示。
            Self::SkyboxYaw => environment.map(|value| value.normalized_skybox_yaw_degrees()), // 始终回显等价零至三百六十度。
        }
    }

    /// 把 UI 显示值写入完整场景环境。
    pub(super) fn set_environment_value(
        self,
        environment: &mut SceneEnvironmentDescriptor,
        value: f32,
    ) -> Result<bool, String> {
        let value = match self {
            Self::SkyboxBrightness
            | Self::EnvironmentDiffuseIntensity
            | Self::EnvironmentSpecularIntensity
                if value.is_finite() =>
            {
                value.clamp(0.0, SKYBOX_MAX_BRIGHTNESS) // 公共规格已规范化，业务层仍防御范围。
            }
            Self::SkyboxYaw if value.is_finite() => value.rem_euclid(360.0), // 精确输入和左拖保存稳定等价角。
            _ => return Err(format!("{}必须是有限数字", self.label())),      // NaN和无穷失败关闭。
        };
        let slot = match self {
            Self::SkyboxBrightness => &mut environment.skybox_brightness, // 只写亮度字段。
            Self::EnvironmentDiffuseIntensity => &mut environment.environment_diffuse_intensity, // 只写PBR漫反射补光字段。
            Self::EnvironmentSpecularIntensity => &mut environment.environment_specular_intensity, // 只写PBR镜面反射字段。
            Self::SkyboxYaw => &mut environment.skybox_yaw_degrees, // 只写方向字段。
        };
        if *slot == value {
            return Ok(false); // 同值不改变草稿。
        }
        *slot = value; // F12天空请求下一Presentation帧即时读取新值。
        Ok(true) // 本字段产生真实变化。
    }
}

/// BSN右栏中的固定数值挂载槽。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct EnvironmentNumericInputSlot(pub(crate) EnvironmentNumericKind);

/// 公共NumericInput根上的环境字段marker。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct EnvironmentNumericInput(pub(super) EnvironmentNumericKind); // lib.rs注册带该Query的系统，marker类型需保持crate可见。

/// 场景环境字符输入与左拖冻结的稳定作者身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EnvironmentNumericTarget {
    pub(super) scene_id: SceneId,           // 防止同名动态场景跨切换串写。
    pub(super) authoring_revision: u64, // 任何外部作者变化都使旧环境输入失效。
}

/// 一次中键输入冻结根、字段和可选场景环境目标。
#[derive(Clone, Debug)]
pub(super) struct EnvironmentNumericFocus {
    pub(super) entity: Entity,               // 提交必须来自同一公共根。
    pub(super) kind: EnvironmentNumericKind, // 四项字段不得在失焦时串槽。
    pub(super) target: EnvironmentNumericTarget,
}

/// 一次左拖的场景环境起点。
#[derive(Debug)]
pub(super) enum EnvironmentNumericScrubStart {
    Authored {
        target: EnvironmentNumericTarget, // 稳定场景与作者修订。
        before: SceneEnvironmentDescriptor, // 唯一环境撤销前完整快照。
        before_value: f32,                // 当前逻辑字段未归一化的精确作者原值。
    },
}

/// 四个根共享的唯一左拖事务。
#[derive(Debug)]
pub(super) struct EnvironmentNumericScrub {
    pub(super) entity: Entity, // Changed与Committed必须来自同一根。
    pub(super) kind: EnvironmentNumericKind, // 拖动期间字段身份不可切换。
    pub(super) start: EnvironmentNumericScrubStart, // 完整场景环境起点。
    pub(super) initial_value: f32, // UI显示起点，用于判定Yaw等价角度是否拖回起点。
    pub(super) latest_value: f32, // 保存该字段最近一次实时采用的精确原始值。
    pub(super) aborted: bool,  // 冲突后保留取消态到松手，防止同次拖动重绑。
}

/// 四个环境根共享中键焦点和唯一左拖事务。
#[derive(Resource, Default)]
pub(crate) struct EnvironmentNumericInputState {
    pub(super) focus: Option<EnvironmentNumericFocus>, // 当前字符输入稳定身份。
    pub(super) scrub: Option<EnvironmentNumericScrub>, // 当前左值拖动事务。
}

/// 当前草稿是否提供可持久编辑的动态场景环境目标。
pub(super) fn authored_target(
    session: &EditorAuthoringSession,
) -> Option<EnvironmentNumericTarget> {
    let draft = session.draft.as_ref()?; // 没有草稿时天空字段禁用。
    (draft.manifest.is_data_driven()).then(|| EnvironmentNumericTarget {
        scene_id: draft.scene_id.clone(),         // 冻结真实场景身份。
        authoring_revision: session.revision, // 冻结作者分支身份。
    })
}
