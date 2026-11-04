//! 本库主题的滑块入口，刻度实体树复用共享控件。
use crate::colors;
use bevy::prelude::*;
pub use feathrise_ui_widgets::{SliderTick, SliderVisual};
pub fn spawn_slider<T: Component, H: Component>(
    parent: &mut ChildSpawnerCommands,
    track_marker: T,
    thumb_marker: H,
    font: Handle<Font>,
    thumb_left: f32,
    ticks: &[SliderTick],
) {
    spawn_slider_with_visual(
        parent,
        track_marker,
        thumb_marker,
        font,
        thumb_left,
        ticks,
        SliderVisual::default(),
    );
}
pub fn spawn_slider_with_visual<T: Component, H: Component>(
    parent: &mut ChildSpawnerCommands,
    track_marker: T,
    thumb_marker: H,
    font: Handle<Font>,
    thumb_left: f32,
    ticks: &[SliderTick],
    visual: SliderVisual,
) {
    feathrise_ui_widgets::spawn_slider_with_palette(
        parent,
        track_marker,
        thumb_marker,
        font,
        thumb_left,
        ticks,
        feathrise_ui_widgets::SliderPalette {
            track: colors::TRACK_BG,
            border: colors::INPUT_BORDER,
            tick: colors::TEXT_MUTED,
            thumb: colors::THUMB_BG,
            shadow: crate::helpers::text_shadow(),
        },
        visual,
    );
}
