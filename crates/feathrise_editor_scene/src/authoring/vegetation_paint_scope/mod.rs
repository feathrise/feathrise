//! 跨网格刷草的场景级事务；本地画布、后台刷印、原子预览及历史相互独立。
mod model;
mod publication;
mod resources;
mod systems;
#[cfg(test)]
mod tests;
mod types;
mod worker;
#[cfg(test)]
pub(crate) use tests::painted_draft;

pub(crate) use model::apply_history;
pub(crate) use resources::{VegetationPaintSettings, VegetationPaintState};
pub(crate) use types::{PaintScope, VegetationPaintChange};

pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<VegetationPaintSettings>()
        .init_resource::<VegetationPaintState>()
        .add_systems(
            Update,
            systems::poll.in_set(crate::SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            systems::input
                .in_set(crate::SceneEditorSystemSet::Presentation)
                .before(super::apply_vegetation_brush),
        )
        .add_systems(
            Update,
            publication::publish
                .in_set(crate::SceneEditorSystemSet::Presentation)
                .after(super::sync_live_blade_density),
        );
}
