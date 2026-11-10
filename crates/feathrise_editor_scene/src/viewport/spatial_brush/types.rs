//! 空间笔刷的应用方式、完整请求身份和冻结手柄；不持有地形密度副本。
use super::super::placement_move::{MoveCamera, MoveHandle};
use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState, types::EditorTool};
use bevy::prelude::*;
use feathrise_scene_content::{SceneId, SceneTerrainKind};
use feathrise_terrain::volume::VolumeBrush;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub(crate) struct SpatialBrushGizmos; // 固定笔刷可移入地下，独立前置轮廓不改变普通地表刷圈；允许嵌入视口只读投影。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ApplyMode {
    #[default]
    Point,
    Line,
    Path,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Mouse,
    Spatial,
    Point,
    Line,
    Path,
    Place,
    Start,
    End,
    Execute,
    Clear,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    pub scene_id: SceneId,
    pub folder: String,
    pub revision: u64,
    pub terrain: u64,
    pub structure: u64,
    pub tool: EditorTool,
}
impl Identity {
    pub fn capture(editor: &SceneEditorState, session: &EditorAuthoringSession) -> Option<Self> {
        let draft = session.draft.as_ref()?;
        (editor.open
            && !editor.close_requested
            && editor.tool.is_volume_sculpt()
            && editor.selected_scene_id.as_ref() == Some(&draft.scene_id)
            && draft.terrain_manifest().terrain.kind == SceneTerrainKind::SparseClipmap
            && super::super::super::authoring::terrain_documents::canvas_writable(session, editor))
            .then(|| Self {
                scene_id: draft.scene_id.clone(),
                folder: draft.terrain_folder().to_owned(),
                revision: session.revision,
                terrain: session.terrain_revision,
                structure: session.placement_structure_revision,
                tool: editor.tool,
            })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SpatialBrushRequest {
    pub(super) identity: Identity,
    pub brush: VolumeBrush,
    pub path: Option<std::sync::Arc<feathrise_terrain::volume::VolumePathPlan>>,
}
impl SpatialBrushRequest {
    #[cfg(test)]
    pub(crate) fn for_test(
        editor: &SceneEditorState,
        session: &EditorAuthoringSession,
        brush: VolumeBrush,
    ) -> Self {
        Self {
            identity: Identity::capture(editor, session).expect("测试必须使用真实场景作者身份"),
            brush,
            path: None,
        }
    }
    pub(crate) fn matches(
        &self,
        editor: &SceneEditorState,
        session: &EditorAuthoringSession,
    ) -> bool {
        Identity::capture(editor, session).as_ref() == Some(&self.identity)
    }
}

pub(super) struct SpatialDrag {
    pub identity: Identity,
    pub origin: Vec3,
    pub grabbed: Vec3,
    pub handle: MoveHandle,
    pub camera: MoveCamera,
    pub ui_size: Vec2,
    pub grid: f32,
}
