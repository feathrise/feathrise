//! 场景模型植被的根画布；复用已有图层与历史，独立于当前地形的二维密度。
use super::TerrainCanvas;
use crate::authoring::EditorSceneDraft;
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};
use feathrise_scene_content::SceneVegetationAsset;

/// 只借用一张画布。根槽停放后仍由场景持有，不能误取当前地形的私有植被。
pub(crate) struct VegetationSource<'a> {
    pub vegetation: Option<&'a SceneVegetationAsset>,
    pub densities: &'a BTreeMap<String, Arc<Vec<u8>>>,
    pub author_densities: &'a BTreeMap<String, Arc<Vec<u8>>>,
    pub dirty_layers: &'a BTreeSet<String>,
    pub author_dirty_layers: &'a BTreeSet<String>,
    pub registration_source_hash: Option<&'a String>,
    structural_dirty: bool,
}
impl<'a> VegetationSource<'a> {
    pub(crate) fn active(draft: &'a EditorSceneDraft) -> Self {
        Self { vegetation: draft.vegetation.as_ref(), densities: &draft.vegetation_densities,
            author_densities: &draft.vegetation_author_densities, dirty_layers: &draft.vegetation_dirty_layers,
            author_dirty_layers: &draft.vegetation_author_dirty_layers,
            registration_source_hash: draft.vegetation_registration_source_hash.as_ref(),
            structural_dirty: draft.terrain_documents.root_vegetation_dirty }
    }
    fn parked(canvas: &'a TerrainCanvas) -> Self {
        Self { vegetation: canvas.vegetation.as_ref(), densities: &canvas.vegetation_densities,
            author_densities: &canvas.vegetation_author_densities, dirty_layers: &canvas.vegetation_dirty_layers,
            author_dirty_layers: &canvas.vegetation_author_dirty_layers,
            registration_source_hash: canvas.vegetation_registration_source_hash.as_ref(),
            structural_dirty: canvas.structural_dirty }
    }
    /// 打开空图产生的默认组只是占位；首次作者修改或已有实际层才成为保存/预览来源。
    pub(crate) fn has_document(&self) -> bool {
        self.structural_dirty || self.registration_source_hash.is_some()
            || !self.dirty_layers.is_empty() || !self.author_dirty_layers.is_empty()
            || self.vegetation.is_some_and(|vegetation| !vegetation.layers.is_empty()
                || !vegetation.model_surfaces.is_empty()
                || vegetation.authoring.as_ref().is_some_and(|authoring| !authoring.layers.is_empty()))
    }
}
pub(crate) fn source(draft: &EditorSceneDraft) -> Option<VegetationSource<'_>> {
    if draft.terrain_documents.detached { return None; }
    if let Some(canvas) = draft.terrain_documents.root_vegetation.as_deref() {
        Some(VegetationSource::parked(canvas))
    } else if draft.terrain_documents.active.is_none() {
        Some(VegetationSource::active(draft))
    } else { None }
}

pub(super) fn park(draft: &mut EditorSceneDraft, previous: &mut TerrainCanvas) {
    let mut root = TerrainCanvas::default();
    macro_rules! take { ($($f:ident),*) => { $(root.$f = std::mem::take(&mut previous.$f);)* }; }
    take!(vegetation, vegetation_densities, vegetation_nonzero_layers, vegetation_density_stats,
        vegetation_author_densities, vegetation_conflicts, vegetation_dirty_layers,
        vegetation_author_dirty_layers, vegetation_registration_source_hash);
    root.structural_dirty = std::mem::take(&mut draft.terrain_documents.root_vegetation_dirty);
    draft.terrain_documents.root_vegetation = Some(Arc::new(root));
}

pub(crate) fn restore(draft: &mut EditorSceneDraft) {
    let Some(root) = draft.terrain_documents.root_vegetation.take() else { return; };
    let mut root = Arc::try_unwrap(root).unwrap_or_else(|shared| (*shared).clone());
    macro_rules! swap { ($($f:ident),*) => { $(std::mem::swap(&mut root.$f, &mut draft.$f);)* }; }
    swap!(vegetation, vegetation_densities, vegetation_nonzero_layers, vegetation_density_stats,
        vegetation_author_densities, vegetation_conflicts, vegetation_dirty_layers,
        vegetation_author_dirty_layers, vegetation_registration_source_hash);
    draft.terrain_documents.active_dirty |= root.structural_dirty;
    draft.terrain_documents.root_vegetation_dirty = root.structural_dirty;
}

pub(crate) fn clear_saved(draft: &mut EditorSceneDraft) {
    if let Some(root) = &mut draft.terrain_documents.root_vegetation {
        Arc::make_mut(root).clear_dirty();
    }
    draft.terrain_documents.root_vegetation_unregistered_hash = None;
    draft.terrain_documents.root_vegetation_dirty = false;
}

/// 首次作者操作才登记根植被，所用摘要来自打开时的后台读取，帧内不读取文件。
pub(crate) fn mark_registration(draft: &mut EditorSceneDraft) {
    if draft.terrain_documents.detached || draft.terrain_documents.active.is_some() { return; }
    draft.terrain_documents.root_vegetation_dirty = true;
    if draft.vegetation_registration_source_hash.is_none() {
        draft.vegetation_registration_source_hash = draft.terrain_documents.root_vegetation_unregistered_hash.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::{ModelSurfaceVegetation, SceneVegetationAsset};

    #[test]
    fn parked_model_root_keeps_its_source_and_registration_separate_from_private_canvas() {
        let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0;121]));
        draft.manifest.terrains = Some(Arc::default());
        draft.terrain_documents.root_vegetation_unregistered_hash = Some("missing".into());
        let root = SceneVegetationAsset { scene_id:draft.scene_id.clone(),
            model_surfaces:vec![Arc::new(ModelSurfaceVegetation::empty("root-model",[1;32],0.25))].into(), ..Default::default() };
        draft.vegetation = Some(root);
        assert!(source(&draft).unwrap().registration_source_hash.is_none(), "只打开空图不承担登记责任");
        mark_registration(&mut draft);
        let mut previous = TerrainCanvas::snapshot(&draft);
        park(&mut draft, &mut previous);
        draft.vegetation = Some(SceneVegetationAsset { scene_id:draft.scene_id.clone(),
            model_surfaces:vec![Arc::new(ModelSurfaceVegetation::empty("private-model",[2;32],0.25))].into(), ..Default::default() });
        let parked = source(&draft).unwrap();
        assert_eq!(parked.registration_source_hash.map(String::as_str),Some("missing"));
        assert_eq!(parked.vegetation.unwrap().model_surfaces[0].topology,[1;32]);
        assert!(draft.stored_terrain_changes());
        restore(&mut draft);
        assert_eq!(draft.vegetation.as_ref().unwrap().model_surfaces[0].topology,[1;32]);
        assert_eq!(draft.vegetation_registration_source_hash.as_deref(),Some("missing"));
    }
}
