//! 数值框使用本库主题，节点树和输入协议由共享控件维护。
use crate::colors;
use crate::helpers::{numeric_input_palette, text_shadow};
use bevy::prelude::*;
pub use feathrise_ui_widgets::{NumericScrubFieldOptions, NumericScrubFieldStyle};
pub fn spawn_numeric_scrub_field<M: Component>(
    parent: &mut ChildSpawnerCommands,
    marker: M,
    font: Handle<Font>,
    mut options: NumericScrubFieldOptions,
) -> Entity {
    // 作者页共用可读字号，具体字段仍保留单位、精度、范围及原冻结提交目标。
    options.style.label_font_size = options.style.label_font_size.max(crate::metrics::FONT_HINT);
    options.style.value_font_size = options.style.value_font_size.max(crate::metrics::FONT_CONTROL);
    options.style.unit_font_size = options.style.unit_font_size.max(10.0);
    feathrise_ui_widgets::spawn_numeric_scrub_field_with_palette(
        parent,
        marker,
        font,
        options,
        feathrise_ui_widgets::NumericScrubFieldPalette {
            input: numeric_input_palette(),
            fill: colors::TAB_ACTIVE_BG,
            label: colors::TEXT_MUTED,
            value: colors::TEXT_PRIMARY,
            unit: colors::TEXT_HINT,
            shadow: text_shadow(),
        },
    )
}

pub fn spawn_localized_numeric_scrub_field<M: Component>(
    parent: &mut ChildSpawnerCommands,
    marker: M,
    font: Handle<Font>,
    options: NumericScrubFieldOptions,
) -> Entity {
    use crate::localization::LocalizedText;
    use feathrise_ui_widgets::components::{
        NumericInputDisplayText, NumericInputScrubSensitivityText, NumericInputScrubValueText,
    };

    let label = options.label.clone();
    let unit = options.unit.clone();
    let root = spawn_numeric_scrub_field(parent, marker, font, options);
    // 仅为内置标签和单位绑定语言。原有数值、输入焦点与精确编辑仍由共享控件维护。
    parent.commands().queue(move |world: &mut World| {
        for (source, is_unit) in [(label, false), (unit, true)] {
            if source.is_empty() {
                continue;
            }
            let node = world.get::<Children>(root).and_then(|children| {
                children.iter().find(|child| {
                    world.get::<Text>(*child).is_some_and(|text| text.as_str() == source.as_str())
                        && world.get::<NumericInputDisplayText>(*child).is_none()
                        && world.get::<NumericInputScrubSensitivityText>(*child).is_none()
                        && world.get::<NumericInputScrubValueText>(*child).is_some() == is_unit
                })
            });
            if let Some(node) = node {
                world.entity_mut(node).insert(LocalizedText::label(&source));
            }
        }
    });
    root
}
