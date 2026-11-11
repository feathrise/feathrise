//! 移动手柄跨域回归夹具：使用真实已有建筑校验系统，仅允许延迟采用后台结果。

use super::{
    index::current_identity, resources::BuildingAuthoringState, types::BuildingSpatialIndex,
    validation,
};
use crate::authoring::EditorAuthoringSession;
use crate::ui::building_presets::BuildingPresetEditorState;
use crate::viewport::placement_move::handle_placement_move;
use bevy::prelude::*;
use feathrise_scene_content::{
    BUILDING_PRESET_SCHEMA_VERSION, BuildingPresetCatalogAsset, BuildingPresetDescriptor,
    BuildingPresetRegistry, ResolvedBuildingPresetCatalog,
};
use std::sync::Arc;

#[derive(Resource, Default)]
pub(crate) struct ValidationPollingEnabled(pub bool);

pub(crate) fn install_existing_edit_validator(app: &mut App, preset: BuildingPresetDescriptor) {
    let registry = BuildingPresetRegistry {
        catalog: Handle::default(),
        revision: 1,
        resolved: ResolvedBuildingPresetCatalog::build(&BuildingPresetCatalogAsset {
            schema_version: BUILDING_PRESET_SCHEMA_VERSION,
            presets: vec![preset],
        })
        .unwrap(),
        last_error: None,
    };
    let identity =
        current_identity(app.world().resource::<EditorAuthoringSession>(), &registry).unwrap();
    app.insert_resource(registry)
        .insert_resource(BuildingAuthoringState {
            active_identity: Some(identity),
            active_index: Some(Arc::new(BuildingSpatialIndex::default())),
            ..default()
        })
        .init_resource::<BuildingPresetEditorState>()
        .init_resource::<ValidationPollingEnabled>()
        .add_systems(
            Update,
            (
                validation::poll_validation
                    .run_if(|enabled: Res<ValidationPollingEnabled>| enabled.0),
                validation::request_existing_edit,
            )
                .chain()
                .after(handle_placement_move),
        ); // 请求在手柄松手后受理，采用仍走完整原子校验门。
}
