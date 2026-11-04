//! 植被作者图层连续参数的公共数值框接线。
//!
//! 【功能设计】左键拖动只更新当前草稿中的轻量参数与面板文字，松开时才压入一条
//! `VegetationAuthoringChanged`并启动一次single-flight编译；右键只修改本字段灵敏度，
//! 中键进入精确输入。拖动目标始终冻结地图、图层和组的稳定ID。

use bevy::prelude::*; // 业务marker、资源和BSN槽位使用Bevy ECS类型。
use feathrise_map_content::MapVegetationAuthorGroupMode; // 组容量和覆盖阈值按正式组规则决定可用性。
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::MapEditorScrubField; // 强类型键保证五份持久倍率不串槽。
use feathrise_editor_ui::{
    NumericInputScrubConfig, NumericInputScrubMapping, NumericInputSpec, NumericValue,
}; // 公共控件负责手势、规范化、精确输入和灵敏度消息。

mod scene; // 固定槽位与开箱即用公共控件子树。
mod systems; // 精确输入、拖动事务、灵敏度和Presentation同步。
mod transaction; // 稳定目标解析与完整前后快照收口。

pub(crate) use scene::vegetation_numeric_slots; // 静态植被面板嵌入五行槽位。
pub(crate) use systems::{
    apply_vegetation_numeric_scrub, handle_vegetation_numeric_input,
    setup_vegetation_numeric_inputs, sync_vegetation_numeric_inputs,
}; // 插件分别挂到Input与Presentation阶段。

const SCRUB_DRAG_THRESHOLD_PIXELS: f32 = 4.0; // 四像素以内的左右键位移保持无动作单击。
const SCRUB_SENSITIVITY_MIN: f32 = 0.01; // 最慢允许标准手感的百分之一。
const SCRUB_SENSITIVITY_MAX: f32 = 100.0; // 最快限制为标准手感的一百倍。
const SCRUB_SENSITIVITY_PIXELS_PER_OCTAVE: f32 = 48.0; // 右拖四十八像素让倍率翻倍或减半。
const EXPONENTIAL_SCRUB_PER_PIXEL: f32 = std::f32::consts::LN_2 / 48.0; // 权重每四十八像素翻倍。

/// 五个字段共享一套输入基础设施，但业务值域、显示单位和规则相关性保持独立。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum VegetationNumericKind {
    #[default]
    Priority, // 有符号整数优先级。
    Weight,                // 正数竞争权重。
    OpacityPercent,        // 图层归一化透明度的百分比显示。
    GroupCapacityPercent,  // 共享容量组的百分比总容量。
    ClaimThresholdPercent, // 高优先级覆盖组的百分比认领阈值。
}

impl VegetationNumericKind {
    /// 公共字段左侧的稳定短标签。
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Priority => "优先级",
            Self::Weight => "权重",
            Self::OpacityPercent => "透明度",
            Self::GroupCapacityPercent => "组容量",
            Self::ClaimThresholdPercent => "覆盖阈值",
        }
    }

    /// 百分比字段显示百分号，其余字段无需伪造业务单位。
    pub(super) const fn unit(self) -> &'static str {
        match self {
            Self::OpacityPercent | Self::GroupCapacityPercent | Self::ClaimThresholdPercent => "%",
            Self::Priority | Self::Weight => "",
        }
    }

    /// 精确输入和拖动使用同一个正式值域，避免两套合法数据口径。
    pub(super) fn spec(self) -> NumericInputSpec {
        match self {
            Self::Priority => NumericInputSpec::i32()
                .with_min(NumericValue::I32(i32::MIN))
                .with_max(NumericValue::I32(i32::MAX))
                .with_step(NumericValue::I32(1))
                .with_step_buttons(false)
                .with_replace_on_first_key(true),
            Self::Weight => NumericInputSpec::f32(2)
                .with_min(NumericValue::F32(0.05))
                .with_max(NumericValue::F32(16.0))
                .with_step(NumericValue::F32(0.05))
                .with_step_buttons(false)
                .with_replace_on_first_key(true),
            Self::OpacityPercent | Self::GroupCapacityPercent | Self::ClaimThresholdPercent => {
                NumericInputSpec::f32(0)
                    .with_min(NumericValue::F32(0.0))
                    .with_max(NumericValue::F32(100.0))
                    .with_step(NumericValue::F32(1.0))
                    .with_step_buttons(false)
                    .with_replace_on_first_key(true)
            }
        }
    }

    /// 字段默认显示值；真正选择会在同一Presentation阶段同步进来。
    pub(super) const fn initial_value(self) -> NumericValue {
        match self {
            Self::Priority => NumericValue::I32(0),
            Self::Weight => NumericValue::F32(1.0),
            Self::OpacityPercent | Self::GroupCapacityPercent | Self::ClaimThresholdPercent => {
                NumericValue::F32(0.0)
            }
        }
    }

    /// 每个字段的基础手感和吸附精度；用户倍率只在此基础上缩放。
    pub(super) fn scrub_config(self, settings: &EditorSettings) -> NumericInputScrubConfig {
        let (mapping, drag_step, minimum, maximum) = match self {
            Self::Priority => (
                NumericInputScrubMapping::Linear {
                    units_per_pixel: 0.25, // 四像素至少形成一个整数档，避免轻微手抖跨多级。
                },
                1.0,
                i32::MIN as f32,
                i32::MAX as f32,
            ),
            Self::Weight => (
                NumericInputScrubMapping::Exponential {
                    exponent_per_pixel: EXPONENTIAL_SCRUB_PER_PIXEL,
                },
                0.05,
                0.05,
                16.0,
            ),
            Self::OpacityPercent | Self::GroupCapacityPercent => (
                NumericInputScrubMapping::Linear {
                    units_per_pixel: 1.0, // 一像素对应一个百分点。
                },
                1.0,
                0.0,
                100.0,
            ),
            Self::ClaimThresholdPercent => (
                NumericInputScrubMapping::Linear {
                    units_per_pixel: 1.0, // 精确到一个百分点，右拖倍率可进一步降速。
                },
                1.0,
                0.0,
                100.0,
            ),
        };
        NumericInputScrubConfig {
            mapping,
            drag_step,
            minimum,
            maximum,
            sensitivity: self.sensitivity(settings),
            minimum_sensitivity: SCRUB_SENSITIVITY_MIN,
            maximum_sensitivity: SCRUB_SENSITIVITY_MAX,
            drag_threshold_pixels: SCRUB_DRAG_THRESHOLD_PIXELS,
            sensitivity_pixels_per_octave: SCRUB_SENSITIVITY_PIXELS_PER_OCTAVE,
        }
    }

    /// 从统一设置读取本字段倍率；非法手改值在控件边界回退到一倍。
    pub(super) fn sensitivity(self, settings: &EditorSettings) -> f32 {
        let value = settings
            .map_editor_scrub_sensitivities
            .get(self.scrub_field()); // 公共设置表集中维护字段到存储槽映射。
        if value.is_finite() {
            value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX)
        } else {
            1.0 // NaN与无穷不能进入指数映射。
        }
    }

    /// 松开右键时仅改写对应设置槽，自动保存因此最多触发一次。
    pub(super) fn set_sensitivity(self, settings: &mut EditorSettings, value: f32) {
        settings.map_editor_scrub_sensitivities.set(
            self.scrub_field(),
            value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX),
        ); // 强类型入口写设置前再次收紧公共安全范围。
    }

    /// 只读判断倍率是否真的变化，调用侧据此避免无意义触发EditorSettings自动保存。
    pub(super) fn sensitivity_changed(self, settings: &EditorSettings, value: f32) -> bool {
        settings
            .map_editor_scrub_sensitivities
            .get(self.scrub_field())
            != value.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX)
    }

    /// 返回统一设置表中的稳定字段键。
    const fn scrub_field(self) -> MapEditorScrubField {
        match self {
            Self::Priority => MapEditorScrubField::VegetationPriority,
            Self::Weight => MapEditorScrubField::VegetationWeight,
            Self::OpacityPercent => MapEditorScrubField::VegetationOpacity,
            Self::GroupCapacityPercent => MapEditorScrubField::VegetationGroupCapacity,
            Self::ClaimThresholdPercent => MapEditorScrubField::VegetationClaimThreshold,
        }
    }

    /// 只有与当前组规则直接相关的共享参数才允许编辑。
    pub(super) const fn relevant_for_group(self, mode: MapVegetationAuthorGroupMode) -> bool {
        match self {
            Self::GroupCapacityPercent => {
                matches!(mode, MapVegetationAuthorGroupMode::WeightedPartition)
            }
            Self::ClaimThresholdPercent => {
                matches!(mode, MapVegetationAuthorGroupMode::PriorityOverride)
            }
            Self::Priority | Self::Weight | Self::OpacityPercent => true,
        }
    }
}

/// 每个动态公共输入根携带稳定字段身份，消息无需按实体名称反查。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VegetationNumericInput(pub(crate) VegetationNumericKind);

/// BSN只创建固定空槽，公共控件在Added帧一次生成完整子树。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct VegetationNumericInputSlot(pub(crate) VegetationNumericKind);

/// 字符焦点与唯一左拖事务共享一个业务资源，稳定帧不分配。
#[derive(Resource, Default)]
pub(crate) struct VegetationNumericInputState {
    focus: Option<transaction::VegetationNumericFocus>, // 中键精确输入冻结的作者身份。
    scrub: Option<transaction::VegetationNumericScrub>, // 左拖第一次真实变化后建立的前后快照。
    aborted_scrub: Option<(Entity, VegetationNumericKind)>, // 异常回滚后拒绝同一次物理拖动的后续Changed直到松键。
}

impl VegetationNumericInputState {
    /// 异常回滚已结束作者事务，但共享 UI 控件仍可能继续发布同一次按住期间的Changed。
    fn abort_scrub_until_release(
        &mut self,                   // 唯一植被专业输入状态。
        entity: Entity,              // 当前仍被物理捕获的公共根。
        kind: VegetationNumericKind, // 五个字段的松键不能互相清除拒绝门。
    ) {
        self.aborted_scrub = Some((entity, kind)); // 只冻结Copy身份，不保留作者快照或R8 Arc。
    }

    /// 共享 UI 控件全局只有一个专业拖动，拒绝门存在时所有后续Changed都应丢弃。
    const fn scrub_is_aborted(&self) -> bool {
        self.aborted_scrub.is_some() // 防止回滚后的下一帧重新begin并覆盖外部作者结果。
    }

    /// 对应Scrub Committed只释放拒绝门，旧作者事务已经完成回滚。
    fn release_aborted_scrub(
        &mut self,                   // 唯一植被专业输入状态。
        entity: Entity,              // 必须与异常时物理捕获根一致。
        kind: VegetationNumericKind, // 稳定字段身份也必须一致。
    ) -> bool {
        if self.aborted_scrub != Some((entity, kind)) {
            return false; // 其他字段或根的提交继续走自己的正常路径。
        }
        self.aborted_scrub = None; // 当前按住周期结束，下一次独立按下可以建立新事务。
        true
    }
}

#[cfg(test)]
mod sensitivity_tests {
    use super::*; // 只读倍率比较不需要启动Bevy App或作者编译任务。

    #[test]
    fn unchanged_vegetation_sensitivities_do_not_request_a_settings_write() {
        let settings = EditorSettings::default(); // 五个逻辑字段默认各自为一倍。
        for kind in [
            VegetationNumericKind::Priority,
            VegetationNumericKind::Weight,
            VegetationNumericKind::OpacityPercent,
            VegetationNumericKind::GroupCapacityPercent,
            VegetationNumericKind::ClaimThresholdPercent,
        ] {
            assert!(!kind.sensitivity_changed(&settings, 1.0)); // 拖走再回起点保持只读。
            assert!(kind.sensitivity_changed(&settings, 2.0)); // 不同终点才允许触发自动保存。
        }
    }
}
