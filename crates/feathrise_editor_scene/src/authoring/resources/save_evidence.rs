//! 保存请求等待期间的整网格水Scene0证据登记与有界缓存资源。

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque}; // 稳定记录、反向索引与固定状态事件环。

use bevy::asset::AssetId; // 相关GLB热更以类型化Scene0资产ID精确失效。
use bevy::prelude::*; // Entity与Resource。
use bevy::world_serialization::WorldAsset; // 缓存只冻结句柄身份，不持有World内容。
use feathrise_scene_content::{SceneId, ScenePlacementRecord}; // 保存请求冻结场景与非空水记录快照。

/// 单个Scene0根对一条冻结水记录的最近一次有界观察。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlacementWaterCandidateObservation {
    Ineligible,     // 根已退役、跨场景、失去精确预览归属或覆盖快照不再匹配。
    Pending,        // 根身份匹配，但World、UV或动态水材质仍可能成功收敛。
    Ready,          // 当前完整记录快照已经由真实Scene0根验证通过。
    Failed(String), // 当前匹配根已经给出确定运行时或递归资产加载失败。
}

/// 保存请求冻结的一项精确候选证据；后续等待只按实体直接查询，不重扫世界。
#[derive(Clone, Debug)]
pub(crate) struct PlacementWaterEvidenceCandidate {
    pub(crate) entity: Entity, // 实体代次包含在Entity中，despawn后不会借用同下标新根。
    pub(crate) record_index: usize, // 指向最多256项冻结非空水记录的小型数组。
    pub(crate) scene_asset_id: AssetId<WorldAsset>, // 只让同Scene0的AssetEvent唤醒本缓存。
    pub(crate) last_status_revision: u64, // 只记录该实体已经观察到的状态事件序号，不能用其他根的全局序号授信。
    pub(crate) required_status_revision: Option<u64>, // Scene0事件发生时冻结下限；晚于它的新状态到达前旧Ready失效。
    pub(crate) observation: PlacementWaterCandidateObservation, // 计数更新前保存旧分类。
}

/// 一条冻结水记录的增量聚合计数。
#[derive(Debug, Default)]
pub(crate) struct PlacementWaterRecordEvidence {
    pub(crate) ready_count: usize, // 任一精确Ready即可证明该条记录可保存。
    pub(crate) pending_count: usize, // 没有Ready但仍有Pending时继续保留保存请求。
    pub(crate) failures: BTreeMap<Entity, String>, // 全部候选确定失败时稳定选择首个具体诊断。
}

/// 已失效证据代次的容器所有权；保存系统按固定元素预算逐帧释放。
///
/// 【帧关键设计】marker增删可能让最多4096个候选同时失效，不能在成员变化帧
/// 对`Vec`/`HashMap`/失败字符串执行深析构。活动容器先O(1)移交到这里，再逐项回收。
#[derive(Debug, Default)]
pub(crate) struct PlacementWaterRetiredEvidenceGeneration {
    pub(crate) candidates: Vec<PlacementWaterEvidenceCandidate>,
    pub(crate) candidate_indices: std::collections::hash_map::IntoIter<Entity, usize>,
    pub(crate) record_evidence: Vec<PlacementWaterRecordEvidence>,
    pub(crate) pending_candidates: BTreeSet<usize>,
    pub(crate) missing_records: BTreeSet<usize>,
    pub(crate) waiting_records: BTreeSet<usize>,
    pub(crate) failed_records: BTreeSet<usize>,
    pub(crate) scene_asset_counts: std::collections::hash_map::IntoIter<AssetId<WorldAsset>, usize>,
    pub(crate) scene_asset_status_floors:
        std::collections::hash_map::IntoIter<AssetId<WorldAsset>, u64>,
    pub(crate) water_records: Vec<ScenePlacementRecord>, // 作者修订换代只O(1)移交旧完整记录，随后逐项析构。
    pub(crate) water_record_indices: std::collections::btree_map::IntoIter<String, usize>, // ID字符串同样进入拥有型游标，避免换代帧深析构。
}

impl PlacementWaterRetiredEvidenceGeneration {
    pub(crate) fn is_empty(&self) -> bool {
        self.candidates.is_empty()
            && self.candidate_indices.len() == 0
            && self.record_evidence.is_empty()
            && self.pending_candidates.is_empty()
            && self.missing_records.is_empty()
            && self.waiting_records.is_empty()
            && self.failed_records.is_empty()
            && self.scene_asset_counts.len() == 0
            && self.scene_asset_status_floors.len() == 0
            && self.water_records.is_empty()
            && self.water_record_indices.len() == 0
    }
}

/// 已完成证据聚合后的O(1)保存结论；只有候选真实变化时才重算文本。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum PlacementWaterCachedReadiness {
    #[default]
    Collecting, // 记录或证据根仍在固定预算游标中，不能提前声明缺根。
    Ready,           // 全部冻结非空水记录都有至少一个精确Ready根。
    Waiting(String), // 至少一条记录仍有可能收敛的Pending根。
    Failed(String),  // 缺根、身份漂移或全部匹配根确定失败。
}

/// 一个不可丢失的状态替换通知；环形日志只保存实体和单调序号。
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacementWaterStatusEvent {
    pub(crate) revision: u64,  // 保存缓存以本序号判断是否落后于有界事件环。
    pub(crate) entity: Entity, // Changed只唤醒这一根，不重验全部候选或记录。
}

/// 全局整网格水证据根登记表，由组件生命周期observer增量维护。
///
/// 【功能设计】保存按钮不能从Query首项重新枚举并在后续帧使用`skip(cursor)`；
/// observer提供稳定连续实体表和O(1)反向索引，移除或despawn同步推进成员代次。
#[derive(Resource, Debug, Default)]
pub(crate) struct PlacementWaterEvidenceRegistry {
    pub(crate) entities: Vec<Entity>, // 只含当前带ScenePlacementWaterEvidence的根。
    pub(crate) indices: HashMap<Entity, usize>, // Add/Remove均以O(1)维护连续表。
    pub(crate) total_count: usize,    // 超出缓存硬上限后仍计数，直到全部异常根释放才恢复。
    pub(crate) overflowed: bool,      // 异常爆量失败关闭，绝不静默截断当前Scene0证明。
    pub(crate) membership_revision: u64, // 任一marker增删让在途实体游标从新稳定表重建。
    pub(crate) status_revision: u64,  // 任一状态Add/Replace/Remove推进，不依赖稳定帧Changed查询。
    pub(crate) status_events: VecDeque<PlacementWaterStatusEvent>, // 固定容量环供保存缓存定向追赶。
    pub(crate) status_revisions_by_entity: HashMap<Entity, u64>, // marker根最近状态序号供事件环溢出或成员重建后定向授信。
}

/// 保存请求等待期间分帧建立的整网格水记录与真实Scene0证据缓存。
///
/// 【功能设计】完整草稿最多8192条普通placement，证据根还可能同时包含活动批次、
/// 隐藏候选和作者预览；记录、成员与候选分别按固定游标推进。完成后稳定Waiting只
/// 定向轮询Pending实体或消费Changed事件，不再构建全局BTreeMap，也不重验256条记录。
#[derive(Debug, Default)]
pub(crate) struct PlacementWaterSavePreflightCache {
    pub(crate) scene_id: Option<SceneId>, // 当前冻结场景；跨场景同名根永远不能继承缓存信用。
    pub(crate) author_revision: Option<u64>, // 任一placements数值修改都会冻结为新的保存请求身份。
    pub(crate) structure_revision: Option<u64>, // 增删、换源或绑定结构变化显式撤销旧实体归属。
    pub(crate) next_record: usize,    // 下一条待检查placement下标，单帧只推进固定预算。
    pub(crate) total_records: usize,  // 防御未推进修订但数组长度已变化的错误写侧。
    pub(crate) total_surface_count: usize, // 增量累计公共256绑定预算，避免缓存本身无界增长。
    pub(crate) water_records: Vec<ScenePlacementRecord>, // 仅克隆至少含一项水绑定的完整冻结记录。
    pub(crate) water_record_indices: BTreeMap<String, usize>, // 根ID直接定位冻结记录，不逐根线性查找。
    pub(crate) complete: bool, // true表示全部8192记录前缀已经按预算扫描完毕。
    pub(crate) evidence_registry_revision: Option<u64>, // 证据游标只解释同一成员表代次。
    pub(crate) evidence_total_roots: usize, // 开始聚合时冻结的登记根数量。
    pub(crate) next_evidence_root: usize, // 下一项登记实体下标，Query只按Entity直接读取。
    pub(crate) evidence_complete: bool, // 完整枚举后才允许把零候选解释为缺根。
    pub(crate) candidates: Vec<PlacementWaterEvidenceCandidate>, // 硬上限内的精确同快照候选。
    pub(crate) candidate_indices: HashMap<Entity, usize>, // 状态事件O(1)定位候选槽。
    pub(crate) record_evidence: Vec<PlacementWaterRecordEvidence>, // 与water_records等长的聚合计数。
    pub(crate) pending_candidates: BTreeSet<usize>, // 稳定Waiting只公平轮询当前未决候选。
    pub(crate) pending_poll_cursor: usize,          // BTreeSet范围游标避免总是重复首批Pending根。
    pub(crate) missing_records: BTreeSet<usize>,    // 缺少精确候选的冻结记录集合。
    pub(crate) waiting_records: BTreeSet<usize>,    // 没有Ready但仍有Pending的记录集合。
    pub(crate) failed_records: BTreeSet<usize>,     // 全部精确候选确定失败的记录集合。
    pub(crate) scene_asset_counts: HashMap<AssetId<WorldAsset>, usize>, // 相关GLB事件O(1)命中候选集合。
    pub(crate) scene_asset_status_floors: HashMap<AssetId<WorldAsset>, u64>, // 相关资产事件冻结当时状态序号，强制同资产候选等待新生命周期。
    pub(crate) scene_asset_invalidation_pending: bool, // 相关事件即使遇到其他保存门也保持到候选复验真正启动。
    pub(crate) scene_asset_event_backlog_status_floor: Option<u64>, // 超过单帧消息预算时整段积压共用首批状态下限，避免后段事件晚读死锁。
    pub(crate) retired_evidence: VecDeque<Box<PlacementWaterRetiredEvidenceGeneration>>, // 旧成员代次按固定预算释放，存在时暂停建立新代次。
    pub(crate) status_event_cursor: u64, // 已消费状态事件序号；事件环溢出时转为全候选分帧复验。
    pub(crate) forced_poll_cursor: usize, // runtime/gate或相关GLB事件触发的全候选有界复验游标。
    pub(crate) forced_poll_remaining: usize, // 零表示没有待完成的全候选复验周期。
    pub(crate) runtime_scene_id: Option<SceneId>, // 正式批次切换场景会撤销旧gate信用。
    pub(crate) runtime_phase_tag: Option<u8>, // Waiting/Building/Ready/Failed语义变化唤醒候选资格。
    pub(crate) runtime_revision: u64,    // 新活动批次采用必须复验旧Pending/Active归属。
    pub(crate) runtime_attempt_revision: u64, // 新失败或新候选尝试同样可能退役旧根。
    pub(crate) cached_readiness: PlacementWaterCachedReadiness, // 稳定帧O(1)复用最终结论。
}
