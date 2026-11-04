//! GLB浏览器可见树行的固定预算分帧构建与原子发布。

mod status; // 稳定态摘要与深行构建游标分离，避免主实现越过模块拆分阈值。

use std::collections::BTreeMap; // 单实例最多64项水绑定先渐进建立稳定身份索引。

use super::helpers::{asset_file_name, revalidate_selection, water_kind_label}; // 行文字和选择校验复用稳定身份规则。
use super::retirement::{BrowserRetiredGeneration, BrowserRetirementBudget, BrowserRetirementStep}; // 发布旧树和取消草稿都进入同一有界退役队列。
use super::{
    CachedGltfAssetPhase, EditorGltfBrowserEntry, EditorGltfBrowserEntryKind,
    EditorGltfBrowserState, EditorGltfMeshSelection, GLTF_BROWSER_ROW_COUNT,
    MAX_GLTF_BROWSER_CHILD_ENTRIES, MAX_GLTF_BROWSER_ENTRIES, MAX_GLTF_BROWSER_PLACEMENTS,
    MAX_GLTF_ENTRY_OPERATIONS_PER_FRAME, MAX_GLTF_ENTRY_PATH_SEGMENTS_PER_FRAME,
    MAX_GLTF_ENTRY_ROWS_PER_FRAME, MAX_GLTF_ENTRY_STRING_BYTES_PER_FRAME,
}; // 所有硬预算和公开行类型集中在模块入口。
use status::idle_status; // 行草稿原子发布后复用唯一轻量状态摘要。

/// 当前分帧构建正在处理一个实例的哪个子阶段。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BrowserEntryBuildStage {
    /// 发布实例根并决定是否读取Ready网格。
    #[default]
    Placement,
    /// 把当前实例最多64项作者水绑定按稳定身份渐进建立索引。
    WaterBindingIndex(usize),
    /// 按缓存索引逐条发布真实Scene0网格。
    Mesh(usize),
    /// 真实网格已从索引移除命中项，逐条发布剩余失效绑定。
    MissingWaterBinding,
}

/// 尚未发布的完整场景树草稿；每帧只推进固定操作和行数。
pub(super) struct BrowserEntriesBuild {
    projection: super::projection::RootProjection,
    /// 请求时的行修订；完成时与最新修订不一致则丢弃并从新状态重建。
    revision: u64,
    /// 请求时活动摆放结构代次；签名原子换代后旧任务不得采用。
    signature_revision: u64,
    /// 当前正在处理的摆放签名索引。
    placement_index: usize,
    /// 当前实例内部的根、网格或绑定阶段。
    stage: BrowserEntryBuildStage,
    /// 完成前不暴露给固定UI行池的拥有型结果。
    entries: Vec<EditorGltfBrowserEntry>,
    /// 已发布的网格与失效绑定子行数；全部placement根始终保留分页入口。
    child_entries: usize,
    /// 本次构建是否触及实例、资产或全局行硬上限。
    truncated: bool,
    /// 当前实例尚未被真实网格命中的水绑定；真实网格阶段只做O(log 64)查找。
    unmatched_water_bindings:
        BTreeMap<(Vec<String>, String), (u32, feathrise_map_content::MapPlacementWaterKind)>,
}

impl BrowserEntriesBuild {
    /// 从当前活动签名建立空行草稿，不同步扫描任何实例。
    fn new(state: &EditorGltfBrowserState) -> Self {
        Self {
            projection: super::projection::RootProjection::new(&state.object_search),
            revision: state.entries_revision, // 锁定当前最新行请求修订。
            signature_revision: state.observation.placement_structure_revision, // 锁定当前活动签名身份。
            placement_index: 0,                       // 下一次从第一个实例根开始。
            stage: BrowserEntryBuildStage::Placement, // 每个实例必须先发布根。
            entries: Vec::with_capacity(MAX_GLTF_BROWSER_ENTRIES), // 最多8192根加4096条展开子行，固定上限不随异常资产增长。
            child_entries: 0, // 根行不消耗展开预算，合法地图尾部实例始终可见。
            truncated: state.source_placement_count > MAX_GLTF_BROWSER_PLACEMENTS, // 原始实例超限立即记诊断。
            unmatched_water_bindings: BTreeMap::new(), // 每个实例重新渐进填入最多64项绑定。
        }
    }

    /// 取消或迟到的行草稿逐条清空；深选择路径不会随`Option=None`集中析构。
    pub(super) fn retirement_step(
        &mut self,
        budget: &mut BrowserRetirementBudget,
    ) -> BrowserRetirementStep {
        if let Some(entry) = self.entries.last() {
            let mut bytes = entry.display_name.len(); // 显示文本始终是行内拥有字符串。
            let mut segments = 0usize; // 根行没有节点路径。
            if let EditorGltfBrowserEntryKind::Placement { placement_id, .. } = &entry.kind {
                bytes = bytes.saturating_add(placement_id.len());
            }
            if let Some(selection) = &entry.selection {
                bytes = bytes
                    .saturating_add(selection.placement_id.len())
                    .saturating_add(selection.mesh_name.len())
                    .saturating_add(selection.node_path.iter().map(String::len).sum::<usize>());
                segments = selection.node_path.len();
            }
            if !budget.try_charge(1, segments, bytes) {
                return BrowserRetirementStep::Blocked;
            }
            self.entries.pop(); // 一次只释放一条完整草稿行。
            return BrowserRetirementStep::Progressed;
        }
        if let Some(((path, mesh_name), _)) = self.unmatched_water_bindings.first_key_value() {
            if !budget.try_charge_mesh_identity(path, mesh_name) {
                return BrowserRetirementStep::Blocked;
            }
            self.unmatched_water_bindings.pop_first(); // 单个稳定身份受公共路径硬门约束。
            return BrowserRetirementStep::Progressed;
        }
        if !budget.try_charge(1, 0, 1024) { return BrowserRetirementStep::Blocked; }
        if !self.projection.retire_one() {
            return BrowserRetirementStep::Progressed;
        }
        BrowserRetirementStep::Complete // 其余游标与标量可O(1)析构。
    }
}

/// 请求一次新的完整行构建；已有任务继续有界推进，完成时按修订拒绝过期结果。
pub(super) fn request_visible_entries_rebuild(state: &mut EditorGltfBrowserState) {
    state.entries_revision = state.entries_revision.wrapping_add(1); // 结构、展开和缓存阶段均拥有统一行身份。
    if state.entries_build.is_none() && state.signature_build.is_none() {
        state.entries_build = Some(BrowserEntriesBuild::new(state)); // 没有在途任务时立即建立O(1)空草稿。
    }
    state.status = "场景树：正在分帧构建，可见行完成后一次更新".to_owned(); // 旧活动行保持完整但输入系统会暂时拒绝点击。
}

/// 完成当前实例并清理只属于它的网格身份集合。
fn advance_to_next_placement(build: &mut BrowserEntriesBuild) {
    build.placement_index = build.placement_index.saturating_add(1); // 精确推进一个活动签名索引。
    build.stage = BrowserEntryBuildStage::Placement; // 下一实例重新从根行开始。
    debug_assert!(
        build.unmatched_water_bindings.is_empty(),
        "进入下一实例前必须已经逐项消费当前水绑定索引"
    ); // 禁止用clear把最多64条深路径集中析构。
}

/// 可见行除了操作和行数，还必须显式限制深路径遍历及字符串复制量。
#[derive(Default)]
struct EntryBuildFrameBudget {
    path_segments: usize, // 本帧已经遍历/复制的节点路径段近似数。
    string_bytes: usize,  // 本帧已经复制或释放的UTF-8字符串字节近似数。
    charged_any: bool,    // 单个合法大项可独占新鲜帧，避免预算永久饿死。
}

impl EntryBuildFrameBudget {
    /// 预算足够时原子计费；不足时调用方保持当前stage与索引供下一帧继续。
    fn try_charge(&mut self, path_segments: usize, string_bytes: usize) -> bool {
        let next_segments = self.path_segments.saturating_add(path_segments);
        let next_bytes = self.string_bytes.saturating_add(string_bytes);
        if next_segments > MAX_GLTF_ENTRY_PATH_SEGMENTS_PER_FRAME
            || next_bytes > MAX_GLTF_ENTRY_STRING_BYTES_PER_FRAME
        {
            if self.charged_any {
                return false; // 本帧已有输出时余额不足，完整行留到下一Update。
            }
            self.path_segments = MAX_GLTF_ENTRY_PATH_SEGMENTS_PER_FRAME; // 大项独占后耗尽路径额度。
            self.string_bytes = MAX_GLTF_ENTRY_STRING_BYTES_PER_FRAME; // 大项独占后耗尽字符额度。
            self.charged_any = true; // 下一条必然留到下一Update。
            return true; // 公共硬门保护的单行即使近似略超预算也不会永久卡住。
        }
        self.path_segments = next_segments; // 本帧路径段额度精确推进。
        self.string_bytes = next_bytes; // 本帧字符串额度精确推进。
        self.charged_any = true; // 后续项必须严格服从剩余额度。
        true
    }
}

/// 计算一个稳定路径当前拥有字符串的字节近似量，段数由调用方按复制份数计费。
fn path_string_bytes(path: &[String]) -> usize {
    path.iter().map(String::len).sum::<usize>() // 正式Scene缓存总长另受4096字符硬门。
}

/// 将完成且身份仍匹配的行草稿一次替换为公开活动树。
fn publish_completed_entries(
    state: &mut EditorGltfBrowserState, // 当前活动签名、缓存、选择与分页。
    build: BrowserEntriesBuild,         // 已完成且修订匹配的拥有型草稿。
) -> Result<(), BrowserEntriesBuild> {
    if !state.entries.is_empty() && !state.retirement.can_enqueue_normal() {
        return Err(build); // 旧活动树尚无退役槽时保留完整新草稿，最新修订继续合并。
    }
    let BrowserEntriesBuild {
        entries, truncated, ..
    } = build; // 身份已经由调用方复核，其余游标均为固定小标量。
    let old_entries = std::mem::replace(&mut state.entries, entries); // 新树仍一次原子换指针。
    let mut retired = BrowserRetiredGeneration::default(); // 旧根行与展开子行只移动所有权。
    retired.entries(old_entries); // 后续Update按元素/路径/字节预算逐条释放。
    state.retirement.enqueue_normal_reserved(retired); // release构建同样无条件转移旧树。
    state.truncated = truncated || state.truncated_asset_count > 0; // O(1)汇总单资产World截断。
    let old_selection = state.selection.take(); // 避免同时借用状态和选择字段。
    state.selection = old_selection.and_then(|selection| {
        revalidate_selection(state, selection, &state.duplicate_placement_ids) // 热更、释放和绑定结构换代后重新确认。
    });
    let max_page = state.entries.len().saturating_sub(1) / GLTF_BROWSER_ROW_COUNT; // 空树仍稳定在第零页。
    state.page = state.page.min(max_page); // 行数减少后不能停在空页。
    state.status = idle_status(state); // 原子发布后恢复常规资产摘要。
    Ok(()) // 新树已完整发布且旧树进入有界退役。
}

/// 以固定操作与输出预算推进当前行草稿；返回true表示本帧发布了新活动树。
pub(super) fn advance_visible_entries_build(state: &mut EditorGltfBrowserState) -> bool {
    if state.signature_build.is_some() {
        return false; // 活动签名尚未换代时不为必然过期的结构继续复制行。
    }
    if state.entries_build.is_none() {
        return false; // 稳定帧O(1)短路。
    }
    let mut build = state.entries_build.take().expect("上方已经确认行构建存在"); // 独立拥有任务以便只读状态其余字段。
    if build.signature_revision != state.observation.placement_structure_revision {
        if !state.retirement.can_enqueue_normal() {
            state.entries_build = Some(build); // 退役背压时保留唯一旧草稿，不创建第二份。
            state.status = "场景树：等待上一代缓存分帧释放，最新结构已合并".to_owned();
            return false;
        }
        let mut retired = BrowserRetiredGeneration::default();
        retired.entries_build(Some(build)); // 过期深行和水绑定索引逐项释放。
        state.retirement.enqueue_normal_reserved(retired); // 不在debug_assert内隐藏所有权转移。
        build = BrowserEntriesBuild::new(state); // 签名已经原子换代，旧草稿不读取新索引。
    }
    let mut operations = 0usize; // 根、网格和绑定检查全部计入固定预算。
    if build.revision != state.entries_revision {
        if !state.retirement.can_enqueue_normal() { state.entries_build = Some(build); return false; }
        let mut retired = BrowserRetiredGeneration::default();
        retired.entries_build(Some(build));
        state.retirement.enqueue_normal_reserved(retired);
        build = BrowserEntriesBuild::new(state);
    }
    if !build.projection.advance(state) { state.entries_build = Some(build); return false; }
    let mut rows = 0usize; // 实际字符串/路径复制另受更紧的行预算。
    let mut copy_budget = EntryBuildFrameBudget::default(); // 深路径和字符串不能藏在单行额度内。
    let mut completed = false; // 只有全部签名或全局行上限结束才可发布。
    while operations < MAX_GLTF_ENTRY_OPERATIONS_PER_FRAME && rows < MAX_GLTF_ENTRY_ROWS_PER_FRAME {
        let linear_token = super::projection::RootToken::Placement { index: build.placement_index, depth: 0 };
        let token = if build.projection.linear {
            (build.placement_index < state.signature.len()).then_some(&linear_token)
        } else { build.projection.roots.get(build.placement_index) };
        let Some(token) = token else {
            completed = true;
            break; // 已处理全部活动签名。
        };
        let (source_index, group_depth) = match token {
            super::projection::RootToken::Group { name, count, collapsed } => {
                build.entries.push(EditorGltfBrowserEntry {
                    kind: EditorGltfBrowserEntryKind::Group { name: name.clone(), count: *count, collapsed: *collapsed },
                    display_name: format!("{} {}（{}项）", if *collapsed { "[+]" } else { "[-]" }, name, count),
                    depth: 0, selection: None,
                });
                operations += 1; rows += 1; build.placement_index += 1; continue;
            }
            super::projection::RootToken::Placement { index, depth } => (*index, *depth),
        };
        let Some(placement) = state.signature.get(source_index) else { completed = true; break; };
        match build.stage {
            BrowserEntryBuildStage::Placement => {
                let expandable = placement.asset_target.is_scene0(); // 单primitive没有Scene0节点树可供展开。
                let builtin = placement.asset_target.is_builtin();
                let source_name = if matches!(placement.asset_target, feathrise_map_content::MapPlacementAssetTarget::EditableCuboid { .. } | feathrise_map_content::MapPlacementAssetTarget::EditableMesh { .. }) {
                    "网格 · 编辑网格"
                } else if builtin { "网格 · 立方体" } else { asset_file_name(&placement.asset_path) };
                let target_hint = if expandable || builtin {
                    String::new() // 历史Scene0根保持现有紧凑名称。
                } else {
                    format!(" · {}", placement.asset_target.label()) // 子资产根明确显示持久化数字身份。
                };
                let root_bytes = placement
                    .placement_id
                    .len()
                    .saturating_mul(2) // kind稳定ID与显示文本各拥有一份副本。
                    .saturating_add(source_name.len())
                    .saturating_add(target_hint.len())
                    .saturating_add(placement.editor.name.len())
                    .saturating_add(96); // 分隔符及阶段/重复诊断使用固定保守余量。
                if !copy_budget.try_charge(0, root_bytes) {
                    break; // 根ID或显示文本留到下一Update完整物化。
                }
                operations += 1; // 一个根判断消耗一个固定操作。
                rows += 1; // 根行拥有实例ID和文件名字符串。
                let collapsed =
                    !expandable || !state.expanded_placements.contains(&placement.placement_id); // 子资产固定为不可展开，Scene0默认折叠。
                let cache = expandable
                    .then(|| state.assets.get(&placement.asset_path))
                    .flatten(); // primitive根绝不借用同路径Scene0缓存或显示加载阶段。
                let phase_hint = if collapsed {
                    "" // 折叠根没有Scene0需求，不能误报加载中。
                } else {
                    match cache.map(|cache| (cache.phase, cache.active_ready)) {
                        Some((CachedGltfAssetPhase::Loading, true)) => " · 更新中",
                        Some((CachedGltfAssetPhase::Loading, false)) | None => " · 加载中",
                        Some((CachedGltfAssetPhase::Failed, true)) => " · 更新失败，沿用旧缓存",
                        Some((CachedGltfAssetPhase::Failed, false)) => " · 加载失败",
                        Some((CachedGltfAssetPhase::Ready, _)) => "",
                    }
                };
                let duplicate_hint = if state
                    .duplicate_placement_ids
                    .contains(&placement.placement_id)
                {
                    " · 重复ID" // 重复根只供诊断，输入系统拒绝展开。
                } else {
                    "" // 唯一根无需附加提示。
                };
                build.entries.push(EditorGltfBrowserEntry {
                    kind: EditorGltfBrowserEntryKind::Placement {
                        placement_id: placement.placement_id.clone(), // 点击仍使用稳定作者ID。
                        collapsed,  // 箭头直接反映本次完整草稿的展开偏好。
                        expandable, // 输入系统据此拒绝为直接primitive产生Scene0需求。
                    },
                    display_name: format!(
                        "{} · {}{target_hint}{phase_hint}{duplicate_hint}{}{}",
                        if placement.editor.name.is_empty() { &placement.placement_id } else { &placement.editor.name }, // 名称只属于这一份摆放，稳定ID仍在kind中定位。
                        source_name, // 内置形状没有GLB路径，明确显示其正式几何来源。
                        if placement.editor.hidden { " · 隐藏" } else { "" },
                        if placement.editor.locked { " · 锁定" } else { "" },
                    ),
                    depth: group_depth,
                    selection: None, // 根行只负责展开/折叠。
                });
                if !expandable || collapsed || !cache.is_some_and(|cache| cache.active_ready) {
                    advance_to_next_placement(&mut build); // 折叠、加载和失败根没有当前子行。
                } else if build.child_entries >= MAX_GLTF_BROWSER_CHILD_ENTRIES {
                    build.truncated = true; // 子行预算耗尽只折叠后续内容，绝不截掉合法尾部根。
                    advance_to_next_placement(&mut build);
                } else {
                    build.stage = BrowserEntryBuildStage::WaterBindingIndex(0); // 先按预算建立最多64项绑定索引。
                }
            }
            BrowserEntryBuildStage::WaterBindingIndex(binding_index) => {
                if build.child_entries >= MAX_GLTF_BROWSER_CHILD_ENTRIES {
                    build.truncated = true;
                    build.stage = BrowserEntryBuildStage::MissingWaterBinding; // 已复制的深绑定仍按预算逐项释放。
                    continue; // 剩余未复制绑定和真实网格无需再读取。
                }
                operations += 1; // 索引检查即使到达末尾也占一个固定操作。
                let Some(binding) = placement.water_bindings.get(binding_index) else {
                    build.stage = BrowserEntryBuildStage::Mesh(0); // 完整小索引建立后才读取网格。
                    continue;
                };
                let binding_bytes =
                    path_string_bytes(&binding.node_path).saturating_add(binding.mesh_name.len()); // 索引键拥有路径和网格名副本。
                if !copy_budget.try_charge(binding.node_path.len(), binding_bytes) {
                    break; // 当前绑定保持未复制，下一Update从同一下标继续。
                }
                build.unmatched_water_bindings.insert(
                    (binding.node_path.clone(), binding.mesh_name.clone()), // 稳定身份只复制一次进入有界索引。
                    (binding.mesh_index, binding.kind), // 诊断索引和类别为Copy小值。
                );
                build.stage = BrowserEntryBuildStage::WaterBindingIndex(
                    binding_index.saturating_add(1), // 精确推进一个作者绑定。
                );
            }
            BrowserEntryBuildStage::Mesh(mesh_index) => {
                if build.child_entries >= MAX_GLTF_BROWSER_CHILD_ENTRIES {
                    build.truncated = true;
                    build.stage = BrowserEntryBuildStage::MissingWaterBinding; // 跳过剩余网格，已建绑定索引继续有界排空。
                    continue;
                }
                operations += 1; // 即使缓存阶段已变化也计入一步。
                let Some(cache) = state
                    .assets
                    .get(&placement.asset_path)
                    .filter(|cache| cache.active_ready)
                else {
                    advance_to_next_placement(&mut build); // 过期草稿最终会按修订拒绝，此处先安全结束实例。
                    continue;
                };
                let Some(mesh) = cache.meshes.get(mesh_index) else {
                    build.stage = BrowserEntryBuildStage::MissingWaterBinding; // 索引中只剩真正失效的作者绑定。
                    continue;
                };
                let path_bytes = path_string_bytes(&mesh.node_path); // 预先O(depth)核算，depth受64硬门。
                let row_segments = mesh.node_path.len().saturating_mul(4); // 查找键、选择、join与命中键释放各遍历一份路径。
                let row_bytes = path_bytes
                    .saturating_mul(5) // 查找键、命中键、选择、join及最终display的保守总量。
                    .saturating_add(mesh.mesh_name.len().saturating_mul(4))
                    .saturating_add(placement.placement_id.len())
                    .saturating_add(96); // 类别/不可引用诊断和分隔符固定余量。
                if !copy_budget.try_charge(row_segments, row_bytes) {
                    break; // 不建立临时查找键，也不复制半条网格行。
                }
                rows += 1; // 一条网格行复制完整节点路径和网格名。
                let binding = build
                    .unmatched_water_bindings
                    .remove(&(mesh.node_path.clone(), mesh.mesh_name.clone())); // 命中项移除后不会参与失效行。
                let valid = mesh.valid
                    && !state
                        .duplicate_placement_ids
                        .contains(&placement.placement_id); // 重复ID无法稳定回写。
                let selection = EditorGltfMeshSelection {
                    placement_id: placement.placement_id.clone(), // 绑定当前实例而非共享路径。
                    node_path: mesh.node_path.clone(),            // 保存完整Blender对象层级。
                    mesh_name: mesh.mesh_name.clone(),            // 缺名保持空并由valid拒绝。
                    mesh_index: mesh.mesh_index, // 当前导出索引仅作诊断，不参与稳定匹配或候选筛选。
                    valid,                       // 显式名称与唯一实例ID稳定后才允许写回作者数据。
                    orphaned_water_binding: false, // 当前行来自真实Ready网格。
                };
                let path = if mesh.node_path.is_empty() {
                    "[缺少节点路径]".to_owned() // 占位只供诊断。
                } else {
                    mesh.node_path.join("/") // 显示完整Blender层级。
                };
                let mesh_name = if mesh.mesh_name.is_empty() {
                    "[未命名网格]" // 不把占位写回稳定选择。
                } else {
                    mesh.mesh_name.as_str() // 复用缓存名称显示。
                };
                let binding_hint = if mesh.ambiguous {
                    " · 路径/网格名歧义，请在Blender重命名对象".to_owned()
                } else {
                    match (binding, valid) {
                        (Some((_, kind)), true) => format!(" · {}", water_kind_label(kind)), // 有效已赋水网格回显类别。
                        (Some((_, kind)), false) => {
                            format!(" · {}绑定不可用", water_kind_label(kind)) // 无效身份仍说明既有绑定。
                        }
                        (None, true) => String::new(), // 普通可引用网格保持紧凑。
                        (None, false) => " · 不可引用".to_owned(), // 缺名或重复ID明确诊断。
                    }
                };
                build.entries.push(EditorGltfBrowserEntry {
                    kind: EditorGltfBrowserEntryKind::Mesh, // 点击后保存完整选择。
                    display_name: format!("{path} · {mesh_name}{binding_hint}"), // 路径、网格名和水类别同显。
                    depth: group_depth + mesh.node_path.len().clamp(1, 5), // 组内网格保留逻辑目录缩进。
                    selection: Some(selection),              // 无效网格也能选中查看诊断。
                });
                build.child_entries += 1; // 只有网格/失效绑定消耗全树4096条展开预算。
                build.stage = BrowserEntryBuildStage::Mesh(mesh_index.saturating_add(1)); // 下一帧预算继续同一实例。
            }
            BrowserEntryBuildStage::MissingWaterBinding => {
                operations += 1; // 取下一失效绑定或确认索引为空均占一个固定操作。
                let Some(((node_path, mesh_name), _)) =
                    build.unmatched_water_bindings.first_key_value()
                else {
                    advance_to_next_placement(&mut build); // 当前实例全部绑定已经检查。
                    continue;
                };
                let path_bytes = path_string_bytes(node_path); // 正式作者绑定总长受4096字符硬门。
                let output_row = build.child_entries < MAX_GLTF_BROWSER_CHILD_ENTRIES; // 额度耗尽后仍逐项释放已建索引，但不截掉后续根。
                let row_segments = node_path.len().saturating_mul(3); // 旧键释放、选择clone与join各遍历一份。
                let row_bytes = path_bytes
                    .saturating_mul(4) // 旧键、选择、join及display的保守总量。
                    .saturating_add(mesh_name.len().saturating_mul(3))
                    .saturating_add(placement.placement_id.len())
                    .saturating_add(128); // 失效前缀、类别和分隔符固定余量。
                if !copy_budget.try_charge(row_segments, row_bytes) {
                    break; // 当前BTree首项保持完整，下一Update继续。
                }
                rows += 1; // 只有真实失效绑定才复制一条诊断行。
                let ((node_path, mesh_name), (mesh_index, kind)) = build
                    .unmatched_water_bindings
                    .pop_first()
                    .expect("上方已确认首个失效绑定存在"); // 移出后本实例索引单调收敛。
                if !output_row {
                    build.truncated = true;
                    continue; // 当前深身份已经服从路径/字节预算释放，下一项继续渐进排空。
                }
                let path = if node_path.is_empty() {
                    "[缺少节点路径]".to_owned() // 非法旧数据仍能定位并清除。
                } else {
                    node_path.join("/") // 使用作者保存的原路径核对Blender重命名。
                };
                build.entries.push(EditorGltfBrowserEntry {
                    kind: EditorGltfBrowserEntryKind::MissingWaterBinding, // 专用危险行不是当前真实网格。
                    display_name: format!(
                        "[失效水绑定] {path} · {} · {}",
                        mesh_name,              // 原Mesh datablock名。
                        water_kind_label(kind), // 原绑定类别。
                    ),
                    depth: group_depth + node_path.len().clamp(1, 5),
                    selection: Some(EditorGltfMeshSelection {
                        placement_id: placement.placement_id.clone(), // 清除动作定位所属实例。
                        node_path, // 原路径所有权直接移入选择，不额外复制第三份Vec。
                        mesh_name, // 原网格名所有权直接移入选择。
                        mesh_index: Some(mesh_index), // 旧索引只作诊断。
                        valid: false, // 任何赋水或数值动作必须拒绝。
                        orphaned_water_binding: true, // 属性区只开放取消水体。
                    }),
                });
                build.child_entries += 1; // 失效绑定与真实网格共享同一展开子行硬门。
            }
        }
    }

    if completed {
        if build.revision == state.entries_revision
            && build.signature_revision == state.observation.placement_structure_revision
        {
            match publish_completed_entries(state, build) {
                Ok(()) => return true, // 身份仍匹配才一次发布完整树。
                Err(build) => {
                    state.entries_build = Some(build); // 退役槽释放后原草稿原子重试，不重建第二份。
                    state.status = "场景树：新树已完成，等待上一代分帧释放".to_owned();
                    return false;
                }
            }
        }
        if !state.retirement.can_enqueue_normal() {
            state.entries_build = Some(build); // 迟到完整草稿继续占唯一槽并合并最新修订。
            state.status = "场景树：等待上一代分帧释放，最新行请求已合并".to_owned();
            return false;
        }
        let mut retired = BrowserRetiredGeneration::default();
        retired.entries_build(Some(build)); // 不直接Drop最多12288条迟到行。
        state.retirement.enqueue_normal_reserved(retired); // 迟到深草稿逐条释放。
        state.entries_build = Some(BrowserEntriesBuild::new(state)); // 退役已接管旧草稿，再从最新状态重建。
    } else {
        state.status = format!(
            "场景树：正在分帧构建 {}/{} 实例 · 已生成{}行{}",
            build.placement_index, // 已完整经过的实例前缀。
            state.signature.len(), // 当前活动有界签名总数。
            build.entries.len(),   // 未发布草稿行进度。
            if build.revision != state.entries_revision {
                " · 有更新排队" // 缓存或展开在途变化会让本结果完成后重启。
            } else {
                "" // 当前草稿仍是最新请求。
            },
        );
        state.entries_build = Some(build); // 保存完整游标供下一Update继续。
    }
    false // 本帧尚未发布新活动树。
}

#[cfg(test)]
#[path = "entries/tests.rs"]
mod tests; // 最坏折叠根与单GLB多网格均不得突破每帧操作/行预算。
