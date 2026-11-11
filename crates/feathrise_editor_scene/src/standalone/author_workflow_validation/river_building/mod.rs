//! 隔离工程中的原河流逐点绘制、建筑受检落印及两域原撤销重做；最终落盘交给父场景保存。

mod pointer;
mod resources;
mod systems;
mod types;

use bevy::prelude::*;

pub(super) fn install(app: &mut App) {
    if std::env::var("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS").as_deref() == Ok("1")
        && std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").is_some()
        && std::env::var("NEW_EDITOR_NATIVE_SMOKE_SAVE").as_deref() == Ok("1")
    {
        app.init_resource::<resources::Fixture>().add_systems(
            Update,
            pointer::inject
                .in_set(crate::SceneEditorSystemSet::Cursor)
                .before(crate::viewport::sync_editor_viewport_camera),
        );
    }
}

pub(super) fn begin(world: &mut World) -> Result<(), String> {
    systems::begin(world)
}

pub(super) fn advance(world: &mut World) -> Option<Result<serde_json::Value, String>> {
    systems::advance(world)
}
