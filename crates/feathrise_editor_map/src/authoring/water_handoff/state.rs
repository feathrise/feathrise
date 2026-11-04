//! 正式水域文件交接的纯状态机。

use std::time::Duration;

use feathrise_map_content::MapId;
use feathrise_scene::{DynamicWaterPhase, MapPlacementRuntimePhase};


/// 保存成功发布与失败后恢复旧磁盘版本共享同一事件门，但身份换代语义不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaterFileHandoffPurpose {
    SavePublish,     // 证明刚保存的新正式批次已被运行时原子采用。
    RollbackRestore, // 证明失败事务恢复的旧正式批次已重新覆盖AssetServer缓存。
}

/// 不依赖ECS的正式水域完成门身份与跨帧状态。
#[derive(Clone, Debug)]
pub(super) struct WaterFileHandoffGate {
    pub(super) purpose: WaterFileHandoffPurpose,
    pub(super) map_id: MapId,
    pub(super) water_revision: u64,
    pub(super) terrain_revision: u64,
    pub(super) baseline_generation: u64,
    #[allow(dead_code)] // 保留发布开始时间作为诊断元数据，不再以总耗时拒收大水体。
    pub(super) started_at: Duration,
    pub(super) all_inputs_succeeded_frame: Option<u32>,
}

/// 当前帧用于纯状态门判断的全部运行时证据。
#[derive(Clone, Copy)]
pub(super) struct WaterFileHandoffEvidence<'a> {
    pub(super) active_map_id: Option<&'a MapId>,
    pub(super) status_map_id: Option<&'a MapId>,
    pub(super) phase: DynamicWaterPhase,
    pub(super) input_draft_revision: Option<u64>, // 作者Ready/Failed均不能冒充正式磁盘输入的采用证据。
    pub(super) desired_generation: u64,
    pub(super) applied_generation: u64,
    #[allow(dead_code)]
    pub(super) now: Duration,
    pub(super) frame: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaterFileHandoffDecision {
    Waiting,
    Complete,
    RuntimeFailed,
}

/// 正式摆放发布后的纯完成门；资产内容无变化的失败重试也沿同一身份等待。
#[derive(Clone, Debug)]
pub(super) struct PlacementFileHandoffGate {
    pub(super) map_id: MapId,
    pub(super) author_revision: u64,
    pub(super) baseline_revision: u64,
    pub(super) baseline_attempt_revision: u64,
    /// 最近一次已经观察到的当前地图真实运行时推进修订。
    pub(super) last_progress_revision: u64,
    /// 最近一次匹配地图推进的真实时间；总耗时可以超过三十秒。
    pub(super) last_progress_at: Duration,
}

/// 当前帧的正式摆放运行时证据。
#[derive(Clone, Copy)]
pub(super) struct PlacementFileHandoffEvidence<'a> {
    pub(super) active_map_id: Option<&'a MapId>,
    pub(super) status_map_id: Option<&'a MapId>,
    pub(super) phase: MapPlacementRuntimePhase,
    pub(super) revision: u64,
    pub(super) attempt_revision: u64,
    pub(super) progress_revision: u64,
    pub(super) now: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlacementFileHandoffDecision {
    Waiting,
    Complete,
    RuntimeFailed,
}

/// 只接受发布后严格更新的当前地图Ready；旧Failed必须先出现新尝试修订才可拒绝保存。
pub(super) fn placement_file_handoff_decision(
    gate: &mut PlacementFileHandoffGate,
    evidence: PlacementFileHandoffEvidence<'_>,
) -> PlacementFileHandoffDecision {
    if evidence.status_map_id == Some(&gate.map_id)
        && evidence.progress_revision > gate.last_progress_revision
    {
        gate.last_progress_revision = evidence.progress_revision;
        gate.last_progress_at = evidence.now; // 只接受匹配当前正式地图的单调真实推进，另一地图不能续命。
    }

    if evidence.active_map_id != Some(&gate.map_id) {
        return PlacementFileHandoffDecision::Complete; // 已离开地图时下次进入会从正式缓存完整重建。
    }
    if evidence.status_map_id != Some(&gate.map_id) {
        return PlacementFileHandoffDecision::Waiting; // 运行时尚未切到本次正式目标，不能借另一地图状态放行。
    }
    if evidence.phase == MapPlacementRuntimePhase::Failed
        && evidence.attempt_revision > gate.baseline_attempt_revision
    {
        return PlacementFileHandoffDecision::RuntimeFailed; // 严格新尝试失败才恢复dirty并阻止自动关闭。
    }
    if evidence.phase == MapPlacementRuntimePhase::Ready
        && evidence.revision > gate.baseline_revision
        && evidence.attempt_revision > gate.baseline_attempt_revision
    {
        return PlacementFileHandoffDecision::Complete; // 候选也必须在建门后开始，排除旧Building恰好随后完成。
    }
    PlacementFileHandoffDecision::Waiting
}

/// 正式文件一旦开始发布或恢复，缓存责任只跟随地图，不会被后续作者修订绕过。
pub(super) fn water_file_handoff_decision(
    gate: &WaterFileHandoffGate,
    evidence: WaterFileHandoffEvidence<'_>,
) -> WaterFileHandoffDecision {


    if let Some(success_frame) = gate.all_inputs_succeeded_frame
        && evidence.frame != success_frame
    {
        if evidence.active_map_id != Some(&gate.map_id) {
            return WaterFileHandoffDecision::Complete; // 已离开的地图只需证明缓存文件完整恢复。
        }
        let status_matches = evidence.status_map_id == Some(&gate.map_id)
            && evidence.input_draft_revision.is_none();
        if status_matches
            && evidence.phase == DynamicWaterPhase::Failed
            && evidence.desired_generation > gate.baseline_generation
        {
            return WaterFileHandoffDecision::RuntimeFailed;
        }
        if status_matches
            && evidence.phase == DynamicWaterPhase::Ready
            && evidence.desired_generation > gate.baseline_generation
            && evidence.applied_generation == evidence.desired_generation
        {
            return WaterFileHandoffDecision::Complete;
        }
    }

    WaterFileHandoffDecision::Waiting
}
