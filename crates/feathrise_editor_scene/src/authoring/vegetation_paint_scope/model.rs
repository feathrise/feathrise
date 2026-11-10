//! 植被域浅快照的受检安装与跨网格历史；所有成员校验后才改变作者数据。
use super::{
    super::{EditorSceneDraft, terrain_documents::TerrainCanvas},
    types::*,
};
use std::sync::Arc;

impl VegetationValues {
    pub(super) fn capture(canvas: &TerrainCanvas) -> Self {
        Self {
            definition: canvas.vegetation.clone(),
            authors: canvas.vegetation_author_densities.clone(),
            outputs: canvas.vegetation_densities.clone(),
            nonzero: canvas.vegetation_nonzero_layers.clone(),
            stats: canvas.vegetation_density_stats.clone(),
            conflicts: canvas.vegetation_conflicts.clone(),
        }
    }
    pub(super) fn install(&self, canvas: &mut TerrainCanvas) {
        canvas.vegetation = self.definition.clone();
        canvas.vegetation_author_densities = self.authors.clone();
        canvas.vegetation_densities = self.outputs.clone();
        canvas.vegetation_nonzero_layers = self.nonzero.clone();
        canvas.vegetation_density_stats = self.stats.clone();
        canvas.vegetation_conflicts = self.conflicts.clone();
        canvas
            .vegetation_dirty_layers
            .extend(self.outputs.keys().cloned());
        canvas
            .vegetation_author_dirty_layers
            .extend(self.authors.keys().cloned());
        canvas.structural_dirty = true; // 删除最后成员同样必须保存空作者描述。
    }
}

pub(super) fn install_values(
    draft: &mut EditorSceneDraft,
    id: &feathrise_scene_content::TerrainId,
    values: &VegetationValues,
) {
    if draft
        .terrain_documents
        .active
        .as_ref()
        .is_some_and(|active| active.definition.id == *id)
    {
        let mut canvas = TerrainCanvas::default();
        canvas.swap(draft);
        values.install(&mut canvas);
        canvas.swap(draft);
    } else if let Some(canvas) = draft.terrain_documents.values.get_mut(id) {
        values.install(Arc::make_mut(canvas)); // 像素仍为Arc，主线程只复制小型描述与句柄。
        draft.terrain_documents.dirty_stored.insert(id.clone());
    }
    draft.terrain_documents.saved.remove(id); // 保存后继续画也必须重新承担脏责任。
}

pub(crate) fn apply_history(
    draft: &mut EditorSceneDraft,
    change: &VegetationPaintChange,
    undo: bool,
) -> Result<(), String> {
    for id in change.canvases.keys() {
        if !draft
            .terrain_documents
            .active
            .as_ref()
            .is_some_and(|a| a.definition.id == *id)
            && !draft.terrain_documents.values.contains_key(id)
        {
            return Err(format!("跨网格历史的作者画布{}已失效", id.as_str()));
        }
    }
    // 只恢复植被；地形姿态由自己的历史控制，移动后撤销刷草不会把网格移回原处。
    for (id, (before, after)) in &change.canvases {
        install_values(draft, id, if undo { before } else { after });
    }
    draft.manifest.vegetation_paint_groups = if undo {
        change.before_groups.clone()
    } else {
        change.after_groups.clone()
    };
    draft.terrain_documents.paint_groups_dirty = true;
    Ok(())
}
