//! 保存水身份分帧捕获；只复制路径等小型元数据，像素继续由原作者Arc持有。
use std::{collections::BTreeSet, ops::Bound::{Excluded, Unbounded}, sync::Arc};
use feathrise_map_content::{MapId, MapTerrainObject, MapWaterAsset, TerrainId};
use crate::authoring::{EditorAuthoringSession, EditorMapDraft,
    resources::TerrainWaterSaveHandoffSnapshot, water_handoff::WaterFileHandoff};

#[derive(Default)]
pub(super) struct WaterSaveCapture {
    identity: Option<(MapId, u64, u64, u64, Option<TerrainId>)>,
    objects: Option<Arc<[MapTerrainObject]>>,
    object_cursor: usize,
    runtime_enabled: BTreeSet<TerrainId>,
    after: Option<TerrainId>,
    current: Option<(TerrainWaterSaveHandoffSnapshot, usize)>,
    targets: Vec<TerrainWaterSaveHandoffSnapshot>,
    complete: bool,
    root_cursor: usize,
    root_dirty_layers: Vec<(String, String, String)>,
    root_complete: bool,
}

#[cfg(test)]
mod tests;
struct Canvas<'a> {
    water: &'a MapWaterAsset,
    path: &'a str,
    dirty: &'a BTreeSet<String>,
    registration: bool,
}
fn canvas<'a>(draft: &'a EditorMapDraft, id: &TerrainId) -> Option<Canvas<'a>> {
    if draft.terrain_documents.active.as_ref().is_some_and(|active| &active.definition.id == id) {
        return Some(Canvas { water: draft.water.as_ref()?, path: &draft.water_asset_path,
            dirty: &draft.water_dirty_layers, registration: draft.water_catalog_registration_required
                || draft.terrain_documents.active.as_ref()?.definition.water_path.is_none() });
    }
    if draft.terrain_documents.saved.contains(id) { return None; }
    let canvas = draft.terrain_documents.values.get(id).filter(|canvas| canvas.dirty())?;
    Some(Canvas { water: canvas.water.as_ref()?, path: &canvas.water_asset_path,
        dirty: &canvas.water_dirty_layers, registration: canvas.water_catalog_registration_required
            || canvas.identity.as_ref().is_some_and(|identity| identity.definition.water_path.is_none()) })
}
impl WaterSaveCapture {
    pub(super) fn step(&mut self, session: &EditorAuthoringSession, handoff: &WaterFileHandoff) -> bool {
        let Some(draft) = &session.draft else { *self = Self::default(); return true; };
        let active = draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone());
        let identity = Some((draft.map_id.clone(), session.revision, session.water_revision, session.terrain_revision, active.clone()));
        let same_objects = match (&self.objects, &draft.manifest.terrains) {
            (Some(before), Some(now)) => Arc::ptr_eq(before, now),
            (None, None) => true,
            _ => false,
        };
        if self.identity != identity || !same_objects {
            // 捕获持有集合Arc，任何结构写入都经COW换指针；作者/水/地形修订或切画布也整批作废。
            *self = Self { identity, objects: draft.manifest.terrains.clone(), ..Default::default() };
        }
        if !self.root_complete {
            let source = crate::authoring::terrain_documents::root_water::source(draft);
            if let Some(water) = source.water {
                let end = (self.root_cursor + 64).min(water.layers.len());
                for layer in &water.layers[self.root_cursor..end] {
                    if source.dirty_layers.contains(&layer.id) {
                        self.root_dirty_layers.push((layer.id.clone(), layer.mask_source_path.clone(), layer.mask_texture_path.clone()));
                    }
                }
                self.root_cursor = end;
                if end != water.layers.len() { return false; }
                if end != 0 {
                    self.root_complete = true;
                    return false; // 根水与私有水共用64层预算，不能在完成根层的同帧再扫描私有层。
                }
            }
            self.root_complete = true;
        }
        if self.complete || self.objects.is_none() { return true; }
        let objects = self.objects.as_ref().unwrap();
        if self.object_cursor < objects.len() {
            // 启用资格来自本批容器清单，不能借可能过期的活动/停放画布定义；索引同样按8对象分帧。
            let end = (self.object_cursor + 8).min(objects.len());
            for object in &objects[self.object_cursor..end] {
                if object.enabled { self.runtime_enabled.insert(object.id.clone()); }
            }
            self.object_cursor = end;
            return false;
        }
        let mut layers_left = 64;
        for _ in 0..8 {
            if let Some((target, cursor)) = &mut self.current {
                let Some(source) = canvas(draft, &target.terrain_id) else { self.current = None; continue; };
                let end = (*cursor + layers_left).min(source.water.layers.len());
                for layer in &source.water.layers[*cursor..end] {
                    if source.dirty.contains(&layer.id) {
                        target.dirty_layers.push((layer.id.clone(), layer.mask_source_path.clone(), layer.mask_texture_path.clone()));
                    }
                }
                layers_left -= end - *cursor;
                *cursor = end;
                if end != source.water.layers.len() { return false; }
                let (target, _) = self.current.take().unwrap();
                self.targets.push(target); // 按稳定ID递增，之后检查当前画布只需二分查找。
                if layers_left == 0 { return false; }
            }
            let stored = match &self.after {
                Some(after) => draft.terrain_documents.values.range((Excluded(after), Unbounded)).next(),
                None => draft.terrain_documents.values.iter().next(),
            }.map(|(id, _)| id);
            let active = active.as_ref().filter(|id| self.after.as_ref().is_none_or(|after| *id > after));
            let next = match (stored, active) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let Some(id) = next.cloned() else { self.complete = true; return true; };
            self.after = Some(id.clone());
            let Some(source) = canvas(draft, &id) else { continue; };
            self.current = Some((TerrainWaterSaveHandoffSnapshot {
                terrain_id: id.clone(), water_asset_path: source.path.to_owned(), baselines: Vec::new(),
                dirty_layers: Vec::new(), catalog_registration_required: source.registration,
                retry_required: handoff.terrain_water_retry_required_for(&draft.map_id, &id),
                runtime_required: self.runtime_enabled.contains(&id),
            }, 0));
        }
        false
    }
    pub(super) fn take(&mut self) -> Vec<TerrainWaterSaveHandoffSnapshot> {
        let targets = std::mem::take(&mut self.targets);
        *self = Self::default();
        targets
    }
    pub(super) fn take_root(&mut self) -> Vec<(String, String, String)> {
        std::mem::take(&mut self.root_dirty_layers)
    }
}
