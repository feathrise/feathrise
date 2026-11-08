//! 编辑器UI库：三套可选工具主题和可复用作者控件。
//! F12 通过本库取得控件；主题独立，底层输入与游戏共用同一运行时。
pub mod colors;
pub mod metrics; // 地图与角色、装备、技能等分库共用参考图的排版尺度。
pub mod localization;
pub mod context_help; // 所有分库共用隐藏帮助正文，由根工作台的唯一浮层显露。
pub mod button_input; // 普通按钮统一在原按钮内松开时执行，真实Interaction仅用于视觉与拖动。
pub use button_input::{EditorButtonClick, EditorButtonInput, EditorButtonInputPlugin, update_editor_button_clicks};
pub mod feedback; // 跨分库反馈统一交给外壳底栏，不在参数正文中重复占位。
pub use feedback::PanelStatusText;
mod dialog;
pub use dialog::EditorDialogBackdrop;
pub mod theme; // 三套可选主题只作用于显式所属的编辑器绘制项。
pub use theme::{EditorCodeSurface, EditorLiteralColor, EditorPalette, EditorThemePlugin, EditorThemeRoot};
pub mod helpers;
pub use feathrise_ui_widgets::types;
pub mod widgets;
pub mod workspace_tabs; // 预设分库共用原有中央标签布局与互斥命中区。
use bevy::prelude::*;
pub use feathrise_ui_widgets::UiWidgetsSystemSet as EditorUiSystemSet;
pub use feathrise_ui_widgets::InteractionButton;
pub use feathrise_ui_widgets::components;
pub use feathrise_ui_widgets::messages;
pub use feathrise_ui_widgets::resources;
pub use components::*;
pub use helpers::*;
pub use messages::*;
pub use resources::*;
pub use types::*;
pub use widgets::*;
pub use feathrise_ui_widgets::button_color_system; // 业务呈现可显式排在共享三态着色后，避免同帧相互覆盖。
pub struct EditorUiPlugin;
impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<EditorButtonInputPlugin>() { app.add_plugins(EditorButtonInputPlugin); }
        if !app.is_plugin_added::<EditorThemePlugin>() { app.add_plugins(EditorThemePlugin); }
        if !app.is_plugin_added::<feathrise_ui_widgets::UiWidgetsPlugin>() {
            app.add_plugins(feathrise_ui_widgets::UiWidgetsPlugin); // 同 App 共享唯一焦点和消息处理。
        }
        localization::install(app);
    }
}

#[cfg(test)]
mod tests;

mod text_input;
pub use text_input::single_line_input;
