//! 作者内容分类与常数时间的数量、行号映射，不扫描场景实体或完整角色集合。

use crate::authoring::EditorSceneDraft;
use super::inventory::LayerInventory;

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

/// 场景树的固定作者分类。分类选择不绑定任何成员的作者画布。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuthorLayerCategory {
    Water,
    Vegetation,
}

impl AuthorLayerCategory {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Water => "水域",
            Self::Vegetation => "植被图层",
        }
    }

    pub(super) fn icon(self) -> &'static str {
        match self {
            Self::Water => "icons/editor/tree_water.png",
            Self::Vegetation => "icons/editor/tree_leaf.png",
        }
    }
}

impl LayerList {
    pub(super) fn author_category(self) -> Option<AuthorLayerCategory> {
        match self {
            Self::Water => Some(AuthorLayerCategory::Water),
            Self::Vegetation => Some(AuthorLayerCategory::Vegetation),
            _ => None,
        }
    }

    pub(super) fn label(self, domain: Option<&crate::game_authoring::scene_interaction::SceneInteractionState>) -> &'static str {
        match self {
            Self::Water => AuthorLayerCategory::Water.label(),
            Self::Vegetation => AuthorLayerCategory::Vegetation.label(),
            Self::DomainLayers => domain.and_then(|state| state.snapshot.as_ref()).map_or("", |snapshot| snapshot.layer_label),
            Self::DomainObjects => domain.and_then(|state| state.snapshot.as_ref()).map_or("", |snapshot| snapshot.object_label),
        }
    }

    pub(super) fn count(self, draft: &EditorSceneDraft, inventory: &LayerInventory, domain: Option<&crate::game_authoring::scene_interaction::SceneInteractionState>) -> usize {
        match self {
            Self::Water => inventory.count(true),
            Self::Vegetation => inventory.count(false),
            Self::DomainLayers => domain.and_then(|state| state.snapshot_for(draft)).map_or(0, |snapshot| snapshot.layer_count),
            Self::DomainObjects => domain.and_then(|state| state.snapshot_for(draft)).map_or(0, |snapshot| snapshot.object_count),
        }
    }
}
