//! GLB浏览器旧代拥有型数据的有界退役队列。
//!
//! 发布、热更和F12关闭只把旧容器O(1)移入这里；实际析构按元素、路径段和
//! 字符串字节近似预算逐帧推进，避免4096/8192项深字符串集中落在一个Update。

use std::collections::hash_map;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque}; // 每种活动索引保持原容器形态并逐项取出。 // owning IntoIter保证HashMap每个桶最多经过一次。

use bevy::asset::AssetId; // Scene0反向索引使用类型化资产ID。
use bevy::world_serialization::WorldAsset; // 退役强句柄缓存的资产类型。

use super::entries::BrowserEntriesBuild; // 未发布行草稿拥有深选择路径。
use super::scene_build::BrowserSceneBuild; // Scene0 DFS草稿拥有路径链、网格和去重键。
use super::signature::BrowserSignatureBuild; // 摆放签名草稿拥有完整水绑定身份。
use super::{
    BrowserAssetDemandTransition, BrowserLoadingTicket, CachedGltfAsset, CachedGltfMesh,
    EditorGltfBrowserEntry, EditorGltfMeshSelection, PlacementSignature,
}; // 活动资源中的所有大型拥有型容器都由本模块接管。

mod budget; // 元素、路径段和字符串三维帧预算独立维护。
mod queue; // 最多两代的single-flight背压与每帧推进独立维护。
mod session; // F12关闭把活动资源字段一次移动成一代退役工作。

pub(super) use budget::{BrowserRetirementBudget, BrowserRetirementStep}; // 各草稿类型复用同一退役计费边界。
pub(super) use queue::BrowserRetirement; // Editor资源只暴露队列壳，不接触工作项细节。
pub(super) use session::take_browser_session_generation; // 会话编排保持简短且不接触容器细节。

/// 签名Vec需要先逐条释放单记录内最多64项水绑定，不能直接pop整条深记录。
struct RetiredSignatures {
    remaining: Vec<PlacementSignature>, // 尚未进入细粒度释放的记录后缀。
    current: Option<PlacementSignature>, // 当前记录逐水绑定清空后才释放ID和资产路径。
}

impl RetiredSignatures {
    fn new(remaining: Vec<PlacementSignature>) -> Self {
        Self {
            remaining,     // Vec所有权O(1)转移，不复制8192项。
            current: None, // 第一帧再按预算取出末项。
        }
    }

    fn advance(&mut self, budget: &mut BrowserRetirementBudget) -> BrowserRetirementStep {
        if self.current.is_none() {
            self.current = self.remaining.pop(); // 移动一个记录本身不析构任何深字段。
        }
        let Some(current) = self.current.as_mut() else {
            return BrowserRetirementStep::Complete; // Vec已经空且没有当前记录。
        };
        if let Some(binding) = current.water_bindings.last() {
            if !budget.try_charge_mesh_identity(&binding.node_path, &binding.mesh_name) {
                return BrowserRetirementStep::Blocked; // 一条合法绑定完整留到下一帧。
            }
            current.water_bindings.pop(); // 仅释放一条有公共路径硬门的水绑定。
            return BrowserRetirementStep::Progressed;
        }
        let shell_bytes = current
            .placement_id
            .len()
            .saturating_add(current.asset_path.len()); // 记录壳只剩两个拥有字符串。
        if !budget.try_charge(1, 0, shell_bytes) {
            return BrowserRetirementStep::Blocked;
        }
        self.current = None; // 水绑定Vec已空，释放记录壳为O(1)。
        BrowserRetirementStep::Progressed
    }
}

/// BTreeMap缓存先逐网格清空一个资产，再释放该路径和强句柄。
struct RetiredAssets {
    remaining: BTreeMap<String, CachedGltfAsset>, // 路径顺序与活动缓存一致，可O(log n) pop_first。
    current: Option<(String, CachedGltfAsset)>,   // 当前资产的网格Vec逐条释放。
}

impl RetiredAssets {
    fn new(remaining: BTreeMap<String, CachedGltfAsset>) -> Self {
        Self {
            remaining,     // BTreeMap根指针O(1)转移。
            current: None, // 下一帧再取首个缓存。
        }
    }

    /// 单路径折叠无需先新建BTreeMap节点，直接从当前资产槽开始逐网格退役。
    fn single(path: String, cached: CachedGltfAsset) -> Self {
        Self {
            remaining: BTreeMap::new(),    // 单缓存不建立额外树分配。
            current: Some((path, cached)), // 第一帧直接清理其网格Vec。
        }
    }

    fn advance(&mut self, budget: &mut BrowserRetirementBudget) -> BrowserRetirementStep {
        if self.current.is_none() {
            self.current = self.remaining.pop_first(); // 一次只把一个缓存移出树。
        }
        let Some((path, cached)) = self.current.as_mut() else {
            return BrowserRetirementStep::Complete; // 所有缓存和网格均已清空。
        };
        if let Some(mesh) = cached.meshes.last() {
            if !budget.try_charge_cached_mesh(mesh) {
                return BrowserRetirementStep::Blocked;
            }
            cached.meshes.pop(); // 深路径受上方路径段/字节预算保护。
            return BrowserRetirementStep::Progressed;
        }
        if !budget.try_charge_string(path) {
            return BrowserRetirementStep::Blocked;
        }
        self.current = None; // 空网格Vec、强句柄和单路径在一个固定元素额度内释放。
        BrowserRetirementStep::Progressed
    }
}

/// 一个退役工作项只拥有一种容器，清空前始终保留原完整所有权。
enum BrowserRetiredWork {
    Entries(Vec<EditorGltfBrowserEntry>), // 已发布行或未发布草稿行中的深选择路径。
    Signatures(RetiredSignatures),        // 活动签名按记录/绑定两层释放。
    SignatureBuild(BrowserSignatureBuild), // 取消的结构草稿自行按内部游标释放。
    EntriesBuild(BrowserEntriesBuild),    // 取消或迟到的可见行草稿。
    SceneBuild(BrowserSceneBuild),        // 取消的World DFS、Arc路径链与去重表。
    Assets(RetiredAssets),                // 全缓存关闭按资产/网格两层释放。
    CachedMeshes(Vec<CachedGltfMesh>),    // 单资产原子热更替换下来的旧网格表。
    Selection(Option<EditorGltfMeshSelection>), // F12关闭时立即撤下但分帧释放其路径。
    StringSet(BTreeSet<String>),          // 展开ID、重复ID、待需求路径和失效路径。
    StringUsizeTree(BTreeMap<String, usize>), // 路径需求计数及旧代过渡表。
    StringStringMap {
        remaining: hash_map::IntoIter<String, String>, // owning游标单调经过桶数组。
        current: Option<(String, String)>,             // 预算不足时保留当前完整键值。
    }, // 实例ID到资产路径索引。
    StringUsizeMap {
        remaining: hash_map::IntoIter<String, usize>, // 不重复从首桶扫描。
        current: Option<(String, usize)>,             // 当前实例ID等待字符预算。
    }, // 实例ID到作者记录下标索引。
    StringU64Map {
        remaining: hash_map::IntoIter<String, u64>, // Loading修订索引owning游标。
        current: Option<(String, u64)>,             // 当前路径等待字符预算。
    }, // Loading票据最新内容修订索引。
    AssetPathMap {
        remaining: hash_map::IntoIter<AssetId<WorldAsset>, String>, // 资产反向索引owning游标。
        current: Option<(AssetId<WorldAsset>, String)>,             // 当前资产路径等待预算。
    }, // 类型化资产ID反向路径索引。
    LoadingQueue(VecDeque<BrowserLoadingTicket>), // 每路径唯一的公平轮询票。
    String(Option<String>),               // 状态、场景ID和热更游标等单字符串。
}

impl BrowserRetiredWork {
    fn advance(&mut self, budget: &mut BrowserRetirementBudget) -> BrowserRetirementStep {
        match self {
            Self::Entries(entries) => {
                let Some(entry) = entries.last() else {
                    return BrowserRetirementStep::Complete;
                };
                if !budget.try_charge_entry(entry) {
                    return BrowserRetirementStep::Blocked;
                }
                entries.pop(); // 一条公开行的全部深字段一起在已计费预算内释放。
                BrowserRetirementStep::Progressed
            }
            Self::Signatures(signatures) => signatures.advance(budget),
            Self::SignatureBuild(build) => build.retirement_step(budget),
            Self::EntriesBuild(build) => build.retirement_step(budget),
            Self::SceneBuild(build) => build.retirement_step(budget),
            Self::Assets(assets) => assets.advance(budget),
            Self::CachedMeshes(meshes) => {
                let Some(mesh) = meshes.last() else {
                    return BrowserRetirementStep::Complete;
                };
                if !budget.try_charge_cached_mesh(mesh) {
                    return BrowserRetirementStep::Blocked;
                }
                meshes.pop(); // 一次只释放一个完整稳定网格身份。
                BrowserRetirementStep::Progressed
            }
            Self::Selection(selection) => {
                let Some(current) = selection.as_ref() else {
                    return BrowserRetirementStep::Complete;
                };
                let bytes = current
                    .placement_id
                    .len()
                    .saturating_add(current.mesh_name.len())
                    .saturating_add(current.node_path.iter().map(String::len).sum::<usize>());
                if !budget.try_charge(1, current.node_path.len(), bytes) {
                    return BrowserRetirementStep::Blocked;
                }
                *selection = None; // 公开选择早已清空，此处只释放旧路径所有权。
                BrowserRetirementStep::Progressed
            }
            Self::StringSet(values) => {
                let Some(value) = values.first() else {
                    return BrowserRetirementStep::Complete;
                };
                if !budget.try_charge_string(value) {
                    return BrowserRetirementStep::Blocked;
                }
                values.pop_first(); // BTreeSet无需克隆键即可逐项取出。
                BrowserRetirementStep::Progressed
            }
            Self::StringUsizeTree(values) => {
                let Some((key, _)) = values.first_key_value() else {
                    return BrowserRetirementStep::Complete;
                };
                if !budget.try_charge_string(key) {
                    return BrowserRetirementStep::Blocked;
                }
                values.pop_first(); // 值为Copy，主要成本来自路径String分配。
                BrowserRetirementStep::Progressed
            }
            Self::StringStringMap { remaining, current } => {
                if current.is_none() {
                    *current = remaining.next(); // 游标只前进，不因跨帧预算回扫桶前缀。
                }
                let Some((key, value)) = current.as_ref() else {
                    return BrowserRetirementStep::Complete;
                };
                if !budget.try_charge(1, 0, key.len().saturating_add(value.len())) {
                    return BrowserRetirementStep::Blocked;
                }
                *current = None; // 一次只释放当前拥有键值。
                BrowserRetirementStep::Progressed
            }
            Self::StringUsizeMap { remaining, current } => {
                if current.is_none() {
                    *current = remaining.next();
                }
                let Some((key, _)) = current.as_ref() else {
                    return BrowserRetirementStep::Complete;
                };
                if !budget.try_charge_string(key) {
                    return BrowserRetirementStep::Blocked;
                }
                *current = None; // usize为Copy，释放当前键后继续后续桶。
                BrowserRetirementStep::Progressed
            }
            Self::StringU64Map { remaining, current } => {
                if current.is_none() {
                    *current = remaining.next();
                }
                let Some((key, _)) = current.as_ref() else {
                    return BrowserRetirementStep::Complete;
                };
                if !budget.try_charge_string(key) {
                    return BrowserRetirementStep::Blocked;
                }
                *current = None; // u64为Copy，当前键独立释放。
                BrowserRetirementStep::Progressed
            }
            Self::AssetPathMap { remaining, current } => {
                if current.is_none() {
                    *current = remaining.next();
                }
                let Some((_, path)) = current.as_ref() else {
                    return BrowserRetirementStep::Complete;
                };
                if !budget.try_charge_string(path) {
                    return BrowserRetirementStep::Blocked;
                }
                *current = None; // AssetId为Copy，不增加深析构成本。
                BrowserRetirementStep::Progressed
            }
            Self::LoadingQueue(values) => {
                let Some(ticket) = values.front() else {
                    return BrowserRetirementStep::Complete;
                };
                if !budget.try_charge_string(&ticket.asset_path) {
                    return BrowserRetirementStep::Blocked;
                }
                values.pop_front(); // 票据修订为Copy，只释放一个资产路径。
                BrowserRetirementStep::Progressed
            }
            Self::String(value) => {
                let Some(current) = value.as_ref() else {
                    return BrowserRetirementStep::Complete;
                };
                if !budget.try_charge_string(current) {
                    return BrowserRetirementStep::Blocked;
                }
                *value = None; // 单字符串按同一元素/字节模型释放。
                BrowserRetirementStep::Progressed
            }
        }
    }
}

/// 一次原子换代或关闭形成一代；内部工作项数量固定，不随内容规模增长。
#[derive(Default)]
pub(super) struct BrowserRetiredGeneration {
    work: VecDeque<BrowserRetiredWork>, // 前项清空后才转到下一容器，避免跨代交错失控。
}

impl BrowserRetiredGeneration {
    /// 只有非空工作才进入队列，空容器壳可立即O(1)释放。
    fn push(&mut self, work: BrowserRetiredWork, nonempty: bool) {
        if nonempty {
            self.work.push_back(work); // 每代工作项数量受资源字段数固定上限约束。
        }
    }

    pub(super) fn entries(&mut self, values: Vec<EditorGltfBrowserEntry>) {
        let nonempty = !values.is_empty();
        self.push(BrowserRetiredWork::Entries(values), nonempty);
    }

    pub(super) fn signatures(&mut self, values: Vec<PlacementSignature>) {
        let nonempty = !values.is_empty();
        self.push(
            BrowserRetiredWork::Signatures(RetiredSignatures::new(values)),
            nonempty,
        );
    }

    pub(super) fn signature_build(&mut self, value: Option<BrowserSignatureBuild>) {
        if let Some(value) = value {
            self.work
                .push_back(BrowserRetiredWork::SignatureBuild(value));
        }
    }

    pub(super) fn entries_build(&mut self, value: Option<BrowserEntriesBuild>) {
        if let Some(value) = value {
            self.work.push_back(BrowserRetiredWork::EntriesBuild(value));
        }
    }

    pub(super) fn scene_build(&mut self, value: Option<BrowserSceneBuild>) {
        if let Some(value) = value {
            self.work.push_back(BrowserRetiredWork::SceneBuild(value));
        }
    }

    pub(super) fn assets(&mut self, values: BTreeMap<String, CachedGltfAsset>) {
        let nonempty = !values.is_empty();
        self.push(
            BrowserRetiredWork::Assets(RetiredAssets::new(values)),
            nonempty,
        );
    }

    pub(super) fn cached_asset(&mut self, path: String, cached: CachedGltfAsset) {
        self.work
            .push_back(BrowserRetiredWork::Assets(RetiredAssets::single(
                path, cached,
            ))); // 单路径同样复用逐网格预算。
    }

    pub(super) fn cached_meshes(&mut self, values: Vec<CachedGltfMesh>) {
        let nonempty = !values.is_empty();
        self.push(BrowserRetiredWork::CachedMeshes(values), nonempty);
    }

    pub(super) fn selection(&mut self, value: Option<EditorGltfMeshSelection>) {
        let nonempty = value.is_some();
        self.push(BrowserRetiredWork::Selection(value), nonempty);
    }

    pub(super) fn string_set(&mut self, values: BTreeSet<String>) {
        let nonempty = !values.is_empty();
        self.push(BrowserRetiredWork::StringSet(values), nonempty);
    }

    pub(super) fn string_usize_tree(&mut self, values: BTreeMap<String, usize>) {
        let nonempty = !values.is_empty();
        self.push(BrowserRetiredWork::StringUsizeTree(values), nonempty);
    }

    pub(super) fn string_string_map(&mut self, values: HashMap<String, String>) {
        let nonempty = !values.is_empty();
        self.push(
            BrowserRetiredWork::StringStringMap {
                remaining: values.into_iter(), // O(1)取得owning桶游标。
                current: None,                 // 第一帧再读取首项。
            },
            nonempty,
        );
    }

    pub(super) fn string_usize_map(&mut self, values: HashMap<String, usize>) {
        let nonempty = !values.is_empty();
        self.push(
            BrowserRetiredWork::StringUsizeMap {
                remaining: values.into_iter(),
                current: None,
            },
            nonempty,
        );
    }

    pub(super) fn string_u64_map(&mut self, values: HashMap<String, u64>) {
        let nonempty = !values.is_empty();
        self.push(
            BrowserRetiredWork::StringU64Map {
                remaining: values.into_iter(),
                current: None,
            },
            nonempty,
        );
    }

    pub(super) fn asset_path_map(&mut self, values: HashMap<AssetId<WorldAsset>, String>) {
        let nonempty = !values.is_empty();
        self.push(
            BrowserRetiredWork::AssetPathMap {
                remaining: values.into_iter(),
                current: None,
            },
            nonempty,
        );
    }

    pub(super) fn loading_queue(&mut self, values: VecDeque<BrowserLoadingTicket>) {
        let nonempty = !values.is_empty();
        self.push(BrowserRetiredWork::LoadingQueue(values), nonempty);
    }

    pub(super) fn string(&mut self, value: Option<String>) {
        let nonempty = value.is_some();
        self.push(BrowserRetiredWork::String(value), nonempty);
    }

    pub(super) fn demand_transition(&mut self, value: Option<BrowserAssetDemandTransition>) {
        if let Some(value) = value {
            self.string_usize_tree(value.stale_paths); // 旧路径计数逐键释放。
            self.string_set(value.wanted_paths); // 新路径集合逐键释放。
        }
    }

    fn is_empty(&self) -> bool {
        self.work.is_empty() // 工作项创建时已过滤空容器。
    }

    fn advance(&mut self, budget: &mut BrowserRetirementBudget) -> BrowserRetirementStep {
        let Some(work) = self.work.front_mut() else {
            return BrowserRetirementStep::Complete;
        };
        match work.advance(budget) {
            BrowserRetirementStep::Complete => {
                self.work.pop_front(); // 当前项内部容器已经清空，析构壳为O(1)。
                BrowserRetirementStep::Progressed
            }
            other => other, // 实际元素推进或预算不足直接上报。
        }
    }
}

#[cfg(test)]
#[path = "retirement/tests.rs"]
mod tests; // 4096行、8192签名/缓存与连续换代背压均必须跨帧清理。
