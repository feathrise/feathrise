//! 水网控制点四个公共专业数值框的首次挂载。

use bevy::prelude::*; // Commands、Added槽位和统一字体句柄。
use feathrise_editor_ui::{
    NumericScrubFieldOptions,  // 组合业务标签、单位、数值规格和视觉样式。
    NumericScrubFieldStyle,    // 水网仅适配可换行槽位，保留公共水墨铜木外观。
    NumericValue,              // 尚未绑定控制点时使用安全零值。
    spawn_numeric_scrub_field, // 完整标签、进度填充、数值和单位由公共控件一次生成。
};

use super::{WaterNetworkNumericInput, WaterNetworkNumericInputSlot}; // 槽位身份转换为业务根marker。
use crate::ui::scene::FONT_PATH; // 动态子树继续使用F12统一中文字体。

const WATER_NETWORK_COMPACT_FIELD_WIDTH: f32 = 100.0; // builder首帧占位宽度；返回根立即适配实际槽位，不再约束侧栏列宽。

/// 在四个BSN固定槽位内挂载开箱即用公共控件；每个槽位只在Added帧创建一次。
pub(crate) fn setup_water_network_numeric_inputs(
    mut commands: Commands, // 为新增槽位创建一次固定子树。
    slots: Query<(Entity, &WaterNetworkNumericInputSlot), Added<WaterNetworkNumericInputSlot>>, // 四个槽位各处理一次。
    asset_server: Res<AssetServer>, // 公共控件接收明确字体句柄。
    settings: Res<feathrise_editor_core::EditorSettings>, // 初始右拖倍率来自持久设置。
) {
    for (slot, marker) in &slots {
        let kind = marker.0; // 冻结当前槽位字段身份供完整控件配置复用。
        let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // 与静态左栏文字共享项目字体。
        let spec = kind.spec(); // 字符输入与左拖范围保持同一权威规格。
        let initial = NumericValue::F32(0.0); // 尚未绑定控制点时显示零并由同步系统禁用。
        let compact_style = NumericScrubFieldStyle {
            width: WATER_NETWORK_COMPACT_FIELD_WIDTH, // 公共根创建后按实际槽位伸缩。
            ..default() // 高度、圆角、间距和三级字号继续沿用公共专业字段默认值。
        };
        commands.entity(slot).with_children(|parent| {
            let root = spawn_numeric_scrub_field(
                parent,                         // 公共控件完整子树直接挂入固定槽位。
                WaterNetworkNumericInput(kind), // 消息按根实体反查四项业务之一。
                font,                           // 标签、数值和单位统一使用编辑器字体。
                NumericScrubFieldOptions::new(
                    kind.label(),                 // 左侧中文字段名。
                    kind.unit(),                  // 右侧米或米每秒单位。
                    initial,                      // 首帧安全占位值。
                    spec,                         // 范围、精度和精确输入规则。
                    kind.scrub_config(&settings), // 左值拖动与右倍率拖动配置。
                )
                .with_style(compact_style), // 双列只改变宽度，不改变输入手势、值范围或格式。
            ); // 返回根只用于一次布局适配，业务marker仍由原Query定位。
            parent
                .commands()
                .entity(root)
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                    node.max_width = percent(100);
                }); // 只适配原控件根到可换行槽位；数值协议、精度、拖动与输入子树完全不变。
        });
    }
}
