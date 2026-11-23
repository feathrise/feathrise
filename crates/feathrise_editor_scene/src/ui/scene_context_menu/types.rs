//! 场景树菜单只冻结一条已呈现的对象身份；操作仍由原作者域执行。

use bevy::prelude::*;
use feathrise_scene_content::{SceneId, SceneInstanceId, TerrainId};

use crate::ui::gltf_browser::EditorGltfMeshSelection;
use crate::{
    authoring::{EditorAuthoringSession, blender_mesh},
    resources::SceneEditorState,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SceneObjectKey {
    PlacementTreeRoot,
    PlacementFolder { empty: bool },
    MainSceneGlb,
    MainSceneAnimation(Option<usize>), // None 是当前尚无标准动画通道的命名草稿。
    SourceGlbAnimation { index: usize, id: String, clip_index: usize },
    Placement {
        index: usize,
        id: String,
        mesh: Option<EditorGltfMeshSelection>,
    },
    Group {
        first_member: usize,
        id: String,
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
        terrain: Option<TerrainId>,
        /// 尚未载入作者画布时记录清单槽及正式定义的句柄。
        cold: Option<(usize, AssetId<feathrise_scene_content::SceneVegetationAsset>)>,
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
    EditInBlender,
    RevealGlb,
    NewSceneAnimation,
    EditSourceGlbAnimation,
    NewSourceGlbAnimation,
    NewPlacementFolder,
    NewEmptyGlb,
    ChooseExistingGlb,
    NewSourceFolder,
    DeleteSourceFolder,
    RenameSourceNode,
}

impl SceneMenuAction {
    pub(super) const ALL: [Self; 17] = [
        Self::Focus,
        Self::RevealGlb,
        Self::NewSceneAnimation,
        Self::EditSourceGlbAnimation,
        Self::NewSourceGlbAnimation,
        Self::NewPlacementFolder,
        Self::NewEmptyGlb,
        Self::ChooseExistingGlb,
        Self::NewSourceFolder,
        Self::EditInBlender,
        Self::Rename,
        Self::RenameSourceNode,
        Self::Copy,
        Self::Delete,
        Self::DeleteSourceFolder,
        Self::Visibility,
        Self::Lock,
    ];

    pub(super) fn is_blender(self) -> bool {
        self == Self::EditInBlender
    }

    pub(crate) fn is_source_animation(self) -> bool {
        matches!(self, Self::EditSourceGlbAnimation | Self::NewSourceGlbAnimation)
    }

    pub(super) fn requests_name(self) -> bool {
        matches!(self, Self::Rename | Self::RenameSourceNode | Self::NewPlacementFolder | Self::NewEmptyGlb | Self::NewSourceFolder)
    }

    pub(super) fn label(self, target: &SceneObjectTarget) -> &'static str {
        match self {
            Self::Focus => "聚焦到对象",
            Self::Rename | Self::RenameSourceNode => "重命名",
            Self::Copy if !matches!(target.key, SceneObjectKey::Placement { .. }) => "复制副本",
            Self::Copy => "复制",
            Self::Delete | Self::DeleteSourceFolder => "删除",
            Self::Visibility if target.hidden => "显示",
            Self::Visibility => "隐藏",
            Self::Lock if target.locked => "解锁",
            Self::Lock => "锁定",
            Self::EditInBlender => "在 Blender 编辑",
            Self::RevealGlb => "定位 GLB 文件",
            Self::NewSceneAnimation => "新建动画",
            Self::EditSourceGlbAnimation => "编辑源 GLB 动画",
            Self::NewSourceGlbAnimation => "在源 GLB 新建动画",
            Self::NewPlacementFolder | Self::NewSourceFolder => "新建文件夹",
            Self::NewEmptyGlb => "新建空 GLB",
            Self::ChooseExistingGlb => "选择已有 GLB",
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
            SceneObjectKey::PlacementTreeRoot => true,
            SceneObjectKey::PlacementFolder { .. } => draft.object_folders.contains(&self.name),
            SceneObjectKey::MainSceneGlb => draft.placement_glb_hash.is_some(),
            SceneObjectKey::MainSceneAnimation(index) => draft.placement_glb_hash.is_some()
                && draft.motion.editable_clip_index() == *index && draft.motion.clip_name().is_some(),
            SceneObjectKey::Placement { index, id, .. } | SceneObjectKey::SourceGlbAnimation { index, id, .. } => draft
                .placements
                .placements
                .get(*index)
                .is_some_and(|record| &record.id == id),
            SceneObjectKey::Group { first_member, id } => draft.placements.placements
                .get(*first_member).is_some_and(|record| &record.id == id
                    && draft.object_folders.contains_member(&self.name, &record.editor.group)),
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
            SceneObjectKey::Vegetation { terrain, cold: Some((slot, _)), .. } => {
                draft.manifest.terrains.as_ref().and_then(|objects| objects.get(*slot))
                    .is_some_and(|object| terrain.as_ref() == Some(&object.id) && object.vegetation_path.is_some())
            }
            SceneObjectKey::Vegetation { index, id, output, terrain, cold: None } => {
                crate::ui::scene_layers::vegetation_source(draft, terrain.as_ref()).is_some_and(|vegetation| {
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

    pub(super) fn menu_has(
        &self,
        action: SceneMenuAction,
        session: &EditorAuthoringSession,
    ) -> bool {
        if matches!(self.key, SceneObjectKey::MainSceneGlb) {
            return self.supports(action) && session.draft.as_ref().is_some_and(|draft|
                draft.scene_id == self.scene_id && draft.placement_glb_hash.is_some()
                    && (action != SceneMenuAction::NewSceneAnimation
                        || (draft.motion.available() && draft.motion.clip_name().is_none())));
        }
        if action.is_source_animation() {
            return self.supports(action) && self.source_animation_path(session).is_some();
        }
        if action == SceneMenuAction::RevealGlb {
            return self.supports(action) && self.glb_path(session).is_some();
        }
        if matches!(action, SceneMenuAction::RenameSourceNode | SceneMenuAction::NewSourceFolder | SceneMenuAction::DeleteSourceFolder) {
            return self.supports(action) && self.glb_path(session).is_some()
                && matches!(&self.key, SceneObjectKey::Placement { index, .. }
                    if session.draft.as_ref().is_some_and(|draft|
                        draft.placements.placements.get(*index).is_some_and(|record| record.asset_target.is_scene0())));
        }
        if !action.is_blender() {
            return self.supports(action);
        }
        let (Some(draft), SceneObjectKey::Placement { index, id, mesh: None }) =
            (session.draft.as_ref(), &self.key)
        else {
            return false;
        };
        // 会话对应完整摆放，避免右键子网格时替换它所属的整个 GLB。
        draft.scene_id == self.scene_id && draft.placements.placements.get(*index)
            .is_some_and(|record| &record.id == id && blender_mesh::supports(draft, record))
    }

    pub(crate) fn glb_path<'a>(&self, session: &'a EditorAuthoringSession) -> Option<&'a str> {
        let (SceneObjectKey::Placement { index, id, .. } | SceneObjectKey::SourceGlbAnimation { index, id, .. }) = &self.key else {
            return None;
        };
        let draft = session.draft.as_ref().filter(|draft| draft.scene_id == self.scene_id)?;
        let record = draft.placements.placements.get(*index)
            .filter(|record| &record.id == id && !record.asset_target.is_builtin())?;
        // 子网格与完整实例都定位持久化记录引用的 GLB 文件。
        let path = std::path::Path::new(&record.asset_path);
        path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
            .then_some(record.asset_path.as_str())
    }

    pub(crate) fn source_animation_path<'a>(&self, session: &'a EditorAuthoringSession) -> Option<&'a str> {
        let path = self.glb_path(session)?;
        let draft = session.draft.as_ref()?;
        // 主文件始终由场景事务持有，不能从某条引用绕进独立动画保存器。
        (!draft.placement_glb_hash.is_some() || draft.placements_path() != path).then_some(path)
    }

    fn supports(&self, action: SceneMenuAction) -> bool {
        if action == SceneMenuAction::RenameSourceNode {
            return matches!(&self.key, SceneObjectKey::Placement { mesh: Some(mesh), .. }
                if mesh.node_index.is_some() && !mesh.orphaned_water_binding);
        }
        if action == SceneMenuAction::NewSourceFolder {
            return matches!(&self.key, SceneObjectKey::Placement { mesh, .. }
                if mesh.as_ref().is_none_or(|mesh| mesh.is_source_folder()));
        }
        if action == SceneMenuAction::DeleteSourceFolder {
            return matches!(&self.key, SceneObjectKey::Placement { mesh: Some(mesh), .. }
                if mesh.is_source_folder() && mesh.empty_source_folder);
        }
        if action == SceneMenuAction::NewPlacementFolder {
            return matches!(self.key, SceneObjectKey::MainSceneGlb | SceneObjectKey::PlacementTreeRoot
                | SceneObjectKey::PlacementFolder { .. } | SceneObjectKey::Group { .. });
        }
        if matches!(action, SceneMenuAction::NewEmptyGlb | SceneMenuAction::ChooseExistingGlb) {
            // 完整 GLB 摆放只登记在外层场景树，可以从空白处或外层文件夹发起。
            return matches!(self.key, SceneObjectKey::PlacementTreeRoot | SceneObjectKey::PlacementFolder { .. } | SceneObjectKey::Group { .. });
        }
        if matches!(self.key, SceneObjectKey::PlacementTreeRoot | SceneObjectKey::PlacementFolder { .. }) {
            return action == SceneMenuAction::Delete && matches!(self.key, SceneObjectKey::PlacementFolder { empty: true });
        }
        if matches!(self.key, SceneObjectKey::MainSceneGlb) {
            return matches!(action, SceneMenuAction::EditInBlender | SceneMenuAction::NewSceneAnimation);
        }
        // 游戏领域仍使用原六个菜单动作的位掩码，编辑器专属命令不进入该协议。
        if action == SceneMenuAction::NewSceneAnimation {
            return false;
        }
        if action.is_source_animation() {
            return matches!(&self.key, SceneObjectKey::Placement { mesh, .. }
                if mesh.as_ref().is_none_or(|mesh| mesh.valid && mesh.node_index.is_some()))
                || (action == SceneMenuAction::EditSourceGlbAnimation
                    && matches!(self.key, SceneObjectKey::SourceGlbAnimation { .. }));
        }
        if matches!(self.key, SceneObjectKey::SourceGlbAnimation { .. }) { return false; }
        if action.is_blender() {
            return matches!(self.key, SceneObjectKey::Placement { mesh: None, .. });
        }
        if action == SceneMenuAction::RevealGlb {
            return matches!(self.key, SceneObjectKey::Placement { .. });
        }
        match (&self.key, action) {
            (SceneObjectKey::MainSceneAnimation(_), action) => matches!(action, SceneMenuAction::Rename | SceneMenuAction::Delete),
            (SceneObjectKey::Placement { mesh: Some(mesh), .. }, SceneMenuAction::Focus)
                if mesh.is_source_folder() => false,
            (SceneObjectKey::Placement { mesh: Some(mesh), .. },
                SceneMenuAction::Rename | SceneMenuAction::Copy | SceneMenuAction::Delete
                | SceneMenuAction::Visibility | SceneMenuAction::Lock) if mesh.node_index.is_some() => false,
            (SceneObjectKey::Group { .. }, action) => action == SceneMenuAction::Focus,
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
                SceneMenuAction::Focus | SceneMenuAction::Visibility | SceneMenuAction::Lock
                | SceneMenuAction::RevealGlb | SceneMenuAction::NewSceneAnimation | SceneMenuAction::NewPlacementFolder
                | SceneMenuAction::NewEmptyGlb | SceneMenuAction::ChooseExistingGlb => {
                    true
                }
                SceneMenuAction::Rename | SceneMenuAction::RenameSourceNode => !self.locked,
                SceneMenuAction::Delete if matches!(self.key, SceneObjectKey::MainSceneAnimation(_)) => !self.locked,
                SceneMenuAction::NewSourceFolder | SceneMenuAction::DeleteSourceFolder => !self.locked && !self.hidden,
                SceneMenuAction::Delete if matches!(self.key, SceneObjectKey::PlacementFolder { empty: true }) => true,
                SceneMenuAction::EditSourceGlbAnimation | SceneMenuAction::NewSourceGlbAnimation => !self.locked,
                SceneMenuAction::EditInBlender => {
                    !self.locked && !self.hidden
                        && (self.copyable || matches!(self.key, SceneObjectKey::MainSceneGlb))
                }
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
