//! 编辑器普通按钮的释放点击协议；共享三态投影负责现代拾取的按压反馈及连续拖动。

mod components;
mod resources;
mod systems;
#[cfg(test)]
mod tests;

use crate::InteractionButton;
pub use components::EditorButtonClick;
pub use systems::{EditorButtonInput, update_editor_button_clicks};

use bevy::{input::InputSystems, prelude::*, ui::UiSystems};

/// 独立工具和真实UI夹具也可只安装点击协议，不必启动数值编辑或主题渲染。
pub struct EditorButtonInputPlugin;

impl Plugin for EditorButtonInputPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::UiInteractionPlugin>() {
            app.add_plugins(crate::UiInteractionPlugin);
        }
        register(app);
    }
}

pub(super) fn register(app: &mut App) {
    // 只增加旁路点击状态，不接管游戏按钮；编辑器动作显式读取本协议才改变触发时机。
    app.register_required_components::<InteractionButton, EditorButtonClick>()
        .register_required_components::<InteractionButton, bevy::ui::RelativeCursorPosition>() // 官方Pressed可能在移出后保留；视觉以真实落点区分按压与移出。
        .init_resource::<resources::ButtonCapture>()
        .add_systems(
            PreUpdate,
            update_editor_button_clicks
                .after(UiSystems::Focus)
                .after(crate::UiInteractionSystems::Sync) // 普通动作必须读取同帧现代拾取完成后的三态。
                .after(InputSystems),
        );
}
