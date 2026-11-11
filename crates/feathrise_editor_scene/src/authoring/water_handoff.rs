//! 保存后的正式动态水域文件交接门。
//!
//! 【功能设计】保存事务只在当前玩法场景上建立交接；脏R8作者源各自持有强句柄并等待
//! 新成功事件，JSON-only保存也必须跨过建立帧。最终只接受运行时公开的更新代次已经完整
//! 原子采用；失败或真实时间超时会恢复dirty，并保留可重试但不可绕过的同图关闭锁。

#[cfg(test)]
use std::time::Duration; // 测试构造长耗时等待；正式交接不按总时长拒绝。

use bevy::asset::{AssetEvent, AssetId, AssetLoadFailedEvent}; // 独立读取R8成功与失败事件，不消费动态场景读者。
use bevy::diagnostic::FrameCount; // 最后一张R8成功后必须至少跨过一个真实帧。
use bevy::prelude::*; // Resource、Handle、MessageReader和系统参数。
use feathrise_scene_content::{
    ActiveDynamicScene, SceneId, SceneManifestAsset, SceneWaterAsset, SceneWaterMaskAsset,
}; // 稳定场景身份、固定层预算与本次JSON/R8正式资产。
use feathrise_scene::ScenePlacementRuntimeStatus; // 水域证据由对象感知SystemParam提供，摆放仍读取公开采用状态。

use super::resources::{EditorAuthoringSession, PlacementPreviewHandoffSnapshot}; // 失败时保留保存批次的精确版本回收证据。
use super::placement_deletion::PlacementDeletionSnapshot; // 删除确认独立于预览实体，纯删除保存也必须等待严格新Ready。
use crate::resources::SceneEditorState; // 失败恢复dirty并取消等待关闭。


mod begin; // 保存发布与失败回滚在任何reload前原子取得全部强资产身份。
mod failure; // 任一正式域失败时共同恢复dirty、写入责任与摆放Scene0预览。
mod placement_previews; // 严格新Ready后按保存批次实体快照精确回收Scene0。
mod state; // purpose差异、代次证据与超时判定保持为可单测纯状态机。
mod terrain; // 独立水画布复用同一关闭责任，按固定预算逐个交接真实正式输入。
use terrain::{TerrainWaterBatch, WaterHandoffRuntime};

use failure::fail_scene_file_handoff;
#[cfg(test)]
use failure::restore_pending_authoring_inputs;
pub(crate) use placement_previews::{
    cleanup_placement_handoff_previews, schedule_placement_preview_cleanup,
};
use placement_previews::{placement_strictly_ready, schedule_late_ready_placement_preview_cleanup, schedule_placement_save_cleanup};

use state::{
    PlacementFileHandoffDecision, PlacementFileHandoffEvidence, PlacementFileHandoffGate,
    WaterFileHandoffDecision, WaterFileHandoffEvidence, WaterFileHandoffGate,
    WaterFileHandoffPurpose, placement_file_handoff_decision, water_file_handoff_decision,
};

/// 一项保存后必须观察到新成功事件的正式资产。
struct PendingWaterAsset<A: Asset> {
    _handle: Handle<A>, // 强句柄保证显式reload和事件等待期间资产身份持续存活。
    id: AssetId<A>,     // 固定身份用于零路径比较成功/失败消息。
    succeeded: bool,    // 只接受本次交接建立并清空旧事件之后的新成功消息。
}

/// 一次保存后的完整正式水域交接事务。
struct PendingWaterFileHandoff {
    gate: WaterFileHandoffGate, // 纯完成门与资源拥有权分开，便于零渲染状态测试。
    manifest: PendingWaterAsset<SceneManifestAsset>, // 水网范围、高度参数和输入路径必须同批交接。
    water: Option<PendingWaterAsset<SceneWaterAsset>>, // 兼容根场景或当前串行独立画布的正式JSON。
    masks: Vec<PendingWaterAsset<SceneWaterMaskAsset>>, // 本次真正改写的最多八张raw R8。
    textures: Vec<PendingWaterAsset<Image>>, // 同批KTX2必须完成数值纹理重载，不能依赖文件监听或旧缓存。
    dirty_layer_ids: Vec<String>,            // 失败重试必须重新写入并reload原脏R8，而非只重发JSON。
    catalog_registration_required: bool, // 首次water_path登记未被正式代次证明时，失败重试必须再次发布目录。
    manifest_asset_path: String,
    root_pending: bool, // 根水先交接，再逐一处理同批私有画布；不得用地形ID伪装独立根水。
    terrains: Option<TerrainWaterBatch>, // 元数据整批冻结；一次仅武装一个对象，不逐帧扫描全部画布。
}

/// 一次保存发布后的正式摆放原子采用责任。
struct PendingPlacementFileHandoff {
    gate: PlacementFileHandoffGate, // 只等待公开修订，不另持有或轮询场景实体。
    editor_previews: Vec<PlacementPreviewHandoffSnapshot>, // 保存批次精确拥有的草稿版本只在严格新Ready后回收。
    deleted: Vec<PlacementDeletionSnapshot>, // 失败与迟到Ready仍使用IO启动时的原票据，不能重读当前删除表。
}

/// 严格Ready后仍在分帧回收的精确预览批次；场景身份把作者冻结延续到最后一根销毁。
struct PlacementPreviewCleanup {
    scene_id: SceneId,                   // 另一场景的草稿不因本批跨帧清理而错误冻结。
    previews: Vec<PlacementPreviewHandoffSnapshot>, // 保存时冻结地址和作者版本；游标推进期间不移动或重组整批。
    deleted: Vec<PlacementDeletionSnapshot>, // 删除确认与实体回收共用唯一清理槽和每帧64额度。
    deletion_cursor: usize, // 纯删除批次没有预览树，也必须逐项精确确认。
    quiesce_cursor: usize, // 先分帧隐藏并移除每个直接根的WorldAssetRoot，停止热更继续长树。
    tree_cursor: usize,    // 所有直接根静默后，逐棵执行非递归深度优先回收。
    stack: Vec<Entity>,    // 只随当前树深增长，不一次物化宽树全部实体。
}

/// 交接失败后释放强句柄，但同图在下一次完整保存修复前仍不可关闭编辑器。
struct UnresolvedWaterFileHandoff {
    scene_id: SceneId,
    reason: String,
    placement_retry_required: bool, // 原样重试通过显式重建请求恢复，不改写相同正式资产。
    late_placement: Option<PendingPlacementFileHandoff>, // Failed/超时后候选若迟到Ready，仍按原快照精确回收预览。
    terrain_recovery: Option<TerrainWaterBatch>, // 失败后仍拥有全部对象责任，按每帧8画布恢复到各自作者槽。
}

/// 场景编辑器保存后最多一个正式水域文件交接。
#[derive(Resource, Default)]
pub(crate) struct WaterFileHandoff {
    pending: Option<PendingWaterFileHandoff>, // 新保存原子替换旧等待，稳定帧为空时零工作。
    placement: Option<PendingPlacementFileHandoff>, // 与水域共用一个轮询系统和关闭锁，不新增稳定帧路径。
    unresolved: Option<UnresolvedWaterFileHandoff>, // 可重试保存、不可再次绕过关闭的明确故障态。
    preview_cleanup: Option<PlacementPreviewCleanup>, // 正式Ready后仍按场景冻结作者值，直到独立小系统清完精确实体。
}

impl WaterFileHandoff {
    /// 同图正式水域文件正在保存发布或回滚恢复时，作者Clipmap草稿必须暂时让出输入优先级。
    #[cfg(test)]
    pub(crate) fn formal_water_revision_in_flight_for(&self, scene_id: &SceneId) -> Option<u64> {
        self.pending
            .as_ref()
            .filter(|pending| &pending.gate.scene_id == scene_id)
            .map(|pending| pending.gate.water_revision)
    }

    /// 停放脏画布保存时，不得清掉同图未改水画布的作者输入；有序冻结表支持固定路径二分查找。
    pub(crate) fn formal_water_revision_for_canvas(&self, map: &SceneId, id: Option<&feathrise_scene_content::TerrainId>) -> Option<u64> {
        let pending = self.pending.as_ref().filter(|pending| &pending.gate.scene_id == map)?;
        match (&pending.terrains, id) {
            (Some(batch), Some(id)) => batch.owns(id).then_some(batch.water_revision),
            (_, None) => pending.root_pending.then_some(pending.gate.water_revision),
            _ => None,
        }
    }

    pub(crate) fn terrain_water_retry_required_for(&self, map: &SceneId, id: &feathrise_scene_content::TerrainId) -> bool {
        self.unresolved.as_ref().filter(|unresolved| &unresolved.scene_id == map)
            .and_then(|unresolved| unresolved.terrain_recovery.as_ref()).is_some_and(|batch| batch.owns_recovery(id))
    }

    pub(crate) fn terrain_water_retry_pending_for(&self, map: &SceneId) -> bool {
        self.unresolved.as_ref().filter(|unresolved| &unresolved.scene_id == map)
            .and_then(|unresolved| unresolved.terrain_recovery.as_ref()).is_some_and(|batch| !batch.targets.is_empty())
    }

    /// 当前场景的正式placement候选是否仍在原子交接。
    ///
    /// 【内部协调】F12只用该门短暂冻结网格水作者值，避免保存后的
    /// 未保存覆盖污染隐藏候选；未解决失败不算在途，作者必须能继续修复。
    pub(crate) fn placement_handoff_in_flight_for(&self, scene_id: &SceneId) -> bool {
        self.placement
            .as_ref()
            .is_some_and(|pending| &pending.gate.scene_id == scene_id)
            || self
                .preview_cleanup
                .as_ref()
                .is_some_and(|cleanup| &cleanup.scene_id == scene_id)
        // 正式Ready只证明候选可替代预览；最后一个旧预览真正销毁前仍禁止作者写到它。
    }

    /// 精确实体批次尚未按固定帧预算释放时，保存完成采用必须继续排队。
    pub(crate) fn placement_preview_cleanup_pending(&self) -> bool {
        self.preview_cleanup.is_some()
    }

    /// 主动交接必须串行；未解决故障只允许同一场景用下一次完整保存接管。
    pub(crate) fn blocks_new_save_for(&self, scene_id: Option<&SceneId>) -> bool {
        self.preview_cleanup.is_some()
            || self.pending.is_some()
            || self.placement.is_some()
            || self
                .unresolved
                .as_ref()
                .is_some_and(|unresolved| scene_id != Some(&unresolved.scene_id)
                    || unresolved.terrain_recovery.as_ref().is_some_and(|batch| batch.cursor < batch.targets.len()))
    }

    /// 保存发布、回滚恢复和未解决故障都跟随正式场景缓存，不可被作者修订变化绕过。
    pub(crate) fn pending_for(
        &self,
        scene_id: &SceneId,
        _water_revision: u64,
        _terrain_revision: u64,
    ) -> bool {
        self.preview_cleanup.is_some()
            || self
                .unresolved
                .as_ref()
                .is_some_and(|unresolved| &unresolved.scene_id == scene_id)
            || self
                .pending
                .as_ref()
                .is_some_and(|pending| &pending.gate.scene_id == scene_id)
            || self
                .placement
                .as_ref()
                .is_some_and(|pending| &pending.gate.scene_id == scene_id)
    }

    /// 无法证明磁盘或AssetServer完整时进入可重试、不可关闭的明确状态。
    pub(crate) fn mark_unresolved(&mut self, scene_id: SceneId, reason: String) {
        self.mark_scene_unresolved(scene_id, reason, false); // 回滚不完整/建门失败也不能遗失先前摆放的严格门和原批次。
    }

    /// 建门失败同样保留摆放原样重试责任，避免下一次因磁盘字节相同而静默跳过恢复。
    pub(crate) fn mark_placement_unresolved(&mut self, scene_id: SceneId, reason: String) {
        self.mark_scene_unresolved(scene_id, reason, true);
    }

    /// 正式摆放失败后，同图下一次保存必须重发正式重建请求，即使JSON内容已经落盘未变。
    pub(crate) fn placement_retry_required_for(&self, scene_id: &SceneId) -> bool {
        self.unresolved.as_ref().is_some_and(|unresolved| {
            &unresolved.scene_id == scene_id && unresolved.placement_retry_required
        })
    }

    /// 统一记录水域或摆放交接失败并释放全部强句柄/等待门。
    fn mark_scene_unresolved(
        &mut self,
        scene_id: SceneId,
        reason: String,
        placement_retry_required: bool,
    ) {
        if self.unresolved.as_ref().is_some_and(|old| old.scene_id != scene_id)
            || self.pending.as_ref().is_some_and(|pending| pending.gate.scene_id != scene_id)
            || self.placement.as_ref().is_some_and(|pending| pending.gate.scene_id != scene_id)
        {
            return; // 单槽责任只能由同图更新；错误路径也不能丢弃另一场景或把它的批次继承到本图。
        }
        let terrain_recovery = self.pending.take().and_then(|mut pending| pending.terrains.take())
            .map(|mut batch| { batch.cursor = 0; batch });
        let placement = self.placement.take(); // 同批水域门不能在另一域失败后继续单独放行。
        let retry_required = placement_retry_required || placement.is_some();
        if let Some(unresolved) = self.unresolved.as_mut() {
            unresolved.reason = reason;
            unresolved.placement_retry_required |= retry_required;
            if placement.is_some() { unresolved.late_placement = placement; }
            if terrain_recovery.is_some() { unresolved.terrain_recovery = terrain_recovery; }
            // 同图后续错误只接管确实存在的新批；空水/摆放结果不能覆盖旧严格门、预览或恢复游标。
        } else {
            self.unresolved = Some(UnresolvedWaterFileHandoff {
                scene_id,
                reason,
                placement_retry_required: retry_required,
                late_placement: placement,
                terrain_recovery,
            });
        }
    }

    /// 完整保存的新水门只接管旧水责任；旧摆放仍需自己的合法新门证明接管。
    fn adopt_water_save_handoff(&mut self) {
        if let Some(unresolved) = self.unresolved.as_mut() {
            unresolved.terrain_recovery = None; // 完整保存已冻结所有待重试画布，旧批不再重复恢复脏责任。
            if unresolved.placement_retry_required || unresolved.late_placement.is_some() {
                return; // 必须同时保留旧严格gate与精确预览快照，不能只留下一个retry布尔值。
            }
        }
        self.unresolved = None;
    }

    pub(crate) fn unresolved_reason_for(&self, scene_id: &SceneId) -> Option<&str> {
        self.unresolved
            .as_ref()
            .filter(|unresolved| &unresolved.scene_id == scene_id)
            .map(|unresolved| unresolved.reason.as_str())
    }
}

/// 取得表示一项正式输入已经产生新可用内容的标准资产事件身份。
fn successful_asset_event_id<A: Asset>(event: &AssetEvent<A>) -> Option<AssetId<A>> {
    match event {
        AssetEvent::Added { id }
        | AssetEvent::Modified { id }
        | AssetEvent::LoadedWithDependencies { id } => Some(*id), // 与正式动态水输入观察器使用相同成功变体。
        AssetEvent::Removed { .. } | AssetEvent::Unused { .. } => None, // 移除和失去最后弱引用不能证明新文件可用。
    }
}

/// 消费本系统自己的JSON/R8事件读者，并轮询正式动态水公开代次完成门。
#[allow(clippy::too_many_arguments)]
pub(crate) fn poll_water_file_handoff(
    mut handoff: ResMut<WaterFileHandoff>, // 保存成功后唯一强句柄与跨帧事务。
    mut session: ResMut<EditorAuthoringSession>, // 场景/修订换代作废旧门，失败时恢复本次脏输入。
    mut editor: ResMut<SceneEditorState>,    // 失败恢复dirty/关闭，成功发布明确状态。
    active_scene: Res<ActiveDynamicScene>,     // 完成时必须仍是当前玩法场景。
    mut runtime: WaterHandoffRuntime, // 根兼容证据、对象活动/候选证据及定点请求收拢，保持系统参数预算。
    placements_status: Res<ScenePlacementRuntimeStatus>, // 新Ready/Failed公开修订是摆放发布的唯一完成证据。
    frame_count: Res<FrameCount>,                      // 资产成功帧后至少跨一帧。
    real_time: Res<Time<Real>>,                        // 十秒截止不受F12虚拟暂停影响。
    mut manifest_events: feathrise_scene_core::residency::ResidentAssetEventReader<SceneManifestAsset>, // 世界范围与高度参数必须同批完成。
    mut manifest_failures: MessageReader<AssetLoadFailedEvent<SceneManifestAsset>>, // 清单失败不得用旧范围构建新水网。
    mut water_events: feathrise_scene_core::residency::ResidentAssetEventReader<SceneWaterAsset>, // 最终JSON直接替换或reload都必须产生新成功消息。
    mut water_failures: MessageReader<AssetLoadFailedEvent<SceneWaterAsset>>, // JSON失败恢复完整保存重试入口。
    mut mask_events: feathrise_scene_core::residency::ResidentAssetEventReader<SceneWaterMaskAsset>, // 独立读者完整消费R8成功消息。
    mut mask_failures: MessageReader<AssetLoadFailedEvent<SceneWaterMaskAsset>>, // 当前待成功R8失败立即恢复作者。
    mut texture_events: Option<feathrise_scene_core::residency::ResidentAssetEventReader<Image>>, // 无渲染测试可不注册Image消息；正式应用必须等待KTX2新内容。
    mut texture_failures: Option<MessageReader<AssetLoadFailedEvent<Image>>>, // 数值纹理失败不能拿旧GPU缓存冒充成功。
) {
    terrain::restore_failed_inputs(&mut handoff, &mut session); // 一次最多8画布；收口前后续保存继续排队。
    for event in manifest_events.read() {
        let Some(id) = successful_asset_event_id(event) else {
            continue;
        };
        if let Some(pending) = handoff.pending.as_mut()
            && pending.manifest.id == id
        {
            pending.manifest.succeeded = true;
        }
    }
    for event in water_events.read() {
        let Some(id) = successful_asset_event_id(event) else {
            continue;
        };
        if let Some(pending) = handoff.pending.as_mut()
            && let Some(water) = pending.water.as_mut().filter(|water| water.id == id)
        {
            water.succeeded = true;
        }
    }
    for event in mask_events.read() {
        let Some(id) = successful_asset_event_id(event) else {
            continue; // 非成功变体仍被完整消费，不进入后续会话。
        };
        if let Some(pending) = handoff.pending.as_mut() {
            for mask in &mut pending.masks {
                if mask.id == id {
                    mask.succeeded = true; // 同一ID重复成功事件保持幂等，不重置跨帧起点。
                }
            }
        }
    }
    if let Some(texture_events) = texture_events.as_mut() {
        for event in texture_events.read() {
            let Some(id) = successful_asset_event_id(event) else {
                continue;
            };
            if let Some(pending) = handoff.pending.as_mut() {
                for texture in &mut pending.textures {
                    if texture.id == id {
                        texture.succeeded = true;
                    }
                }
            }
        }
    }
    if let Some(pending) = handoff.pending.as_mut()
        && pending.gate.all_inputs_succeeded_frame.is_none()
        && pending.manifest.succeeded
        && pending.water.as_ref().is_some_and(|water| water.succeeded)
        && (pending.root_pending || pending.terrains.as_ref().is_none_or(|batch| batch.armed))
        && pending.masks.iter().all(|mask| mask.succeeded)
        && pending.textures.iter().all(|texture| texture.succeeded)
    {
        pending.gate.all_inputs_succeeded_frame = Some(frame_count.0); // 冻结最终JSON或R8最后成功所在真实帧。
    }
    let mut input_failure = None; // 同帧只保留第一条当前事务失败诊断。
    for failure in manifest_failures.read() {
        if handoff
            .pending
            .as_ref()
            .is_some_and(|pending| pending.manifest.id == failure.id && !pending.manifest.succeeded)
        {
            input_failure.get_or_insert_with(|| {
                format!("场景清单{}加载失败：{}", failure.path, failure.error)
            });
        }
    }
    for failure in water_failures.read() {
        if handoff
            .pending
            .as_ref()
            .is_some_and(|pending| pending.water.as_ref().is_some_and(|water| water.id == failure.id && !water.succeeded))
        {
            input_failure.get_or_insert_with(|| {
                format!("水域JSON{}加载失败：{}", failure.path, failure.error)
            }); // 同ID本帧已有成功时拒绝迟到失败。
        }
    }
    for failure in mask_failures.read() {
        if handoff.pending.as_ref().is_some_and(|pending| {
            pending
                .masks
                .iter()
                .any(|mask| mask.id == failure.id && !mask.succeeded)
        }) {
            input_failure.get_or_insert_with(|| {
                format!("R8作者源{}加载失败：{}", failure.path, failure.error)
            }); // 同ID本帧已有成功时拒绝迟到失败，保持与动态水仲裁一致。
        }
    }
    if let Some(texture_failures) = texture_failures.as_mut() {
        for failure in texture_failures.read() {
            if handoff.pending.as_ref().is_some_and(|pending| {
                pending
                    .textures
                    .iter()
                    .any(|texture| texture.id == failure.id && !texture.succeeded)
            }) {
                input_failure.get_or_insert_with(|| {
                    format!("KTX2遮罩{}加载失败：{}", failure.path, failure.error)
                });
            }
        }
    }
    if let Some(reason) = input_failure {
        fail_scene_file_handoff(&mut handoff, &mut session, &mut editor, &reason); // 当前正式输入失败优先发布具体路径诊断。
        return;
    }
    schedule_late_ready_placement_preview_cleanup(&mut handoff, &placements_status, active_scene.scene_id.as_ref()); // 失败后候选迟到成功也只登记原批精确清理。
    if handoff.pending.is_none() && handoff.placement.is_none() {
        return; // 空闲稳定帧只消费消息，不分配或修改其他资源。
    }

    let placement_decision = handoff.placement.as_mut().map(|pending| {
        placement_file_handoff_decision(
            &mut pending.gate,
            PlacementFileHandoffEvidence {
                active_scene_id: active_scene.scene_id.as_ref(), // 当前玩法场景身份。
                status_scene_id: placements_status.scene_id.as_ref(), // 当前摆放运行时公开目标。
                phase: placements_status.phase, // Waiting/Building/Ready/Failed完整阶段。
                revision: placements_status.revision, // 最近完整原子采用修订。
                attempt_revision: placements_status.attempt_revision, // 区分发布后的新失败。
                progress_revision: placements_status.progress_revision, // 只有真实候选推进才刷新停滞截止。
                now: real_time.elapsed(), // 截止使用F12暂停期间仍推进的真实时间。
            },
        )
    });
    let mut completed_placement = None; // 最终提示需要保留场景与作者修订，不能只丢弃门。
    match placement_decision {
        Some(PlacementFileHandoffDecision::RuntimeFailed) => {
            let detail = placements_status.detail.clone(); // 可变清理前冻结运行时具体实例/网格诊断。
            fail_scene_file_handoff(
                &mut handoff,
                &mut session,
                &mut editor,
                &format!("正式摆放新修订构建失败：{detail}"),
            );
            return;
        }
        Some(PlacementFileHandoffDecision::Complete) => {
            if handoff.preview_cleanup.is_some() {
                return; // 前一批按64个世界操作/帧释放完再取得当前门，精确实体Vec所有权仍留在placement。
            }
            let mut completed = handoff.placement.take().expect("完成决定必须仍持有摆放门");
            if placement_strictly_ready(&completed, &placements_status) {
                let previews = std::mem::take(&mut completed.editor_previews);
                let deleted = if active_scene.scene_id.as_ref() == Some(&completed.gate.scene_id) {
                    std::mem::take(&mut completed.deleted)
                } else { Vec::new() }; // 离图放行不证明驻留旧批次退役，不能借旧图残留Ready确认删除。
                schedule_placement_save_cleanup(&mut handoff, &completed.gate.scene_id, previews, deleted)
                    .expect("上方已确认清理槽为空"); // 只有严格新Ready能替代仍可见的作者Scene0。
            }
            completed_placement = Some(completed); // 本域已原子采用，不因同批水域后续失败要求重复发布。
        }
        Some(PlacementFileHandoffDecision::Waiting) | None => {}
    }

    let mut water_failure_detail = None;
    let water_decision = handoff.pending.as_mut().map(|pending| {
        if !pending.root_pending && pending.terrains.is_some() {
            return terrain::advance(pending, &mut runtime, &active_scene, frame_count.0, &mut water_failure_detail);
        }
        let dynamic_water = &runtime.legacy;
        let decision = water_file_handoff_decision(
            &pending.gate,
            WaterFileHandoffEvidence {
                active_scene_id: active_scene.scene_id.as_ref(), // 当前玩法场景身份。
                status_scene_id: dynamic_water.scene_id.as_ref(), // 当前动态水公开目标。
                phase: dynamic_water.phase,                // 当前动态水阶段。
                input_draft_revision: dynamic_water.input_draft_revision,
                desired_generation: dynamic_water.desired_generation, // 当前期望输入代次。
                applied_generation: dynamic_water.applied_generation, // 最近原子安装代次。
                now: real_time.elapsed(),                  // 当前真实时间。
                frame: frame_count.0,                      // 当前真实帧。
            },
        );
        if decision == WaterFileHandoffDecision::Complete && pending.terrains.is_some() {
            // 根正式水已原子采用；释放它的作者覆盖门，后续帧才武装第一个私有画布。
            pending.root_pending = false;
            pending.water = None;
            pending.masks.clear();
            pending.textures.clear();
            pending.gate.all_inputs_succeeded_frame = None;
            return WaterFileHandoffDecision::Waiting;
        }
        decision
    });
    let mut completed_water = None; // 两个正式域都完成后才发布最终保存成功提示。
    match water_decision {
        Some(WaterFileHandoffDecision::RuntimeFailed) => {
            let detail = water_failure_detail.unwrap_or_else(|| runtime.legacy.detail.clone()); // 取出对象自己的失败项后再可变清理handoff。
            fail_scene_file_handoff(
                &mut handoff,
                &mut session,
                &mut editor,
                &format!("动态水新代次构建失败：{detail}"),
            );
            return;
        }
        Some(WaterFileHandoffDecision::Complete) => {
            completed_water = handoff.pending.take(); // 保存最终提示仍需读取purpose与作者修订。
        }
        Some(WaterFileHandoffDecision::Waiting) | None => {}
    }

    if handoff.pending.is_none() && handoff.placement.is_none() {
        if let Some(completed) = completed_water {
            editor.status = if completed.gate.purpose == WaterFileHandoffPurpose::RollbackRestore {
                format!(
                    "场景{}失败事务的磁盘旧版本已重新载入；当前草稿仍未保存",
                    completed.gate.scene_id
                )
            } else if editor.dirty
                || session.water_revision != completed.terrains.as_ref().map_or(completed.gate.water_revision, |batch| batch.water_revision)
                || session.terrain_revision != completed.gate.terrain_revision
            {
                format!(
                    "场景{}已保存版本完成正式水域与摆放交接；当前仍有后续修改未保存",
                    completed.gate.scene_id
                )
            } else {
                format!(
                    "场景{}已保存；正式水域与摆放已完成新修订原子热交接",
                    completed.gate.scene_id
                )
            };
        } else if let Some(completed) = completed_placement {
            editor.status = if editor.dirty || session.revision != completed.gate.author_revision {
                format!(
                    "场景{}已保存版本完成正式摆放交接；当前仍有后续修改未保存",
                    completed.gate.scene_id
                )
            } else {
                format!(
                    "场景{}已保存；正式摆放已完成新修订原子热交接",
                    completed.gate.scene_id
                )
            };
        }
        info!("{}", editor.status);
    }
}

#[cfg(test)]
mod tests; // 纯门、修订换代与失败重试回归独立维护，避免交接系统文件再次过长。
