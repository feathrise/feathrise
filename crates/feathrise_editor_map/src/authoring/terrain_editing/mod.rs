//! 所选地形的几何作者事务：复用共享雕刻核心，IO/CPU分池，版本路径最后由对象清单采用。
mod input;
mod native_fields;
mod resources;
mod sources;
#[cfg(test)]
mod tests;
mod types;
mod voxel;
pub(crate) use native_fields::edit_fields;
pub(crate) use sources::edit;
pub(crate) use types::NativeFieldInput;
pub(crate) use types::ObjectBrushInput;
pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<resources::BrushGesture>().add_systems(
        Update,
        input::brush
            .in_set(crate::MapEditorSystemSet::Cursor)
            .after(crate::viewport::source_fence::update)
            .before(crate::viewport::select_editor_viewport_content)
            .run_if(crate::map_residency::document_active),
    ); // 只消费本帧相机、真实命中及来源范围验证后的落点。
}
