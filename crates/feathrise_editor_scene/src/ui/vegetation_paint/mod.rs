//! 植被绘制范围与场景级绘制组面板；目标勾选不改变检查器或活动作者画布。
mod components;
mod scene;
mod systems;
#[cfg(test)]
mod tests;
mod types;
pub(super) use scene::panel;
pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.add_systems(
        Update,
        systems::handle.in_set(crate::SceneEditorSystemSet::Input),
    )
    .add_systems(
        Update,
        (scene::setup, systems::sync)
            .chain()
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
}
