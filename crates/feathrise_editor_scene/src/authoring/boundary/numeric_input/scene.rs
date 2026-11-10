//! 墙厚固定槽位首次挂载完整公共专业数值框。

use bevy::prelude::*; // Commands、Added槽位与字体句柄。
use feathrise_editor_core::EditorSettings; // 初始右拖倍率来自持久设置。
use feathrise_editor_ui::{
    NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue, spawn_numeric_scrub_field,
}; // 开箱生成标签、进度填充、数值和单位。

use super::{BoundaryField, BoundaryThicknessInput, BoundaryThicknessInputSlot, input_contract}; // 子树严格复用父模块业务契约。
use crate::ui::scene::FONT_PATH; // 与左栏静态文字共享统一中文字体。

/// BSN槽位首次出现时挂载完整公共“标签+进度+数值+单位”控件。
pub(crate) fn setup_boundary_thickness_input(
    mut commands: Commands, // 在新增固定槽位下创建一次子树。
    slots: Query<Entity, Added<BoundaryThicknessInputSlot>>, // 正常唯一槽位，测试重复时各自安全创建。
    asset_server: Res<AssetServer>,                          // 公共控件接收统一字体句柄。
    settings: Res<EditorSettings>,                             // 初始右拖倍率来自持久设置。
) {
    for slot in &slots {
        let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH); // 与左栏静态文字使用同一字体。
        for field in [
            BoundaryField::Thickness,
            BoundaryField::Bottom,
            BoundaryField::Top,
        ] {
            let (spec, scrub) = input_contract(&settings, field); // 精确和拖动共享同一范围。
            commands.entity(slot).with_children(|parent| {
                let root = spawn_numeric_scrub_field(
                    parent,                        // 完整公共控件直接挂在固定槽位。
                    BoundaryThicknessInput(field), // 消息根使用领域marker路由。
                    font.clone(),                  // 中文标签与单位统一字体。
                    NumericScrubFieldOptions::new(
                        field.label(),          // 明确编辑完整空气墙厚度。
                        "m",                    // 世界米单位。
                        NumericValue::F32(2.0), // 首帧安全默认值，随后同步当前路径。
                        spec,                   // 字符范围与显示精度。
                        scrub,                  // 左值和右倍率拖动配置。
                    )
                    .with_style(NumericScrubFieldStyle {
                        width: 206.0,
                        height: 24.0,
                        ..default()
                    }),
                );
                parent.commands().entity(root).entry::<Node>().and_modify(
                    |mut node| {
                        node.width = percent(100);
                        node.min_width = px(0);
                        node.max_width = percent(100);
                    },
                ); // 只适配控件根宽度；数值规格、实时拖动及单墙撤销沿用原协议。
            });
        }
    }
}
