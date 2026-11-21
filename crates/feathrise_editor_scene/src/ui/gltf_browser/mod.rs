//! 已摆放GLB实例的Scene0节点与网格浏览器。
//!
//! 本模块只建立可供后续作者动作使用的稳定网格选择，不直接修改水域或摆放数据。

mod cache; // 展开需求驱动的Scene0强句柄、有界轮询和热更缓存。
mod demand; // 作者历史结构观察和展开实例到唯一资产路径的纯需求计算。
mod entries; // 可见场景树按固定操作/输出预算分帧构建并在完成后原子替换。
mod helpers; // Scene0遍历、稳定引用校验和可见行组合保持为无系统参数的纯逻辑。
mod retirement; // 旧活动树、签名、Scene草稿与资产缓存按固定预算分帧析构。
mod scene; // 右栏上半区的BSN静态外壳。
pub(crate) use scene::{
    SceneBaseGltfArrow, SceneBaseGltfRow, SceneBaseTerrainDetails, SceneMeshObjectsHeading,
    sync_scene_base_gltf_row, scene_display_panel,
}; // 场景显示面板与摆放实例列表各自占用独立布局。
mod scene_build; // Scene0根发现、DFS与路径物化均按固定预算分帧并原子采用。
mod signature; // 全部摆放结构签名按每帧记录预算渐进生成。
mod systems; // 固定行池、连续滚动、展开点击与样式同步系统。
mod scene_menu; // 已呈现行的稳定对象身份供右键菜单使用。
pub(crate) use scene_menu::bind_scene_menu_rows;
pub(crate) use row_controls::{PlacementTreeExpander, PlacementTreeToggle, sync as sync_gltf_row_toggles};
mod projection; // 逻辑组的有界根投影，不改变真实实例层级。
mod row_controls; // 摆放与逻辑组共享固定行池的箭头和三种开关。

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque}; // 路径缓存、展开需求与有界加载队列。

use bevy::asset::AssetId; // 热更事件按类型化Scene0资产ID直接定位当前强句柄路径。
use bevy::prelude::*; // Component、Handle、Resource与WorldAsset。
use bevy::world_serialization::WorldAsset; // 每个唯一GLB路径强持有Scene0句柄。
use feathrise_scene_content::{ScenePlacementAssetTarget, ScenePlacementWaterKind, ScenePlacementWaterSurface}; // 浏览签名缓存文件内目标及失效绑定诊断所需的小型稳定身份。

use entries::BrowserEntriesBuild; // 资源只持有可中断的分帧行构建状态。
use retirement::BrowserRetirement; // 关闭和换代只转移所有权，深容器由后续Update逐项释放。
use scene_build::BrowserSceneBuild; // 同一时刻只渐进读取一个展开Scene0。
use signature::BrowserSignatureBuild; // 资源只持有当前结构代次的分帧签名草稿。

/// 主右栏只组合本模块公开的完整上半区，不把内部BSN模块提升到UI同级。
pub(super) fn gltf_browser_panel() -> impl Scene {
    scene::gltf_browser_panel()
}
pub(crate) use cache::refresh_gltf_browser_cache; // Tasks阶段只处理展开需求和固定加载预算。
pub(crate) use helpers::{mesh_index_from_primitive_label, stable_mesh_reference}; // 视口点击复用Scene0稳定网格身份规则。
pub(crate) use systems::{
    handle_gltf_browser_input, spawn_gltf_browser_rows, sync_gltf_browser_rows,
}; // lib.rs按Input与Presentation阶段显式注册。

/// 每帧最多逐条检查的WorldAsset事件；超出时保守失效全部当前需求并O(1)清空其余reader。
pub(super) const MAX_GLTF_ASSET_EVENTS_PER_FRAME: usize = 256;
/// 每帧最多调和的展开资产需求路径数；结构换代不会在一帧加载或释放全部Scene0。
pub(super) const MAX_GLTF_ASSET_DEMAND_PATHS_PER_FRAME: usize = 32;
/// 每帧最多轮询的展开Scene0句柄数；队列轮转保证大量展开项不会饿死。
pub(super) const MAX_GLTF_ASSET_POLLS_PER_FRAME: usize = 8;
/// Scene0根发现每帧最多检查的实体槽位。
pub(super) const MAX_GLTF_SCENE_ROOT_DISCOVERY_PER_FRAME: usize = 256;
/// Scene0层级遍历每帧最多读取的节点或子关系步骤。
pub(super) const MAX_GLTF_SCENE_TRAVERSAL_OPERATIONS_PER_FRAME: usize = 256;
/// Scene0分帧遍历每帧最多完成的去重网格记录。
pub(super) const MAX_GLTF_SCENE_MESH_ROWS_PER_FRAME: usize = 64;
/// Scene0分帧遍历每帧最多复制的有界节点路径段；深层路径不会藏在单次网格输出里。
pub(super) const MAX_GLTF_SCENE_PATH_SEGMENT_COPIES_PER_FRAME: usize = 512;
/// 每帧最多复制的摆放结构记录数；剩余记录留给下一帧。
pub(super) const MAX_GLTF_SIGNATURE_PLACEMENTS_PER_FRAME: usize = 256;
/// 每帧最多复制的作者水绑定数；记录预算不能隐藏单条记录内最多64项深路径。
pub(super) const MAX_GLTF_SIGNATURE_WATER_BINDINGS_PER_FRAME: usize = 64;
/// 每帧签名构建最多复制的水绑定路径段；最深合法路径会自然独占较大份额。
pub(super) const MAX_GLTF_SIGNATURE_PATH_SEGMENTS_PER_FRAME: usize = 512;
/// 每帧签名构建最多复制的字符串字节近似量，覆盖实例ID、资产路径和水绑定身份。
pub(super) const MAX_GLTF_SIGNATURE_STRING_BYTES_PER_FRAME: usize = 128 * 1024;
/// 每帧场景树构建最多执行的根/网格/绑定步骤数，未输出匹配绑定也计入预算。
pub(super) const MAX_GLTF_ENTRY_OPERATIONS_PER_FRAME: usize = 512;
/// 每帧场景树草稿最多新增的拥有型行数，限制节点路径和网格名字符串复制峰值。
pub(super) const MAX_GLTF_ENTRY_ROWS_PER_FRAME: usize = 256;
/// 每帧可见行物化最多遍历/复制的节点路径段；深层网格不能藏在“256行”之内。
pub(super) const MAX_GLTF_ENTRY_PATH_SEGMENTS_PER_FRAME: usize = 512;
/// 每帧可见行物化最多复制的字符串字节近似预算；UTF-8名称仍受公共字符硬门保护。
pub(super) const MAX_GLTF_ENTRY_STRING_BYTES_PER_FRAME: usize = 128 * 1024;

/// 后续“把网格指定为水面”动作所需的完整稳定引用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EditorGltfMeshSelection {
    /// 当前场景摆放记录的稳定实例ID。
    pub(crate) placement_id: String,
    /// 从Scene0场景根之后开始的Blender节点Name层级。
    pub(crate) node_path: Vec<String>,
    /// Blender/glTF网格名；未命名资产保留空字符串并令引用无效。
    pub(crate) mesh_name: String,
    /// glTF内部Mesh索引；无法从primitive子资产标签恢复时为None。
    pub(crate) mesh_index: Option<u32>,
    /// false表示只能在浏览器中诊断，任何后续作者动作都必须拒绝采用。
    pub(crate) valid: bool,
    /// true表示该项来自作者数据但已无法在当前Ready GLB中匹配，只允许执行清除动作。
    pub(crate) orphaned_water_binding: bool,
}

/// 一条固定行可显示的场景树节点类型。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EditorGltfBrowserEntryKind {
    Group { name: String, collapsed: bool, count: usize, first_member: usize },
    /// 一个已摆放GLB实例根，有子网格时由独立箭头展开或收起。
    Placement {
        /// 草稿中的稳定实例ID。
        placement_id: String,
        /// 当前实例是否已折叠。
        collapsed: bool,
        /// 只有完整Scene0根允许产生展开需求；直接primitive根只承担整项选择。
        expandable: bool,
        /// 从完整投影的源文件确定图标，GLB 根和网格子行各有自己的身份。
        glb: bool,
    },
    /// 一个含Mesh3d的Blender节点/网格组合。
    Mesh,
    /// 作者数据仍有水体语义、但当前Ready GLB已删除或改名的稳定网格身份。
    MissingWaterBinding,
}

/// 右栏对crate其余作者功能公开的一条扁平场景树记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EditorGltfBrowserEntry {
    /// 行类型决定选中目标以及是否显示独立的展开箭头。
    pub(crate) kind: EditorGltfBrowserEntryKind,
    /// 已含树形前缀与必要诊断的短显示文本。
    pub(crate) display_name: String,
    /// 视觉缩进深度；primitive的自动Name从不计入深度或路径。
    pub(crate) depth: usize,
    /// 只有网格行携带稳定选择值，实例根行为None。
    pub(crate) selection: Option<EditorGltfMeshSelection>,
}

/// 一项整网格水绑定在场景树诊断中所需的有界稳定快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlacementWaterBindingSignature {
    /// Blender对象节点的完整显式名称路径。
    pub(super) node_path: Vec<String>,
    /// Blender Mesh datablock的显式名称。
    pub(super) mesh_name: String,
    /// 当前作者文件保存的非权威诊断索引。
    pub(super) mesh_index: u32,
    /// 列表直接回显原绑定类别，避免失效后只能打开JSON辨认。
    pub(super) kind: ScenePlacementWaterKind,
}

impl From<&ScenePlacementWaterSurface> for PlacementWaterBindingSignature {
    fn from(surface: &ScenePlacementWaterSurface) -> Self {
        Self {
            node_path: surface.node_path.clone(), // 失效行必须保留原完整稳定路径供作者核对。
            mesh_name: surface.mesh_name.clone(), // 清除动作按原网格名定位所属记录项。
            mesh_index: surface.mesh_index,       // 索引只随诊断行携带，不参与最终身份比较。
            kind: surface.kind,                   // 当前水体类别用于失效行短标签。
        }
    }
}

/// 草稿摆放签名保存会改变浏览树身份或失效绑定行的字段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlacementSignature {
    pub(super) editor: std::sync::Arc<feathrise_scene_content::ScenePlacementEditorMetadata>,
    /// 稳定实例ID。
    pub(super) placement_id: String,
    /// Scene0来源路径。
    pub(super) asset_path: String,
    /// 文件内目标决定根是否可展开，以及根行显示Scene0还是精确primitive。
    pub(super) asset_target: ScenePlacementAssetTarget,
    /// 当前摆放记录内全部合法整网格水绑定，单实例受公共64项上限约束。
    pub(super) water_bindings: Vec<PlacementWaterBindingSignature>,
}

impl PlacementSignature {
    fn is_embedded_in(&self, main_glb_path: Option<&str>) -> bool {
        // 内置网格尚可处于草稿。Blender 网格由本场景 GLB 中的子资产表示。
        main_glb_path.is_some_and(|path| {
            self.asset_target.is_builtin()
                || (matches!(self.asset_target, ScenePlacementAssetTarget::Primitive { .. })
                    && self.asset_path == path)
        })
    }
}

/// 浏览器对作者会话保存的O(1)确定性结构观察，用于识别真正结构变化。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct BrowserSessionObservation {
    pub(super) scene_objects_revision: u64,
    /// 当前是否已有完整草稿。
    pub(super) draft_available: bool,
    /// 当前草稿场景身份；同结构切换场景也必须清理选择与展开态。
    pub(super) draft_scene_id: Option<String>,
    /// 原始摆放记录数；新增或删除无需扫描即可先识别。
    pub(super) source_placement_count: usize,
    /// 作者会话在载入、摆放增删、资产或水绑定结构变化后显式推进的确定性代次。
    pub(super) placement_structure_revision: u64,
}

/// 一次结构换代后，旧需求释放和新需求请求都以路径为单位分帧调和。
pub(super) struct BrowserAssetDemandTransition {
    /// 上一结构代次的路径计数按键逐步检查，不再需要时释放强句柄。
    pub(super) stale_paths: BTreeMap<String, usize>,
    /// 新结构代次全部非零需求路径在签名构建途中渐进收集，随后逐步请求Scene0。
    pub(super) wanted_paths: BTreeSet<String>,
}

/// WorldAsset事件突发超过单帧预算时，按路径游标保守渐进失效当前全部缓存。
#[derive(Default)]
pub(super) struct BrowserHotReloadSweep {
    /// 上一帧已经处理的最后路径；None表示从当前BTreeMap首项开始。
    pub(super) after_path: Option<String>,
}

/// Loading公平队列的一张内容修订票；同路径旧票在热更后只能被丢弃，不能重新轮转。
pub(super) struct BrowserLoadingTicket {
    pub(super) asset_path: String,
    pub(super) content_revision: u64,
}

/// 一个Scene0内去重后的Blender节点/网格缓存模板。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CachedGltfMesh {
    /// 不含场景根和primitive自动Name的节点层级。
    pub(super) node_path: Vec<String>,
    /// 原始GltfMeshName；缺失时为空。
    pub(super) mesh_name: String,
    /// 从`MeshN/PrimitiveM`恢复的glTF网格索引。
    pub(super) mesh_index: Option<u32>,
    /// 节点和网格名称是否足以形成稳定作者引用。
    pub(super) valid: bool,
    /// 同一Scene0中是否有多个对象命中完全相同的节点路径与网格名。
    pub(super) ambiguous: bool,
}

/// 唯一GLB Scene0缓存的生命周期。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum CachedGltfAssetPhase {
    /// 新句柄、热更或移除后等待完整依赖重新就绪。
    #[default]
    Loading,
    /// Scene0已经按本次资产内容遍历并缓存。
    Ready,
    /// 资源或任一依赖加载失败；匹配的后续资产事件可重新尝试。
    Failed,
}

/// 每个唯一asset_path只保存一个强Scene0句柄和一次遍历结果。
pub(super) struct CachedGltfAsset {
    /// 强句柄保证至少一个实例展开期间Scene0不会因无实例引用被卸载。
    pub(super) handle: Handle<WorldAsset>,
    /// 只在资产成功加载或热更时重建的去重网格表。
    pub(super) meshes: Vec<CachedGltfMesh>,
    /// true表示`meshes`属于上一份完整采用结果；热更构建期间继续原子保留供UI使用。
    pub(super) active_ready: bool,
    /// 稳定帧据此直接跳过World遍历。
    pub(super) phase: CachedGltfAssetPhase,
    /// Scene0 是否遇到不可表示的实体索引或网格引用路径。
    pub(super) truncated: bool,
    /// 每次相关资产事件递增；分帧构建完成时必须仍匹配才能采用。
    pub(super) content_revision: u64,
}

/// 已摆放GLB浏览器的公开选择、可见行与私有强句柄缓存。
#[derive(Resource, Default)]
pub(crate) struct EditorGltfBrowserState {
    /// 组与GLB均默认收起，只有显式点箭头才改变当前场景的展开偏好。
    pub(super) expanded_groups: BTreeSet<String>,
    /// 主 GLB 自有折叠状态，身份与普通引用模型分开。
    pub(super) expanded_main_glb: Option<feathrise_scene_content::SceneId>,
    pub(crate) selected_group: Option<String>,
    /// 当前展开状态的完整扁平树，行池按滚动位置取其窗口。
    pub(crate) entries: Vec<EditorGltfBrowserEntry>,
    /// 与活动签名一起采用的主 GLB 文件路径。
    main_glb_path: Option<String>,
    /// 完整扁平树的内嵌分支前缀。行索引仍用于原摆放和分组操作。
    pub(super) main_glb_entry_count: usize,
    /// 最近点击的完整网格引用；无效引用仍保留以便明确拒绝和诊断。
    pub(crate) selection: Option<EditorGltfMeshSelection>,
    /// 当前选中的完整摆放实例；根行和任一子网格点击都会更新该稳定ID。
    pub(crate) selected_placement_id: Option<String>,
    /// 任一实例、Scene0或可见行触及硬上限时为true。
    pub(crate) truncated: bool,
    /// 当前加载、失败和截断摘要，供右栏状态行显示。
    pub(crate) status: String,
    /// 首次运行与真实空草稿必须可区分。
    pub(super) initialized: bool,
    /// false表示草稿仍未加载，true时零摆放才代表真实空场景。
    pub(super) draft_available: bool,
    /// 上一次已观察的场景身份、数量和显式结构代次；非结构数值变化得到同一轻量值。
    pub(super) observation: BrowserSessionObservation,
    /// 当前结构代次尚未完成的有界签名草稿；完成前不替换活动签名。
    signature_build: Option<BrowserSignatureBuild>,
    /// 只有确定性摆放结构代次变化时才扫描重建的有界身份表。
    pub(super) signature: Vec<PlacementSignature>,
    pub(super) has_object_groups: bool, // 随签名增量统计，普通旧场景无需新增全树投影阶段。
    /// 活动签名内无法稳定回写的重复实例ID，由签名分帧构建途中增量识别。
    pub(super) duplicate_placement_ids: BTreeSet<String>,
    /// 唯一实例ID到资产路径的O(1)索引；重复ID故意不进入，点击时明确拒绝。
    pub(super) placement_asset_paths: HashMap<String, String>,
    /// 唯一实例ID到当前作者记录下标；只与完整结构签名一起原子换代。
    pub(super) placement_record_indices: HashMap<String, usize>,
    /// 原始摆放数量由Vec长度O(1)取得，超过浏览硬上限仍能正确显示摘要。
    pub(super) source_placement_count: usize,
    /// 每个唯一路径的Scene0强句柄和热更缓存。
    pub(super) assets: BTreeMap<String, CachedGltfAsset>,
    /// Loading阶段路径计数；状态栏不得为摘要同步扫描最多8192项缓存。
    pub(super) loading_asset_count: usize,
    /// Failed阶段路径计数；随缓存生命周期O(1)维护。
    pub(super) failed_asset_count: usize,
    /// 触及World遍历上限的Ready路径计数；行发布时O(1)汇总截断状态。
    pub(super) truncated_asset_count: usize,
    /// 类型化Scene0资产ID到当前展开路径的反向索引，热更事件不扫描全部缓存。
    pub(super) asset_paths_by_id: HashMap<AssetId<WorldAsset>, String>,
    /// 每个路径当前唯一排队的内容修订；连续热更只允许最新修订拥有有效票据。
    pub(super) queued_revision_by_path: HashMap<String, u64>,
    /// Loading路径的公平轮转票据队列，每帧只处理固定数量。
    pub(super) loading_queue: VecDeque<BrowserLoadingTicket>,
    /// 当前展开实例按唯一资产路径汇总的引用计数；单次展开/折叠只修改一个键。
    pub(super) asset_demand_counts: BTreeMap<String, usize>,
    /// 精确网格选择最多借用一个缓存做身份校验，独立于树上分支是否展开。
    pub(super) selection_validation_path: Option<String>,
    /// UI单次展开/折叠产生的O(1)路径需求变化，由Tasks固定预算采用。
    pub(super) asset_demand_changes: BTreeSet<String>,
    /// 结构换代后的旧/新路径集合以固定预算逐项调和。
    pub(super) asset_demand_transition: Option<BrowserAssetDemandTransition>,
    /// 资产事件爆发时不丢热更正确性，也不在一帧扫描全部展开缓存。
    pub(super) hot_reload_sweep: Option<BrowserHotReloadSweep>,
    /// 已消费事件但尚未在退役背压下采用的路径；同路径连续事件只保留一个最新请求。
    pub(super) pending_asset_invalidations: BTreeSet<String>,
    /// 当前唯一Scene0分帧读取任务；折叠、热更或关闭会O(1)取消。
    scene_build: Option<BrowserSceneBuild>,
    /// 实例根默认折叠；只有显式点击展开的稳定ID进入此集合并请求Scene0。
    pub(super) expanded_placements: BTreeSet<String>,
    /// 每次签名、展开或缓存阶段变化递增；旧行任务完成后若过期则重新开始而不发布半成品。
    pub(super) entries_revision: u64,
    /// 当前已发布的行修订。等长换代也必须作废旧行上尚未释放的点击。
    pub(super) published_entries_revision: u64,
    /// 当前按固定操作和行数预算构建的场景树草稿，完成后原子替换公开entries。
    entries_build: Option<BrowserEntriesBuild>,
    /// 大型旧代拥有型数据最多保留两代，并按元素、路径段和字符串预算逐帧释放。
    retirement: BrowserRetirement,
}

impl EditorGltfBrowserState {
    /// 标签中的稳定选择必须等新场景签名完整采用，不能在分帧构建时被旧签名清掉。
    pub(crate) fn document_selection_ready(&self, map: &feathrise_scene_content::SceneId) -> bool {
        self.initialized
            && self.signature_build.is_none()
            && self.observation.draft_scene_id.as_deref() == Some(map.as_str())
    }

    /// 采用中央视口命中的直接primitive根，不制造Scene0网格选择或展开需求。
    pub(crate) fn select_primitive_placement_from_viewport(
        &mut self,
        placement_id: String,
    ) -> bool {
        let valid = !self.duplicate_placement_ids.contains(&placement_id); // 重复ID不能授权变换、复制或删除中的任一条记录。
        self.selected_group = None;
        self.selected_placement_id = Some(placement_id); // 属性区仍以完整ScenePlacementRecord作为操作目标。
        self.selection = None; // 直接primitive没有Blender对象路径，不能冒充整网格水或Scene0高亮身份。
        valid // 调用方据此显示可编辑或重复ID失败诊断。
    }

    /// 视口或标签恢复只同步网格选择；树的展开状态属于作者显式操作。
    pub(crate) fn select_mesh_from_viewport(
        &mut self,
        mut selection: EditorGltfMeshSelection,
    ) -> bool {
        selection.valid &= !self
            .duplicate_placement_ids
            .contains(&selection.placement_id); // 重复摆放ID即使世界恰好命中一根也不能授权后续作者动作。
        selection.orphaned_water_binding = false; // 世界实际命中不可能是只存在于旧作者数据的失效绑定。
        self.selected_group = None;
        let valid = selection.valid; // 移入资源前冻结调用方需要的授权结论。
        self.selected_placement_id = Some(selection.placement_id.clone()); // 收起时仍可从完整摆放根接续操作。
        self.selection = Some(selection); // 精确网格继续驱动淡青覆盖与整网格水属性。
        valid
    }

    /// 接收已经校验过发布代次的分支。改变投影和资产需求，保留作者选择。
    pub(crate) fn toggle_tree_branch(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else { return; };
        match &entry.kind {
            EditorGltfBrowserEntryKind::Group { name, .. } => {
                if !self.expanded_groups.remove(name) { self.expanded_groups.insert(name.clone()); }
            }
            EditorGltfBrowserEntryKind::Placement { placement_id, expandable: true, .. } => {
                let Some(path) = self.placement_asset_paths.get(placement_id) else { return; };
                if self.expanded_placements.remove(placement_id) {
                    if let Some(count) = self.asset_demand_counts.get_mut(path) {
                        *count = count.saturating_sub(1);
                        if *count == 0 { self.asset_demand_counts.remove(path); }
                    }
                } else {
                    self.expanded_placements.insert(placement_id.clone());
                    let count = self.asset_demand_counts.entry(path.clone()).or_default();
                    *count = count.saturating_add(1); // 同一文件的展开实例共享原Scene0强句柄。
                }
                self.asset_demand_changes.insert(path.clone()); // Tasks按原固定路径预算收拢需求。
            }
            _ => return,
        }
        entries::request_visible_entries_rebuild(self);
    }

    /// 清除中央视口拥有的摆放与精确网格选择，不改变作者手动展开的场景树。
    pub(crate) fn clear_viewport_selection(&mut self) {
        self.selected_group = None;
        self.selected_placement_id = None; // 资产变换区同帧收起。
        self.selection = None; // 精确网格覆盖由下一Cursor阶段隐藏。
    }

    /// 返回当前完整结构代次中的唯一作者记录下标；在途签名尚未发布时拒绝旧索引。
    pub(crate) fn placement_index_ready(&self, revision: u64) -> bool {
        self.observation.placement_structure_revision == revision && self.signature_build.is_none()
    }

    pub(crate) fn placement_record_index(
        &self,
        placement_id: &str,
        placement_structure_revision: u64,
    ) -> Option<usize> {
        (self.observation.placement_structure_revision == placement_structure_revision)
            .then(|| self.placement_record_indices.get(placement_id).copied())
            .flatten() // 数值拖动只推进普通作者修订，因此整个拖动期间保持稳定O(1)索引。
    }

    #[cfg(test)]
    pub(crate) fn select_indexed_test_placement(&mut self, id: &str, index: usize, revision: u64) {
        self.selected_placement_id = Some(id.into());
        self.observation.placement_structure_revision = revision;
        self.placement_record_indices.insert(id.into(), index);
    } // 无窗口交互夹具只注入已采用的唯一结构索引，签名构建本身另有预算回归。
}

/// 场景树摆放列表的滚动视区。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorGltfBrowserList;

/// 按扁平树长度占位，固定行池只挂在这一层。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorGltfBrowserListContent;

/// 场景树加载和无效引用提示文本。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorGltfBrowserStatusText;

/// 主 GLB 及其动画作为网格列表的固定前缀，复用摆放行池。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MainSceneGlbRow {
    File,
    Animation,
    Empty,
}

/// 固定场景树行槽及其当前绑定条目。
#[derive(Component)]
pub(crate) struct SceneEditorGltfBrowserRow {
    /// 固定行池槽位。
    pub(super) slot: usize,
    /// 当前扁平条目索引；隐藏槽为None。
    pub(super) entry_index: Option<usize>,
    pub(super) entries_revision: u64,
    pub(crate) main_glb: Option<MainSceneGlbRow>,
}

/// 场景树行的唯一文本子节点。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorGltfBrowserRowText;
