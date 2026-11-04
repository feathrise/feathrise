//! 网格水两个字段的固定槽位与公共专业数值控件挂载。
//!
//! 【功能设计】左键拖实际数值、右键拖字段独立灵敏度、中键进入精确输入；
//! Esc只在字符编辑期间恢复进入前数值，流速与高度的作者事务语义保持不变。

use bevy::prelude::*; // BSN静态场景、Commands与字体资产。
use feathrise_editor_ui::{
    NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue, spawn_numeric_scrub_field,
}; // builder一次复用共享 UI 控件字符编辑、三种鼠标手势、文字与自动fill。

use super::{
    PlacementWaterNumericInput, PlacementWaterNumericInputSlot, PlacementWaterNumericKind,
}; // 槽位字段身份与业务输入根标记。
use crate::ui::scene::FONT_PATH; // 动态文字继续使用F12统一中文字体。

const NUMERIC_CONTROL_WIDTH: f32 = 164.0; // 公共builder的初始像素基准；创建帧改为整栏百分比宽度。
const NUMERIC_FIELD_HEIGHT: f32 = 24.0; // 数值框沿用原命中高度，类别动作另用26px。
const NUMERIC_FIELD_FONT_SIZE: f32 = 9.5; // 紧凑右栏仍保持数值清晰可读。

/// 单个字段的空槽位；Presentation阶段只在Added帧挂载一次公共输入根。
fn numeric_input_slot(kind: PlacementWaterNumericKind) -> impl Scene {
    bsn! {
        PlacementWaterNumericInputSlot(kind)
        Node {
            width: percent(100), // 每个字段独占一行，低宽度右栏不压缩数值。
            min_width: px(0),
            height: px(NUMERIC_FIELD_HEIGHT), // 固定点击高度与类别按钮一致。
            flex_shrink: 0.0, // 右栏整体滚动，输入行自身不得被压扁。
        }
    }
}

/// 属性面板中的两个固定专业数值槽；顺序稳定为流速、世界Y偏移。
pub(in crate::ui::placement_water) fn placement_water_numeric_input_slots() -> impl Scene {
    bsn! {
        Node {
            width: percent(100), // 数值组填满属性栏内容宽度。
            min_width: px(0),
            flex_direction: FlexDirection::Column, // 两个较长数值框垂直排列避免低分辨率挤压。
            row_gap: px(3), // 与属性区其他紧凑行保持同一间距。
            flex_shrink: 0.0, // 低高度窗口由右栏滚动访问完整数值组。
        }
        Children [
            (numeric_input_slot(PlacementWaterNumericKind::FlowSpeed)), // 第一行编辑沿UV0.U输运的米每秒流速。
            (numeric_input_slot(PlacementWaterNumericKind::HeightOffset)), // 第二行编辑世界Y米制偏移。
        ]
    }
}

/// 在两个BSN槽位内用editor_ui builder挂载完整数值控件。
pub(crate) fn setup_placement_water_numeric_inputs(
    mut commands: Commands, // 只给新增槽位创建一次固定子树。
    slots: Query<(Entity, &PlacementWaterNumericInputSlot), Added<PlacementWaterNumericInputSlot>>, // 每个字段只处理Added帧。
    asset_server: Res<AssetServer>, // 动态文字加载统一字体句柄。
    settings: Res<feathrise_editor_core::EditorSettings>, // 初始ScrubConfig读取各自持久倍率。
) {
    for (slot, marker) in &slots {
        let kind = marker.0; // 冻结当前槽位字段身份供整个子树复用。
        let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // 与静态属性文字使用同一字体资源。
        let spec = kind.spec(); // 精确范围、小数位和无步进按钮规则保持字段独立。
        let initial = NumericValue::F32(0.0); // 尚无有效水绑定时显示零并由同步系统禁用。
        let style = NumericScrubFieldStyle {
            width: NUMERIC_CONTROL_WIDTH, // 建立原公共子树，根宽度随后跟随检查器。
            height: NUMERIC_FIELD_HEIGHT, // 完整填满24像素固定槽位。
            border_radius: 2.0,           // 与右侧属性区其他紧凑控件一致。
            column_gap: 3.0,              // 延续原标签、数值和单位间隔。
            label_font_size: NUMERIC_FIELD_FONT_SIZE, // 标签保持原来字号。
            value_font_size: NUMERIC_FIELD_FONT_SIZE, // 数值保持原来字号。
            unit_font_size: 9.0,          // 单位继续使用较小辅助字号。
        };
        let options = NumericScrubFieldOptions::new(
            kind.label(),                 // 左侧标签明确流速或Y偏移。
            kind.unit(),                  // 流速显示m/s，高度显示m。
            initial,                      // builder建立首帧公共数值模型。
            spec,                         // 中键精确输入保留字段独立范围与精度。
            kind.scrub_config(&settings), // 左拖真实值与右拖独立倍率保持原映射。
        )
        .with_style(style); // 只覆盖网格水右栏尺寸，交互与视觉子树由公共builder拥有。
        commands.entity(slot).with_children(|slot| {
            let root = spawn_numeric_scrub_field(
                slot,                             // 完整控件作为当前固定槽位的唯一子项。
                PlacementWaterNumericInput(kind), // 业务marker直接挂在builder返回根上。
                font.clone(),                     // 标签、数值与单位共享F12字体。
                options,                          // builder生成文字、fill、精确输入与左右拖动结构。
            );
            slot.commands()
                .entity(root)
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                    node.max_width = percent(100);
                }); // 与变换组同宽，只调整创建帧布局，不重建正在编辑的公共控件。
        });
    }
}
