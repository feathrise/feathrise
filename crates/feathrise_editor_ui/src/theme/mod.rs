//! 三套编辑器主题仅接管所属UI的最终绘制色；保留逻辑色、原生控件状态和实体身份。

mod button_feedback;
mod components;
mod constants;
mod extract;
#[cfg(test)]
mod feedback_tests;
mod resources;
#[cfg(test)]
mod tests;
mod transparent_feedback;

use bevy::{
    prelude::*,
    render::{ExtractSchedule, RenderApp},
    ui_render::RenderUiSystems,
};
pub use components::{EditorCodeSurface, EditorLiteralColor, EditorThemeRoot};
pub use resources::EditorPalette;

pub struct EditorThemePlugin;
impl Plugin for EditorThemePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorPalette>()
            .init_resource::<resources::ThemeTopology>()
            .add_observer(extract::parent_inserted)
            .add_observer(extract::parent_removed)
            .add_observer(extract::root_inserted)
            .add_observer(extract::root_removed);
    }

    fn finish(&self, app: &mut App) {
        // 等所有插件建立RenderApp后再安装提取钩子，独立工具和不同插件顺序保持一致。
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .init_resource::<resources::ThemeExtraction>()
                .add_systems(
                    ExtractSchedule,
                    extract::colors
                        .after(RenderUiSystems::ExtractCursor)
                        .after(RenderUiSystems::ExtractImages)
                        .after(RenderUiSystems::ExtractBorders),
                )
                .add_systems(
                    ExtractSchedule,
                    transparent_feedback::extract.after(extract::colors),
                );
        }
    }
}
