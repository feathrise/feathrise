//! 四个历史环境专业数值框的首次公共子树挂载。

use bevy::prelude::*; // Commands、Added槽位与字体句柄。
use feathrise_editor_ui::{NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue, spawn_numeric_scrub_field}; // 开箱生成标签、进度填充、数值和单位。

use super::{EnvironmentNumericInput, EnvironmentNumericInputSlot}; // 槽位字段身份转换为消息根marker。
use crate::ui::scene::FONT_PATH; // 动态控件沿用F12统一中文字体。

/// 在四个 BSN 槽位内各挂载一次公共专业数值框。
pub(crate) fn setup_environment_numeric_inputs(
    mut commands: Commands, // 为Added槽位创建一次固定子树。
    slots: Query<(Entity, &EnvironmentNumericInputSlot), Added<EnvironmentNumericInputSlot>>, // 四个槽位分别处理一次。
    asset_server: Res<AssetServer>, // 公共控件需要明确字体句柄。
    settings: Res<feathrise_editor_core::EditorSettings>, // 初始倍率来自持久设置。
) {
    for (slot, marker) in &slots {
        let kind = marker.0; // 冻结当前槽位字段身份。
        let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // 与右栏静态文字共享字体。
        let initial = match kind {
            super::EnvironmentNumericKind::SkyboxBrightness => 1_000.0, // 与地图环境默认天空盒日间亮度保持一致。
            super::EnvironmentNumericKind::EnvironmentDiffuseIntensity => 300.0, // 默认克制人物与地形的天空染色。
            super::EnvironmentNumericKind::EnvironmentSpecularIntensity => 1_000.0, // 默认保留水面斜视天空反射。
            super::EnvironmentNumericKind::SkyboxYaw => 0.0, // 默认 IBL 方向零度。
        };
        commands.entity(slot).with_children(|parent| {
            let root = spawn_numeric_scrub_field(
                parent,                        // 完整公共控件挂入固定右栏槽位。
                EnvironmentNumericInput(kind), // 根 marker 路由四项环境消息。
                font,                          // 标签、数值和单位共享字体。
                NumericScrubFieldOptions::new(
                    kind.label(),                 // 中文字段标签。
                    kind.unit(),                  // cd/m²、角度或百分比。
                    NumericValue::F32(initial),   // 首帧安全占位值。
                    kind.spec(),                  // 精度、范围和Yaw循环语义。
                    kind.scrub_config(&settings), // 左值与右倍率拖动配置。
                )
                .with_style(NumericScrubFieldStyle {
                    height: 24.0,
                    label_font_size: 9.5,
                    column_gap: 3.0,
                    ..default()
                }),
            );
            parent
                .commands()
                .entity(root)
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                    node.max_width = percent(100);
                }); // 仅创建时适配栏宽，不替换原规格、数值或精确输入缓冲。
        });
    }
}
