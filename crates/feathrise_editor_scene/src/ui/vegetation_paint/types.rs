//! 固定行池的轻量投影缓存；保留Arc身份，避免地址重用及稳定帧遍历全部组。
use crate::authoring::vegetation_paint_scope::PaintScope;
use feathrise_scene_content::{SceneId, SceneTerrainObject, SceneVegetationPaintGroup, TerrainId};
use std::{collections::BTreeSet, sync::Arc};
pub(super) struct PanelCache {
    pub scene_id: Option<SceneId>,
    pub groups: Option<Arc<[SceneVegetationPaintGroup]>>,
    pub objects: Option<Arc<[SceneTerrainObject]>>,
    pub scope: PaintScope,
    pub group: Option<String>,
    pub selected: Option<usize>,
    pub hidden: Arc<BTreeSet<TerrainId>>,
    pub locked: BTreeSet<TerrainId>,
}
