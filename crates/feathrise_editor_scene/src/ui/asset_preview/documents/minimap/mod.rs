//! 中央文本编辑的代码轮廓与拖动导航；复用原生排版和EditableText.viewport，不创建第二个编辑器。

use bevy::prelude::*;

mod control;
mod math;
mod raster;
mod systems;
#[cfg(test)]
mod tests;

pub(super) use control::handle_input;

pub(super) fn install(app: &mut App) {
    app.init_resource::<super::resources::DocumentMinimapState>()
        .add_systems(
            Update,
            handle_input
                .after(crate::scene_browser::handle_scene_browser_input)
                .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
                .before(super::interaction::actions)
                .in_set(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            systems::sync_visibility
                .after(super::presentation::sync)
                .in_set(crate::SceneEditorSystemSet::Presentation),
        )
        .add_systems(
            PostUpdate,
            (
                systems::update_outline,
                systems::sync_viewport.after(super::navigation::NavigationSystemSet::Reveal),
            )
                .chain()
                .after(super::presentation::restore_source_scroll)
                .before(bevy::ui_widgets::ImeSystems::UpdatePosition)
                .in_set(bevy::ui::UiSystems::PostLayout),
        ); // 缩略框读取最终滚动，候选定位同帧读取同一偏移。
}
