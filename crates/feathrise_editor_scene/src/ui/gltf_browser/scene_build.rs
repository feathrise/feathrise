//! Scene0根发现、层级遍历和稳定路径物化的可取消分帧事务。

use std::collections::BTreeMap; // 稳定身份映射到首行，重复路径可把全部同名目标失败关闭。
use std::sync::Arc; // 遍历帧共享持久路径链，展开宽节点不复制整条祖先路径。

use bevy::asset::AssetId; // 构建身份同时冻结强句柄子资产ID。
use bevy::ecs::entity::EntityIndex; // 按实体槽位游标读取，避免每帧`skip(cursor)`重复扫描。
use bevy::gltf::GltfMeshName; // 只有原始glTF网格显式命名时loader才添加此组件。
use bevy::prelude::*; // World、Entity、Children、Name与Mesh3d只读访问。
use bevy::world_serialization::WorldAsset; // Scene0强句柄的资产类型。
use feathrise_scene_content::{
    MAX_SCENE_PLACEMENT_WATER_NAME_CHARS, MAX_SCENE_PLACEMENT_WATER_NODE_PATH_DEPTH,
}; // 稳定作者身份本身已有公共名称与层级硬门。

use super::helpers::stable_mesh_reference; // 完整路径物化后复用唯一稳定引用规则。
use super::retirement::{BrowserRetirementBudget, BrowserRetirementStep}; // 取消DFS按路径和元素预算释放。
use super::{
    CachedGltfMesh, MAX_GLTF_MESHES_PER_ASSET, MAX_GLTF_SCENE_MESH_ROWS_PER_FRAME,
    MAX_GLTF_SCENE_PATH_SEGMENT_COPIES_PER_FRAME, MAX_GLTF_SCENE_ROOT_DISCOVERY_PER_FRAME,
    MAX_GLTF_SCENE_TRAVERSAL_OPERATIONS_PER_FRAME, MAX_GLTF_WORLD_ENTITIES,
}; // 每个推进维度都有独立固定预算。

/// 一段持久Blender节点路径；宽层级的每个子帧只克隆Arc，不复制全部祖先名称。
struct ScenePathNode {
    parent: Option<Arc<ScenePathNode>>, // 根方向链表在网格输出时按固定预算反向物化。
    name: String,                       // 名称已按公共256字符硬门截断。
    depth: usize,                       // 超过公共64层后不再扩张路径链。
}

/// DFS中的一个节点及其逐子项游标。
struct SceneTraversalFrame {
    entity: Entity, // World内稳定实体身份；资产修订变化会取消整个任务。
    parent_path: Option<Arc<ScenePathNode>>, // 进入节点前的祖先链。
    resolved_path: Option<Arc<ScenePathNode>>, // 进入普通节点后包含自身的路径。
    path_names_valid: bool, // 缺名、超长或超深后代都不能形成作者引用。
    entered: bool,  // false先解析当前节点，true后每步只读取一个Child。
    next_child: usize, // 一个拥有8192子项的节点也严格跨帧推进。
    scene_root: bool, // Scene0根Name与根自身Mesh沿用既有规则，不进入Blender路径。
}

impl SceneTraversalFrame {
    fn root(entity: Entity) -> Self {
        Self {
            entity,
            parent_path: None,
            resolved_path: None,
            path_names_valid: true,
            entered: false,
            next_child: 0,
            scene_root: true,
        }
    }

    fn child(
        entity: Entity,
        parent_path: Option<Arc<ScenePathNode>>,
        path_names_valid: bool,
    ) -> Self {
        Self {
            entity,
            parent_path,
            resolved_path: None,
            path_names_valid,
            entered: false,
            next_child: 0,
            scene_root: false,
        }
    }
}

/// 一个Mesh节点正在按路径段预算物化最终拥有型缓存行。
struct PendingSceneMesh {
    cursor: Option<Arc<ScenePathNode>>, // 从叶到根逐段读取。
    reversed_path: Vec<String>,         // 至多公共64段。
    path_materialized: bool,            // true时已反转成根到叶顺序。
    path_names_valid: bool,             // 层级读取期间累计的稳定性。
    mesh_name: String,                  // 已按公共名称上限截断。
    mesh_index: Option<u32>,            // 仅作诊断，不参与后续稳定匹配。
}

/// 单个Scene0完整新修订的未发布读取状态。
pub(super) struct BrowserSceneBuild {
    /// 需求路径；折叠释放后缓存不再存在，完成结果自然拒收。
    pub(super) asset_path: String,
    /// 构建开始时的Scene0句柄身份。
    pub(super) asset_id: AssetId<WorldAsset>,
    /// 缓存内容修订；任何相关热更事件都会推进并取消旧结果。
    pub(super) content_revision: u64,
    /// 下一个待读实体槽位，不借用跨帧World迭代器。
    entity_slot_cursor: u32,
    /// 本修订允许检查的槽位前缀，超过8192明确截断。
    entity_slot_limit: u32,
    /// 根发现阶段渐进收集的实体身份。
    roots: Vec<Entity>,
    /// DFS下一根索引；根列表不在阶段切换帧整批复制到栈。
    next_root: usize,
    /// 显式DFS帧，子关系每次只推进一项。
    stack: Vec<SceneTraversalFrame>,
    /// 当前网格的路径正在按段物化。
    pending_mesh: Option<PendingSceneMesh>,
    /// 已完成且尚未发布的去重网格前缀。
    meshes: Vec<CachedGltfMesh>,
    /// 稳定路径与网格名映射到首个输出行；第二次命中会把该身份标记为歧义。
    dedupe: BTreeMap<(Vec<String>, String), usize>,
    /// DFS实际进入的节点数；循环或异常关系也不能突破8192项。
    visited_entities: usize,
    /// 槽位、DFS、层级或网格任一硬门触发。
    truncated: bool,
    /// 根发现完成后才允许读取Children层级。
    roots_discovered: bool,
    /// 完整或触及硬门后停止继续读取World。
    completed: bool,
}

impl BrowserSceneBuild {
    /// 新任务只读取World的O(1)实体槽位长度，不枚举任何实体。
    pub(super) fn new(
        asset_path: String,
        asset_id: AssetId<WorldAsset>,
        content_revision: u64,
        world: &World,
    ) -> Self {
        let slot_count = world.entities().len();
        let slot_limit = slot_count.min(MAX_GLTF_WORLD_ENTITIES as u32);
        Self {
            asset_path,
            asset_id,
            content_revision,
            entity_slot_cursor: 0,
            entity_slot_limit: slot_limit,
            roots: Vec::new(),
            next_root: 0,
            stack: Vec::new(),
            pending_mesh: None,
            meshes: Vec::with_capacity(MAX_GLTF_MESHES_PER_ASSET),
            dedupe: BTreeMap::new(),
            visited_entities: 0,
            truncated: slot_count > slot_limit,
            roots_discovered: false,
            completed: false,
        }
    }

    /// 完整结果只移动最终网格；根、去重键和DFS壳继续留在原草稿中交给有界退役。
    pub(super) fn take_result(&mut self) -> BrowserSceneBuildResult {
        debug_assert!(self.completed, "只有完整Scene0任务可以生成采用结果");
        BrowserSceneBuildResult {
            asset_path: std::mem::take(&mut self.asset_path), // 发布帧不析构完成草稿中的深索引。
            meshes: std::mem::take(&mut self.meshes), // 最终缓存一次换指针，草稿其余字段仍逐项清理。
            truncated: self.truncated,
        }
    }

    /// 热更、折叠或关闭后的Scene0任务逐项释放，Arc路径链最多公共64层并计入段预算。
    pub(super) fn retirement_step(
        &mut self,
        budget: &mut BrowserRetirementBudget,
    ) -> BrowserRetirementStep {
        if let Some(pending) = self.pending_mesh.as_ref() {
            let cursor_depth = pending.cursor.as_ref().map_or(0, |path| path.depth); // Arc链最坏深度受64层硬门限制。
            let path_bytes = pending
                .reversed_path
                .iter()
                .map(String::len)
                .sum::<usize>()
                .saturating_add(pending.mesh_name.len());
            if !budget.try_charge(
                1,
                cursor_depth.saturating_add(pending.reversed_path.len()),
                path_bytes,
            ) {
                return BrowserRetirementStep::Blocked;
            }
            self.pending_mesh = None; // 唯一pending路径和有界Arc祖先链一起释放。
            return BrowserRetirementStep::Progressed;
        }
        if let Some(mesh) = self.meshes.last() {
            if !budget.try_charge_mesh_identity(&mesh.node_path, &mesh.mesh_name) {
                return BrowserRetirementStep::Blocked;
            }
            self.meshes.pop(); // 已物化网格逐条释放。
            return BrowserRetirementStep::Progressed;
        }
        if let Some(((path, mesh_name), _)) = self.dedupe.first_key_value() {
            if !budget.try_charge_mesh_identity(path, mesh_name) {
                return BrowserRetirementStep::Blocked;
            }
            self.dedupe.pop_first(); // 去重键拥有第二份路径，必须单独计费。
            return BrowserRetirementStep::Progressed;
        }
        if let Some(frame) = self.stack.last() {
            let parent_depth = frame.parent_path.as_ref().map_or(0, |path| path.depth);
            let resolved_depth = frame.resolved_path.as_ref().map_or(0, |path| path.depth);
            let depth = parent_depth.max(resolved_depth); // 共享Arc最终释放时最坏递归不超过此深度。
            if !budget.try_charge(
                1,
                depth,
                depth.saturating_mul(MAX_SCENE_PLACEMENT_WATER_NAME_CHARS),
            ) {
                return BrowserRetirementStep::Blocked;
            }
            self.stack.pop(); // 一次只释放一个DFS帧及其有界Arc引用。
            return BrowserRetirementStep::Progressed;
        }
        if !self.roots.is_empty() {
            if !budget.try_charge(1, 0, 0) {
                return BrowserRetirementStep::Blocked;
            }
            self.roots.pop(); // Entity为Copy且固定小成本。
            return BrowserRetirementStep::Progressed;
        }
        if !self.asset_path.is_empty() {
            if !budget.try_charge_string(&self.asset_path) {
                return BrowserRetirementStep::Blocked;
            }
            self.asset_path.clear(); // 清空后结构壳析构不再释放路径内容。
            return BrowserRetirementStep::Progressed;
        }
        BrowserRetirementStep::Complete // 所有拥有型集合已为空。
    }
}

/// Scene0任务完整结束后的原子采用值。
pub(super) struct BrowserSceneBuildResult {
    pub(super) asset_path: String,
    pub(super) meshes: Vec<CachedGltfMesh>,
    pub(super) truncated: bool,
}

/// 复制至多公共名称字符上限；异常长资产名不会在一个Update制造无界字符串工作。
fn bounded_name(value: &str, missing: &str) -> (String, bool) {
    if value.is_empty() {
        return (missing.to_owned(), false);
    }
    let mut chars = value.chars();
    let bounded = chars
        .by_ref()
        .take(MAX_SCENE_PLACEMENT_WATER_NAME_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        (format!("{bounded}…"), false) // 超长名称仅供诊断，永不写回作者身份。
    } else {
        (bounded, true)
    }
}

/// 每帧按实体槽位检查固定前缀；不使用会重复扫描旧前缀的`Iterator::skip`。
fn advance_root_discovery(build: &mut BrowserSceneBuild, world: &World) {
    for _ in 0..MAX_GLTF_SCENE_ROOT_DISCOVERY_PER_FRAME {
        if build.entity_slot_cursor >= build.entity_slot_limit {
            build.roots_discovered = true;
            break;
        }
        let raw_index = build.entity_slot_cursor;
        build.entity_slot_cursor = build.entity_slot_cursor.saturating_add(1);
        let Some(index) = EntityIndex::from_raw_u32(raw_index) else {
            build.truncated = true;
            build.roots_discovered = true;
            break;
        };
        if !world.entities().is_index_spawned(index) {
            continue; // 空槽仍计入本帧固定检查预算。
        }
        let entity = world.entities().resolve_from_index(index);
        let Ok(entity_ref) = world.get_entity(entity) else {
            continue; // 同一个只读World中通常不会发生，异常槽位按缺失处理。
        };
        if !entity_ref.contains::<ChildOf>() {
            build.roots.push(entity); // 根身份小型复制，不读取Children或Name。
        }
    }
    if build.entity_slot_cursor >= build.entity_slot_limit {
        build.roots_discovered = true;
    }
}

/// 把当前pending网格推进到最终去重缓存，严格消费路径段与行双预算。
fn advance_pending_mesh(
    build: &mut BrowserSceneBuild,
    path_copies: &mut usize,
    mesh_rows: &mut usize,
) -> bool {
    let Some(pending) = build.pending_mesh.as_mut() else {
        return true;
    };
    while *path_copies < MAX_GLTF_SCENE_PATH_SEGMENT_COPIES_PER_FRAME {
        let Some(node) = pending.cursor.take() else {
            break;
        };
        pending.reversed_path.push(node.name.clone());
        pending.cursor = node.parent.clone(); // Arc克隆为O(1)，名称克隆计入预算。
        *path_copies += 1;
    }
    if pending.cursor.is_some() {
        return false; // 深路径剩余段留给下一Update。
    }
    if !pending.path_materialized {
        pending.reversed_path.reverse(); // 长度受公共64层硬门，固定小预算内只移动所有权。
        pending.path_materialized = true;
    }
    let copies_needed = pending.reversed_path.len().saturating_add(1); // 去重键路径及网格名副本。
    if path_copies.saturating_add(copies_needed) > MAX_GLTF_SCENE_PATH_SEGMENT_COPIES_PER_FRAME
        || *mesh_rows >= MAX_GLTF_SCENE_MESH_ROWS_PER_FRAME
    {
        return false; // 输出行和完整路径键都不能越过本帧剩余额度。
    }
    let pending = build
        .pending_mesh
        .take()
        .expect("上方已确认pending网格存在");
    *path_copies += copies_needed;
    let key = (pending.reversed_path.clone(), pending.mesh_name.clone());
    if let Some(existing_row) = build.dedupe.get(&key).copied() {
        if let Some(mesh) = build.meshes.get_mut(existing_row) {
            mesh.valid = false; // 同一路径和网格名会让运行时命中多个对象，UI必须在作者写入前失败关闭。
            mesh.ambiguous = true; // 可见行给出具体重命名诊断，而不是误报为普通缺名。
        }
    } else {
        let row = build.meshes.len();
        build.dedupe.insert(key, row);
        build.meshes.push(CachedGltfMesh {
            valid: stable_mesh_reference(
                &pending.reversed_path,
                &pending.mesh_name,
                pending.mesh_index,
                pending.path_names_valid,
            ),
            node_path: pending.reversed_path,
            mesh_name: pending.mesh_name,
            mesh_index: pending.mesh_index,
            ambiguous: false, // 首次命中先视为唯一，后续同身份会回写首行歧义标记。
        });
        *mesh_rows += 1;
        if build.meshes.len() >= MAX_GLTF_MESHES_PER_ASSET {
            build.truncated = true;
            build.completed = true; // 达到单资产行硬门后不再读取剩余层级。
        }
    }
    true
}

/// 推进DFS固定操作预算；路径与网格输出另受独立预算约束。
fn advance_traversal(
    build: &mut BrowserSceneBuild,
    world: &World,
    mut resolve_mesh_index: impl FnMut(&Mesh3d) -> Option<u32>,
) {
    let mut operations = 0usize;
    let mut path_copies = 0usize;
    let mut mesh_rows = 0usize;
    while operations < MAX_GLTF_SCENE_TRAVERSAL_OPERATIONS_PER_FRAME && !build.completed {
        if build.pending_mesh.is_some()
            && !advance_pending_mesh(build, &mut path_copies, &mut mesh_rows)
        {
            break; // 当前Update的路径或行额度已耗尽。
        }
        if build.completed {
            break;
        }
        if build.stack.is_empty() {
            let Some(root) = build.roots.get(build.next_root).copied() else {
                build.completed = true; // 所有发现根均完整遍历。
                break;
            };
            build.next_root = build.next_root.saturating_add(1);
            build.stack.push(SceneTraversalFrame::root(root)); // 每次只播种一个根。
        }
        let mut frame = build.stack.pop().expect("上方已保证DFS栈非空");
        if !frame.entered {
            if !frame.scene_root && path_copies >= MAX_GLTF_SCENE_PATH_SEGMENT_COPIES_PER_FRAME {
                build.stack.push(frame);
                break; // 非根节点至少复制一个有界名称，预算不足时不能先重复计入visited。
            }
            if build.visited_entities >= MAX_GLTF_WORLD_ENTITIES {
                build.truncated = true;
                build.completed = true;
                break;
            }
            operations += 1;
            build.visited_entities += 1;
            let Ok(entity_ref) = world.get_entity(frame.entity) else {
                continue; // 只读World不会变；断链节点按已消费操作跳过。
            };
            if frame.scene_root {
                frame.entered = true; // Scene0根本身不形成Blender路径或网格行。
                frame.resolved_path = frame.parent_path.clone();
                build.stack.push(frame);
                continue;
            }
            if let Some(mesh) = entity_ref.get::<Mesh3d>() {
                let original = entity_ref.get::<feathrise_scene::StaticMeshLodOriginal>()
                    .map(|source| Mesh3d(source.mesh.clone()));
                let mesh = original.as_ref().unwrap_or(mesh); // 子资产身份来自原GLB，显示LOD句柄没有稳定作者路径。
                let raw_name = entity_ref
                    .get::<GltfMeshName>()
                    .map(|name| name.0.as_str())
                    .unwrap_or_default();
                let (mesh_name, mesh_name_valid) = bounded_name(raw_name, "[未命名网格]");
                path_copies += 1; // 复制一个有界网格名也占路径/名称预算。
                build.pending_mesh = Some(PendingSceneMesh {
                    cursor: frame.parent_path,
                    reversed_path: Vec::new(),
                    path_materialized: false,
                    path_names_valid: frame.path_names_valid && mesh_name_valid,
                    mesh_name,
                    mesh_index: resolve_mesh_index(mesh),
                });
                continue; // primitive Name与子节点不进入Blender路径。
            }
            let raw_name = entity_ref
                .get::<Name>()
                .map(|name| name.as_str())
                .unwrap_or_default();
            let (node_name, name_valid) = bounded_name(raw_name, "[未命名节点]");
            path_copies += 1;
            let parent_depth = frame.parent_path.as_ref().map_or(0, |path| path.depth);
            if parent_depth < MAX_SCENE_PLACEMENT_WATER_NODE_PATH_DEPTH {
                frame.resolved_path = Some(Arc::new(ScenePathNode {
                    parent: frame.parent_path.clone(),
                    name: node_name,
                    depth: parent_depth + 1,
                }));
            } else {
                frame.resolved_path = frame.parent_path.clone(); // 超深后代继续遍历但不再增长路径内存。
                frame.path_names_valid = false;
                build.truncated = true;
            }
            frame.path_names_valid &= name_valid;
            frame.entered = true;
            build.stack.push(frame);
            continue;
        }

        operations += 1;
        let Ok(entity_ref) = world.get_entity(frame.entity) else {
            continue;
        };
        let child = entity_ref
            .get::<Children>()
            .and_then(|children| children.get(frame.next_child).copied());
        let Some(child) = child else {
            continue; // 当前节点全部子项结束，帧不再压回栈。
        };
        frame.next_child = frame.next_child.saturating_add(1);
        let child_frame =
            SceneTraversalFrame::child(child, frame.resolved_path.clone(), frame.path_names_valid);
        build.stack.push(frame); // 父帧保留下一子项游标。
        build.stack.push(child_frame); // LIFO保持原Children顺序的深度优先遍历。
    }
}

/// 每次Update推进一次Scene0任务；true只表示完整结果已经可按修订尝试采用。
pub(super) fn advance_scene_build(
    build: &mut BrowserSceneBuild,
    world: &World,
    resolve_mesh_index: impl FnMut(&Mesh3d) -> Option<u32>,
) -> bool {
    if build.completed {
        return true;
    }
    if !build.roots_discovered {
        advance_root_discovery(build, world);
    }
    if build.roots_discovered && !build.completed {
        advance_traversal(build, world, resolve_mesh_index);
    }
    build.completed
}

#[cfg(test)]
#[path = "scene_build/tests.rs"]
mod tests; // 8192实体多帧、过期结果拒收和关闭取消由浏览器级回归共同锁定。
