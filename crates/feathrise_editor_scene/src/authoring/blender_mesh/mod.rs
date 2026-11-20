//! 将选中的模型交给 Blender，保存后通过普通 GLB 加载和摆放事务更新该对象。

mod files;
mod systems;

pub(crate) use systems::{BlenderMeshAction, BlenderMeshSession, supports};

use bevy::prelude::*;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<systems::BlenderMeshSession>()
        .add_message::<BlenderMeshAction>()
        .add_systems(
            Update,
            systems::update
                .in_set(crate::SceneEditorSystemSet::Tasks)
                .after(super::poll_draft_save),
        );
}
