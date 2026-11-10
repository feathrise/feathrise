//! 场景树菜单只冻结一条已呈现的对象身份；操作仍由原作者域执行。

use bevy::prelude::*;
use feathrise_scene_content::{SceneId, SceneInstanceId, TerrainId};

use crate::ui::gltf_browser::EditorGltfMeshSelection;
use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SceneObjectKey {
    Placement {
        index: usize,
        id: String,
        mesh: Option<EditorGltfMeshSelection>,
    },
    Terrain {
        index: usize,
        id: TerrainId,
    },
    Subscene {
        index: usize,
        id: SceneInstanceId,
    },
    Water {
        kind: crate::types::WaterAuthoringFeatureKind,
        index: usize,
        id: String,
        terrain: Option<TerrainId>, // None是场景根水，私有网格可具有完全相同的层ID。
    },
    Vegetation {
        index: usize,
        id: String,
        /// 旧场景只有正式输出层时仍显示内容，但不把合并输出冒充可写作者层。
        output: bool,
    },
    Domain {
        kind: crate::game_authoring::scene_interaction::DomainItemKind,
        index: usize,
        id: String,
        hash: String,
        actions: u8,
        copy_locked: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SceneObjectTarget {
    pub scene_id: SceneId,
    pub revision: u64,
    pub key: SceneObjectKey,
    pub name: String,
    pub hidden: bool,
    pub locked: bool,
    /// 雕刻派生的摆放不能绕过其地形作者入口复制或删除。
    pub copyable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneMenuAction {
    Focus,
    Rename,
    Copy,
    Delete,
    Visibility,
    Lock,
}

impl SceneMenuAction {
    pub(super) const ALL: [Self; 6] = [
        Self::Focus,
        Self::Rename,
        Self::Copy,
        Self::Delete,
        Self::Visibility,
        Self::Lock,
    ];

    pub(super) fn label(self, target: &SceneObjectTarget) -> &'static str {
        match self {
            Self::Focus => "聚焦到对象",
            Self::Rename => "重命名",
            Self::Copy if !matches!(target.key, SceneObjectKey::Placement { .. }) => "复制副本",
            Self::Copy => "复制",
            Self::Delete => "删除",
            Self::Visibility if target.hidden => "显示",
            Self::Visibility => "隐藏",
            Self::Lock if target.locked => "解锁",
            Self::Lock => "锁定",
        }
    }
}

impl SceneObjectTarget {
    pub(crate) fn validate_name(&self, name: &str) -> Result<(), &'static str> {
        if name.chars().count() > 128 || name.chars().any(char::is_control) {
            return Err("名称最多128个字符，不能包含控制字符");
        }
        if name.is_empty() && !matches!(self.key, SceneObjectKey::Placement { .. }) {
            return Err("名称不能为空");
        }
        if matches!(self.key, SceneObjectKey::Subscene { .. }) && name.len() > 256 {
            return Err("子场景名称过长，请缩短后重试");
        }
        Ok(())
    }

    pub(crate) fn current(
        &self,
        editor: &SceneEditorState,
        session: &EditorAuthoringSession,
    ) -> bool {
        let Some(draft) = session.draft.as_ref().filter(|draft| {
            editor.open
                && !editor.close_requested
                && !editor.scene_transition_in_progress
                && editor.pending_scene_transition.is_none()
                && !session.saving
                && editor.selected_scene_id.as_ref() == Some(&self.scene_id)
                && draft.scene_id == self.scene_id
                && session.revision == self.revision
        }) else {
            return false;
        };
        match &self.key {
            SceneObjectKey::Placement { index, id, .. } => draft
                .placements
                .placements
                .get(*index)
                .is_some_and(|record| &record.id == id),
            SceneObjectKey::Terrain { index, id } => draft
                .manifest
                .terrains
                .as_ref()
                .map_or(*index == 0 && *id == TerrainId::legacy(), |objects| {
                    objects.get(*index).is_some_and(|object| &object.id == id)
                }),
            SceneObjectKey::Subscene { index, id } => draft
                .manifest
                .subscenes
                .get(*index)
                .is_some_and(|record| &record.id == id),
            SceneObjectKey::Water { kind, index, id, terrain } => {
                crate::authoring::terrain_documents::root_water::listed_source(draft, terrain.as_ref()).is_some_and(|water| {
                    use crate::types::WaterAuthoringFeatureKind::*;
                    match kind {
                        StillWater => water.layers.get(*index).map(|item| &item.id),
                        River => water.rivers.get(*index).map(|item| &item.id),
                        Waterfall => water.waterfalls.get(*index).map(|item| &item.id),
                    }
                    .is_some_and(|current| current == id)
                })
            }
            SceneObjectKey::Vegetation { index, id, output } => {
                draft.vegetation.as_ref().is_some_and(|vegetation| {
                    if *output {
                        vegetation
                            .layers
                            .get(*index)
                            .is_some_and(|item| &item.id == id)
                    } else {
                        vegetation
                            .authoring
                            .as_ref()
                            .and_then(|authoring| authoring.layers.get(*index))
                            .is_some_and(|item| &item.id == id)
                    }
                })
            }
            SceneObjectKey::Domain { hash, .. } => draft.adopted_domain_hash() == hash,

        }
    }

    pub(super) fn supports(&self, action: SceneMenuAction) -> bool {
        match (&self.key, action) {
            (SceneObjectKey::Water { kind, .. }, action) => {
                matches!(action, SceneMenuAction::Focus | SceneMenuAction::Delete)
                    || (matches!(kind, crate::types::WaterAuthoringFeatureKind::StillWater)
                        && action == SceneMenuAction::Copy)
            }
            (SceneObjectKey::Vegetation { output: true, .. }, action) => {
                action == SceneMenuAction::Focus
            }
            (SceneObjectKey::Vegetation { .. }, SceneMenuAction::Rename) => false,
            (SceneObjectKey::Domain { actions, .. }, action) => actions & (1 << action as u8) != 0,
            (SceneObjectKey::Terrain { .. }, SceneMenuAction::Copy) => false,
            (
                SceneObjectKey::Subscene { .. },
                SceneMenuAction::Visibility | SceneMenuAction::Lock,
            ) => false,
            _ => true,
        }
    }

    pub(crate) fn allows(&self, action: SceneMenuAction) -> bool {
        self.supports(action)
            && match action {
                SceneMenuAction::Focus | SceneMenuAction::Visibility | SceneMenuAction::Lock => {
                    true
                }
                SceneMenuAction::Rename => !self.locked,
                SceneMenuAction::Copy
                    if matches!(self.key, SceneObjectKey::Domain { copy_locked: true, .. }) =>
                {
                    self.copyable
                } // 原角色协议允许只读复制锁定层。
                SceneMenuAction::Copy | SceneMenuAction::Delete => {
                    !self.locked
                        && self.copyable
                        && (!self.hidden || !matches!(self.key, SceneObjectKey::Placement { .. }))
                }
            }
    }
}

#[derive(Clone)]
pub(super) struct MenuTarget {
    pub row: Entity,
    pub object: SceneObjectTarget,
}

#[derive(Clone)]
pub(crate) struct SceneObjectRequest {
    pub target: SceneObjectTarget,
    pub action: SceneMenuAction,
    pub name: String,
}
