//! 区域边界墙厚公共数值框的精确输入与左右拖动契约。

use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::MapEditorScrubField; // 只读取墙厚独立持久灵敏度。
use feathrise_editor_ui::{
    NumericInputScrubConfig, NumericInputScrubMapping, NumericInputSpec, NumericValue,
}; // 共享 UI 控件完成字符规范化、线性拖动和倍率调整。

use super::{BoundaryField, SCRUB_SENSITIVITY_MAX, SCRUB_SENSITIVITY_MIN}; // 数值与灵敏度范围必须与业务门一致。

/// 公共字符规格和左右拖动配置。
pub(super) fn input_contract(
    settings: &EditorSettings,
    field: BoundaryField,
) -> (NumericInputSpec, NumericInputScrubConfig) {
    let (minimum, maximum) = field.range(); // 高度和墙厚复用输入但有不同范围。
    let spec = NumericInputSpec::f32(2)
        .with_min(NumericValue::F32(minimum)) // 正式非退化墙体下界。
        .with_max(NumericValue::F32(maximum)) // 正式静态碰撞上界。
        .with_step(NumericValue::F32(0.01)) // 厘米级精确输入与拖动吸附。
        .with_step_buttons(false) // 旧墙厚加减按钮已由专业字段取代。
        .with_live_update(false) // 字符输入只在Enter或失焦时提交。
        .with_replace_on_first_key(true); // 中键输入首字符替换当前值。
    let stored = settings
        .map_editor_scrub_sensitivities
        .get(MapEditorScrubField::BoundaryThickness); // 只读取墙厚自己的设置键。
    let sensitivity = if stored.is_finite() {
        stored.clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX) // 手改设置仍服从公共范围。
    } else {
        1.0 // 非有限配置回退标准一倍。
    };
    let scrub = NumericInputScrubConfig {
        mapping: NumericInputScrubMapping::Linear {
            units_per_pixel: 0.05, // 一倍灵敏度每二十像素改变一米墙厚。
        },
        drag_step: 0.01,                            // 结果吸附厘米。
        minimum: minimum,                           // 左拖不得产生退化墙体。
        maximum: maximum,                           // 左拖不得超过正式上限。
        sensitivity,                                // 墙厚字段自己的持久倍率。
        minimum_sensitivity: SCRUB_SENSITIVITY_MIN, // 右拖固定最细倍率。
        maximum_sensitivity: SCRUB_SENSITIVITY_MAX, // 右拖固定最快倍率。
        drag_threshold_pixels: 4.0,                 // 四像素内不误判为左拖。
        sensitivity_pixels_per_octave: 48.0,        // 右拖四十八像素翻倍或减半。
    };
    (spec, scrub) // 场景和同步共享同一公共契约。
}
