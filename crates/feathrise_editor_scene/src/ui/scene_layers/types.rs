//! 作者内容分类与常数时间的数量、行号映射，不扫描场景实体或完整角色集合。

use crate::authoring::EditorSceneDraft;

pub(super) type CameraIdentity = (
    bevy::prelude::Vec3,
    f32,
    f32,
    f32,
    bevy::prelude::UVec2,
    bool,
);

pub(super) struct PendingFocus {
    pub target: crate::ui::scene_context_menu::SceneObjectTarget,
    pub camera: CameraIdentity,
    pub task: bevy::tasks::Task<Option<(bevy::prelude::Vec3, bevy::prelude::Vec3)>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum LayerList {
    #[default]
    Water,
    Vegetation,
    DomainLayers,
    DomainObjects,
}

impl LayerList {
    pub(super) fn label(self, domain: Option<&crate::game_authoring::scene_interaction::SceneInteractionState>) -> &'static str {
        match self {
            Self::Water => "水域",
            Self::Vegetation => "植被图层",
            Self::DomainLayers => domain.and_then(|state| state.snapshot.as_ref()).map_or("", |snapshot| snapshot.layer_label),
            Self::DomainObjects => domain.and_then(|state| state.snapshot.as_ref()).map_or("", |snapshot| snapshot.object_label),
        }
    }

    pub(super) fn count(self, draft: &EditorSceneDraft, domain: Option<&crate::game_authoring::scene_interaction::SceneInteractionState>) -> usize {
        match self {
            Self::Water => crate::authoring::terrain_documents::root_water::listed_count(draft),
            Self::Vegetation => draft.vegetation.as_ref().map_or(0, |vegetation| {
                vegetation
                    .authoring
                    .as_ref()
                    .map_or(vegetation.layers.len(), |authoring| authoring.layers.len())
            }),
            Self::DomainLayers => domain.and_then(|state| state.snapshot_for(draft)).map_or(0, |snapshot| snapshot.layer_count),
            Self::DomainObjects => domain.and_then(|state| state.snapshot_for(draft)).map_or(0, |snapshot| snapshot.object_count),
        }
    }
}
