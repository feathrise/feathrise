//! 地图根水与旧网格私有水分槽：切换只移动字段，保存始终读取真正的根文档。
use super::{types::RootWaterCanvas, TerrainCanvas};
use crate::authoring::EditorMapDraft;
use feathrise_map_content::MapWaterAsset;
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

/// 地图根水常驻，当前网格的私有水单独列出；两个来源即使层ID相同也不是同一对象。
pub(crate) fn listed_sources(draft: &EditorMapDraft) -> [Option<(Option<&feathrise_map_content::TerrainId>, &MapWaterAsset)>; 2] {
    let root = source(draft).water.map(|water| (None, water));
    let private = (!draft.terrain_documents.detached)
        .then_some(draft.terrain_documents.active.as_ref()).flatten()
        .and_then(|active| draft.water.as_ref().map(|water| (Some(&active.definition.id), water)));
    [root, private]
}

pub(crate) fn feature_count(water: &MapWaterAsset) -> usize {
    water.layers.len().saturating_add(water.rivers.len()).saturating_add(water.waterfalls.len())
}

/// 只读取固定两个文档的数组长度，稳定帧不遍历停放网格或水域层。
pub(crate) fn listed_count(draft: &EditorMapDraft) -> usize {
    listed_sources(draft).into_iter().flatten().map(|(_, water)| feature_count(water)).sum()
}

/// 按冻结的来源读取水文档；停放私有槽仅用于身份复验，不冒充当前可写画布。
pub(crate) fn listed_source<'a>(draft: &'a EditorMapDraft, terrain: Option<&feathrise_map_content::TerrainId>) -> Option<&'a MapWaterAsset> {
    let Some(terrain) = terrain else { return source(draft).water; };
    if draft.terrain_documents.active.as_ref().is_some_and(|active| &active.definition.id == terrain) {
        return draft.water.as_ref();
    }
    draft.terrain_documents.values.get(terrain)?.water.as_ref()
}

pub(crate) struct WaterCanvasRef<'a> {
    pub water: Option<&'a MapWaterAsset>,
    pub masks: &'a BTreeMap<String, Arc<Vec<u8>>>,
    pub asset_path: &'a str,
    pub dirty_layers: &'a BTreeSet<String>,
    pub registration_required: bool,
    pub registration_source_hash: Option<&'a String>,
}

/// 私有保存投影仍读取自身；地图容器即使正编辑网格，也只能发布停放的根水。
pub(crate) fn source(draft: &EditorMapDraft) -> WaterCanvasRef<'_> {
    if draft.manifest.terrains.is_some() && !draft.terrain_documents.detached && !draft.manifest.terrain.is_empty() {
        return WaterCanvasRef { water: None, masks: &draft.water_masks, asset_path: "",
            dirty_layers: &draft.water_dirty_layers, registration_required: false, registration_source_hash: None };
    } // 兼容容器的原主水仍归primary，不能再次作为根水保存或发布。
    if draft.manifest.terrains.is_some() && !draft.terrain_documents.detached
        && draft.terrain_documents.active.is_some()
    {
        if let Some(root) = &draft.terrain_documents.root_water {
            return WaterCanvasRef { water: root.water.as_ref(), masks: &root.water_masks,
                asset_path: &root.water_asset_path, dirty_layers: &root.water_dirty_layers,
                registration_required: root.water_catalog_registration_required,
                registration_source_hash: root.water_registration_source_hash.as_ref() };
        }
        // 从旧主网格迁移来的画布没有独立根水，不能把私有水再次写到地图目录。
        return WaterCanvasRef { water: None, masks: &draft.water_masks,
            asset_path: "", dirty_layers: &draft.water_dirty_layers,
            registration_required: false, registration_source_hash: None };
    }
    WaterCanvasRef { water: draft.water.as_ref(), masks: &draft.water_masks,
        asset_path: &draft.water_asset_path, dirty_layers: &draft.water_dirty_layers,
        registration_required: draft.water_catalog_registration_required,
        registration_source_hash: draft.water_registration_source_hash.as_ref() }
}

pub(super) fn park(draft: &mut EditorMapDraft, previous: &mut TerrainCanvas) {
    let mut root = RootWaterCanvas { revision: draft.terrain_documents.root_water_revision,
        structural_dirty: previous.structural_dirty, ..Default::default() };
    macro_rules! take { ($($field:ident),* $(,)?) => { $(root.$field = std::mem::take(&mut previous.$field);)* }; }
    take!(water, water_masks, water_asset_path, water_dirty_layers,
        water_catalog_registration_required, water_registration_source_hash);
    draft.terrain_documents.root_water = Some(root);
}

/// 调用者先停放当前私有画布；根槽恢复后active=None，水域使用单位源姿态。
pub(crate) fn restore(draft: &mut EditorMapDraft) {
    let Some(mut root) = draft.terrain_documents.root_water.take() else { return; };
    draft.terrain_documents.root_water_revision = root.revision;
    root.swap(draft);
}

pub(crate) fn remember_revision(draft: &mut EditorMapDraft, current: u64) {
    if draft.terrain_documents.active.is_none() {
        draft.terrain_documents.root_water_revision = current;
    }
}

/// 私有网格笔划不使停放根水换代，允许同源根水后台结果在切换后继续正式发布。
pub(crate) fn revision(draft: &EditorMapDraft, current: u64) -> u64 {
    if draft.terrain_documents.active.is_some() && !draft.terrain_documents.detached {
        draft.terrain_documents.root_water.as_ref().map_or(current, |root| root.revision)
    } else { current }
}

impl RootWaterCanvas {
    /// 撤销和画布切换共享同一组水字段；不交换网格、地表或植被数据。
    pub(crate) fn swap(&mut self, draft: &mut EditorMapDraft) {
        macro_rules! swap { ($($field:ident),* $(,)?) => { $(std::mem::swap(&mut self.$field, &mut draft.$field);)* }; }
        swap!(water, water_masks, water_asset_path, water_dirty_layers,
            water_catalog_registration_required, water_registration_source_hash);
        std::mem::swap(&mut self.structural_dirty, &mut draft.terrain_documents.active_dirty);
    }
    pub(crate) fn dirty(&self) -> bool {
        self.structural_dirty || !self.water_dirty_layers.is_empty() || self.water_catalog_registration_required
            || self.water_registration_source_hash.is_some()
    }
}

pub(crate) fn clear_saved(draft: &mut EditorMapDraft) {
    if let Some(root) = &mut draft.terrain_documents.root_water {
        root.structural_dirty = false;
        root.water_dirty_layers.clear();
        root.water_catalog_registration_required = false;
        root.water_registration_source_hash = None;
    }
}

pub(crate) fn restore_dirty(draft: &mut EditorMapDraft, ids: impl IntoIterator<Item = String>, registration_required: bool) {
    if draft.terrain_documents.active.is_some() && !draft.terrain_documents.detached {
        if let Some(root) = &mut draft.terrain_documents.root_water {
            root.structural_dirty = true;
            root.water_dirty_layers.extend(ids);
            root.water_catalog_registration_required |= registration_required;
        }
    } else {
        draft.terrain_documents.active_dirty = true; // JSON交接失败即使没有R8修改，也不能丢失重试责任。
        draft.water_dirty_layers.extend(ids);
        draft.water_catalog_registration_required |= registration_required;
    }
}

/// 用户明确新建独立根水时使用；旧网格画布完整停放，不能丢失未保存的私有水。
pub(crate) fn focus(draft: &mut EditorMapDraft) -> bool {
    let changed = draft.terrain_documents.active.is_some();
    if changed { super::transitions::park(draft); }
    restore(draft);
    changed
}

/// 同次视口候选还可能属于没有水文档的地表画布；恢复完整缓存槽不能依赖water存在。
pub(crate) fn focus_cached(draft: &mut EditorMapDraft, terrain: Option<&feathrise_map_content::TerrainId>) -> Result<bool, String> {
    validate_cached_focus(draft, terrain)?;
    let Some(terrain) = terrain else { return Ok(focus(draft)); };
    if draft.terrain_documents.active.as_ref().is_some_and(|active| &active.definition.id == terrain) {
        return Ok(false);
    }
    let stored = draft.terrain_documents.values.remove(terrain).unwrap();
    super::transitions::park(draft);
    let mut canvas = Arc::try_unwrap(stored).unwrap_or_else(|shared| (*shared).clone());
    if draft.terrain_documents.saved.remove(terrain) {
        canvas.clear_dirty();
        if let Some(digest) = draft.terrain_documents.saved_digests.get(terrain) {
            canvas.terrain_source_sha256 = Some(*digest);
        }
        canvas.terrain_overwrite_authorized_sha256 = None;
    } // 仅采用本缓存保存后的正式基线，未保存的新来源继续保留其实际读盘摘要。
    draft.terrain_documents.dirty_stored.remove(terrain);
    canvas.swap(draft);
    Ok(true)
}

pub(crate) fn validate_listed_focus(draft: &EditorMapDraft, terrain: Option<&feathrise_map_content::TerrainId>) -> Result<(), String> {
    if listed_source(draft, terrain).is_none() { return Err("水域来源已变化，请重新选择当前列表项".into()); }
    validate_cached_focus(draft, terrain)
}

/// 先核对完整当前定义与缓存资格，调用方通过事务门后才能停放任何来源。
pub(crate) fn validate_cached_focus(draft: &EditorMapDraft, terrain: Option<&feathrise_map_content::TerrainId>) -> Result<(), String> {
    if draft.terrain_documents.loading {
        return Err("作者画布正在载入，请完成后重新选择".into());
    }
    let Some(terrain) = terrain else { return Ok(()); };
    if let Some(active) = draft.terrain_documents.active.as_ref().filter(|active| &active.definition.id == terrain) {
        if draft.manifest.terrains.as_ref().is_some_and(|objects|
            !objects.iter().any(|object| &object.id == terrain && active.definition.as_ref() == object)) {
            return Err("水域所属网格画布已变化，请先重新选择该网格".into());
        }
        return Ok(()); // 相同ID仍须核对完整当前定义，旧槽不能提前解除同ID重载的输入门。
    }
    let object = draft.manifest.terrains.as_ref().and_then(|objects| objects.iter().find(|object| &object.id == terrain))
        .ok_or("水域所属网格已移除，请重新选择当前列表项")?;
    draft.terrain_documents.values.get(terrain)
        .filter(|canvas| !canvas.vegetation_only && canvas.identity.as_ref().is_some_and(|identity| identity.definition.as_ref() == object))
        .ok_or("水域所属网格画布已变化，请先重新选择该网格")?;
    Ok(()) // 完整来源校验通过后才允许调用方scope和取槽，失败不得改变历史归属。
}
