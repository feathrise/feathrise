//! GLB浏览器摆放结构签名的固定预算分帧构建。

use std::collections::hash_map;
use std::collections::{BTreeMap, BTreeSet, HashMap}; // 渐进识别重复ID、展开路径与共享资产计数。 // 取消草稿时用owning游标单调经过HashMap桶。

use feathrise_scene_content::{
    ScenePlacementRecord, ScenePlacementWaterSurface,
}; // 每条记录及单记录水绑定数继续服从公共作者硬门。

use super::retirement::{BrowserRetirementBudget, BrowserRetirementStep}; // 取消草稿也必须逐项退役。
use super::{
    BrowserSessionObservation, MAX_GLTF_BROWSER_PLACEMENTS,
    MAX_GLTF_SIGNATURE_PATH_SEGMENTS_PER_FRAME, MAX_GLTF_SIGNATURE_PLACEMENTS_PER_FRAME,
    MAX_GLTF_SIGNATURE_STRING_BYTES_PER_FRAME, MAX_GLTF_SIGNATURE_WATER_BINDINGS_PER_FRAME,
    PlacementSignature, PlacementWaterBindingSignature,
}; // 活动状态只在完整签名完成后原子采用。

/// 取消草稿中的实例ID到资产路径索引；预算不足时保留当前键值而不回扫桶前缀。
struct RetiringAssetPathIndex {
    remaining: hash_map::IntoIter<String, String>, // owning游标保证每个桶最多经过一次。
    current: Option<(String, String)>,             // 已取出但尚未取得字符预算的键值。
}

impl RetiringAssetPathIndex {
    fn advance(&mut self, budget: &mut BrowserRetirementBudget) -> BrowserRetirementStep {
        if self.current.is_none() {
            self.current = self.remaining.next(); // 跨帧只从上次桶位置继续。
        }
        let Some((key, value)) = self.current.as_ref() else {
            return BrowserRetirementStep::Complete;
        };
        if !budget.try_charge(1, 0, key.len().saturating_add(value.len())) {
            return BrowserRetirementStep::Blocked;
        }
        self.current = None; // 当前两个String在已计费后释放。
        BrowserRetirementStep::Progressed
    }
}

/// 取消草稿中的实例ID到作者下标索引，同样持有owning桶游标。
struct RetiringRecordIndex {
    remaining: hash_map::IntoIter<String, usize>, // 不使用每项keys().next()重复找首桶。
    current: Option<(String, usize)>,             // 当前实例ID等待预算。
}

impl RetiringRecordIndex {
    fn advance(&mut self, budget: &mut BrowserRetirementBudget) -> BrowserRetirementStep {
        if self.current.is_none() {
            self.current = self.remaining.next();
        }
        let Some((key, _)) = self.current.as_ref() else {
            return BrowserRetirementStep::Complete;
        };
        if !budget.try_charge_string(key) {
            return BrowserRetirementStep::Blocked;
        }
        self.current = None; // usize为Copy，只释放当前String键。
        BrowserRetirementStep::Progressed
    }
}

/// 签名复制除“完成记录数”外，还必须显式限制记录内水绑定、路径段和字符串总量。
#[derive(Default)]
struct SignatureBuildFrameBudget {
    water_bindings: usize, // 本帧已经复制的水绑定项数。
    path_segments: usize,  // 本帧已经复制的节点路径段数。
    string_bytes: usize,   // ID、资产路径、节点名和网格名的UTF-8字节近似量。
    charged_any: bool,     // 单个合法大项可在新鲜帧独占推进，避免永久饿死。
}

impl SignatureBuildFrameBudget {
    /// 三维预算原子计费；失败时调用方不推进记录或绑定游标。
    fn try_charge(
        &mut self,
        water_bindings: usize,
        path_segments: usize,
        string_bytes: usize,
    ) -> bool {
        let next_bindings = self.water_bindings.saturating_add(water_bindings);
        let next_segments = self.path_segments.saturating_add(path_segments);
        let next_bytes = self.string_bytes.saturating_add(string_bytes);
        if next_bindings > MAX_GLTF_SIGNATURE_WATER_BINDINGS_PER_FRAME
            || next_segments > MAX_GLTF_SIGNATURE_PATH_SEGMENTS_PER_FRAME
            || next_bytes > MAX_GLTF_SIGNATURE_STRING_BYTES_PER_FRAME
        {
            if self.charged_any {
                return false; // 本帧已有复制时，完整单项留到下一Update。
            }
            self.water_bindings = MAX_GLTF_SIGNATURE_WATER_BINDINGS_PER_FRAME;
            self.path_segments = MAX_GLTF_SIGNATURE_PATH_SEGMENTS_PER_FRAME;
            self.string_bytes = MAX_GLTF_SIGNATURE_STRING_BYTES_PER_FRAME;
            self.charged_any = true;
            return true; // 单个仍受公共硬门保护的合法项独占本帧，不能永久Blocked。
        }
        self.water_bindings = next_bindings;
        self.path_segments = next_segments;
        self.string_bytes = next_bytes;
        self.charged_any = true;
        true
    }
}

/// 一项水绑定只扫描有界路径一次来估算即将产生的拥有字符串字节。
fn water_binding_string_bytes(surface: &ScenePlacementWaterSurface) -> usize {
    surface
        .node_path
        .iter()
        .fold(surface.mesh_name.len(), |bytes, segment| {
            bytes.saturating_add(segment.len())
        }) // 公共校验最多64段、4096个Unicode字符，UTF-8字节仍按真实长度计费。
}

/// 记录完成时会为唯一/重复索引产生数份ID和资产路径副本，使用保守上界统一计费。
fn signature_finalize_string_bytes(signature: &PlacementSignature) -> usize {
    signature
        .placement_id
        .len()
        .saturating_mul(4) // seen、路径索引、作者下标索引及临时稳定ID最多各一份。
        .saturating_add(signature.asset_path.len().saturating_mul(3)) // 路径索引、需求计数和wanted集合最多各一份。
}

/// 当前结构代次尚未发布的签名、重复身份与展开资产需求草稿。
pub(super) struct BrowserSignatureBuild {
    pub(super) has_object_groups: bool,
    /// 构建开始时冻结的确定性作者结构身份；变化后整个草稿失效并重启。
    pub(super) observation: BrowserSessionObservation,
    /// 下一条待读取的摆放索引。
    pub(super) next_placement: usize,
    /// 当前记录的ID、资产路径和已按预算复制的水绑定前缀；完成前不进入签名Vec。
    pending_signature: Option<PlacementSignature>,
    /// 当前记录下一项待读水绑定下标；单记录64项也能跨Update继续。
    next_water_binding: usize,
    /// 已完成的有界签名前缀；绝不在完成前暴露给UI。
    pub(super) signature: Vec<PlacementSignature>,
    /// 已见稳定ID用于O(log n)增量识别重复项。
    pub(super) seen_ids: BTreeSet<String>,
    /// 当前已经确认重复、因此不能展开或回写的实例ID。
    pub(super) duplicate_ids: BTreeSet<String>,
    /// 当前唯一实例ID到资产路径索引；第二次见到同ID时立即移除。
    pub(super) placement_asset_paths: HashMap<String, String>,
    /// 当前唯一实例ID到作者`placements`数组下标；水体属性编辑据此保持O(1)定位。
    pub(super) placement_record_indices: HashMap<String, usize>,
    /// 原展开集合中仍存在且唯一的实例ID，完成时原子替换旧集合。
    pub(super) expanded_live: BTreeSet<String>,
    /// 已展开唯一实例按资产路径汇总的引用计数。
    pub(super) asset_demand_counts: BTreeMap<String, usize>,
    /// 所有非零需求路径同步渐进收集，供后续分帧请求Scene0。
    pub(super) wanted_asset_paths: BTreeSet<String>,
    /// 仅退役阶段使用：先逐绑定清空一条签名，再释放记录壳。
    retiring_signature: Option<PlacementSignature>,
    /// 退役阶段接管`placement_asset_paths`全部桶的单调owning游标。
    retiring_asset_paths: Option<RetiringAssetPathIndex>,
    /// 退役阶段接管`placement_record_indices`全部桶的单调owning游标。
    retiring_record_indices: Option<RetiringRecordIndex>,
}

impl BrowserSignatureBuild {
    /// 为一个确定性结构观察建立空草稿；预留容量本身受8192固定硬上限约束。
    pub(super) fn new(observation: BrowserSessionObservation) -> Self {
        let capacity = observation
            .source_placement_count
            .min(MAX_GLTF_BROWSER_PLACEMENTS); // 异常作者数据也不按原始总数无限预留。
        Self {
            has_object_groups: false,
            observation,                              // 冻结构身份供每帧继续前复核。
            next_placement: 0,                        // 从第一条摆放开始。
            pending_signature: None,                  // 第一帧再复制首条记录壳。
            next_water_binding: 0,                    // 当前尚无记录内绑定前缀。
            signature: Vec::with_capacity(capacity),  // 最终拥有型签名至多8192项。
            seen_ids: BTreeSet::new(),                // 尚未观察任何实例ID。
            duplicate_ids: BTreeSet::new(),           // 尚无重复诊断。
            placement_asset_paths: HashMap::new(),    // 尚无O(1)点击索引。
            placement_record_indices: HashMap::new(), // 尚无O(1)作者记录索引。
            expanded_live: BTreeSet::new(),           // 展开偏好随记录渐进确认。
            asset_demand_counts: BTreeMap::new(),     // 尚无Scene0强句柄需求。
            wanted_asset_paths: BTreeSet::new(),      // 尚无待请求路径。
            retiring_signature: None, // 活动构建不会进入该槽；取消后才逐项移动签名。
            retiring_asset_paths: None, // 活动构建继续使用普通HashMap。
            retiring_record_indices: None, // 取消时才O(1)转换为IntoIter。
        }
    }

    /// 取消后的签名草稿按一个集合/索引/绑定元素推进；永不在`Option=None`时整批析构。
    pub(super) fn retirement_step(
        &mut self,
        budget: &mut BrowserRetirementBudget,
    ) -> BrowserRetirementStep {
        if self.retiring_signature.is_none() {
            self.retiring_signature = self
                .pending_signature
                .take()
                .or_else(|| self.signature.pop()); // 先清理中途记录，再逐条移动完整签名后缀。
        }
        if let Some(current) = self.retiring_signature.as_mut() {
            if let Some(binding) = current.water_bindings.last() {
                if !budget.try_charge_mesh_identity(&binding.node_path, &binding.mesh_name) {
                    return BrowserRetirementStep::Blocked;
                }
                current.water_bindings.pop(); // 一次只释放一项有公共路径硬门的水绑定。
                return BrowserRetirementStep::Progressed;
            }
            let bytes = current
                .placement_id
                .len()
                .saturating_add(current.asset_path.len()); // 空绑定记录只剩两个拥有字符串。
            if !budget.try_charge(1, 0, bytes) {
                return BrowserRetirementStep::Blocked;
            }
            self.retiring_signature = None; // 记录壳和空Vec为O(1)析构。
            return BrowserRetirementStep::Progressed;
        }

        macro_rules! retire_string_set {
            ($field:expr) => {
                if let Some(value) = $field.first() {
                    if !budget.try_charge_string(value) {
                        return BrowserRetirementStep::Blocked;
                    }
                    $field.pop_first(); // BTreeSet无需复制键即可移出一个元素。
                    return BrowserRetirementStep::Progressed;
                }
            };
        }
        retire_string_set!(self.seen_ids);
        retire_string_set!(self.duplicate_ids);
        retire_string_set!(self.expanded_live);
        retire_string_set!(self.wanted_asset_paths);

        if self.retiring_asset_paths.is_none() && !self.placement_asset_paths.is_empty() {
            self.retiring_asset_paths = Some(RetiringAssetPathIndex {
                remaining: std::mem::take(&mut self.placement_asset_paths).into_iter(), // O(1)接管桶数组。
                current: None, // 下一步再读取首桶。
            });
        }
        if let Some(retiring) = self.retiring_asset_paths.as_mut() {
            match retiring.advance(budget) {
                BrowserRetirementStep::Complete => self.retiring_asset_paths = None, // 空迭代器壳O(1)释放。
                step => return step,
            }
        }
        if self.retiring_record_indices.is_none() && !self.placement_record_indices.is_empty() {
            self.retiring_record_indices = Some(RetiringRecordIndex {
                remaining: std::mem::take(&mut self.placement_record_indices).into_iter(), // 不扫描桶。
                current: None,
            });
        }
        if let Some(retiring) = self.retiring_record_indices.as_mut() {
            match retiring.advance(budget) {
                BrowserRetirementStep::Complete => self.retiring_record_indices = None,
                step => return step,
            }
        }
        if let Some((path, _)) = self.asset_demand_counts.first_key_value() {
            if !budget.try_charge_string(path) {
                return BrowserRetirementStep::Blocked;
            }
            self.asset_demand_counts.pop_first();
            return BrowserRetirementStep::Progressed;
        }
        if let Some(scene_id) = self.observation.draft_scene_id.as_ref() {
            if !budget.try_charge_string(scene_id) {
                return BrowserRetirementStep::Blocked;
            }
            self.observation.draft_scene_id = None; // 最后释放草稿场景身份。
            return BrowserRetirementStep::Progressed;
        }
        BrowserRetirementStep::Complete // 所有深容器均为空，结构壳可以O(1)析构。
    }
}

/// 为一个仍唯一且展开的实例增加共享资产路径计数。
fn increment_asset_demand(build: &mut BrowserSignatureBuild, asset_path: &str) {
    let count = build
        .asset_demand_counts
        .entry(asset_path.to_owned())
        .or_default(); // 同模型多个实例共享唯一强句柄。
    *count = count.saturating_add(1); // 异常重复规模也不允许usize溢出。
    build.wanted_asset_paths.insert(asset_path.to_owned()); // 非零路径后续进入分帧请求集合。
}

/// ID后来被证明重复时撤销首次记录产生的展开资产需求。
fn decrement_asset_demand(build: &mut BrowserSignatureBuild, asset_path: &str) {
    let Some(count) = build.asset_demand_counts.get_mut(asset_path) else {
        return; // 首次记录原本未展开时没有需求需要撤销。
    };
    *count = count.saturating_sub(1); // 每个唯一实例最多增加一次。
    if *count == 0 {
        build.asset_demand_counts.remove(asset_path); // 零计数不进入活动目标表。
        build.wanted_asset_paths.remove(asset_path); // 结构采用后无需请求该Scene0。
    }
}

/// 把一条已经按绑定预算完整复制的记录并入所有辅助索引。
fn finish_signature(
    build: &mut BrowserSignatureBuild,
    signature: PlacementSignature,
    placement_index: usize,
    expanded_placements: &BTreeSet<String>,
) {
    let placement_id = signature.placement_id.clone(); // 索引与最终签名各自拥有稳定ID。
    if build.seen_ids.insert(placement_id.clone()) {
        build
            .placement_asset_paths
            .insert(placement_id.clone(), signature.asset_path.clone()); // 首次ID暂视为唯一。
        build
            .placement_record_indices
            .insert(placement_id.clone(), placement_index); // 与同代次签名一起原子发布。
        if signature.asset_target.is_scene0() && expanded_placements.contains(&placement_id) {
            build.expanded_live.insert(placement_id); // 只有旧偏好中仍存在的唯一ID继续展开。
            increment_asset_demand(build, &signature.asset_path); // 同步渐进构造唯一路径引用计数。
        }
    } else {
        build.duplicate_ids.insert(placement_id.clone()); // 第二项及之后全部明确不可稳定回写。
        build.expanded_live.remove(&placement_id); // 重复ID不得继续隐式加载多条不确定根。
        if let Some(first_path) = build.placement_asset_paths.remove(&placement_id) {
            decrement_asset_demand(build, &first_path); // 撤销第一次记录暂时产生的需求。
        }
        build.placement_record_indices.remove(&placement_id); // 重复ID没有可安全写回的唯一数组下标。
    }
    build.signature.push(signature); // 活动签名仍保留重复根供作者诊断。
}

/// 处理固定数量记录，并让单记录水绑定也服从独立项数、路径段和字符串预算。
pub(super) fn advance_signature_build(
    build: &mut BrowserSignatureBuild, // 持续跨帧拥有全部未发布草稿。
    placements: &[ScenePlacementRecord], // 当前结构代次的权威Vec切片。
    expanded_placements: &BTreeSet<String>, // 上一活动代次要保留的稳定展开偏好。
) -> bool {
    let total = placements.len().min(MAX_GLTF_BROWSER_PLACEMENTS); // 只浏览固定上限前缀，原始总数另行显示。
    let mut completed_placements = 0usize; // 只有完整进入辅助索引的记录才消耗记录额度。
    let mut frame_budget = SignatureBuildFrameBudget::default(); // 深字段不会藏在256条记录门内。
    while build.next_placement < total
        && completed_placements < MAX_GLTF_SIGNATURE_PLACEMENTS_PER_FRAME
    {
        let placement = &placements[build.next_placement]; // 当前结构观察保证切片身份跨本次推进稳定。
        if build.pending_signature.is_none() {
            build.has_object_groups |= !placement.editor.group.is_empty();
            let base_bytes = placement
                .id
                .len()
                .saturating_add(placement.asset_path.len()); // 先计费签名本体各一份ID和资产路径。
            if !frame_budget.try_charge(0, 0, base_bytes) {
                break; // 当前记录壳下一Update再完整复制。
            }
            build.pending_signature = Some(PlacementSignature {
                editor: placement.editor.clone(),
                placement_id: placement.id.clone(),
                asset_path: placement.asset_path.clone(),
                asset_target: placement.asset_target, // 固定大小数字目标不增加深复制预算。
                water_bindings: Vec::with_capacity(
                    placement
                        .water_surfaces
                        .len(),
                ), // 固定64项公共门内预留，不按损坏数组原始长度分配。
            });
            build.next_water_binding = 0;
        }

        let binding_count = placement
            .water_surfaces
            .len();
        if build.next_water_binding < binding_count {
            let surface = &placement.water_surfaces[build.next_water_binding];
            if !frame_budget.try_charge(
                1,
                surface.node_path.len(),
                water_binding_string_bytes(surface),
            ) {
                break; // 当前绑定游标与完整作者引用保持不变，下一Update继续。
            }
            build
                .pending_signature
                .as_mut()
                .expect("当前记录壳已在上方建立")
                .water_bindings
                .push(PlacementWaterBindingSignature::from(surface)); // 一次只复制一个受公共硬门保护的稳定身份。
            build.next_water_binding = build.next_water_binding.saturating_add(1);
            continue;
        }

        let pending = build
            .pending_signature
            .as_ref()
            .expect("当前记录的全部绑定已经按预算复制");
        if !frame_budget.try_charge(0, 0, signature_finalize_string_bytes(pending)) {
            break; // 辅助索引的拥有字符串副本同样不能越过本帧字符额度。
        }
        let signature = build
            .pending_signature
            .take()
            .expect("上方已确认完整待发布记录");
        let placement_index = build.next_placement; // 可变借用前冻结当前作者数组下标。
        finish_signature(build, signature, placement_index, expanded_placements);
        build.next_placement = build.next_placement.saturating_add(1);
        build.next_water_binding = 0;
        completed_placements += 1;
    }
    build.next_placement >= total && build.pending_signature.is_none() // 完整记录边界才允许原子采用。
}

#[cfg(test)]
#[path = "signature/tests.rs"]
mod tests; // 8192条短记录、单记录深绑定预算和取消退役分别锁定。
