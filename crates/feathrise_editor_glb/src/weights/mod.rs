//! 蒙皮源文件、草稿计算和另存的后台实现。

mod asset;
mod edit;
mod file;
mod influences;
mod posed;
mod spatial;

use asset::Document;
use bevy::prelude::*;
use feathrise_editor_glb_model::{
    GlbTask,
    weights::{
        EditRequest, Prepared, SurfaceInfo, VertexWeights, WeightDocument, WeightOperations,
    },
};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

pub(super) fn operations() -> WeightOperations {
    WeightOperations {
        load: |root, path, cancel| Box::pin(file::load(root, path, cancel)),
    }
}

impl WeightDocument for Document {
    fn surface_count(&self) -> usize {
        self.surfaces.len()
    }

    fn surface(&self, index: usize) -> &SurfaceInfo {
        &self.surfaces[index].info
    }

    fn rigid_count(&self) -> usize {
        self.rigid.len()
    }

    fn rigid_primitive(&self, index: usize) -> (usize, usize) {
        let part = &self.rigid[index];
        (part.mesh, part.primitive)
    }

    fn rest_positions(&self, surface: usize) -> &[Vec3] {
        &self.surfaces[surface].points.positions
    }

    fn rest_visible(&self, ray: Ray3d, point: Vec3, bias: f32) -> bool {
        self.occlusion.visible(ray, point, bias)
    }

    fn prepare(&self, request: EditRequest, cancel: &AtomicBool) -> Result<Prepared, String> {
        edit::prepare(self, request, cancel)
    }

    fn save(
        self: Arc<Self>,
        root: PathBuf,
        draft: Vec<Arc<[VertexWeights]>>,
        cancel: Arc<AtomicBool>,
    ) -> GlbTask<String> {
        Box::pin(file::save(root, self, draft, cancel))
    }
}
