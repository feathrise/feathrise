//! 帧内只冻结最多4个生产快照Arc；像素验证、定义复制与编码均在唯一后台任务完成。

use super::resources::*;
use crate::authoring::{AuthoringRetirement, EditorAuthoringSession};
use bevy::{prelude::*, tasks::{AsyncComputeTaskPool, futures::check_ready}};
use feathrise_map_content::{MapId, TerrainId, embedded_preview::*};
use feathrise_scene::{DynamicBladeDraft, DynamicBladeDraftSnapshot, SubMapActive, SubMapInstance, TerrainObjectAuthoringTarget};
use feathrise_scene_core::residency::InstanceResourceTable;
use std::{ops::Bound::{Excluded, Unbounded}, sync::Arc};

type Frozen = (Option<TerrainId>, Version, Arc<DynamicBladeDraftSnapshot>);

fn source(world: &World, state: &EmbeddedVegetationState) -> Option<Source> {
    if !state.enabled || state.session == 0 || state.generation == 0 { return None; }
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    if draft.terrain_documents.loading || !draft.manifest.is_data_driven()
        || ((draft.manifest.terrains.is_none() || draft.terrain_documents.active.is_some())
            && session.vegetation_compiled_structure_revision != session.vegetation_structure_revision)
        || world.get_resource::<crate::resources::MapEditorState>()
            .is_some_and(|editor| editor.map_transition_in_progress)
    { return None; }
    Some(Source { map: draft.map_id.clone(), session: state.session, epoch: state.source_epoch, generation: state.generation,
        structure_revision: session.vegetation_structure_revision,
        terrain_revision: session.terrain_revision })
}

fn retire<T: Send + Sync + 'static>(world: &mut World, value: T) {
    world.resource_mut::<AuthoringRetirement>().push(value);
}

pub(super) fn advance(world: &mut World) {
    let mut state = world.remove_resource::<EmbeddedVegetationState>().unwrap();
    let mut current = source(world, &state);
    if current != state.source {
        state.source_epoch = state.source_epoch.wrapping_add(1);
        if let Some(identity) = current.as_mut() { identity.epoch = state.source_epoch; }
        state.source = current.clone();
        state.source_floor = world.get_resource::<EditorAuthoringSession>()
            .map_or(0, |session| session.vegetation_compiled_revision);
        state.error = None;
        state.cursor = None;
        state.root_next = true;
        state.offered.clear();
        state.failed.clear();
        retire(world, std::mem::take(&mut state.acknowledged));
        if let Some(old) = state.ready.take() { retire(world, old); }
        // 不丢弃仍运行的任务；旧完成值只丧失采用资格，不借新源再开第二个槽。
    }
    if let Some((identity, versions, result)) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        if current.as_ref() == Some(&identity) {
            match result {
                Ok(ready) => { state.offered = versions; state.ready = Some(ready); state.error = None; }
                Err(error) => { state.failed = versions; state.error = Some(error); }
            }
        } else if let Ok(stale) = result { retire(world, stale); }
    }
    if state.ready.as_ref().is_some_and(|ready| ready.sequence == state.acknowledged_sequence) {
        // 唯一待确认批次最多4项；ACK提交的就是该批版本，更新期间其它terrain仍由生产源持有。
        for (id, version) in std::mem::take(&mut state.offered) { state.acknowledged.insert(id, version); }
        if let Some(old) = state.ready.take() { retire(world, old); }
    }
    if let Some(identity) = current
        && state.task.is_none() && state.ready.is_none()
    {
        let frozen = capture(world, &mut state, &identity.map);
        let versions: Versions = frozen.iter().map(|(id, version, _)| (id.clone(), version.clone())).collect();
        if !frozen.is_empty() && versions != state.failed {
            if let Some(sequence) = state.next_sequence.checked_add(1) {
                state.next_sequence = sequence;
                state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
                    let result = encode(&identity, sequence, frozen);
                    (identity, versions, result)
                }));
            } else { state.error = Some("植被预览序号已耗尽，请重新连接工程".into()); }
        }
    }
    state.pending = state.task.is_some() || state.ready.is_some();
    world.insert_resource(state);
}

fn freeze(output: &DynamicBladeDraft, map: &MapId, id: Option<TerrainId>, state: &EmbeddedVegetationState) -> Option<Frozen> {
    let snapshot = output.frozen_for_map(map)?;
    let version = Version { published: output.revision_for_map(map)?,
        layout: output.layout_revision_for_map(map)?, source: snapshot.source_revision };
    (state.acknowledged.get(&id) != Some(&version)).then_some((id, version, snapshot))
}

/// 轮转检查资源表的固定前缀。一次编码/ACK之后继续上次游标，根画布不能饿死兄弟画布。
fn capture(world: &World, state: &mut EmbeddedVegetationState, map: &MapId) -> Vec<Frozen> {
    let mut result = Vec::new();
    let legacy = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap().manifest.terrains.is_none();
    let target = world.get_resource::<TerrainObjectAuthoringTarget>();
    let (session, generation) = (state.session, state.generation);
    let known = |id: &TerrainId| world.get_resource::<super::super::embedded_author_preview::EmbeddedAuthorPreviewState>()
        .is_some_and(|credit| credit.applied_terrain(map, id, session, generation));
    let owner = target.and_then(TerrainObjectAuthoringTarget::instance);
    // 根作者资源的归属沿用正式TerrainVegetationDrafts：已完成逻辑画布交接即可发布。
    // instance可能暂为PLACEHOLDER；不能再以本地物理实例是否存在否定原编译器的完整来源。
    let root_id = if legacy { Some(None) } else {
        world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_documents.active.as_ref()
            .map(|active| &active.definition.id)
            .filter(|id|target.is_some_and(|target|target.map.as_ref()==Some(map)
                && target.terrain.as_ref()==Some(*id)&&target.applied_to(map,id)))
            .map(|id|Some(id.clone()))
    };
    let root_id = root_id.filter(|id| id.as_ref().is_none_or(&known));
    if state.root_next {
        state.root_next = false;
        if let Some(id) = root_id.clone()
            && let Some(value) = world.get_resource::<DynamicBladeDraft>().and_then(|output| freeze(output, map, id, state))
            && value.1.source >= state.source_floor
        { result.push(value); } // 结构/画布换代后，旧显示快照不能借用新源身份；连续同源刷印不提高此门槛。
    }
    if let Some(table) = world.get_resource::<InstanceResourceTable<DynamicBladeDraft>>() {
        for _ in 0..MAX_VEGETATION_PREVIEW_CANVASES.saturating_sub(result.len()) {
            let next = state.cursor.as_ref().map_or_else(|| table.values.iter().next(),
                |cursor| table.values.range((Excluded(cursor), Unbounded)).next());
            let Some((entity, output)) = next else { state.cursor = None; state.root_next = true; break; };
            state.cursor = Some(*entity);
            if Some(*entity) == owner { continue; }
            let Some(instance) = world.get::<SubMapInstance>(*entity) else { continue; };
            // 活动标记属于整批根，terrain实体自身从不携带SubMapActive。
            // 正式候选/退役批次仍必须排除，不能为了取到像素直接绕过原子采用门。
            if world.get::<SubMapActive>(instance.batch).is_none() { continue; }
            if instance.path.root_map_id != *map || !instance.path.instances.is_empty() { continue; }
            let Some(id) = instance.path.terrain_id.clone() else { continue; };
            if root_id.as_ref()==Some(&Some(id.clone())) { continue; }
            if legacy && id == TerrainId::legacy() { continue; } // 旧地图主画布已经由根通道捕获。
            if !known(&id) { continue; } // 新目标先由已回执完整快照建立，不能用本地根抢占IPC槽。
            if let Some(value) = freeze(output, map, Some(id), state) { result.push(value); }
        }
    } else { state.root_next = true; }
    result
}

fn encode(source: &Source, sequence: u64, frozen: Vec<Frozen>) -> Result<Arc<EmbeddedVegetationPreview>, String> {
    let mut canvases = Vec::new();
    let mut revision = 0;
    for (terrain_id, version, snapshot) in frozen {
        revision = revision.max(version.source);
        let mut definition = snapshot.definition.clone();
        definition.authoring = None; // 只传正式输出，作者竞争层/笔划历史仍由原作者World唯一拥有。
        canvases.push(VegetationPreviewCanvas { terrain_id, source_revision: version.source,
            layout_revision: version.layout, definition, densities: snapshot.sources.clone(),
            nonzero_layers: snapshot.nonzero_layers.clone() });
    }
    let bytes = encode_vegetation_preview(&VegetationPreviewBatch { map_id: source.map.clone(),
        structure_revision: source.structure_revision, canvases })?;
    Ok(Arc::new(EmbeddedVegetationPreview { map_id: source.map.0.clone(), session: source.session, generation: source.generation,
        sequence, source_revision: revision, structure_revision: source.structure_revision, bytes: Arc::new(bytes) }))
}
