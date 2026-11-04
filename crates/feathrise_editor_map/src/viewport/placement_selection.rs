//! 中央视口已摆放Scene0网格的精确高亮、紧凑身份徽标与双击定位。
//!
//! 【功能设计】右栏选择继续以`MapId + placement_id + node_path + mesh_name`作为稳定身份；
//! 单击只显示当前活动World实例中全部匹配primitive的F12专属覆盖，双击才调整现有相机。
//! 层级解析按固定操作预算跨帧推进，稳定帧只验证缓存根和有限primitive，不扫描全世界或Mesh顶点。

use bevy::camera::primitives::Aabb; // glTF加载器已经为每个primitive提供局部包围盒。
use bevy::prelude::*; // Resource、Handle、GlobalTransform与三维数学类型。
use bevy::world_serialization::InstanceId; // World热重载通过不可伪造实例票据精确失效旧解析。
use feathrise_map_content::MapId; // 同名摆放ID只能在所属地图内构成同一选择。

use super::camera_math::{MAX_EXTENT_METERS, MIN_EXTENT_METERS}; // 双击量程服从既有CAD导航边界。
use super::content_selection::{LayerCandidateState, LayerTarget}; // 统一候选悬停只覆盖表现，不改场景树选择。
use crate::authoring::EditorAuthoringSession; // 当前草稿地图身份和精确摆放预览根属于作者权威。
use crate::resources::MapEditorState; // 关闭F12时选择表现必须完整隐藏。
use crate::types::EditorTool; // 编辑模式继续使用场景树正式网格选择。
use crate::ui::gltf_browser::{EditorGltfBrowserState, EditorGltfMeshSelection}; // Outliner公开完整稳定网格选择。

mod systems; // 活动根选择、固定预算层级解析、覆盖实体同步与相机定位。
mod ui; // 中央视口左上角第二枚紧凑身份徽标。

pub(crate) use systems::{
    draw_selected_placement_highlight_frame, resolve_selected_placement_mesh,
    sync_selected_placement_viewport,
}; // 插件在Cursor与Presentation阶段分别注册解析、表现和范围框。
pub(crate) use ui::setup_selected_placement_badge; // ViewportNode出现时只创建一次固定徽标实体。

const DOUBLE_CLICK_MAX_SECONDS: f64 = 0.36; // 与静水列表保持同一常见双击节奏。
const FOCUS_PADDING_SCALE: f32 = 1.25; // 三维物体定位后四周保留空间关系。
const FOCUS_PADDING_METERS: f32 = 2.0; // 小道具也至少保留两米观察边距。
pub(super) const LOOKUP_OPERATIONS_PER_FRAME: usize = 256; // DFS节点或父链路径段都占一次固定操作。
pub(super) const MAX_LOOKUP_DESCENDANTS: usize = 8_192; // 与GLB浏览器单Scene0实体硬上限一致。
pub(super) const MAX_SELECTED_PRIMITIVES: usize = 64; // 单次精确覆盖限制材质primitive数量，超额不发布半套高亮。
pub(super) const MAX_LOOKUP_PATH_DEPTH: usize = 64; // 与可持久网格水节点路径深度硬门一致。

/// 地图、摆放实例与Blender网格稳定路径共同构成点击、解析和定位身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SelectedPlacementMeshIdentity {
    pub(super) map_id: MapId,        // 切图后同名实例不得继承旧双击或实体缓存。
    pub(super) placement_id: String, // 当前placements记录的稳定实例ID。
    pub(super) whole_instance: bool, // 统一候选选整实例；右栏网格保持精确路径。
    pub(super) node_path: Vec<String>, // Scene0根之后的显式Blender对象路径。
    pub(super) mesh_name: String,    // GltfMeshName与路径共同消除共享网格歧义。
}

impl SelectedPlacementMeshIdentity {
    /// 从当前草稿与Outliner选择冻结一份稳定世界解析身份。
    fn from_selection(map_id: &MapId, selection: &EditorGltfMeshSelection) -> Self {
        Self {
            whole_instance: false,                        // 场景树明确选择只覆盖该网格。
            map_id: map_id.clone(),                       // MapId参与跨图失败关闭。
            placement_id: selection.placement_id.clone(), // 行池换页后仍保留完整实例身份。
            node_path: selection.node_path.clone(),       // 热更索引漂移不影响显式路径。
            mesh_name: selection.mesh_name.clone(),       // 不使用诊断性质的mesh_index。
        }
    }
}

/// 当前Outliner选择用于徽标与运行时解析的小型快照。
#[derive(Clone, Debug)]
pub(super) struct SelectedPlacementMeshInfo {
    pub(super) identity: SelectedPlacementMeshIdentity, // 世界根、primitive和双击共用稳定身份。
    pub(super) valid: bool,                             // false时只显示诊断，不解析或伪造覆盖。
    pub(super) orphaned: bool,                          // 已删除或改名的水绑定只能清除作者记录。
}

/// 双击时间窗只记录最近一次真实有效网格行按下。
#[derive(Clone, Debug)]
struct TimedPlacementMeshClick {
    identity: SelectedPlacementMeshIdentity, // 路径任一部分不同都不能组成双击。
    seconds: f64,                            // F12暂停虚拟时间时仍使用真实时间。
}

/// 当前网格解析在用户可见层面的有限阶段。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum PlacementMeshResolution {
    #[default]
    WaitingForRoot, // 当前作者活动摆放根尚未出现。
    WaitingForWorld, // 根已存在但World实例仍在加载或换代。
    Resolving,       // 按固定预算遍历当前实例层级。
    Ready,           // 全部匹配primitive已经原子发布到缓存。
    Missing,         // 当前完整实例中没有稳定身份对应的primitive。
    Invalid,         // Outliner明确判定歧义、缺名或失效绑定。
    Failed(String),  // 超预算或损坏层级不发布部分高亮。
}

/// 固定预算DFS的一层父节点游标。
#[derive(Clone, Copy, Debug)]
pub(super) struct PlacementHierarchyFrame {
    pub(super) parent: Entity,    // 下一次读取Children的父实体。
    pub(super) next_child: usize, // 已完成前缀，后续帧不会重复访问。
}

/// 命中同名GltfMesh后逐段向根恢复对象路径的跨帧游标。
#[derive(Debug)]
pub(super) struct PendingPlacementCandidatePath {
    pub(super) entity: Entity,        // 最终可能成为精确覆盖源的primitive。
    pub(super) object_node: Entity,   // 同一对象的多primitive允许共同命中。
    pub(super) current: Entity,       // 下一次读取Name与ChildOf的对象祖先。
    pub(super) reversed: Vec<String>, // 每帧最多复制一个受深度硬门约束的路径段。
    pub(super) path_chars: usize,     // 同一候选累计字符数受正式作者路径硬门约束。
}

/// 一次当前World实例内的有界网格身份解析。
#[derive(Debug)]
pub(super) struct PlacementMeshLookup {
    pub(super) identity: SelectedPlacementMeshIdentity, // 完成时必须仍等于公开选择。
    pub(super) root: Entity,                            // 当前活动placement根。
    pub(super) instance_id: Option<InstanceId>,         // 热重载换票立即拒收旧游标。
    pub(super) author_preview: bool,                    // 徽标区分未保存精确预览和正式实例。
    pub(super) frames: Vec<PlacementHierarchyFrame>,    // DFS深度最多受实体硬门限制。
    pub(super) pending_path: Option<PendingPlacementCandidatePath>, // 同名primitive父链逐段恢复。
    pub(super) matches: Vec<Entity>,                    // 完成前只写私有草稿，不发布半套覆盖。
    pub(super) matched_object_node: Option<Entity>,     // 同路径不同对象视为运行时歧义。
    pub(super) descendants_seen: usize,                 // 超过Scene0硬预算时明确失败。
}

impl PlacementMeshLookup {
    /// 为一个已经Ready的当前World实例建立固定预算DFS游标。
    pub(super) fn new(
        identity: SelectedPlacementMeshIdentity,
        root: Entity,
        instance_id: Option<InstanceId>,
        author_preview: bool,
    ) -> Self {
        Self {
            identity,       // 冻结本次完整稳定选择。
            root,           // DFS从实际WorldAssetRoot开始。
            instance_id,    // 根原地热更也会创建新票据。
            author_preview, // 最终Ready缓存继承该来源语义。
            frames: vec![PlacementHierarchyFrame {
                parent: root,  // 第一预算操作读取World根直接子节点。
                next_child: 0, // 尚未访问任何子项。
            }],
            pending_path: None,        // 尚未遇到同名primitive。
            matches: Vec::new(),       // 全部完成前不发布源实体。
            matched_object_node: None, // 第一项完整匹配后再冻结对象身份。
            descendants_seen: 0,       // World根本身不计后代额度。
        }
    }
}

/// 一组局部Aabb经过完整GlobalTransform后的世界轴对齐范围。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PlacementSelectionFrame {
    pub(super) center: Vec3, // 全部匹配primitive联合世界中心。
    pub(super) size: Vec3,   // 联合世界轴对齐完整尺寸。
}

/// 一个primitive的世界轴对齐极值。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PlacementWorldBounds {
    minimum: Vec3, // 经过包含潜在剪切的Affine3A后逐轴最小值。
    maximum: Vec3, // 经过包含潜在剪切的Affine3A后逐轴最大值。
}

impl PlacementWorldBounds {
    /// 合并另一个primitive范围，生成完整选中网格联合边界。
    pub(super) fn merge(self, other: Self) -> Self {
        Self {
            minimum: self.minimum.min(other.minimum), // 每轴保留更小世界极值。
            maximum: self.maximum.max(other.maximum), // 每轴保留更大世界极值。
        }
    }

    /// 转为相机和Gizmo共用的中心与完整尺寸。
    pub(super) fn frame(self) -> PlacementSelectionFrame {
        PlacementSelectionFrame {
            center: (self.minimum + self.maximum) * 0.5, // 联合范围几何中心。
            size: self.maximum - self.minimum,           // 不伪造局部旋转或缩放。
        }
    }
}

/// 当前Scene0网格选择在中央视口中的有限缓存与交互请求。
#[derive(Resource, Default)]
pub(crate) struct SelectedPlacementViewportState {
    pub(super) previewing: bool, // 候选悬停期间暂停正式定位，并保留原点击序列。
    last_click: Option<TimedPlacementMeshClick>, // 只为下一次同身份按下保留一个时间戳。
    pub(super) focus_request: Option<SelectedPlacementMeshIdentity>, // 双击后等待完整世界范围。
    explicit_focus: bool, // 显式取景采用正式选择；双击继续在临时悬停时暂停。
    pub(super) resolved_identity: Option<SelectedPlacementMeshIdentity>, // 当前缓存对应的稳定选择。
    pub(super) root_entity: Option<Entity>, // 稳定帧O(1)复核当前活动摆放根。
    pub(super) instance_id: Option<InstanceId>, // World换票立即失效primitive缓存。
    pub(super) source_is_author_preview: bool, // 文案明确区分未保存实例。
    pub(super) lookup: Option<PlacementMeshLookup>, // 每帧只推进固定层级操作数。
    pub(super) source_entities: Vec<Entity>, // 最多64个完整匹配primitive。
    pub(super) resolution: PlacementMeshResolution, // 徽标和定位请求共享同一完成结论。
    pub(super) highlight_entities: Vec<Entity>, // 覆盖实体池按源primitive槽位复用。
    pub(super) highlight_material: Option<Handle<StandardMaterial>>, // 全部摆放网格共用唯一淡青材质。
    pub(super) frame: Option<PlacementSelectionFrame>, // 铜金框和双击定位读取同一联合范围。
    pub(super) badge_text: Option<String>,             // 文案不变时不触发Text重排。
}

impl SelectedPlacementViewportState {
    /// 快捷键使用已确认选择；候选悬停只是视觉预览，不能成为导航目标。
    pub(crate) fn request_selected_focus(&mut self, editor: &MapEditorState, browser: &EditorGltfBrowserState,
        session: &EditorAuthoringSession, candidates: &LayerCandidateState) -> bool {
        let Some(info) = selected_placement_mesh_info(editor, browser, session, candidates, true) else { return false; };
        if !info.valid || info.orphaned { return false; }
        self.focus_request = Some(info.identity);
        self.explicit_focus = true;
        true
    }

    fn explicit_focus_pending(&self) -> bool { self.explicit_focus && self.focus_request.is_some() }

    pub(crate) fn cancel_focus(&mut self) { self.clear_click_sequence(); }

    /// 记录一次有效网格行按下；只有同一稳定身份在真实时间窗内再次按下才登记定位。
    pub(crate) fn register_mesh_click(
        &mut self,
        map_id: &MapId,
        selection: &EditorGltfMeshSelection,
        now_seconds: f64,
    ) -> bool {
        let identity = SelectedPlacementMeshIdentity::from_selection(map_id, selection); // 冻结完整行身份。
        self.focus_request = None; // 新按下先取消尚未采用的旧定位请求。
        self.explicit_focus = false; // 新鼠标点击恢复双击的原有预览语义。
        if !now_seconds.is_finite() {
            self.last_click = None; // 异常时钟值不得制造永久可匹配点击。
            return false; // 非有限真实时间明确按普通单击处理。
        }
        let double_clicked = self.last_click.as_ref().is_some_and(|last| {
            last.identity == identity // 地图、实例、路径和网格名必须完全一致。
                && now_seconds >= last.seconds // 真实时间倒退时拒绝旧点击。
                && now_seconds - last.seconds <= DOUBLE_CLICK_MAX_SECONDS // 固定窗口内才属于双击。
        });
        if double_clicked {
            self.focus_request = Some(identity); // Cursor阶段等待当前活动实例完整范围。
            self.last_click = None; // 三连击第三下重新作为下一组起点。
        } else {
            self.last_click = Some(TimedPlacementMeshClick {
                identity,             // 保存本次完整稳定身份。
                seconds: now_seconds, // 保存不受玩法暂停影响的真实时钟。
            });
        }
        double_clicked // 输入系统只在true时回显“正在定位”。
    }

    /// 分页、实例根或无效行介入时终止未完成双击及迟到定位请求。
    pub(crate) fn clear_click_sequence(&mut self) {
        self.last_click = None; // 后续网格按下必须重新开始时间窗。
        self.focus_request = None; // 用户已经切换交互上下文，不再抢回相机。
        self.explicit_focus = false;
    }

    /// 选择消失或改变时撤销旧世界解析，但保留可复用覆盖实体和材质。
    pub(super) fn reset_resolution(
        &mut self,
        identity: Option<SelectedPlacementMeshIdentity>,
        resolution: PlacementMeshResolution,
    ) {
        if !self.previewing && self.focus_request.as_ref() != identity.as_ref() {
            self.focus_request = None; // 不同目标不能穿过选择换代迟到移动相机。
        }
        self.resolved_identity = identity; // 下一次解析只接受当前稳定目标。
        self.root_entity = None; // 旧活动根不再提供空间信用。
        self.instance_id = None; // 旧World票据完整失效。
        self.source_is_author_preview = false; // 新根采用前不冒充作者预览。
        self.lookup = None; // 丢弃轻量跨帧游标，不发布其中半成品。
        self.source_entities.clear(); // 覆盖同步系统下一步隐藏旧槽。
        self.resolution = resolution; // 徽标立即显示当前明确阶段。
        self.frame = None; // Gizmo与相机不得继续读取旧范围。
    }
}

/// 从当前草稿和Outliner建立选择快照；纯读取不修改dirty、撤销或作者修订。
pub(super) fn selected_placement_mesh_info(
    editor: &MapEditorState,
    browser: &EditorGltfBrowserState,
    session: &EditorAuthoringSession,
    candidates: &LayerCandidateState, // 只借用当前候选身份，避免克隆整个浏览器或地图。
    ignore_hover: bool, // 显式取景期间优先恢复已确认选择，不导航到临时悬停对象。
) -> Option<SelectedPlacementMeshInfo> {
    if !editor.open {
        return None; // 关闭F12时不保留可见网格选择表现。
    }
    let draft = session.draft.as_ref()?; // 没有当前草稿时不能把旧选择映射到玩法世界。
    // 统一候选覆盖整个实例；右栏明确选mesh后候选已清空，仍使用原精确绑定。
    let whole = if ignore_hover {
        if editor.tool == EditorTool::Inspect && candidates.has_viewport_selection() {
            match candidates.selected.as_ref() { Some(LayerTarget::Placement(id)) => Some(id.as_str()), _ => return None }
        } else { (matches!(editor.tool, EditorTool::Inspect | EditorTool::PlaceAsset) && browser.selection.is_none())
            .then_some(browser.selected_placement_id.as_deref()).flatten() }
    } else { displayed_whole_placement(editor.tool, browser, candidates) };
    if let Some(id) = whole {
        return Some(SelectedPlacementMeshInfo {
            identity: SelectedPlacementMeshIdentity {
                map_id: draft.map_id.clone(), // 不跨地图复用实例。
                placement_id: id.to_owned(),  // 冻结唯一活动实例ID。
                whole_instance: true,         // 与精确网格缓存隔离。
                node_path: Vec::new(),        // 整实例不伪造路径。
                mesh_name: String::new(),     // 不限制部件名称。
            },
            valid: true,     // 后续活动根继续验证。
            orphaned: false, // 不冒充失效水绑定。
        });
    }
    let selection = if ignore_hover { browser.selection.as_ref() }
        else { displayed_mesh_selection(editor.tool, browser.selection.as_ref(), candidates) }?;
    Some(SelectedPlacementMeshInfo {
        identity: SelectedPlacementMeshIdentity::from_selection(&draft.map_id, selection), // 冻结跨帧身份。
        valid: selection.valid, // 缺名、重复ID和歧义行只显示失败诊断。
        orphaned: selection.orphaned_water_binding, // 失效水绑定只能由属性区清除。
    })
}

/// 场景树重复点根会清空候选，但整实例选择仍由浏览器稳定ID保留。
fn displayed_whole_placement<'a>(
    tool: EditorTool,                    // 仅选择模式使用统一候选。
    browser: &'a EditorGltfBrowserState, // 根选择不要求伪造mesh绑定。
    candidates: &'a LayerCandidateState, // 悬停只临时接管视觉。
) -> Option<&'a str> {
    if tool != EditorTool::Inspect {
        return (tool == EditorTool::PlaceAsset && browser.selection.is_none())
            .then_some(browser.selected_placement_id.as_deref()).flatten(); // 摆放工具的根选择也能按整实例聚焦。
    }
    if let Some(target) = candidates.hover.as_ref() {
        return match target {
            LayerTarget::Placement(id) => Some(id), // 临时整实例。
            _ => None,                              // 跨类型悬停不回退旧资产根。
        };
    }
    if candidates.has_viewport_selection()
        && let Some(LayerTarget::Placement(id)) = candidates.selected.as_ref()
    {
        return Some(id); // 地图候选始终选择整实例。
    }
    browser
        .selection
        .is_none()
        .then_some(browser.selected_placement_id.as_deref())
        .flatten() // 明确子网格才退出整实例。
}
/// 悬停其他类别时隐藏旧资产覆盖，悬停资产时只读取其真实命中网格。
fn displayed_mesh_selection<'a>(
    tool: EditorTool,                              // 只在选择模式采用统一候选预览。
    selected: Option<&'a EditorGltfMeshSelection>, // 正式场景树选择始终保持只读。
    candidates: &'a LayerCandidateState,           // 候选命中缓存提供受限的小型网格身份。
) -> Option<&'a EditorGltfMeshSelection> {
    if tool == EditorTool::Inspect
        && let Some(target) = candidates.hover.as_ref()
    {
        // 跨类别悬停不回退旧高亮。
        return match target {
            // 不改写正式选择和作者状态。
            LayerTarget::Placement(id) => candidates.placement_mesh_selection(id), // 缺失或直接primitive不冒充Scene0网格。
            _ => None, // 当前正在预览地表、植被、角色或水体。
        }; // 离开候选后自然重新使用正式选择。
    }
    selected // 普通场景树和编辑模式沿用原身份与双击行为。
}

/// 把局部Aabb八角经过完整世界仿射变换后重新汇总，保留非均匀缩放与父级剪切。
pub(super) fn transformed_placement_bounds(
    global_transform: &GlobalTransform,
    aabb: &Aabb,
) -> PlacementWorldBounds {
    let local_center = Vec3::from(aabb.center); // Bevy Aabb以Vec3A保存局部中心。
    let local_half_extents = Vec3::from(aabb.half_extents); // 取得每轴局部半尺寸。
    let signs = [
        Vec3::new(-1.0, -1.0, -1.0), // 左下后角。
        Vec3::new(-1.0, -1.0, 1.0),  // 左下前角。
        Vec3::new(-1.0, 1.0, -1.0),  // 左上后角。
        Vec3::new(-1.0, 1.0, 1.0),   // 左上前角。
        Vec3::new(1.0, -1.0, -1.0),  // 右下后角。
        Vec3::new(1.0, -1.0, 1.0),   // 右下前角。
        Vec3::new(1.0, 1.0, -1.0),   // 右上后角。
        Vec3::new(1.0, 1.0, 1.0),    // 右上前角。
    ];
    let affine = global_transform.affine(); // 不分解潜在剪切，精确沿用World层级结果。
    let first = affine.transform_point3(local_center + local_half_extents * signs[0]); // 初始化世界极值。
    let (minimum, maximum) = signs.into_iter().skip(1).fold(
        (first, first), // 固定八角计算不随网格顶点数量增长。
        |(minimum, maximum), sign| {
            let corner = affine.transform_point3(local_center + local_half_extents * sign); // 应用完整世界仿射。
            (minimum.min(corner), maximum.max(corner)) // 重新建立世界轴对齐范围。
        },
    );
    PlacementWorldBounds { minimum, maximum } // 调用方再联合全部匹配primitive。
}

/// 根据三维联合范围计算当前透视或正交视图共用的安全量程。
pub(super) fn placement_focus_extent(frame: PlacementSelectionFrame) -> f32 {
    let longest_axis = frame.size.max_element().max(0.1); // 退化薄片仍保持可用最小尺度。
    (longest_axis * FOCUS_PADDING_SCALE + FOCUS_PADDING_METERS)
        .clamp(MIN_EXTENT_METERS, MAX_EXTENT_METERS) // 继续服从CAD相机正式上下界。
}

#[cfg(test)]
mod tests {
    use super::*; // 双击与仿射边界纯逻辑无需启动渲染器。

    fn selection(placement_id: &str, mesh_name: &str) -> EditorGltfMeshSelection {
        EditorGltfMeshSelection {
            placement_id: placement_id.to_owned(), // 测试稳定实例ID。
            node_path: vec!["场景根".to_owned(), "对象".to_owned()], // 完整显式对象路径。
            mesh_name: mesh_name.to_owned(),       // 同路径不同网格必须隔离双击。
            mesh_index: Some(0),                   // 诊断索引不参与身份比较。
            valid: true,                           // 测试目标允许世界解析。
            orphaned_water_binding: false,         // 不是失效作者绑定。
        }
    }

    #[test] // 只有同地图、同实例、同路径和同网格的两次快速按下才定位。
    fn double_click_requires_same_complete_placement_mesh_identity() {
        let map_id = MapId::parse("MAP-placement-focus").expect("测试地图ID合法"); // 当前草稿身份。
        let mut state = SelectedPlacementViewportState::default(); // 独立点击状态。
        assert!(!state.register_mesh_click(&map_id, &selection("tree-1", "trunk"), 1.0)); // 首次只是选择。
        assert!(!state.register_mesh_click(&map_id, &selection("tree-1", "leaves"), 1.1)); // 不同网格重新计时。
        assert!(state.register_mesh_click(&map_id, &selection("tree-1", "leaves"), 1.3)); // 完整同身份快速第二次才定位。
        assert!(state.focus_request.is_some()); // 定位请求等待世界范围完成。
    }

    #[test] // 跨类型悬停只遮蔽资产表现，离开后恢复同一个正式网格引用。
    fn candidate_hover_preserves_formal_mesh_selection() {
        let selected = selection("tree-1", "leaves"); // 场景树当前正式目标。
        let mut candidates = LayerCandidateState::default(); // 无需加载场景或复制浏览器。
        candidates.hover = Some(LayerTarget::Surface("ground-1".to_owned())); // 预览其他类别。
        assert!(
            displayed_mesh_selection(EditorTool::Inspect, Some(&selected), &candidates).is_none()
        ); // 不残留原资产高亮。
        assert!(std::ptr::eq(
            displayed_mesh_selection(EditorTool::Raise, Some(&selected), &candidates).unwrap(),
            &selected
        )); // 编辑模式不消费旧候选预览。
        candidates.hover = None; // 鼠标离开列表恢复已确认目标。
        assert!(std::ptr::eq(
            displayed_mesh_selection(EditorTool::Inspect, Some(&selected), &candidates).unwrap(),
            &selected
        )); // 恢复原引用，没有正式状态写入。
    }

    #[test] // 悬停切换解析目标不能打断已确认网格的双击时间窗和待定位请求。
    fn hover_resolution_keeps_formal_click_state() {
        let map_id = MapId::parse("MAP-placement-hover").expect("测试地图ID合法"); // 当前地图。
        let selected = selection("tree-1", "leaves"); // 正式网格。
        let mut state = SelectedPlacementViewportState::default(); // 有界解析会话。
        state.register_mesh_click(&map_id, &selected, 1.0); // 第一按下。
        state.previewing = true; // 临时悬停跨类型。
        state.reset_resolution(None, PlacementMeshResolution::WaitingForRoot); // 只隐藏旧覆盖。
        assert!(state.register_mesh_click(&map_id, &selected, 1.2)); // 未丢失真实第一次点击。
        let expected = state.focus_request.clone(); // 冻结正式待定位身份。
        state.reset_resolution(None, PlacementMeshResolution::WaitingForRoot); // 再次跨类型预览。
        assert_eq!(state.focus_request, expected); // 暂停定位而不更换正式目标。
        state.previewing = false; // 离开恢复正式选择。
        state.reset_resolution(expected.clone(), PlacementMeshResolution::WaitingForRoot); // 正式身份仍相符。
        assert_eq!(state.focus_request, expected); // 原定位可在完整解析后继续。
    }

    #[test] // 完整GlobalTransform的旋转与非均匀缩放必须进入相机联合范围。
    fn transformed_bounds_use_all_eight_world_affine_corners() {
        let transform = Transform::from_translation(Vec3::new(10.0, 3.0, -4.0))
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
            .with_scale(Vec3::new(2.0, 3.0, 4.0)); // X/Z旋转后交换世界跨度。
        let global = GlobalTransform::from(transform); // 使用生产函数相同的世界仿射接口。
        let aabb = Aabb::from_min_max(Vec3::new(-1.0, -1.0, -2.0), Vec3::new(1.0, 1.0, 2.0)); // 局部尺寸2×2×4。
        let frame = transformed_placement_bounds(&global, &aabb).frame(); // 固定八角重新汇总。
        assert!((frame.center - Vec3::new(10.0, 3.0, -4.0)).length() < 0.001); // 中心只受平移影响。
        assert!((frame.size - Vec3::new(16.0, 6.0, 4.0)).length() < 0.001); // 旋转后局部Z跨度落到世界X。
    }
    #[test] // 清空候选或重复点根不取消整实例，显式子网格与跨类悬停仍优先。
    fn scene_root_selection_survives_cleared_candidates() {
        let mut browser = EditorGltfBrowserState::default(); // 场景树明确选根。
        browser.selected_placement_id = Some("tree-1".to_owned()); // 无精确网格绑定。
        let mut candidates = LayerCandidateState::default(); // 重复点根之后的空候选。
        assert_eq!(
            displayed_whole_placement(EditorTool::Inspect, &browser, &candidates),
            Some("tree-1")
        ); // 稳定根ID仍高亮。
        candidates.hover = Some(LayerTarget::Surface("ground".to_owned())); // 其他类别临时接管。
        assert_eq!(
            displayed_whole_placement(EditorTool::Inspect, &browser, &candidates),
            None
        ); // 不残留资产覆盖。
        candidates.hover = None; // 离开恢复根。
        assert_eq!(
            displayed_whole_placement(EditorTool::Inspect, &browser, &candidates),
            Some("tree-1")
        ); // 正式根未修改。
        browser.selection = Some(selection("tree-1", "leaves")); // 用户明确点子网格。
        assert_eq!(
            displayed_whole_placement(EditorTool::Inspect, &browser, &candidates),
            None
        ); // 精确查看不被整实例夺回。
    }
}
