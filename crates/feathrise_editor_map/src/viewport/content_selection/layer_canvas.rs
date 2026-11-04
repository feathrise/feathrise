//! 点击作者快照只属于活动网格；世界命中统一逆变换后才查询地表与植被。
use crate::authoring::EditorMapDraft;
use bevy::prelude::*;
use feathrise_map_content::{MapSurfaceAsset, MapVegetationAsset, TerrainId};
use std::{collections::BTreeMap, sync::Arc};

pub(super) struct Snapshot {
    pub terrain: Option<TerrainId>,
    pub inverse: Mat4,
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub surface: Option<MapSurfaceAsset>,
    pub indices: Option<Arc<Vec<u8>>>,
    pub weights: Option<Arc<Vec<u8>>>,
    pub vegetation: Option<MapVegetationAsset>,
    pub densities: BTreeMap<String, Arc<Vec<u8>>>,
}

impl Snapshot {
    pub(super) fn capture(draft: &EditorMapDraft) -> Self {
        let active = draft.terrain_documents.active.as_ref();
        let valid = draft.manifest.terrains.is_none() || active.is_some();
        let manifest = draft.terrain_manifest();
        Self {
            terrain: active.map(|active| active.definition.id.clone()),
            inverse: active.map_or(Mat4::IDENTITY, |active| {
                active
                    .definition
                    .transform
                    .as_transform()
                    .to_matrix()
                    .inverse()
            }),
            min: manifest.world_min,
            max: manifest.world_max,
            surface: valid.then(|| draft.surface.clone()).flatten(),
            indices: valid.then(|| draft.surface_indices.clone()).flatten(),
            weights: valid.then(|| draft.surface_weights.clone()).flatten(),
            vegetation: valid.then(|| draft.vegetation.clone()).flatten(),
            densities: if valid {
                draft.vegetation_author_densities.clone()
            } else {
                Default::default()
            },
        } // 只共享作者像素，未载入画布时不能把地图容器占位数据伪装成所点网格。
    }
}

pub(super) fn needs_author_canvas(filter: super::super::modes::SelectionFilter) -> bool {
    use super::super::modes::SelectionFilter;
    matches!(
        filter,
        SelectionFilter::All | SelectionFilter::Surface | SelectionFilter::Vegetation
    )
}
