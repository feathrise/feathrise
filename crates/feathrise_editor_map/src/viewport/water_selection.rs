//! 中央视口静水、河流与瀑布选择高亮、紧凑身份徽标与双击定位。
//!
//! 【功能设计】列表单击或“查看”工具直接点击只改变编辑会话选择，并复用当前完整水面Mesh
//! 绘制编辑器专属覆盖层；水域作者工具中的`Ctrl+左键`执行同一吸取而不写作者数据。
//! 双击才把现有相机中心和量程对准该水面。局部Aabb来自后台水网完成值，稳定帧只验证
//! 已缓存实体，不扫描R8遮罩、Mesh顶点或全部地图实体。

use bevy::camera::primitives::Aabb; // 纯取景数学直接读取水网后台完成的局部包围盒。
use bevy::prelude::*; // Resource、Handle、Transform与三维数学类型。
use feathrise_map_content::MapId; // 点击序列和预览实体通过稳定地图身份隔离。
use feathrise_scene::{ClipmapWaterMaterial, DynamicWaterMaterial, PaintedWaterSurface, WaterSurfaceKind}; // 两种水面高亮均复用正式波形与覆盖，三类marker提供稳定身份。

use super::camera_math::{MAX_EXTENT_METERS, MIN_EXTENT_METERS}; // 定位量程服从既有CAD导航边界。
use super::{LayerCandidateState, LayerTarget}; // 统一候选只提供临时高亮身份，不改变水域正式选择。
use crate::authoring::{EditorAuthoringSession, WaterPreviewState}; // 当前草稿和联合预览修订决定缓存身份。
use crate::resources::MapEditorState; // 纯选择快照只读取当前会话态。
use crate::types::{EditorTool, WaterAuthoringFeatureKind}; // 作者工具、统一列表类别与世界水面类别保持显式映射。

mod systems; // 有限实体解析、精确Mesh覆盖、铜金框与相机定位。
mod highlight; // 完整分块高亮复用正式材质，不能只给首块叠加静止平面。
mod ui; // 中央视口左上角紧凑徽标的创建和增量文字同步。
mod source; // 水面缓存按当前已采用地形实例和完整作者源授权，旧高亮不能授权新手柄。
use source::{WaterLookupSource, WaterSourceIdentity, WaterSurfaceScope};

#[cfg(test)]
mod source_tests;

pub(crate) use systems::{draw_selected_water_highlight_frame, sync_selected_water_viewport}; // 插件在Presentation与Cursor阶段分别注册表现和定位系统。
pub(crate) use ui::setup_selected_water_badge; // ViewportNode出现时只创建一次固定徽标实体。

const DOUBLE_CLICK_MAX_SECONDS: f64 = 0.36; // 常见双击节奏，同时避免两次独立慢选误触定位。
const FOCUS_PADDING_SCALE: f32 = 1.25; // 定位后四周保留足够岸线和空间关系。
const FOCUS_PADDING_METERS: f32 = 2.0; // 小池塘也保留至少两米额外观察边距。
const SOURCE_LOOKUP_RETRY_FRAMES: u8 = 2; // Commands采用边界最多补查两帧，空层随后稳定短路。

/// 地图、水域类别与稳定对象ID共同构成点击、预览和定位身份。
#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectedWaterIdentity {
    map_id: MapId,                   // 切图后同名水体不能继承旧双击或实体缓存。
    kind: WaterAuthoringFeatureKind, // 静水、河流和瀑布同ID也不能互相冒充。
    feature_id: String,              // 删除、撤销和数组重排后仍按作者稳定ID解析。
}

/// 当前水域选择用于徽标与状态回显的小型快照。
#[derive(Clone, Debug)]
struct SelectedWaterInfo {
    identity: SelectedWaterIdentity, // 世界实体解析只读取稳定身份。
    label: String,                   // 人类可读名称显示在中央画面内。
    water_level: Option<f32>,        // 只有水平静水拥有单一世界Y水位。
    point_count: Option<usize>,      // 河流与瀑布回显完整作者控制点数量。
}

/// 只在完整预览代次或稳定选择变化时触发一次实体解析。
#[derive(Clone, Debug, PartialEq)]
struct WaterSourceLookupSignature {
    identity: SelectedWaterIdentity,       // 对象切换必定失效旧实体。
    applied_water_revision: Option<u64>,   // 新水网完成批次采用后切到新Mesh。
    applied_terrain_revision: Option<u64>, // R16变化可能改变静水实际可见轮廓。
    scope: WaterSurfaceScope, // 同图同层ID的兄弟实例不能共享实体缓存。
    source: Option<WaterLookupSource>, // 只按画布和源路径重查；R8和数值修订仅用于手柄授权。
}

/// 双击时间窗只记录最近一次真实水域对象按下。
#[derive(Clone, Debug)]
struct TimedWaterClick {
    identity: SelectedWaterIdentity, // 不同地图、类别或稳定ID永不组成双击。
    seconds: f64,                    // F12暂停虚拟时间时仍使用真实时间。
}

/// 已解析水域对象的世界轴对齐范围，供相机与铜金框共享。
#[derive(Clone, Copy, Debug, PartialEq)]
struct WaterSelectionFrame {
    center: Vec3, // 完整水面Mesh局部Aabb经过实体Transform后的世界中心。
    size: Vec3,   // 八角变换后重新汇总的世界轴对齐完整尺寸。
}

/// 当前水域选择在中央视口中的有限缓存与交互请求。
#[derive(Resource, Default)]
pub(crate) struct SelectedWaterViewportState {
    last_click: Option<TimedWaterClick>, // 只为下一次同身份按下保留一个时间戳。
    focus_request: Option<SelectedWaterIdentity>, // 双击登记后等待当前完整预览可定位。
    explicit_focus: bool, // F或工具条取景忽略临时悬停；原双击仍在悬停期间暂停。
    viewport_identity: Option<SelectedWaterIdentity>, // “查看”工具直接点击的显式水域单选。
    viewport_selection_revision: u64,    // 只在直接点选身份变化时唤醒左栏固定行池。
    resolved_identity: Option<SelectedWaterIdentity>, // 当前缓存属于哪张图的哪个水域对象。
    lookup_signature: Option<WaterSourceLookupSignature>, // 稳定帧禁止重复扫描有限水面实体。
    lookup_retry_frames: u8,             // 只覆盖Commands可见性边界，不演变为永久空层扫描。
    source_entity: Option<Entity>,       // O(1)验证当前正式或作者预览水面根。
    highlight_entity: Option<Entity>,    // 唯一复用的编辑器专属Mesh覆盖实体。
    dynamic_highlight_material: Option<Handle<DynamicWaterMaterial>>, // 独立静水与样条高亮沿用正式波形和顶点透明度。
    highlight_mesh_source: Option<(Entity, bevy::asset::AssetId<Mesh>)>, // 只在已采用几何换代时重建子块，稳定帧不遍历水网。
    highlight_source_child_count: usize, // 同根增加或移走上传块也要失效完整覆盖。
    highlight_chunks: Vec<Entity>, // 覆盖实体的子块随唯一高亮根显隐、退役。
    highlight_build: Option<highlight::HighlightChunkBuild>, // 换代先隐藏，有限回收、镜像完成后再整体显示。
    clipmap_highlight_material: Option<Handle<ClipmapWaterMaterial>>, // 共享环带必须保留原高度页、R8与水深裁切。
    frame: Option<WaterSelectionFrame>, // Gizmo稳定帧只读取这份小型完成值。
    center_source: Option<WaterSourceIdentity>, // 只有当前完整预览可授权水位手柄，旧版高亮仍可独立保留。
    badge_text: Option<String>,         // 文案未变化时不触发Bevy Text重排。
}

impl SelectedWaterViewportState {
    /// 水位手柄只复用已完成的正式对象范围；悬停其他水面时不能借用其中心授权拖动。
    pub(super) fn still_water_center(&self, session: &EditorAuthoringSession, id: &str) -> Option<Vec3> {
        let identity = self.resolved_identity.as_ref()?;
        (session.draft.as_ref().is_some_and(|draft| identity.map_id == draft.map_id)
            && identity.kind == WaterAuthoringFeatureKind::StillWater && identity.feature_id == id
            && self.center_source.as_ref().is_some_and(|source| source.matches(session, id)))
            .then_some(self.frame?.center)
    }

    #[cfg(test)]
    pub(super) fn set_still_water_center_for_tests(&mut self, session: &EditorAuthoringSession, id: &str, center: Vec3) {
        self.resolved_identity = Some(SelectedWaterIdentity {
            map_id: session.draft.as_ref().unwrap().map_id.clone(), kind: WaterAuthoringFeatureKind::StillWater, feature_id: id.into(),
        });
        self.frame = Some(WaterSelectionFrame { center, size: Vec3::new(2.0, 0.0, 2.0) });
        self.center_source = WaterSourceIdentity::capture(session, id);
    }

    pub(crate) fn request_focus(&mut self, map_id: &MapId, kind: WaterAuthoringFeatureKind, id: &str) {
        self.focus_request = Some(SelectedWaterIdentity { map_id: map_id.clone(), kind, feature_id: id.into() });
        self.explicit_focus = true;
    }

    fn explicit_focus_pending(&self) -> bool { self.explicit_focus && self.focus_request.is_some() }

    pub(crate) fn cancel_focus(&mut self) { self.focus_request = None; self.explicit_focus = false; self.clear_click_sequence(); }

    /// 记录一次水域对象按下；只有同一稳定身份在真实时间窗内再次按下才登记定位。
    pub(crate) fn register_water_click(
        &mut self,
        map_id: &MapId,
        kind: WaterAuthoringFeatureKind,
        feature_id: &str,
        now_seconds: f64,
    ) -> bool {
        let identity = SelectedWaterIdentity {
            map_id: map_id.clone(),            // 点击冻结当前草稿地图身份。
            kind,                              // 同名异类对象保持严格隔离。
            feature_id: feature_id.to_owned(), // 行池稳定ID复制后不受下一帧换页影响。
        };
        self.focus_request = None; // 新按下先取消尚未采用的旧定位请求。
        self.explicit_focus = false; // 鼠标双击继续服从原有悬停预览与恢复规则。
        if !now_seconds.is_finite() {
            self.last_click = None; // 异常时钟值不得制造永久可匹配点击。
            return false; // 非有限真实时间明确按普通单击处理。
        }
        let double_clicked = self.last_click.as_ref().is_some_and(|last| {
            last.identity == identity // 地图、类别与水域稳定ID都必须完全一致。
                && now_seconds >= last.seconds // 真实时间不得倒退后仍匹配旧点击。
                && now_seconds - last.seconds <= DOUBLE_CLICK_MAX_SECONDS // 固定窗口内才属于双击。
        });
        if double_clicked {
            self.focus_request = Some(identity); // Cursor阶段等待最新完整水面后再原子调整相机。
            self.last_click = None; // 三连击的第三下重新作为下一组单击起点。
        } else {
            self.last_click = Some(TimedWaterClick {
                identity,             // 保存本次完整稳定身份。
                seconds: now_seconds, // 保存不受玩法暂停影响的真实时钟。
            });
        }
        double_clicked // 调用方可在底栏即时提示正在定位。
    }

    /// “查看”工具直接点选一张完整水面，并复用统一双击定位序列。
    pub(crate) fn select_from_viewport(
        &mut self,
        map_id: &MapId,
        kind: WaterAuthoringFeatureKind,
        feature_id: &str,
        now_seconds: f64,
    ) -> bool {
        let identity = SelectedWaterIdentity {
            map_id: map_id.clone(),            // 世界命中只属于当前真实草稿地图。
            kind,                              // 统一水面marker的类别进入会话身份。
            feature_id: feature_id.to_owned(), // 保存稳定作者ID而不是临时ECS实体。
        };
        if self.viewport_identity.as_ref() != Some(&identity) {
            self.viewport_identity = Some(identity); // 查看工具下一帧按稳定身份解析完整作者对象。
            self.viewport_selection_revision = self.viewport_selection_revision.wrapping_add(1); // 固定行池只在真实换项时刷新。
        }
        self.register_water_click(map_id, kind, feature_id, now_seconds) // 第二次同一水面才登记定位。
    }

    /// 水域作者工具或列表接管选择时只撤销“查看”工具专用身份，不破坏本次双击序列。
    pub(crate) fn clear_explicit_viewport_selection(&mut self) {
        if self.viewport_identity.take().is_some() {
            self.viewport_selection_revision = self.viewport_selection_revision.wrapping_add(1); // 左栏必须清除旧查看高亮。
        }
    }

    /// 选择摆放、植被或空白时完整释放水域单选与迟到定位请求。
    pub(crate) fn clear_viewport_selection(&mut self) {
        self.clear_explicit_viewport_selection(); // 先只在真实身份变化时推进UI修订。
        self.focus_request = None; // 其他内容已经取得单选所有权，旧双击不得迟到移动相机。
        self.clear_click_sequence(); // 下一次水域点击必须重新开始时间窗。
    }

    /// 返回当前地图在“查看”工具中显式选中的类别与稳定ID。
    pub(crate) fn viewport_feature(
        &self,
        map_id: &MapId,
    ) -> Option<(WaterAuthoringFeatureKind, &str)> {
        self.viewport_identity
            .as_ref()
            .filter(|identity| &identity.map_id == map_id)
            .map(|identity| (identity.kind, identity.feature_id.as_str())) // 不向UI暴露内部拥有型身份。
    }

    /// 左栏固定行池只比较这一常数修订，不在稳定帧克隆稳定ID字符串。
    pub(crate) fn viewport_selection_revision(&self) -> u64 {
        self.viewport_selection_revision
    }

    /// 页码、其他对象或其他工具介入时终止未完成双击序列。
    pub(crate) fn clear_click_sequence(&mut self) {
        self.last_click = None; // 后续水域单击必须重新开始完整双击时间窗。
    }
}

/// 从当前草稿建立水域选择快照；纯读取不修改dirty、撤销或作者修订。
fn selected_water_info(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    selection: &SelectedWaterViewportState,
    candidates: Option<&LayerCandidateState>, // 独立水域测试与旧调用允许没有统一候选资源。
) -> Option<SelectedWaterInfo> {
    if !editor.open {
        return None; // 关闭F12后完整隐藏全部水域选择表现。
    }
    let draft = session.draft.as_ref()?; // 没有完整草稿时不存在可解析的稳定地图身份。
    let water = draft.water.as_ref()?; // 三类对象都来自同一完整作者文档。
    let (kind, feature_id) = match editor.tool {
        EditorTool::Water => water
            .layers
            .get(editor.selected_water_layer)
            .map(|layer| (WaterAuthoringFeatureKind::StillWater, layer.id.as_str())),
        EditorTool::RiverSpline => water
            .rivers
            .get(editor.selected_water_river)
            .map(|river| (WaterAuthoringFeatureKind::River, river.id.as_str())),
        EditorTool::Waterfall => water
            .waterfalls
            .get(editor.selected_waterfall)
            .map(|waterfall| (WaterAuthoringFeatureKind::Waterfall, waterfall.id.as_str())),
        EditorTool::Inspect => candidate_water_feature(&draft.map_id, selection, candidates), // 悬停优先且跨类型不回退旧水面。
        _ => None, // 地形、植被、玩法等工具不显示旧水域选择。
    }?;
    let identity = SelectedWaterIdentity {
        map_id: draft.map_id.clone(), // 采用真实编辑草稿而不是可能迟到的目录选择。
        kind,                         // 世界marker按同一类别匹配。
        feature_id: feature_id.to_owned(), // 数组重排后仍只保存稳定作者ID。
    };
    match kind {
        WaterAuthoringFeatureKind::StillWater => water
            .layers
            .iter()
            .find(|layer| layer.id == feature_id)
            .map(|layer| SelectedWaterInfo {
                identity,
                label: layer.label.clone(), // 徽标显示作者名称而不是内部ID。
                water_level: Some(layer.water_level), // 水平静水明确回显世界Y。
                point_count: None,          // R8静水没有样条控制点。
            }),
        WaterAuthoringFeatureKind::River => water
            .rivers
            .iter()
            .find(|river| river.id == feature_id)
            .map(|river| SelectedWaterInfo {
                identity,
                label: river.label.clone(), // 河流作者名称与左栏列表一致。
                water_level: None,          // 三维样条不存在单一水位。
                point_count: Some(river.points.len()), // 回显完整控制点数量。
            }),
        WaterAuthoringFeatureKind::Waterfall => water
            .waterfalls
            .iter()
            .find(|waterfall| waterfall.id == feature_id)
            .map(|waterfall| SelectedWaterInfo {
                identity,
                label: waterfall.label.clone(), // 瀑布作者名称与左栏列表一致。
                water_level: None,              // 水幕沿三维路径变化高度。
                point_count: Some(waterfall.points.len()), // 回显完整控制点数量。
            }),
    }
}

/// 候选资源存在时统一选择拥有高亮权；没有资源的独立水域场景继续兼容原直选。
fn candidate_water_feature<'a>(
    map_id: &MapId,                              // 仅供旧水域直选核对地图身份。
    selection: &'a SelectedWaterViewportState,   // 只读正式选择和双击序列。
    candidates: Option<&'a LayerCandidateState>, // 临时高亮与正式候选共享稳定身份。
) -> Option<(WaterAuthoringFeatureKind, &'a str)> {
    let Some(candidates) = candidates else {
        return selection.viewport_feature(map_id); // 兼容未安装统一候选的调用。
    }; // 正常编辑器始终由统一候选负责选择。
    let target = if selection.explicit_focus_pending() { candidates.selected.as_ref() }
        else { candidates.hover.as_ref().or(candidates.selected.as_ref()) };
    match target {
        Some(LayerTarget::Water(kind, id)) => Some((*kind, id.as_str())), // 同名异类水体仍独立。
        _ => None, // 悬停地表、植被或角色时必须隐藏旧水域覆盖。
    } // 移出候选后自然恢复正式选中的水体。
}

#[cfg(test)]
#[path = "water_selection/candidate_tests.rs"]
mod candidate_tests; // 候选预览隔离与跨类别恢复的纯状态回归。

/// 当前已采用水网批次构成实体解析签名；未采用时两项修订都保持None。
fn source_lookup_signature(
    identity: &SelectedWaterIdentity,
    preview: &WaterPreviewState,
    scope: WaterSurfaceScope,
    source: Option<WaterSourceIdentity>,
) -> WaterSourceLookupSignature {
    let same_applied_map = preview.applied_map_id.as_ref() == Some(&identity.map_id); // 旧地图完成值不能触发当前图扫描。
    WaterSourceLookupSignature {
        identity: identity.clone(), // 每次作者直选都能精确失效旧对象缓存。
        applied_water_revision: same_applied_map.then_some(preview.applied_water_revision), // 新水定义批次才变化。
        applied_terrain_revision: same_applied_map.then_some(preview.applied_terrain_revision), // 新河床批次才变化。
        scope, source: source.map(|identity| identity.source),
    }
}

/// 把作者列表类别映射到统一运行时水面marker类别。
fn surface_kind(kind: WaterAuthoringFeatureKind) -> WaterSurfaceKind {
    match kind {
        WaterAuthoringFeatureKind::StillWater => WaterSurfaceKind::Still,
        WaterAuthoringFeatureKind::River => WaterSurfaceKind::River,
        WaterAuthoringFeatureKind::Waterfall => WaterSurfaceKind::Waterfall,
    }
}

/// 判断世界水面marker是否就是当前作者水域对象。
fn surface_matches(surface: &PaintedWaterSurface, identity: &SelectedWaterIdentity) -> bool {
    surface.kind == surface_kind(identity.kind) // 三类对象必须严格匹配各自统一水面类别。
        && surface.map_id == identity.map_id // 切图边界拒绝同层ID串用。
        && surface.layer_id == identity.feature_id // 数组重排后仍按稳定ID匹配。
}

/// 把局部Aabb八个角经过实体Transform后重新汇总为世界轴对齐取景范围。
fn transformed_water_frame(transform: &Transform, aabb: &Aabb) -> WaterSelectionFrame {
    let local_center = Vec3::from(aabb.center); // Bevy Aabb使用Vec3A存储局部中心。
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
    let first = transform.transform_point(local_center + local_half_extents * signs[0]); // 初始化世界极值。
    let (minimum, maximum) = signs.into_iter().skip(1).fold(
        (first, first), // 固定八角计算不随地图或网格规模增长。
        |(minimum, maximum), sign| {
            let corner = transform.transform_point(local_center + local_half_extents * sign); // 应用平移、旋转和非均匀缩放。
            (minimum.min(corner), maximum.max(corner)) // 重新建立世界轴对齐范围。
        },
    );
    WaterSelectionFrame {
        center: (minimum + maximum) * 0.5, // 相机和铜金框共享真实世界中心。
        size: maximum - minimum,           // 完整尺寸保留薄水平面的零高度语义。
    }
}

/// 根据选中水面三维跨度计算带上下文余量的CAD视口量程。
fn focus_extent(frame: WaterSelectionFrame) -> f32 {
    frame
        .size
        .x
        .max(frame.size.y)
        .max(frame.size.z) // 瀑布在前/侧视图也必须完整容纳世界Y跨度。
        .mul_add(FOCUS_PADDING_SCALE, FOCUS_PADDING_METERS) // 大湖按比例留白，小池按固定米数留白。
        .clamp(MIN_EXTENT_METERS, MAX_EXTENT_METERS) // 与滚轮导航和地图一键取景共用安全边界。
}

#[cfg(test)]
mod tests {
    use super::*; // 纯点击与取景数学测试不启动窗口或RenderApp。

    #[test] // 只有同地图、同类别、同稳定ID的第二次快速按下才登记定位。
    fn double_click_requires_same_water_identity_inside_real_time_window() {
        let map = MapId("MAP-double-click".to_owned()); // 构造合法稳定测试地图身份。
        let other_map = MapId("MAP-other".to_owned()); // 验证跨地图隔离。
        let mut state = SelectedWaterViewportState::default(); // 初始没有点击和定位请求。
        assert!(!state.register_water_click(
            &map,
            WaterAuthoringFeatureKind::StillWater,
            "lake-a",
            1.0,
        )); // 第一下只选择和高亮。
        assert!(
            !state.register_water_click(&map, WaterAuthoringFeatureKind::River, "lake-a", 1.1,)
        ); // 同ID不同类别重新开始时间窗。
        assert!(!state.register_water_click(
            &other_map,
            WaterAuthoringFeatureKind::River,
            "lake-a",
            1.2,
        )); // 同类别同ID跨地图仍不是双击。
        assert!(!state.register_water_click(
            &map,
            WaterAuthoringFeatureKind::StillWater,
            "lake-a",
            2.0,
        )); // 超过窗口的旧点击不得匹配。
        assert!(state.register_water_click(
            &map,
            WaterAuthoringFeatureKind::StillWater,
            "lake-a",
            2.2,
        )); // 同身份快速第二下登记定位。
        assert_eq!(
            state
                .focus_request
                .as_ref()
                .map(|request| request.feature_id.as_str()),
            Some("lake-a")
        ); // 请求完整保留稳定层ID供Cursor阶段采用。
        assert!(state.last_click.is_none()); // 双击完成后第三下重新开始新序列。
    }

    #[test] // 后台局部Aabb经过实体平移后应直接产生世界取景中心与留白量程。
    fn transformed_aabb_drives_camera_center_and_extent_without_mesh_scan() {
        let aabb = Aabb::from_min_max(
            Vec3::new(-2.0, -0.1, -4.0), // 局部最小角。
            Vec3::new(2.0, 0.1, 4.0),    // 局部最大角。
        );
        let transform = Transform::from_translation(Vec3::new(10.0, 19.0, -20.0)); // 模拟静水实体世界水位与位置。
        let frame = transformed_water_frame(&transform, &aabb); // 固定八角变换不读取Mesh资产。
        assert_eq!(frame.center, Vec3::new(10.0, 19.0, -20.0)); // 世界中心包含实体平移。
        assert!((frame.size - Vec3::new(4.0, 0.2, 8.0)).abs().max_element() <= 0.000_01); // 世界平移后的浮点舍入仍必须保持在厘米级远以下。
        assert_eq!(focus_extent(frame), 12.0); // 八米最大跨度乘留白并加两米固定余量。
    }
}
