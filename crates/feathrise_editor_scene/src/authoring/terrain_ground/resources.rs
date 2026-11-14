//! 地面索引与光标各只有一个在途任务；取消后仍轮询至结束，再释放计算槽。
use bevy::{prelude::*, tasks::Task};
use feathrise_scene_content::{SceneId, SceneTerrainObject};
use std::sync::{Arc, Weak};
use super::index::{SurfaceInput, TerrainGroundSnapshot};
pub(super) struct Capture {
    pub scene_id: SceneId, pub revision: u64, pub objects: Arc<[SceneTerrainObject]>,
    pub cursor: Option<Entity>, pub inputs: Vec<SurfaceInput>,
}
pub(super) struct Work {
    pub scene_id: SceneId, pub revision: u64, pub objects: Arc<[SceneTerrainObject]>,
    pub task: Task<Result<Arc<TerrainGroundSnapshot>, String>>,
}
#[derive(Resource, Default)]
pub(crate) struct TerrainGroundCapture { pub(super) capture: Option<Capture>, pub(super) work: Option<Work> }
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct CursorRay { pub origin: Vec3, pub direction: Vec3, pub epoch: u64 }
/// 排队点击也冻结快照分配与内容代次；Weak防止地址复用，不额外延长大查询数据寿命。
#[derive(Clone)]
pub(crate) struct CursorGroundSnapshot { snapshot: Weak<TerrainGroundSnapshot>, generation: u64 }
impl CursorGroundSnapshot {
    pub(crate) fn capture(snapshot: &Arc<TerrainGroundSnapshot>) -> Self {
        Self { snapshot: Arc::downgrade(snapshot), generation: snapshot.generation }
    }
    /// 显式验收只读分配与内容代次，弱票据不能因为诊断延长查询数据寿命。
    #[cfg(feature = "validation")]
    pub(crate) fn validation_identity(&self) -> (usize, u64) {
        (self.snapshot.as_ptr() as usize, self.generation)
    }
    fn matches(&self, snapshot: &Arc<TerrainGroundSnapshot>) -> bool {
        self.generation == snapshot.generation && self.snapshot.as_ptr() == Arc::as_ptr(snapshot)
    }
}
impl PartialEq for CursorGroundSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation && self.snapshot.ptr_eq(&other.snapshot)
    }
}
#[derive(Clone, PartialEq)]
pub(crate) struct CursorIdentity {
    pub scene_id: SceneId, pub revision: u64, pub tool: crate::types::EditorTool,
    pub terrain_revision: u64, pub ground_snapshot: Option<CursorGroundSnapshot>,
    pub asset: Option<String>, pub asset_target: Option<feathrise_scene_content::ScenePlacementAssetTarget>,
    pub portal: Option<feathrise_scene_content::ScenePortalTarget>, pub camera: Mat4, pub size: UVec2,
    pub camera_entity: Entity, pub projection: Mat4, pub view: crate::types::EditorView,
    pub orthographic: bool, pub extent: u32, // 冷启动没有吸附pending替镜头把关，也须冻结实际投影与缩放。
    pub visibility: usize,
    pub relocation: Option<(usize, String, u64, u64)>, // 下标、稳定ID、结构和作者修订；同位置换所选实例也必须取消旧点击。
    pub translation_snap: Option<feathrise_editor_core::EditorTranslationSnap>, // 身份只核对偏好；按下时实际Ctrl语义由capture_click单独冻结。
}
impl CursorIdentity {
    pub(super) fn matches_ground(&self, revision: u64, snapshot: Option<&Arc<TerrainGroundSnapshot>>) -> bool {
        self.terrain_revision == revision && self.ground_snapshot.as_ref().zip(snapshot)
            .is_some_and(|(frozen, current)| current.scene_id == self.scene_id && current.revision == revision && frozen.matches(current))
    }
}
pub(super) struct CursorHit {
    pub terrain: feathrise_scene_content::TerrainId,
    pub point: Vec3,
    pub lower_layer: bool,
    pub volume_surface: bool,
    pub paint_point: Option<Vec3>,
    pub paint_from: Option<Vec3>,
}
/// 只供连续画笔消费的新完成点；世界坐标起点已经由后台验证，不能拿它替代当前悬停或点击。
#[derive(Clone)]
pub(crate) struct CursorPaintHit {
    pub terrain: feathrise_scene_content::TerrainId,
    pub point: Vec3,
    pub from: Option<Vec3>,
}
/// 纯显示可沿用同来源最后完成的真点；不携带按压权限，写入端必须另取paint_hit。
#[derive(Clone)]
pub(crate) struct CursorDisplayHit {
    pub terrain: feathrise_scene_content::TerrainId,
    pub point: Vec3,
    pub volume_surface: bool,
}
pub(super) struct CursorDisplaySample {
    pub identity: CursorIdentity,
    pub ray: CursorRay,
    pub authority: u64,
    pub hit: CursorDisplayHit,
    pub definition: Option<Arc<SceneTerrainObject>>, // 只保留小型来源定义，纯显示不得延长旧Collider/高度页快照的寿命。
}
#[derive(Clone)]
pub(super) struct CursorPaintAnchor {
    pub identity: CursorIdentity,
    pub ray: CursorRay,
    pub authority: u64,
    pub hit: CursorPaintHit,
}
pub(super) struct CursorWork {
    pub snapshot: Arc<TerrainGroundSnapshot>, pub ray: CursorRay, pub identity: CursorIdentity, pub click: bool,
    pub click_ticket: u64, pub click_snap: Option<feathrise_editor_core::EditorTranslationSnap>,
    pub hidden: Arc<std::collections::BTreeSet<feathrise_scene_content::TerrainId>>,
    pub task: Task<Option<CursorHit>>,
}
#[derive(Resource, Default)]
pub(crate) struct TerrainGroundCursor {
    /// 本帧完整鼠标所有权门；画布工具只消费该门授权的按压，不自行绕过模态或失焦。
    pub(super) input_available: bool,
    pub(crate) ray: Option<CursorRay>,
    pub(crate) identity: Option<CursorIdentity>,
    pub(crate) pending_click: Option<(CursorIdentity, CursorRay)>,
    pub(crate) terrain: Option<feathrise_scene_content::TerrainId>,
    /// 真实单击与其位置一起采用；完成帧悬停另一对象不能改变本次地形归属。
    pub(crate) click_terrain: Option<feathrise_scene_content::TerrainId>,
    /// 按下时的实际吸附语义；没有已接受hover或完成前松开Ctrl，也不得改用完成帧偏好。
    pub(crate) click_snap: Option<feathrise_editor_core::EditorTranslationSnap>,
    pub(crate) volume_ray: Option<(Vec3,Vec3)>,
    pub(super) click_ticket: u64,
    pub(super) pending_click_snap: Option<feathrise_editor_core::EditorTranslationSnap>,
    pub(super) work: Option<CursorWork>,
    pub(super) accepted: Option<(Arc<TerrainGroundSnapshot>, CursorRay,
        Arc<std::collections::BTreeSet<feathrise_scene_content::TerrainId>>, Option<CursorHit>)>,
    pub(super) paint_hit: Option<CursorPaintHit>,
    pub(super) paint_anchor: Option<CursorPaintAnchor>,
    pub(super) paint_pending: bool,
    pub(super) display: Option<CursorDisplaySample>,
}

impl TerrainGroundCursor {
    /// 仅调用端路由单测可直接安装命中；连续移动与所有权回归必须走正式后台cursor系统。
    #[cfg(test)]
    pub(crate) fn test_paint_hit(&mut self, terrain: feathrise_scene_content::TerrainId, point: Vec3, from: Option<Vec3>) {
        self.input_available = true;
        self.paint_hit = Some(CursorPaintHit { terrain, point, from });
    }

    pub(crate) fn display_hit(&self) -> Option<&CursorDisplayHit> { self.display.as_ref().map(|sample| &sample.hit) }

    /// 刷圈与完成点共用同一来源姿态；不能借当前选中的另一画布旋转旧显示。
    pub(crate) fn display_transform(&self) -> Option<Transform> {
        let sample = self.display.as_ref()?;
        sample.definition.as_ref().filter(|definition| definition.enabled && definition.id == sample.hit.terrain)
            .map(|definition| definition.transform.as_transform())
    }

    /// 仅为二维刷圈投影当前射线；返回值不进入严格命中、单击或作者刷段，缓存仍保留最后真点。
    pub(crate) fn display_point(&self) -> Option<Vec3> {
        let sample = self.display.as_ref()?;
        let identity = self.identity.as_ref()?;
        let ray = self.ray?;
        if self.input_authority() != Some(sample.authority) || ray.epoch != sample.ray.epoch {
            return None;
        }
        // provider已经按当前活动来源核对小型定义；这里只忽略其允许显示保留的高度换代，
        // 相机、投影、场景、工具、可见性与其他上下文仍须相同，不能以旧圈跨镜头跟随。
        let mut previous = sample.identity.clone();
        previous.revision = identity.revision;
        previous.terrain_revision = identity.terrain_revision;
        previous.ground_snapshot = identity.ground_snapshot.clone();
        if previous != *identity { return None; }
        let planar_tool = matches!(identity.tool, crate::types::EditorTool::Raise
            | crate::types::EditorTool::Lower | crate::types::EditorTool::Smooth
            | crate::types::EditorTool::Flatten | crate::types::EditorTool::Surface
            | crate::types::EditorTool::Vegetation | crate::types::EditorTool::Water
            | crate::types::EditorTool::Puddles);
        if ray == sample.ray || !planar_tool || sample.hit.volume_surface {
            return Some(sample.hit.point); // 已完成当前射线或三维/洞内表面继续显示真实落点。
        }
        let definition = sample.definition.as_ref()
            .filter(|definition| definition.enabled && definition.id == sample.hit.terrain)?;
        let transform = definition.transform.as_transform();
        let normal = transform.rotation * Vec3::Y;
        let denominator = ray.direction.dot(normal);
        if !ray.origin.is_finite() || !ray.direction.is_finite() || denominator.abs() < 0.0001 {
            return None; // 近平行射线不把显示圈投到极远处，归还系统鼠标。
        }
        // 倾斜来源的二维画笔属于本地XZ平面；纯显示延迟也必须沿源Y法向的最后真点平面跟随。
        let distance = (sample.hit.point - ray.origin).dot(normal) / denominator;
        if !distance.is_finite() || !(0.0..=1_000_000.0).contains(&distance) { return None; }
        let point = ray.origin + ray.direction * distance;
        let local = transform.to_matrix().inverse().transform_point3(point);
        let min = Vec2::from_array(definition.world_min);
        let max = Vec2::from_array(definition.world_max);
        if !local.is_finite() || !min.is_finite() || !max.is_finite() || min.cmpge(max).any()
            || local.xz().cmplt(min).any() || local.xz().cmpgt(max).any() {
            return None; // 世界射线必须仍落在该来源的局部作者边界，不能越界借旧圈隐藏鼠标。
        }
        Some(point)
    }

    /// 同一次按住的后台完成点即使稍落后于鼠标，也可刷入原位置；只暴露本帧的新完成或静止点。
    pub(crate) fn paint_hit(&self) -> Option<&CursorPaintHit> { self.paint_hit.as_ref() }

    /// 正常单飞延迟只暂停；缺页、未命中、离开视口和失去输入所有权必须返回false来断笔。
    pub(crate) fn paint_pending(&self) -> bool { self.paint_pending }

    /// 取消代次必须和本帧门一起冻结，避免笔刷系统被run_if跳过后在解锁帧复活旧按压。
    pub(crate) fn input_authority(&self) -> Option<u64> {
        self.input_available.then_some(self.click_ticket)
    }

    /// 原始射线可排在悬停任务之后；临时Ctrl和点击权限必须在按下帧冻结。
    pub(crate) fn capture_click(&mut self, identity: CursorIdentity, ray: CursorRay,
        snap: Option<feathrise_editor_core::EditorTranslationSnap>) {
        self.click_ticket = self.click_ticket.wrapping_add(1);
        self.pending_click = Some((identity, ray));
        self.pending_click_snap = snap;
        self.click_terrain = None;
        self.click_snap = None;
    }

    /// 失去输入所有权后只废止点击权限；保留work句柄，非阻塞轮询结束才释放唯一CPU槽。
    pub(crate) fn cancel_click(&mut self) {
        self.click_ticket = self.click_ticket.wrapping_add(1);
        self.pending_click = None;
        self.pending_click_snap = None;
        self.click_terrain = None;
        self.click_snap = None;
    }
}
