//! 多画布水文件仍共用唯一保存交接门；一次只检查一个对象，冻结表不随检查器选择变化。
use std::{collections::BTreeSet, time::Duration};
use bevy::{ecs::system::SystemParam, prelude::*};
use feathrise_map_content::{ActiveDynamicMap, MapId, TerrainId};
use feathrise_scene::{DynamicWaterPhase, DynamicWaterStatus, TerrainObjectInstance, TerrainObjectResource,
    TerrainWaterDrafts, TerrainWaterRebuildRequest};
use crate::authoring::resources::{EditorAuthoringSession, TerrainWaterSaveHandoffSnapshot};
use super::{PendingWaterFileHandoff, WaterFileHandoff, WaterFileHandoffDecision,
    WaterFileHandoffGate, WaterFileHandoffPurpose, begin::{pending_asset, pending_numeric_image}};

pub(super) struct TerrainWaterBatch {
    pub(super) water_revision: u64, // 私有作者通道使用全局水修订，独立于停放根水的精确修订。
    pub(super) targets: Vec<TerrainWaterSaveHandoffSnapshot>, // 捕获时已按稳定ID递增，不在采用帧排序。
    changed: BTreeSet<TerrainId>,
    rollback_paths: Option<BTreeSet<String>>,
    manifest_path: String,
    pub(super) cursor: usize,
    pub(super) armed: bool,
    asset_cursor: usize,
    runtime_requested: bool,
}
impl TerrainWaterBatch {
    fn eligible(&self, target: &TerrainWaterSaveHandoffSnapshot) -> bool {
        if let Some(paths) = &self.rollback_paths {
            return !target.catalog_registration_required && (paths.contains(&self.manifest_path)
                || paths.contains(&target.water_asset_path)
                || target.dirty_layers.iter().any(|(_, raw, gpu)| paths.contains(raw) || paths.contains(gpu)));
        }
        self.changed.contains(&target.terrain_id) || target.retry_required
    }
    pub(super) fn owns(&self, id: &TerrainId) -> bool {
        self.targets.binary_search_by(|target| target.terrain_id.cmp(id)).ok()
            .is_some_and(|index| self.eligible(&self.targets[index]))
    }
    fn recovery_eligible(&self, target: &TerrainWaterSaveHandoffSnapshot) -> bool {
        // 回滚只等待实际涉及的文件；失败恢复还须继承完整冻结表中未受本次回滚影响的旧重试画布。
        self.eligible(target) || target.retry_required
    }
    pub(super) fn owns_recovery(&self, id: &TerrainId) -> bool {
        self.targets.binary_search_by(|target| target.terrain_id.cmp(id)).ok()
            .is_some_and(|index| self.recovery_eligible(&self.targets[index]))
    }
}

/// 不暴露场景内部实例表；正式Ready只读活动路径，隐藏候选只用于明确失败判定。
#[derive(SystemParam)]
pub(crate) struct WaterHandoffRuntime<'w, 's> {
    pub(super) legacy: Res<'w, DynamicWaterStatus>,
    objects: TerrainObjectResource<'w, DynamicWaterStatus>,
    definitions: Query<'w, 's, &'static TerrainObjectInstance>,
    drafts: TerrainWaterDrafts<'w>,
    requests: MessageWriter<'w, TerrainWaterRebuildRequest>,
    server: Res<'w, AssetServer>,
}
impl WaterHandoffRuntime<'_, '_> {
    fn source_matches(&self, entity: Entity, target: &TerrainWaterSaveHandoffSnapshot) -> bool {
        self.definitions.get(entity).is_ok_and(|object| object.definition.id == target.terrain_id
            && object.definition.water_path.as_deref() == Some(target.water_asset_path.as_str()))
    }
}

impl WaterFileHandoff {
    /// 只移动已冻结的有序表；实际句柄登记、reload与运行时等待随后逐画布推进。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_terrains(
        &mut self, server: &AssetServer, map_id: MapId, water_revision: u64, terrain_revision: u64,
        manifest_path: &str, targets: &mut Vec<TerrainWaterSaveHandoffSnapshot>,
        changed: &mut BTreeSet<TerrainId>, rollback_paths: Option<&[String]>, started_at: Duration,
    ) -> Result<(), String> {
        if self.pending.is_some() || self.placement.is_some() || self.preview_cleanup.is_some() {
            return Err("上一笔正式文件交接尚未完成，不能覆盖独立水画布责任".into());
        }
        if self.unresolved.as_ref().is_some_and(|old| old.map_id != map_id) || manifest_path.is_empty() {
            return Err("正式地图身份或清单路径不完整，不能交接独立水画布".into());
        }
        self.pending = Some(PendingWaterFileHandoff {
            gate: WaterFileHandoffGate { purpose: if rollback_paths.is_some() {
                WaterFileHandoffPurpose::RollbackRestore
            } else { WaterFileHandoffPurpose::SavePublish }, map_id, water_revision, terrain_revision,
                baseline_generation: 0, started_at, all_inputs_succeeded_frame: None },
            manifest: pending_asset(server, manifest_path), water: None, masks: Vec::new(), textures: Vec::new(),
            dirty_layer_ids: Vec::new(), catalog_registration_required: false,
            manifest_asset_path: manifest_path.to_owned(),
            root_pending: false,
            terrains: Some(TerrainWaterBatch { water_revision, targets: std::mem::take(targets), changed: std::mem::take(changed),
                rollback_paths: rollback_paths.map(|paths| paths.iter().cloned().collect()),
                manifest_path: manifest_path.to_owned(), cursor: 0, armed: false,
                asset_cursor: 0, runtime_requested: false }),
        });
        if rollback_paths.is_none() {
            self.adopt_water_save_handoff();
        } // 私有水回滚与根水相同：原placement retry/late及恢复批次持续归旧失败事务拥有。
        Ok(())
    }

    /// 根水与私有水共用保存事务；只附加冻结表，不覆盖已经取得的根输入强句柄。
    pub(crate) fn attach_terrain_batch(
        &mut self, map_id: &MapId, water_revision: u64, manifest_path: &str,
        targets: &mut Vec<TerrainWaterSaveHandoffSnapshot>, changed: &mut BTreeSet<TerrainId>,
        rollback_paths: Option<&[String]>,
    ) -> Result<(), String> {
        let pending = self.pending.as_mut().filter(|pending| &pending.gate.map_id == map_id
            && pending.root_pending && pending.terrains.is_none()
            && pending.manifest_asset_path == manifest_path)
            .ok_or("根水交接身份不完整，不能附加私有水画布")?;
        pending.terrains = Some(TerrainWaterBatch {
            water_revision,
            targets: std::mem::take(targets), changed: std::mem::take(changed),
            rollback_paths: rollback_paths.map(|paths| paths.iter().cloned().collect()),
            manifest_path: manifest_path.to_owned(), cursor: 0, armed: false,
            asset_cursor: 0, runtime_requested: false,
        });
        Ok(())
    }

    pub(crate) fn remember_terrain_water_recovery(&mut self, manifest_path: &str,
        targets: &mut Vec<TerrainWaterSaveHandoffSnapshot>, changed: &mut BTreeSet<TerrainId>) {
        let Some(unresolved) = self.unresolved.as_mut() else { return; };
        unresolved.terrain_recovery = Some(TerrainWaterBatch {
            water_revision: 0, // 恢复脏责任不发布或撤回作者通道。
            targets: std::mem::take(targets), changed: std::mem::take(changed), rollback_paths: None,
            manifest_path: manifest_path.to_owned(), cursor: 0, armed: false, asset_cursor: 0, runtime_requested: false,
        });
    }
}

/// 身份匹配之后仍要求正式文件输入；候选只能报告失败，不能替代已原子采用的活动实例。
fn object_decision(target: &TerrainWaterSaveHandoffSnapshot, map: &MapId,
    entity: Entity, status: &DynamicWaterStatus, active: bool) -> WaterFileHandoffDecision {
    let baseline = target.baselines.iter().find_map(|(old, generation)| (*old == entity).then_some(*generation)).unwrap_or(0);
    if status.map_id.as_ref() != Some(map) || status.input_draft_revision.is_some()
        || status.desired_generation <= baseline { return WaterFileHandoffDecision::Waiting; }
    if status.phase == DynamicWaterPhase::Failed { return WaterFileHandoffDecision::RuntimeFailed; }
    if active && status.phase == DynamicWaterPhase::Ready
        && status.applied_generation == status.desired_generation { return WaterFileHandoffDecision::Complete; }
    WaterFileHandoffDecision::Waiting
}

pub(super) fn advance(pending: &mut PendingWaterFileHandoff, runtime: &mut WaterHandoffRuntime,
    active_map: &ActiveDynamicMap, frame: u32, failure: &mut Option<String>) -> WaterFileHandoffDecision {
    let batch = pending.terrains.as_mut().expect("对象分支持有冻结表");
    // 无水变更的脏画布也可能在捕获表里；每帧最多跳过8项，绝不为它们发送重建。
    for _ in 0..8 {
        let Some(target) = batch.targets.get(batch.cursor) else { return WaterFileHandoffDecision::Complete; };
        if batch.eligible(target) { break; }
        batch.cursor += 1;
    }
    let Some(target) = batch.targets.get(batch.cursor) else { return WaterFileHandoffDecision::Complete; };
    if !batch.eligible(target) { return WaterFileHandoffDecision::Waiting; }
    let map = &pending.gate.map_id;
    runtime.drafts.clear_saved(map, std::slice::from_ref(&target.terrain_id));
    if !batch.armed {
        if pending.water.is_none() {
            pending.water = Some(pending_asset(&runtime.server, &target.water_asset_path));
            runtime.server.reload(target.water_asset_path.clone());
            runtime.server.reload(pending.manifest_asset_path.clone());
        }
        let end = (batch.asset_cursor + 8).min(target.dirty_layers.len());
        for (_, raw, gpu) in &target.dirty_layers[batch.asset_cursor..end] {
            let include = |path: &String| batch.rollback_paths.as_ref().is_none_or(|paths| paths.contains(path));
            if include(raw) {
                pending.masks.push(pending_asset(&runtime.server, raw));
                runtime.server.reload(raw.clone());
            }
            if include(gpu) {
                pending.textures.push(pending_numeric_image(&runtime.server, gpu));
                runtime.server.reload(gpu.clone());
            }
        }
        batch.asset_cursor = end;
        batch.armed = end == target.dirty_layers.len();
        return WaterFileHandoffDecision::Waiting;
    }
    let Some(success_frame) = pending.gate.all_inputs_succeeded_frame.filter(|success| *success != frame) else {
        return WaterFileHandoffDecision::Waiting;
    };
    let _ = success_frame; // 最后一份正式文件完成后至少跨帧，才允许建立新运行时边界。
    if !target.runtime_required || active_map.map_id.as_ref() != Some(map) {
        // 【保存设计】本批明确禁用/删除的对象没有运行实例；仍须完整文件成功并跨帧才归还责任。
        // 暂时缺少实例的启用对象继续等待，不把加载空隙当作正式采用。
        finish_current(pending);
        return WaterFileHandoffDecision::Waiting;
    }
    if !batch.runtime_requested {
        let target = &mut batch.targets[batch.cursor];
        target.baselines.clear();
        for evidence in [runtime.objects.for_saved_object(map, &target.terrain_id),
            runtime.objects.for_preparing_saved_object(map, &target.terrain_id)].into_iter().flatten() {
            target.baselines.push((evidence.0, evidence.1.desired_generation));
        }
        // 【保存设计】边界在完整正式文件事件之后武装；IO期事件、草稿Ready与旧候选均不能冒充本次采用。
        runtime.requests.write(TerrainWaterRebuildRequest { map_id: map.clone(), terrain_id: target.terrain_id.clone() });
        batch.runtime_requested = true;
        return WaterFileHandoffDecision::Waiting;
    }
    let target = &batch.targets[batch.cursor];
    if let Some((entity, status)) = runtime.objects.for_preparing_saved_object(map, &target.terrain_id)
        && runtime.source_matches(entity, target)
        && object_decision(target, map, entity, status, false) == WaterFileHandoffDecision::RuntimeFailed {
        *failure = Some(format!("地形{}：{}", target.terrain_id.as_str(), status.detail));
        return WaterFileHandoffDecision::RuntimeFailed;
    }
    if let Some((entity, status)) = runtime.objects.for_saved_object(map, &target.terrain_id)
        && runtime.source_matches(entity, target) {
        let decision = object_decision(target, map, entity, status, true);
        match decision {
            WaterFileHandoffDecision::Complete => finish_current(pending),
            WaterFileHandoffDecision::RuntimeFailed => {
                *failure = Some(format!("地形{}：{}", target.terrain_id.as_str(), status.detail));
                return decision;
            }
            WaterFileHandoffDecision::Waiting => {}
        }
    }
    WaterFileHandoffDecision::Waiting
}
fn finish_current(pending: &mut PendingWaterFileHandoff) {
    let batch = pending.terrains.as_mut().unwrap();
    batch.cursor += 1; batch.armed = false; batch.asset_cursor = 0; batch.runtime_requested = false;
    pending.water = None; pending.masks.clear(); pending.textures.clear();
    pending.gate.all_inputs_succeeded_frame = None;
}

/// 失败只恢复被冻结目标的脏责任；检查器已经切到兄弟画布时恢复原停放槽。
fn restore_target(target: &TerrainWaterSaveHandoffSnapshot, session: &mut EditorAuthoringSession) {
    let Some(draft) = session.draft.as_mut() else { return; };
    let id = &target.terrain_id;
    draft.terrain_documents.saved.remove(id);
    if draft.terrain_documents.active.as_ref().is_some_and(|active| &active.definition.id == id) {
        draft.terrain_documents.active_dirty = true;
        draft.water_dirty_layers.extend(target.dirty_layers.iter().map(|(id, _, _)| id.clone()));
        draft.water_catalog_registration_required |= target.catalog_registration_required;
    } else if let Some(canvas) = draft.terrain_documents.values.get_mut(id) {
        let canvas = std::sync::Arc::make_mut(canvas); // 只复制小型描述与Arc表头，不复制水遮罩像素。
        canvas.structural_dirty = true;
        canvas.water_dirty_layers.extend(target.dirty_layers.iter().map(|(id, _, _)| id.clone()));
        canvas.water_catalog_registration_required |= target.catalog_registration_required;
        draft.terrain_documents.dirty_stored.insert(id.clone());
    }
}
pub(super) fn restore_failed_inputs(handoff: &mut WaterFileHandoff, session: &mut EditorAuthoringSession) {
    let Some(unresolved) = &mut handoff.unresolved else { return; };
    let Some(batch) = &mut unresolved.terrain_recovery else { return; };
    if session.draft.as_ref().is_none_or(|draft| draft.map_id != unresolved.map_id) { return; }
    for _ in 0..8 {
        let Some(target) = batch.targets.get(batch.cursor) else { break; };
        if batch.recovery_eligible(target) { restore_target(target, session); }
        batch.cursor += 1;
    }
}

#[cfg(test)]
mod tests;
