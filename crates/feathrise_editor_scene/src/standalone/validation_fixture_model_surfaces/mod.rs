//! 显式自有工程中的静态scene.glb双层表面验收；普通构建与会话不安装输入系统。

mod resources;
mod lifecycle;
mod lifecycle_io;
mod systems;
mod types;

use bevy::prelude::*;
use feathrise_scene_content::{SceneVegetationKind, VegetationPresetDescriptor, VegetationPresetRegistry};

pub(super) fn install(app: &mut App) {
    if std::env::var("FEATHRISE_MODEL_SURFACE_VALIDATION").as_deref() != Ok("1")
        || std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").is_none()
        || std::env::var("NEW_EDITOR_NATIVE_SMOKE_SAVE").as_deref() != Ok("1")
    {
        return;
    }
    app.init_resource::<resources::Fixture>()
        .add_systems(Update, systems::lifecycle_input
            .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
            .before(crate::SceneEditorSystemSet::Input))
        .add_systems(
            Update,
            systems::pointer
                .in_set(crate::SceneEditorSystemSet::Cursor)
                .before(crate::viewport::sync_editor_viewport_camera),
        );
}

pub(super) fn begin(world: &mut World) -> Result<bool, String> {
    systems::begin(world)
}
pub(super) fn advance(world: &mut World) -> Option<Result<serde_json::Value, String>> {
    systems::advance(world)
}

pub(super) fn registered_presets(world: &World) -> Option<&[VegetationPresetDescriptor]> {
    systems::registered_presets(world)
}

pub(super) fn preset_uses_source(preset: &VegetationPresetDescriptor, kind: SceneVegetationKind, source: &str) -> bool {
    systems::preset_uses_source(preset, kind, source)
}

pub(super) fn scene_tools_mesh(world: &World) -> Option<&VegetationPresetDescriptor> {
    let registered = registered_presets(world)?;
    (world.get_resource::<VegetationPresetRegistry>()?.presets().len() == registered.len())
        .then(|| registered.iter().find(|preset| preset.kind() == SceneVegetationKind::Mesh)).flatten()
}
