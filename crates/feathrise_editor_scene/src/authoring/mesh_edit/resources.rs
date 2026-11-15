//! 编辑模式唯一状态，UI 和快捷键均发布同一种请求。

use super::{jobs::MeshEditJobs, mesh_geometry::WorldMesh, selection::MeshSelection, topology_ops::SubdivisionOptions, types::*};
use bevy::prelude::*;
use std::sync::Arc;

#[derive(Resource, Default)]
pub(crate) struct MeshEditState {
    pub(crate) embedded_revision: u64,
    pub(crate) embedded_preview: Option<feathrise_scene_content::embedded_author_preview::MeshPreview>,
    pub(crate) embedded_scene: Option<feathrise_scene_content::SceneId>,
    pub(crate) embedded_committed: bool,
    pub(crate) request: Option<MeshEditAction>,
    pub(crate) kind: MeshElementKind,
    pub(crate) tool: MeshEditTool,
    pub(super) target: Option<EditTarget>,
    pub(super) selected: MeshSelection,
    pub(super) world_mesh: Option<Arc<WorldMesh>>,
    pub(super) jobs: MeshEditJobs,
    pub(super) pivot: Option<(MeshElementKind, MeshSelection, Option<Vec3>)>,
    pub(super) hover: Option<usize>,
    pub(super) handle_hover: Option<MoveConstraint>,
    pub(super) movement: Option<ElementMove>,
    pub(super) box_selection: Option<BoxSelection>,
    pub(super) release_capture: bool,
    pub(super) ghost: Option<Entity>,
    pub(super) ghost_mesh: Option<Handle<Mesh>>,
    pub(super) ghost_material: Option<Handle<StandardMaterial>>,
    pub(super) hidden_roots: Vec<HiddenRoot>,
}

impl MeshEditState {
    pub(crate) fn active(&self) -> bool {
        self.target.is_some()
    }
    pub(crate) fn owns_mouse(&self) -> bool {
        self.active() || self.release_capture
    }
    pub(crate) fn moving(&self) -> bool {
        self.movement.is_some() || self.boxing() || self.jobs.waiting().is_some()
    }
    pub(crate) fn boxing(&self) -> bool {
        self.box_selection.is_some()
    }
    pub(crate) fn releasing_mouse(&self) -> bool {
        self.release_capture
    }
    pub(crate) fn selected_count(&self) -> usize {
        self.movement.as_ref().and_then(|movement| movement.result_selection.as_ref())
            .unwrap_or(&self.selected).len()
    }

    pub(super) fn selection_origin(&mut self, mesh: &WorldMesh) -> Option<Vec3> {
        if let Some((kind, selection, origin)) = &self.pivot
            && *kind == self.kind && selection.same_storage(&self.selected)
        {
            return *origin;
        }
        let origin = super::geometry::selection_center(
            &mesh.vertices, &mesh.selected_vertices(self.kind, &self.selected),
        );
        self.pivot = Some((self.kind, self.selected.clone(), origin));
        origin
    }

    pub(crate) fn subdivision_options(&self) -> Option<SubdivisionOptions> {
        match self.movement.as_ref()?.topology? {
            TopologyOperation::Subdivide(options) => Some(options),
            _ => None,
        }
    }

    #[cfg(feature = "validation")]
    pub(crate) fn subdivision_preview_for_validation(
        &self,
    ) -> Option<(SubdivisionOptions, &feathrise_scene_content::ScenePlacementRecord, bool)> {
        let options = self.subdivision_options()?;
        let movement = self.movement.as_ref()?;
        Some((options, &movement.preview, movement.valid))
    }

    #[cfg(any(test, feature = "validation"))]
    pub(crate) fn select_for_test(&mut self, kind: MeshElementKind, selected: u128) {
        self.kind = kind;
        self.selected = selected.into();
    }

    #[cfg(test)]
    pub(crate) fn activate_for_test(&mut self) {
        let record = serde_json::from_value(serde_json::json!({
            "id":"mesh-edit-ui-test","asset_path":"",
            "asset_target":feathrise_scene_content::ScenePlacementAssetTarget::DEFAULT_CUBOID,
            "translation":[0,0,0],"yaw_degrees":0,"scale":[1,1,1]
        }))
        .expect("UI 夹具使用合法内置记录");
        self.target = Some(EditTarget {
            scene_id: feathrise_scene_content::SceneId::parse("MAP-mesh-edit-ui-test").unwrap(),
            index: 0,
            author_revision: 0,
            structure_revision: 0,
            record,
        });
        self.selected = self.kind.all();
    }
}
