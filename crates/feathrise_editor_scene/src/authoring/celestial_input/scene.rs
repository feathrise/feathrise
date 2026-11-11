//! 天体光照专业数值框的公共子树挂载。

use bevy::prelude::*;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_ui::{NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue, spawn_numeric_scrub_field};

use super::contract::{CelestialNumericInput, CelestialNumericInputSlot};
use crate::ui::scene::FONT_PATH;

/// 在全局与场景二十个固定槽位内各创建一次公共 NumericInput。
pub(crate) fn setup_celestial_numeric_inputs(
    mut commands: Commands,
    slots: Query<(Entity, &CelestialNumericInputSlot), Added<CelestialNumericInputSlot>>,
    asset_server: Res<AssetServer>,
    settings: Res<EditorSettings>,
) {
    for (slot, marker) in &slots {
        let key = marker.0;
        let font: Handle<Font> = feathrise_editor_core::editor_font(&asset_server, FONT_PATH);
        commands.entity(slot).with_children(|parent| {
            let root = spawn_numeric_scrub_field(
                parent,
                CelestialNumericInput(key),
                font,
                NumericScrubFieldOptions::new(
                    key.field.label(),
                    key.field.unit(),
                    NumericValue::F32(0.0),
                    key.field.spec(),
                    key.field.scrub_config(&settings),
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
                }); // 全局与场景沿用同一二十个根；重排不重建输入或取消活动缓冲。
        });
    }
}
