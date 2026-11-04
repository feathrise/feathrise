//! 建筑作者后台任务使用的身份、几何快照、BVH 与校验结果普通类型。

use std::collections::BTreeMap; // 分页只保存本批所需的有限精确点。
use std::collections::VecDeque; // Scene0幽灵按固定预算保存尚未隔离的实体队列。
use std::sync::Arc; // 联合整平任务只浅克隆当前GLB网格和不可变编辑索引。

use bevy::prelude::*; // Rect、Vec2 与 Vec3 只作为纯数学值。
use feathrise_map_content::{
    BuildingFoundationDescriptor, BuildingPresetDescriptor, BuildingWorldQuad, MapId,
    MapPlacementAssetTarget, MapPlacementHeightMode, MapPlacementRecord,
}; // 后台请求冻结公共建筑语义与稳定地图身份。
use feathrise_map_terrain::{TerrainEditingIndex, TerrainMeshInput}; // 后台整平读取真实拓扑并返回完整新网格。

use crate::authoring::HeightChange; // 联合命令沿用地形笔刷相同的源顶点Y差分。

/// 建筑幽灵与Gizmo共享的三态颜色语义。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum BuildingPreviewTone {
    #[default]
    Pending, // 尚未完成当前落点校验时使用琥珀色。
    Valid,   // 当前冻结候选已经通过全部后台规则。
    Invalid, // 索引失败或当前候选被后台规则拒绝。
}

/// 唯一建筑幽灵根的有限生命周期阶段。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum BuildingGhostPhase {
    #[default]
    Empty, // 当前没有可复用的模型根。
    Loading,          // Scene0等待WorldInstanceReady；根保持隐藏。
    PrimitiveLoading, // 静态primitive等待Mesh就绪并通过蒙皮安全门。
    Scanning,         // 完整层级正在按固定预算切换专层和材质。
    RevealPending,    // 隔离命令已经提交，下一帧才允许显示根。
    Ready,            // 稳定帧只更新根变换、显隐和少量材质句柄。
}

/// 判断所选模型是否需要替换当前幽灵根的小型稳定键。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BuildingGhostAssetKey {
    pub(super) asset_path: String, // 相对assets根的GLB或GLTF路径。
    pub(super) asset_target: MapPlacementAssetTarget, // Scene0或精确静态primitive。
}

impl BuildingGhostAssetKey {
    /// 只在所选模型真实变化时克隆路径并重建幽灵。
    pub(super) fn from_preset(preset: &BuildingPresetDescriptor) -> Self {
        Self {
            asset_path: preset.asset_path.clone(), // 根拥有稳定路径，不借用热更新目录快照。
            asset_target: preset.asset_target,     // 目标是固定小型Copy值。
        }
    }

    /// 稳定帧用借用比较避免重复分配路径字符串。
    pub(super) fn matches(&self, preset: &BuildingPresetDescriptor) -> bool {
        self.asset_path == preset.asset_path && self.asset_target == preset.asset_target // 路径或子目标变化都必须换根。
    }
}

/// 静态primitive幽灵在生成渲染实体前持有的唯一异步加载状态。
#[derive(Debug)]
pub(super) struct BuildingGhostPrimitiveLoad {
    pub(super) mesh: Handle<Mesh>,   // 强句柄等待真实网格进入主世界Assets。
    pub(super) mesh_index: u32,      // 诊断实体名保留作者选择的GLTF父网格编号。
    pub(super) primitive_index: u32, // 诊断实体名保留父网格内primitive编号。
    pub(super) reported_failure: Option<String>, // 同一失败只向作者和日志报告一次。
}

/// Scene0幽灵一次热加载代次的固定预算层级扫描。
#[derive(Debug)]
pub(super) struct BuildingGhostScan {
    pub(super) root: Entity,              // 迟到扫描不得接管后来替换的根。
    pub(super) pending: VecDeque<Entity>, // 只保存实体ID，不复制Mesh、材质或场景组件。
    pub(super) visited: usize,            // 一次性诊断记录本代实际节点数量。
    pub(super) mesh_count: usize,         // 至少一个标准材质网格才形成可见模型幽灵。
}

impl BuildingGhostScan {
    /// 从已经完整生成后代的WorldAssetRoot开始一次新扫描。
    pub(super) fn new(root: Entity) -> Self {
        Self {
            root,                            // 当前WorldInstanceReady根拥有本代扫描。
            pending: VecDeque::from([root]), // 根和全部后代统一覆盖RenderLayers。
            visited: 0,                      // 首个预算步尚未访问节点。
            mesh_count: 0,                   // 扫描中遇到标准材质网格后递增。
        }
    }
}

/// 一份建筑空间索引必须精确对应的全部作者输入身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BuildingIndexIdentity {
    pub(super) map_id: MapId,           // 禁止切图后的迟到结果污染新地图。
    pub(super) author_revision: u64,    // 任一作者修改都使点击结果失效。
    pub(super) terrain_revision: u64,   // 地形专用修订拒绝派生高度尚未落稳或迟到整平结果。
    pub(super) structure_revision: u64, // placement 增删或来源变化使数组收集游标失效。
    pub(super) catalog_revision: u64,   // 占地或门口热更后必须重建全部建筑几何。
}

/// 已有建筑连接点的世界空间索引值。
#[derive(Clone, Debug)]
pub(super) struct IndexedBuildingConnection {
    pub(super) id: String,               // 预设内稳定点 ID 用于确定性择优和诊断。
    pub(super) kind: String,             // 只有完全相同的显式类型才兼容。
    pub(super) position: Vec3,           // Y 仅用于 Gizmo；吸附首期只比较 X/Z。
    pub(super) outward: Vec3,            // 实例旋转后的水平朝外方向。
    pub(super) outward_yaw_degrees: f32, // 规范九十度世界角避免从浮点方向反推。
}

/// 从一条建筑 placement 派生的不可变世界空间项。
#[derive(Clone, Debug)]
pub(super) struct IndexedBuilding {
    pub(super) terrain_bound: bool,
    pub(super) id: String,                   // 具体非法原因使用稳定实例 ID。
    pub(super) preset_label: String,         // 保存审计使用作者可识别名称，不只报告技术 ID。
    pub(super) footprint: BuildingWorldQuad, // 实体占地用于精确 SAT 重叠。
    pub(super) entrance_clearance: BuildingWorldQuad, // 既有正门不得被新建筑堵塞。
    pub(super) vegetation_clearance: BuildingWorldQuad, // 正式植被派生使用独立可重建净空。
    pub(super) connections: Vec<IndexedBuildingConnection>, // 最多32个地面平面连接点。
    pub(super) bounds: Rect,                 // 三块几何和连接点的联合 AABB 用于 BVH 宽相位。
    pub(super) translation: [f32; 3],        // 五点地基审计核对当前保存根高度。
    pub(super) yaw_degrees: f32,             // 首期建筑只允许九十度档位。
    pub(super) scale: [f32; 3],              // 首期建筑保持单位缩放，避免地基和门口语义漂移。
    pub(super) height_mode: MapPlacementHeightMode, // 建筑使用专用五点固定高度，不走原点跟随。
    pub(super) foundation: BuildingFoundationDescriptor, // 最高地面、模型基准与坡差共享预设参数。
    pub(super) asset_matches_preset: bool,   // 模型路径或文件内目标漂移时保存必须失败关闭。
}

/// 开启连接吸附但无法形成合法候选的确定原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BuildingConnectionSnapFailure {
    IndexUnavailable,  // 当前地图或目录修订的完整 BVH 尚未采用。
    SourceHasNoPoints, // 所选预设不是可连接模块。
    NoPointInRange,    // 所有现有连接点都超过固定吸附半径。
    IncompatibleKind,  // 半径内有点，但没有与任何源点相同的显式类型。
}

impl BuildingConnectionSnapFailure {
    /// 返回点击失败时可直接显示的稳定中文原因。
    pub(super) const fn message(self) -> &'static str {
        match self {
            Self::IndexUnavailable => "当前修订的建筑连接点索引尚未就绪",
            Self::SourceHasNoPoints => "所选建筑预设没有模块连接点；请关闭连接吸附或先配置连接点",
            Self::NoPointInRange => "吸附范围内没有现有建筑连接点",
            Self::IncompatibleKind => "吸附范围内只有不兼容的连接点类型",
        }
    }
}

/// 一对已经确定的源连接点与现有目标连接点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BuildingConnectionSnap {
    pub(super) source_connection_index: usize, // 新模块预设内的源点。
    pub(super) target_entry_index: usize,      // 当前不可变 BVH 中的现有建筑。
    pub(super) target_connection_index: usize, // 现有建筑预设内的目标点。
    pub(super) target_position: Vec3,          // Gizmo 高亮无需再次查表。
    pub(super) target_outward: Vec3,           // 目标朝外方向用于高亮箭头。
}

/// 连接吸附对当前候选姿态的解析状态。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum BuildingConnectionSnapState {
    Disabled,                        // 普通网格点放完全沿用既有行为。
    Snapped(BuildingConnectionSnap), // 已平移并旋转到唯一确定兼容点。
    Failed {
        reason: BuildingConnectionSnapFailure, // 开启模式不静默退回普通点放。
        nearby_target: Option<(Vec3, Vec3)>,   // 不兼容近点仍可用红色Gizmo指出。
    },
}

/// 预览幽灵与点击请求共同消费的唯一平面姿态解析结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BuildingPlacementPose {
    pub(super) world_xz: Vec2,   // 普通网格或连接吸附后的根 X/Z。
    pub(super) yaw_degrees: f32, // 普通控制角或连接对齐后的九十度角。
    pub(super) connection: BuildingConnectionSnapState, // 决定连接点回显和点击门。
}

/// 分帧收集中的唯一最新地图快照。
pub(super) struct BuildingIndexGather {
    pub(super) identity: BuildingIndexIdentity, // 每一帧继续前重新核对完整输入身份。
    pub(super) cursor: usize,                   // 下一条待读取 placements 下标。
    pub(super) total: usize,                    // 开始时冻结数组长度，结构变化会整体换代。
    pub(super) entries: Vec<IndexedBuilding>,   // 只保存具有建筑语义的轻量派生几何。
}

/// BVH 节点；叶节点引用 `order` 中一个连续小范围。
#[derive(Clone, Debug)]
enum BuildingBvhNode {
    Leaf {
        bounds: Rect, // 该叶全部建筑联合范围。
        start: usize, // `order` 起始下标。
        end: usize,   // `order` 排他末尾下标。
    },
    Branch {
        bounds: Rect, // 两个子树联合范围。
        left: usize,  // 左子节点下标。
        right: usize, // 右子节点下标。
    },
}

impl BuildingBvhNode {
    /// 返回节点世界 AABB。
    fn bounds(&self) -> Rect {
        match *self {
            Self::Leaf { bounds, .. } | Self::Branch { bounds, .. } => bounds,
        }
    }
}

/// 后台完整构建并在主线程原子采用的不可变建筑 BVH。
#[derive(Clone, Debug, Default)]
pub(super) struct BuildingSpatialIndex {
    entries: Vec<IndexedBuilding>, // 紧凑拥有全部建筑派生项。
    order: Vec<usize>,             // BVH 叶节点引用的确定性索引排列。
    nodes: Vec<BuildingBvhNode>,   // 根节点固定为零；空索引没有节点。
    vegetation_clearances: Arc<[BuildingWorldQuad]>, // 后台构建时冻结，稳定帧只浅克隆。
    vegetation_floor_heights: Arc<[f32]>, // 与净空四边形同序的真实建筑地基层。
}

impl BuildingSpatialIndex {
    const LEAF_SIZE: usize = 8; // 小叶减少节点开销并保持候选上限明确。

    /// 从完整几何列表构建确定性中位切分 BVH。
    pub(super) fn build(entries: Vec<IndexedBuilding>) -> Result<Self, String> {
        if entries.iter().any(|entry| !finite_rect(entry.bounds)) {
            return Err("现有建筑空间范围包含非有限数值，不能建立索引".to_owned());
        }
        if entries.is_empty() {
            return Ok(Self::default()); // 空地图是完整有效索引。
        }
        let mut index = Self {
            vegetation_clearances: entries.iter().map(|e| e.vegetation_clearance).collect(),
            vegetation_floor_heights: entries
                .iter()
                .map(|e| e.translation[1] + e.foundation.model_base_y * e.scale[1])
                .collect(),
            order: (0..entries.len()).collect(), // 初始作者顺序用于相等中心的稳定回退。
            entries,
            nodes: Vec::new(),
        };
        index.build_node(0, index.order.len()); // 根节点首先占用零号位置。
        Ok(index)
    }

    /// 返回与查询 AABB 相交的少量宽相位候选下标。
    pub(super) fn query(&self, bounds: Rect) -> Vec<usize> {
        if self.nodes.is_empty() || !finite_rect(bounds) {
            return Vec::new(); // 空索引或坏候选不会产生伪命中。
        }
        let mut result = Vec::new(); // 单次后台点击拥有自己的有界候选数组。
        let mut stack = vec![0_usize]; // 二叉树深度对8192项保持很小。
        while let Some(node_index) = stack.pop() {
            let node = &self.nodes[node_index];
            if !rects_overlap(node.bounds(), bounds) {
                continue; // 宽相位不相交时跳过完整子树。
            }
            match *node {
                BuildingBvhNode::Leaf { start, end, .. } => {
                    result.extend(
                        self.order[start..end]
                            .iter()
                            .copied()
                            .filter(|entry| rects_overlap(self.entries[*entry].bounds, bounds)),
                    ); // 叶内继续用单项联合范围收紧候选。
                }
                BuildingBvhNode::Branch { left, right, .. } => {
                    stack.push(right); // 后进先出保证左树先处理，结果稳定。
                    stack.push(left);
                }
            }
        }
        result
    }

    /// 借用一个已由 `query` 返回的现有建筑项。
    pub(super) fn entry(&self, index: usize) -> Option<&IndexedBuilding> {
        self.entries.get(index) // 迟到或损坏候选只能安全返回 None。
    }

    /// 返回当前索引中的规范建筑实例数。
    pub(super) fn len(&self) -> usize {
        self.entries.len() // 状态反馈无需扫描 BVH 叶节点。
    }

    /// 按确定作者顺序借用全部建筑项，供后台完整保存审计使用。
    pub(super) fn entries(&self) -> &[IndexedBuilding] {
        &self.entries // 只在CPU任务中遍历，不暴露给稳定帧系统。
    }

    /// 净空与地基层在后台已经冻结，采用帧仅移交共享句柄。
    pub(super) fn vegetation_clearances(&self) -> (Arc<[BuildingWorldQuad]>, Arc<[f32]>) {
        (
            self.vegetation_clearances.clone(),
            self.vegetation_floor_heights.clone(),
        )
    }

    /// 递归构造一个节点并返回其在紧凑数组中的下标。
    fn build_node(&mut self, start: usize, end: usize) -> usize {
        let bounds = self.bounds_for(start, end); // 当前范围至少包含一个已校验项。
        let node_index = self.nodes.len();
        self.nodes.push(BuildingBvhNode::Leaf {
            bounds, // 递归期间使用合法占位节点。
            start,  // 占位范围与最终节点相同。
            end,    // 完成子树后可能替换为分支。
        });
        if end - start <= Self::LEAF_SIZE {
            return node_index; // 小范围直接成为叶节点。
        }
        let extent = bounds.max - bounds.min;
        let axis = usize::from(extent.y > extent.x); // 沿更长世界轴切分降低重叠。
        self.order[start..end].sort_by(|left, right| {
            let left_center = rect_center(self.entries[*left].bounds)[axis];
            let right_center = rect_center(self.entries[*right].bounds)[axis];
            left_center
                .total_cmp(&right_center)
                .then_with(|| self.entries[*left].id.cmp(&self.entries[*right].id))
        }); // 相同中心按稳定实例ID确定顺序。
        let middle = start + (end - start) / 2;
        let left = self.build_node(start, middle);
        let right = self.build_node(middle, end);
        self.nodes[node_index] = BuildingBvhNode::Branch {
            bounds, // 父范围已覆盖两个重新排列后的子范围。
            left,   // 左半连续范围节点。
            right,  // 右半连续范围节点。
        };
        node_index
    }

    /// 合并 `order` 当前连续范围的全部 AABB。
    fn bounds_for(&self, start: usize, end: usize) -> Rect {
        let mut bounds = self.entries[self.order[start]].bounds;
        for entry in &self.order[start + 1..end] {
            bounds = union_rect(bounds, self.entries[*entry].bounds);
        }
        bounds
    }
}

/// 一次后台 BVH 构建的完整返回值。
pub(super) struct BuildingIndexBuildResult {
    pub(super) identity: BuildingIndexIdentity, // 采用前必须仍是当前期望身份。
    pub(super) result: Result<BuildingIndexSnapshot, String>, // 失败保留旧地图内容但禁止新落点。
    pub(super) discarded: bool, // 后台析构的空回执永远不能成为新打开会话的索引错误。
}

/// 一次后台构建同时发布可查询索引与完整现有建筑审计。
pub(super) struct BuildingIndexSnapshot {
    pub(super) index: Arc<BuildingSpatialIndex>, // 缺页重试只共享已构建BVH，不重复排序整图。
    pub(super) audit_error: Option<String>,      // 保存前明确阻断首个占地、门口或地基问题。
    pub(super) ground_query: Option<BuildingGroundQuery>, // 未完整采样时保存继续等待，空间索引仍可供修复使用。
    pub(super) ground_content: Option<[u64; 7]>, // 根、清单、作者、浅坑和拓扑全部参与采用身份。
}

/// 一次后台有限点集的分页进度；结果只随内容换代失效，不随视角或页淘汰重采。
#[derive(Clone, Debug, Default)]
pub(in crate::authoring) struct BuildingGroundQuery {
    pub(super) content: Option<[u64; 7]>,
    pub(super) points: Vec<Vec2>, // 单栋5点、院落至多1280点、地图审计至多40960点。
    pub(super) cursor: usize,
    pub(super) heights: BTreeMap<(u32, u32), f32>,
    pub(super) waiting_page: Option<(Option<u64>, bool)>, // None、缓存第0代及显示完成门各自独立，避免等待无法恢复。
}

/// 保存入口对当前建筑空间快照的三态结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::authoring) enum BuildingSaveReadiness {
    Ready,           // 当前联合身份已完成索引和整图审计。
    Waiting(String), // 索引、校验或目录仍在推进，保存请求应保留到下一帧。
    Failed(String),  // 当前身份已经得到确定错误，任何文件写入前失败关闭。
}

/// 点击时冻结的单个建筑候选。
#[derive(Clone, Debug)]
pub(super) struct BuildingPlacementRequest {
    /// Some冻结显式对象的作者身份；旧单根清单保持None与单位源坐标。
    pub(super) terrain_frame: Option<Arc<feathrise_map_content::MapTerrainObject>>,
    pub(super) identity: BuildingIndexIdentity, // 地图、作者、结构和目录联合身份。
    pub(super) ticket: u64,                     // 同身份重复点击也不能采用旧任务。
    pub(super) preset: BuildingPresetDescriptor, // 完整公共语义小快照。
    pub(super) world_xz: Vec2,                  // 已按当前网格吸附的世界位置。
    pub(super) yaw_degrees: f32,                // 点击时冻结的九十度朝向。
    pub(super) scale: [f32; 3], // 新建为单位缩放，已有建筑移动与旋转保留原缩放快照。
    pub(super) exclude_id: Option<String>, // 后续移动已有建筑时排除自身；新建为 None。
    pub(super) operation: BuildingPlacementOperation, // 成功后按冻结意图原子新建、修改或复制。
    pub(super) level_terrain: bool, // 只有显式开启且非原地旋转时才允许修改真实地形。
    pub(super) terrain_leveling: Option<TerrainLevelingInput>, // 任务启动前冻结当前GLB、索引和命中层。
    pub(super) ground_content: Option<[u64; 7]>, // Clipmap采用前不能只核对作者修订而遗漏外部高度换源。
    pub(super) input_controls: Option<[u32; 4]>, // 点击冻结手工朝向、网格、连接吸附与整平开关。
}

/// 建筑后台任务执行联合整平所需的完整不可变地形输入。
#[derive(Clone, Debug)]
pub(super) struct TerrainLevelingInput {
    /// 多对象光标不借另一网格的三角号；真实源三角在原CPU校验槽重新定位。
    pub(super) locate_hit_triangle: bool,
    pub(super) mesh: Arc<TerrainMeshInput>, // 当前作者GLB顶点；深复制只允许发生在CPU池。
    pub(super) editing_index: Arc<TerrainEditingIndex>, // 同一拓扑的分桶、焊接组和真实邻接。
    pub(super) hit_position: [f32; 3],      // 视口射线在目标表层的三维命中。
    pub(super) hit_triangle: u32,           // 洞穴分层BFS使用的真实源三角。
    pub(super) resolution: [u32; 2],        // 计算至少一个地形单元对角线的平台外扩。
    pub(super) world_min: [f32; 2],         // 单元X/Z米制跨度来自当前清单。
    pub(super) world_max: [f32; 2],         // 与resolution共同决定有限过渡带。
    pub(super) minimum_height: f32,         // 结果不得越过GLB可编码量程。
    pub(super) maximum_height: f32,         // 平台最高点和全部抬升都必须保持在此范围。
}

/// 后台完整计算后供主线程一次换指针采用的联合整平结果。
#[derive(Clone, Debug)]
pub(super) struct TerrainLevelingOutput {
    pub(super) source_mesh_identity: usize, // 采用前与当前Arc分配身份复核，拒绝同修订异常换源。
    pub(super) mesh: Arc<TerrainMeshInput>, // 已完成全部局部Y修改的新完整网格。
    pub(super) changes: Vec<HeightChange>,  // 同一建筑命令撤销和重做所需的精确源下标差分。
    pub(super) platform_height: f32,        // 建筑模型地基基准对齐的最终平台世界Y。
}

/// 一次后台校验成功后允许执行的唯一作者事务。
#[derive(Clone, Debug)]
pub(super) enum BuildingPlacementOperation {
    Create, // 从左栏当前预设创建一条全新建筑记录。
    ChangeExisting {
        record_index: usize,        // 请求时冻结的O(1)数组槽。
        before: MapPlacementRecord, // 完整撤销起点与迟到采用身份。
        kind: BuildingChangeKind,   // 用户可见反馈区分移动和九十度旋转。
    },
    DuplicateExisting {
        record_index: usize,        // 源记录仍需在同一结构槽中存在。
        source: MapPlacementRecord, // 校验成功前不生成重叠记录、预览或历史。
    },
}

/// 已有建筑允许的两种受检变换。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BuildingChangeKind {
    Relocate,          // 在新X/Z重新执行五点地基解析。
    RotateQuarterTurn, // 原地顺时针旋转九十度并重新检查占地与门口。
    RotateSnapped,     // 手柄一次提交明确90°目标档位，保留原地旋转的完整校验和历史语义。
}

/// 已通过全部规则并可生成作者记录的落点结果。
#[derive(Clone, Debug)]
pub(super) struct ValidatedBuildingPlacement {
    pub(super) terrain_binding: Option<feathrise_map_content::MapTerrainBinding>,
    pub(super) translation: Vec3,   // Y 已按五点最高地面与模型地基基准解析。
    pub(super) minimum_ground: f32, // 诊断与测试保留最低采样高度。
    pub(super) maximum_ground: f32, // 实际地基采用的最高采样高度。
}

/// 单个校验任务的完整结果；失败也保留候选几何供红色 Gizmo 回显。
#[derive(Clone, Debug)]
pub(super) struct BuildingValidationResult {
    pub(super) request: BuildingPlacementRequest, // 采用门重新核对全部冻结输入。
    pub(super) footprint: BuildingWorldQuad,      // 与正式摆放记录使用同一变换公式。
    pub(super) entrance_clearance: BuildingWorldQuad, // 门前净空独立于实体占地。
    pub(super) outcome: Result<ValidatedBuildingPlacement, String>, // 明确合法值或具体非法原因。
    pub(super) terrain_leveling: Option<TerrainLevelingOutput>, // 失败或普通落点绝不携带地形写侧。
    pub(super) ground_query: Option<BuildingGroundQuery>, // Some仅表示分页等待，不是非法落点或可采用结果。
    pub(super) sparse_leveling: Option<SparseBuildingLevelingState>, // 受理等待与完整候选保持明确状态，不靠提示文本驱动流程。
}

#[derive(Clone, Debug)]
pub(super) enum SparseBuildingLevelingState {
    WaitingForAuthor,
    Prepared(Arc<SparseBuildingLevelingOutput>),
}

/// 候选只含单栋实际触及页；Arc回显不复制高度数组，采用时要求唯一所有权。
#[derive(Debug)]
pub(super) struct SparseBuildingLevelingOutput {
    pub(super) edit: feathrise_map_content::SparseTerrainPreparedEdit,
}

/// 任务完成时先归还稀疏作者源，再检查建筑采用身份；取消不会丢失原脏块。
pub(super) struct BuildingValidationTaskOutput {
    pub(super) result: BuildingValidationResult,
    pub(super) lease: Option<crate::authoring::sparse_terrain::SparseTerrainAuthorLease>,
}

impl From<BuildingValidationResult> for BuildingValidationTaskOutput {
    fn from(result: BuildingValidationResult) -> Self {
        Self {
            result,
            lease: None,
        }
    }
}

/// 合并两个世界 AABB。
pub(super) fn union_rect(left: Rect, right: Rect) -> Rect {
    Rect {
        min: left.min.min(right.min), // 逐轴取更小下界。
        max: left.max.max(right.max), // 逐轴取更大上界。
    }
}

/// 判断两个闭 AABB 是否可能相交；精确是否重叠仍由 SAT 决定。
fn rects_overlap(left: Rect, right: Rect) -> bool {
    left.min.x <= right.max.x
        && left.max.x >= right.min.x
        && left.min.y <= right.max.y
        && left.max.y >= right.min.y
}

/// 返回 AABB 中心。
fn rect_center(rect: Rect) -> Vec2 {
    (rect.min + rect.max) * 0.5
}

/// 防御后台排序和比较中的 NaN 或无穷。
fn finite_rect(rect: Rect) -> bool {
    rect.min.is_finite() && rect.max.is_finite() && rect.min.cmple(rect.max).all()
}

#[cfg(test)]
mod tests {
    use super::*; // BVH 回归只使用纯几何。
    use feathrise_map_content::BuildingFootprintRect; // 构造确定性世界占地。

    /// 构造一个只含单位占地和门区的索引项。
    fn entry(id: &str, x: f32) -> IndexedBuilding {
        let rect = BuildingFootprintRect {
            center: [0.0, 0.0],       // 模型根即占地中心。
            half_extents: [1.0, 1.0], // 两米见方。
        };
        let footprint = BuildingWorldQuad::from_local_rect(
            rect,          // 使用公共变换公式。
            [x, 0.0, 0.0], // 沿 X 分散测试项。
            0.0,           // 不旋转。
            [1.0; 3],      // 单位缩放。
        );
        IndexedBuilding {
            terrain_bound: false,
            id: id.to_owned(),                                   // 稳定排序回退。
            preset_label: id.to_owned(),                         // 纯BVH夹具使用同一短标签。
            footprint,                                           // 实体占地。
            entrance_clearance: footprint,                       // 测试只关心宽相位。
            vegetation_clearance: footprint,                     // 净空复制接口保持完整字段。
            connections: Vec::new(),                             // 基线BVH夹具没有模块连接点。
            bounds: footprint.aabb(),                            // 联合范围等于占地。
            translation: [x, 0.0, 0.0],                          // 测试根与四边形一致。
            yaw_degrees: 0.0,                                    // 不旋转。
            scale: [1.0; 3],                                     // 单位缩放符合首期建筑约束。
            height_mode: MapPlacementHeightMode::FixedHeight,    // 建筑地基使用固定最终Y。
            foundation: BuildingFoundationDescriptor::default(), // 审计字段在BVH测试中不参与查询。
            asset_matches_preset: true,                          // 夹具资产语义视为一致。
        }
    }

    #[test]
    fn bvh_returns_only_nearby_aabb_candidates() {
        let index = BuildingSpatialIndex::build(
            (0..32)
                .map(|value| entry(&format!("house-{value}"), value as f32 * 10.0))
                .collect(),
        )
        .expect("有限建筑应形成索引");
        let hits = index.query(Rect::from_corners(
            Vec2::new(49.0, -2.0), // 只覆盖第五项附近。
            Vec2::new(51.0, 2.0),
        ));
        assert_eq!(hits.len(), 1); // 宽相位不退化成返回整图全部项。
        assert_eq!(index.entry(hits[0]).unwrap().id, "house-5"); // 命中身份正确。
    }
}
