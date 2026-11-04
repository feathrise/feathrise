//! 显式验收仅访问原按钮和原系统顺序；生产门面不公开私有UI组件或相机系统参数。
use crate::components::MapEditorActionButton;
use bevy::{ecs::schedule::IntoSystemSet, prelude::*};

pub use crate::{types::EditorAction, viewport::modes::ViewportMode};

/// 返回当前真实存在、已布局且未禁用的动作按钮，不在验收中临时生成动作实体。
pub fn action_button(world: &mut World, action: EditorAction) -> Option<Entity> {
    world
        .query_filtered::<
            (Entity, &MapEditorActionButton, &ComputedNode),
            Without<bevy::ui::InteractionDisabled>,
        >()
        .iter(world)
        .find(|(_, button, node)| button.0 == action && node.size().min_element() > 0.)
        .map(|(entity, _, _)| entity)
}

/// 与真实UI一致，验收的点击请求也只存活一帧，后续可用性仍由原输入系统决定。
pub fn invalidate_action_clicks(world: &mut World) {
    for mut click in world
        .query_filtered::<&mut feathrise_editor_ui::EditorButtonClick, With<MapEditorActionButton>>(
        )
        .iter_mut(world)
    {
        click.invalidate();
    }
}

/// 只暴露原系统的调度身份，保证输入源先于真实相机射线，不另造相机同步系统。
pub fn camera_sync_set() -> impl SystemSet {
    crate::viewport::sync_editor_viewport_camera.into_system_set()
}

/// 当前正式多地形光标的就绪门与点击票据；用于等待真实悬停并解释被取消的按下。
pub fn ground_cursor_snapshot(world: &World) -> serde_json::Value {
    crate::authoring::terrain_ground::cursor_validation_snapshot(world)
}
