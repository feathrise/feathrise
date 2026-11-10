//! 直接投影原笔刷已经发布的正式植被完成值；按住期间持续输出，不借用整场景保存/捕获门。

mod resources;
mod systems;
#[cfg(test)]
mod tests;

use bevy::prelude::*;
pub use resources::{EmbeddedVegetationPreview, EmbeddedVegetationState};

pub struct VegetationPreviewPlugin;

/// 完整场景送出时撤销旧预览候选的采用资格；在途CPU任务仍占唯一槽直到真实结束。
/// 下一轮从当前生产版本重放，避免旧中间R8在新场景之后回退，也不沿用旧ACK漏掉变化。
pub fn invalidate(world: &mut World) {
    let old = if let Some(mut state) = world.get_resource_mut::<EmbeddedVegetationState>() {
        state.source = None;
        state.offered.clear();
        state.failed.clear();
        state.pending = state.task.is_some();
        state.ready.take()
    } else { None };
    if let Some(old) = old { world.resource_mut::<crate::authoring::AuthoringRetirement>().push(old); }
}

impl Plugin for VegetationPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::authoring::AuthoringRetirement>()
            .init_resource::<EmbeddedVegetationState>()
            .add_systems(Update, systems::advance.after(crate::SceneEditorSystemSet::Tasks)
                .after(crate::authoring::sync_live_blade_density));
    }
}
