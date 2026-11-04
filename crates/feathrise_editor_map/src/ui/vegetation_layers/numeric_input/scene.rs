//! 植被连续参数的静态槽位布局。

use bevy::prelude::*; // BSN Scene和紧凑UI布局值。

use super::{VegetationNumericInputSlot, VegetationNumericKind}; // 每个槽位保留明确字段身份。

const VEGETATION_NUMERIC_FIELD_HEIGHT: f32 = 24.0; // 与作者按钮同高，缩放后仍保留完整数值命中区域。

/// 声明一行稳定空槽；公共控件只在Added帧生成一次子树。
fn numeric_slot(kind: VegetationNumericKind) -> impl Scene {
    bsn! {
        VegetationNumericInputSlot(kind) // 动态挂载时无需依赖节点名或顺序猜测字段。
        Node {
            width: percent(100), // 专业字段填满左栏可用宽度。
            min_width: px(0), // 完整数值和标签不反向扩大可调侧栏。
            height: px(VEGETATION_NUMERIC_FIELD_HEIGHT), // 一体式标签、数值和单位共享命中区域。
            flex_shrink: 0.0, // 低高度窗口由外层侧栏滚动，不压扁命中区域。
        }
    }
}

/// 五个连续参数垂直排列；离散规则、锁定和结构操作仍保留普通按钮。
pub(crate) fn vegetation_numeric_slots() -> impl Scene {
    bsn! {
        Node {
            width: percent(100), // 数值组与图层列表对齐。
            min_width: px(0),
            flex_direction: FlexDirection::Column, // 长中文标签在窄侧栏中不互相挤压。
            row_gap: px(3), // 与编辑器其他紧凑属性行保持一致。
            flex_shrink: 0.0, // 外层负责滚动访问全部字段。
        }
        Children [
            (numeric_slot(VegetationNumericKind::Priority)), // 正式有符号整数，不使用浮点冒充。
            (numeric_slot(VegetationNumericKind::Weight)), // 大跨度正数使用指数拖动。
            (numeric_slot(VegetationNumericKind::OpacityPercent)), // UI百分比、作者值零至一。
            (numeric_slot(VegetationNumericKind::GroupCapacityPercent)), // 只在共享容量规则下启用。
            (numeric_slot(VegetationNumericKind::ClaimThresholdPercent)), // 只在高优先级覆盖规则下启用。
        ]
    }
}
