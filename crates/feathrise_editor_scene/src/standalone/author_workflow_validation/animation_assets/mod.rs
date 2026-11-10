//! 自有工程动画作者历史与磁盘资产热更验收；原控件、保存和AssetServer各自保持唯一实现。

mod io;
mod resources;
mod systems;
mod types;

use bevy::prelude::*;

pub(super) fn install(app: &mut App) {
    if std::env::var("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS").as_deref() == Ok("1")
        && std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").is_some()
        && std::env::var("NEW_EDITOR_NATIVE_SMOKE_SAVE").as_deref() == Ok("1")
    {
        app.init_resource::<resources::Fixture>();
    }
}

pub(super) fn begin(world: &mut World) -> Result<(), String> {
    systems::begin(world)
}

pub(super) fn advance(world: &mut World) -> Option<Result<serde_json::Value, String>> {
    systems::advance(world)
}
