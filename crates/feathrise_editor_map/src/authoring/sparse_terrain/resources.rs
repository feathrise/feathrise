//! 稀疏保存的只读建筑/院落完成门；公开系统参数组不暴露建筑域内部资源。
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

#[derive(SystemParam)]
pub(crate) struct SparseBuildingSaveGate<'w> {
    pub(super) building: Option<Res<'w, crate::authoring::buildings::BuildingAuthoringState>>,
    pub(super) registry: Option<Res<'w, feathrise_map_content::BuildingPresetRegistry>>,
    pub(super) courtyard: Option<Res<'w, crate::authoring::courtyards::CourtyardAuthoringState>>,
    pub(super) surface: Option<feathrise_scene::TerrainObjectClipmapGround<'w>>,
}
