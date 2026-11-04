//! Clipmap作者工具的精确地面射线单飞查询；任务只读已采用曲面，未命中绝不回退水平面。
use crate::types::{EditorTool, EditorView};
use bevy::prelude::*; // 地图视口射线、资源与不可变数学值。
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready}; // 一个在途CPU任务，帧内只非阻塞轮询。
use feathrise_map_content::{MapId, MapPlacementAssetTarget, MapPortalTarget}; // 跨图拒收完成值，普通摆放还冻结Scene0或primitive身份。
use feathrise_scene::{ClipmapGroundHit, ClipmapGroundSnapshot}; // 场景唯一已采用曲面查询协议。 // 投影切换必须使旧射线结果失效。

mod click_ownership; // 每个作者域只取消自己的冻结点击，共用查询槽不代表共享操作所有权。
#[cfg(any(test, feature = "runtime_test_support"))]
pub mod test_support; // 私有游戏回归只借用固定阶段夹具，不公开生产查询状态。

/// 明确区分同一查询槽服务的作者对象，禁止植被与静水互相采用迟到命中。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroundCursorTarget {
    TerrainSculpt(EditorTool), // 四种雕刻工具独立冻结身份，连续输入不得跨工具采用旧落点。
    VolumeSculpt(EditorTool),  // 真实三维表面命中，不受高度雕刻的单层投影限制。
    Surface {
        layer_id: String,
        preset_id: Option<String>,
    }, // 同预设独立层和预设选择都不能借用上一目标的迟到命中。
    Puddles,                   // 独立积水潜力画布，与静水层和地表权重隔离。
    Vegetation(String),        // 当前作者植被层稳定ID。
    StillWater(Option<String>), // Some为当前静水层；None只允许为首层取得真实地表初始水位。
    WaterLevel(String),        // “从光标取水位”消费后的独立用途身份。
    River(String),             // 当前河流稳定ID；异步点击不能跨特征追加控制点。
    Waterfall(String),         // 当前瀑布稳定ID；与河流共享查询槽但不共享完成值。
    PlaceAsset {
        asset_path: String,                    // 文件选择变化使旧点击立即失效。
        asset_target: MapPlacementAssetTarget, // 子资产切换不能把旧落点交给另一primitive。
    },
    RelocatePlacement(String), // 当前普通摆放稳定ID；建筑继续使用独立五点地基流程。
    Domain(crate::game_authoring::map_interaction::DomainGroundTarget),
    Portal(MapPortalTarget),   // 目标地图、默认出生点或具体端点完整冻结，不在完成帧换目标。
    BoundaryPoint {
        path_id: String,
        insert: bool,
    }, // 空气墙点击冻结所选墙及普通追加/Ctrl投影插点语义。
    BoundaryDrag {
        path_id: String,
        point_index: usize,
    }, // 控制点编号仅在同一作者修订的单墙拖动中有效。
    Building {
        preset_id: String,
        controls: [u32; 4],
        catalog_revision: u64,
    }, // 只解析点击种子；最终根高度由独立五点地基校验。
    RelocateBuilding {
        placement_id: String,
        duplicate: bool,
        controls: [u32; 4],
        catalog_revision: u64,
    }, // 已有建筑移动与复制仍各自冻结意图。
    Courtyard {
        mode: u8,
        preset_id: Option<String>,
        selected_id: Option<String>,
        quarter_turns: i32,
        catalogs: [u64; 2],
    }, // 模板、朝向、目标副本和两类目录热更不能跨点击混用。
}

impl GroundCursorTarget {
    /// 点击请求可在松键后完成一次；圆刷继续受按压代次约束。
    pub(super) fn is_author_click(&self) -> bool {
        matches!(self, Self::Domain(target) if target.click_qualified) || matches!(
            self,
            Self::River(_)
                | Self::VolumeSculpt(_)
                | Self::Waterfall(_)
                | Self::PlaceAsset { .. }
                | Self::RelocatePlacement(_)
                | Self::Portal(_)
                | Self::BoundaryPoint { .. }
                | Self::Building { .. }
                | Self::RelocateBuilding { .. }
                | Self::Courtyard { .. }
        )
    }
}

/// 同一按压允许顺序使用已经冻结的历史射线；地图、曲面、相机、层或按压改变则整代失效。
#[derive(Clone, Debug, PartialEq, Eq)] // 位模式比较不让NaN或近似位置混淆身份。
pub(super) struct GroundCursorContext {
    // 不含鼠标射线位置，否则连续拖动会永久拒绝前一帧完成值。
    pub(super) map_id: MapId,                      // 当前草稿地图。
    pub(super) target: Option<GroundCursorTarget>, // 当前作者功能域及稳定层身份。
    pub(super) placement_snap: Option<feathrise_editor_core::EditorTranslationSnap>, // 点放冻结吸附基准和Ctrl语义，不影响绘制工具。
    pub(super) terrain_revision: u64,              // F12作者高度必须已经正式采用到当前页组。
    pub(super) missing_pages: [UVec4; 16],         // 缺页状态属于完整采用身份。
    pub(super) surface_revision: [u64; 3],         // 完整已采用地形/页/视图修订。
    pub(super) camera_transform: [u32; 16],        // 轨道、平移或镜头变化使旧采样失效。
    pub(super) view: EditorView,                   // 当前快捷方向或自由状态。
    pub(super) orthographic: bool, // 同方向切投影也使旧射线失效，不能只比较相机位置。
    pub(super) extent: u32,        // 两种投影共用的轨道距离与缩放身份。
    pub(super) render_size: UVec2, // 同一像素坐标不能跨RTT尺寸复用。
    pub(super) input_epoch: u64,   // 一次按下至松手形成独立输入代次。
    pub(super) held: bool,         // 松手后旧按压结果不可能命中当前上下文。
    pub(super) modifiers: u8,      // Shift擦除与Ctrl选层不能使用旧交互语义。
    pub(super) authoring_revision: Option<u64>, // 点击、角色圆刷和边界拖动冻结草稿修订。
} // 固定小型上下文不随地图规模增长。

#[derive(Clone, Debug, PartialEq, Eq)] // 射线每次改变只更新最新请求，不增加任务队列。
struct GroundRequest {
    // 完成值保留真实原射线与序号，绝不用旧交点冒充新射线。
    context: GroundCursorContext, // 冻结全部不可跨代身份。
    sequence: u64,                // 同代按序采用，防止重复写旧落点。
    origin: [u32; 3],             // 原始世界射线起点位模式。
    direction: [u32; 3],          // 原始单位方向位模式。
} // 一个完成值只对应一次真实查询。

#[derive(Clone)]
struct GroundCompletion {
    terrain_mesh: bool, // 后台证明真实三维命中；主帧不重新遍历洞穴BVH。
    // 后台不操作World、ECS或磁盘。
    request: GroundRequest,   // 与纯CPU查询冻结的完全相同身份。
    result: ClipmapGroundHit, // 保留Waiting/Miss/Failed原因供诊断，不把计算在途当缺页。
    point: Option<Vec3>,      // Waiting、Miss和Failed统一不提供可绘制落点。
} // 没有水平面替代值。

#[derive(Resource, Default)] // 安装于F12插件，关闭后仍保留在途槽至自然结束。
pub struct VegetationGroundCursor {
    // 历史类型名保留内部兼容，资源已同时服务Clipmap植被和静水。
    pub(super) captured: Option<(GroundCursorContext, Ray3d, bool)>, // 相机阶段冻结输入与作者点击边沿，场景发布后才查询。
    task: Option<Task<GroundCompletion>>, // 换代不丢任务句柄，避免CPU工作突破单飞预算。
    desired: Option<GroundRequest>,       // 鼠标移动只覆盖最新请求。
    accepted: Option<GroundCompletion>,   // 记录原序号，持续移动时每个完成点只发布一帧。
    pending_author_click: Option<GroundRequest>, // 点击射线在松键后仍由唯一任务完成；新点击只覆盖一项。
    pending_author_surface: Option<GroundCompletion>, // 可见环带只提供冻结XZ种子，等待Mip 0后才完成点击。
    completed_author_click: Option<GroundCompletion>, // 各作者点击业务只消费一次的真实三角命中。
    last_started: u64,  // 稳定未命中不逐帧重试，曲面修订变化会生成新请求。
    next_sequence: u64, // 请求序号跨输入代次单调推进。
    input_epoch: u64,   // 新笔划、松手和交互边界使旧结果立即失效。
    pub pending_same_context: bool, // 正常同代后台延迟保留连接，显式缺页/未命中才断开。
    pub paint_ready: bool, // 显示可保留有效历史命中，写R8只消费本次真实完成或静止同射线。
    pub(super) source_fence_closed: bool, // 内部世界编辑只能在源图完成，不能穿透可见引用写入父图。
    held: bool,         // 捕获实际按压边沿，测试或模态吞边沿时也保持安全。
    accepted_surface_point: Option<Vec3>, // 当前可见三角命中的XZ种子；Y不能进入作者结果。
    accepted_authority_ready: bool, // 只有Mip 0三角面完成后才可发布point。
} // 页内存仅由在途snapshot的Arc共享。

impl VegetationGroundCursor {
    pub fn source_authoring_available(&self) -> bool {
        !self.source_fence_closed
    }

    pub fn frozen_click_snap(&self) -> Option<feathrise_editor_core::EditorTranslationSnap> {
        self.completed_author_click.as_ref()?.request.context.placement_snap
    }
    /// 显示圈允许保留最近有效洞面；作者落笔仍只消费paint_ready严格身份。
    pub fn has_volume_cursor(&self) -> bool {
        self.accepted
            .as_ref()
            .is_some_and(|h| h.terrain_mesh && h.point.is_some())
    }
    /// 返回后台已经证明的三维命中射线；法线投射留给现有体积计算槽。
    pub fn volume_paint_ray(&self) -> Option<(Vec3, Vec3)> {
        if !self.paint_ready {
            return None;
        }
        let hit = self
            .accepted
            .as_ref()
            .filter(|h| h.terrain_mesh && h.point.is_some())?;
        Some((
            Vec3::from_array(hit.request.origin.map(f32::from_bits)),
            Vec3::from_array(hit.request.direction.map(f32::from_bits)),
        ))
    }
    // 主线程方法只处理固定大小身份与就绪结果。
    pub fn completed_status(&self) -> Option<ClipmapGroundHit> {
        self.accepted.as_ref().map(|hit| hit.result)
    } // 显式无命中状态不同于后台尚未完成。

    pub fn accepted_identity(&self) -> Option<([u64; 3], [UVec4; 16])> {
        self.accepted
            .as_ref()
            .filter(|hit| hit.point.is_some())
            .map(|hit| {
                (
                    hit.request.context.surface_revision,
                    hit.request.context.missing_pages,
                )
            })
    } // 笔刷覆盖必须绑定命中原曲面。

    /// 语义预览只能引用真实命中时冻结的地图和高度修订，不能给旧显示点冒领当前会话身份。
    pub(crate) fn accepted_preview_identity(&self) -> Option<(&MapId, u64, [u64; 3], [UVec4; 16])> {
        let hit = self.accepted.as_ref().filter(|hit| hit.point.is_some())?;
        let context = &hit.request.context;
        Some((&context.map_id, context.terrain_revision, context.surface_revision, context.missing_pages))
    }

    /// 雕刻只消费本帧可绘制、已取得基础作者Y且仍属于当前显示修订的命中。
    pub fn accepted_sculpt_hit(
        &self,
        snapshot: &ClipmapGroundSnapshot,
        map_id: &MapId,
        tool: EditorTool,
        terrain_revision: u64,
    ) -> Option<Vec3> {
        self.sculpt_hit_for_identity(
            snapshot.ready,
            snapshot.revision,
            &snapshot.missing_pages,
            snapshot.authoring_height_revision(),
            map_id,
            tool,
            terrain_revision,
        )
    }

    /// 连续作者笔刷只受理当前按压的精确点；显示保留的旧点不能成为新样本。
    pub fn accepted_paint_hit(
        &self,
        snapshot: &ClipmapGroundSnapshot,
        map_id: &MapId,
        target: &GroundCursorTarget,
        terrain_revision: u64,
    ) -> Option<Vec3> {
        self.brush_hit_for_identity(
            snapshot.ready,
            snapshot.revision,
            &snapshot.missing_pages,
            snapshot.authoring_height_revision(),
            map_id,
            target,
            terrain_revision,
        )
    }

    /// 极短的体积点击在松键后仍只消费一次；地图、作者和表面换代均拒收。
    pub fn take_volume_click(
        &mut self,
        snapshot: &ClipmapGroundSnapshot,
        map: &MapId,
        tool: EditorTool,
        revision: u64,
    ) -> Option<ClipmapGroundHit> {
        let target = GroundCursorTarget::VolumeSculpt(tool);
        let completion = self.take_completed_author_click_for(&target)?;
        let context = &completion.request.context;
        (snapshot.ready
            && context.map_id == *map
            && context.authoring_revision == Some(revision)
            && snapshot.authoring_height_revision() == Some(context.terrain_revision)
            && snapshot.revision == context.surface_revision
            && snapshot.missing_pages == context.missing_pages)
            .then_some(completion.result)
    }

    pub fn volume_click_pending(&self, map: &MapId, tool: EditorTool) -> bool {
        let target = GroundCursorTarget::VolumeSculpt(tool);
        let matches = |request: &GroundRequest| {
            request.context.map_id == *map && request.context.target.as_ref() == Some(&target)
        };
        self.pending_author_click.as_ref().is_some_and(matches)
            || self
                .pending_author_surface
                .as_ref()
                .is_some_and(|c| matches(&c.request))
            || self
                .completed_author_click
                .as_ref()
                .is_some_and(|c| matches(&c.request))
    }

    fn sculpt_hit_for_identity(
        &self,
        ready: bool,
        revision: [u64; 3],
        missing_pages: &[UVec4; 16],
        adopted_terrain_revision: Option<u64>,
        map_id: &MapId,
        tool: EditorTool,
        terrain_revision: u64,
    ) -> Option<Vec3> {
        self.brush_hit_for_identity(
            ready,
            revision,
            missing_pages,
            adopted_terrain_revision,
            map_id,
            &GroundCursorTarget::TerrainSculpt(tool),
            terrain_revision,
        )
    }

    fn brush_hit_for_identity(
        &self,
        ready: bool,
        revision: [u64; 3],
        missing_pages: &[UVec4; 16],
        adopted_terrain_revision: Option<u64>,
        map_id: &MapId,
        target: &GroundCursorTarget,
        terrain_revision: u64,
    ) -> Option<Vec3> {
        let accepted = self.accepted.as_ref()?;
        let context = &accepted.request.context;
        (!self.source_fence_closed && self.paint_ready
            && self.accepted_authority_ready
            && self
                .desired
                .as_ref()
                .is_some_and(|desired| desired.context == *context)
            && context.held
            && (!matches!(target, GroundCursorTarget::Surface { .. } | GroundCursorTarget::Puddles
                | GroundCursorTarget::Domain(crate::game_authoring::map_interaction::DomainGroundTarget { continuous: true, .. }) | GroundCursorTarget::BoundaryDrag { .. })
                || context.modifiers & 2 == 0) // Ctrl选择不写二维画布，Shift身份仍由上下文冻结。
            && ready
            && adopted_terrain_revision == Some(terrain_revision)
            && context.terrain_revision == terrain_revision
            && &context.map_id == map_id
            && context.target.as_ref() == Some(target)
            && context.surface_revision == revision
            && &context.missing_pages == missing_pages)
            .then_some(accepted.point?)
    } // 同代连续移动允许消费本帧完成的原射线，但不能复用正在等待的旧点。

    /// 静水层创建和编辑只读取当前地图、目标和高度修订完全一致的真实命中。
    pub fn accepted_still_water_hit(
        &self,
        snapshot: &ClipmapGroundSnapshot,
        map_id: &MapId,
        layer_id: Option<&str>,
    ) -> Option<Vec3> {
        let accepted = self.accepted.as_ref()?;
        let expected = GroundCursorTarget::StillWater(layer_id.map(str::to_owned));
        (!self.source_fence_closed && self.paint_ready
            && self.desired.as_ref().is_some_and(|desired| {
                desired.sequence == accepted.request.sequence
                    && desired.context == accepted.request.context
            })
            && snapshot.ready
            && snapshot.authoring_height_revision()
                == Some(accepted.request.context.terrain_revision)
            && &accepted.request.context.map_id == map_id
            && accepted.request.context.target.as_ref() == Some(&expected)
            && accepted.request.context.surface_revision == snapshot.revision
            && accepted.request.context.missing_pages == snapshot.missing_pages)
            .then_some(accepted.point?)
    }

    /// 冻结一次作者点击；若旧悬停查询仍在途，本请求保留为唯一高优先级待处理项。
    pub(super) fn queue_author_click(&mut self, context: GroundCursorContext, ray: Ray3d) {
        self.retarget(context, ray);
        self.pending_author_click = self.desired.clone();
        self.pending_author_surface = None;
        self.completed_author_click = None; // 新点击明确淘汰尚未消费的旧点击结果。
    }

    /// 查询当前特征是否仍有一次冻结点击等待CPU完成。
    pub fn spline_click_pending(
        &self,
        map_id: &MapId,
        feature_id: &str,
        waterfall: bool,
        authoring_revision: u64,
        terrain_revision: u64,
    ) -> bool {
        let expected = if waterfall {
            GroundCursorTarget::Waterfall(feature_id.to_owned())
        } else {
            GroundCursorTarget::River(feature_id.to_owned())
        };
        self.pending_author_click.as_ref().is_some_and(|request| {
            request.context.map_id == *map_id
                && request.context.target.as_ref() == Some(&expected)
                && request.context.authoring_revision == Some(authoring_revision)
                && request.context.terrain_revision == terrain_revision
        })
    }

    /// 只消费一次与当前地图、特征、作者修订和已采用高度完全一致的点击结果。
    pub fn take_spline_click_result(
        &mut self,
        snapshot: &ClipmapGroundSnapshot,
        map_id: &MapId,
        feature_id: &str,
        waterfall: bool,
        authoring_revision: u64,
    ) -> Option<ClipmapGroundHit> {
        self.take_spline_click_result_for_identity(
            snapshot.ready,
            snapshot.revision,
            &snapshot.missing_pages,
            snapshot.authoring_height_revision(),
            map_id,
            feature_id,
            waterfall,
            authoring_revision,
        )
    }

    /// 把曲面身份校验收口为纯值入口，测试可证明一次性消费而无需构造场景私有分页数据。
    fn take_spline_click_result_for_identity(
        &mut self,
        surface_ready: bool,
        surface_revision: [u64; 3],
        missing_pages: &[UVec4; 16],
        surface_authoring_revision: Option<u64>,
        map_id: &MapId,
        feature_id: &str,
        waterfall: bool,
        authoring_revision: u64,
    ) -> Option<ClipmapGroundHit> {
        let expected = if waterfall {
            GroundCursorTarget::Waterfall(feature_id.to_owned())
        } else {
            GroundCursorTarget::River(feature_id.to_owned())
        };
        let completion = self.take_completed_author_click_for(&expected)?;
        (surface_ready
            && surface_authoring_revision == Some(completion.request.context.terrain_revision)
            && completion.request.context.map_id == *map_id
            && completion.request.context.target.as_ref() == Some(&expected)
            && completion.request.context.authoring_revision == Some(authoring_revision)
            && completion.request.context.surface_revision == surface_revision
            && &completion.request.context.missing_pages == missing_pages)
            .then_some(completion.result)
    }

    /// 水位按钮消费后改写用途身份，迟到笔刷不能继续复用同一个完成值。
    pub fn take_water_level_hit(
        &mut self,
        snapshot: &ClipmapGroundSnapshot,
        map_id: &MapId,
        layer_id: &str,
    ) -> Option<Vec3> {
        let point = self.accepted_still_water_hit(snapshot, map_id, Some(layer_id))?;
        self.accepted.as_mut()?.request.context.target =
            Some(GroundCursorTarget::WaterLevel(layer_id.to_owned()));
        self.paint_ready = false;
        Some(point)
    }

    /// 普通新资产只消费与当前文件、子资产和作者修订完全一致的一次点击。
    pub fn take_place_asset_click_result(
        &mut self,
        snapshot: &ClipmapGroundSnapshot,
        map_id: &MapId,
        asset_path: &str,
        asset_target: MapPlacementAssetTarget,
        authoring_revision: u64,
    ) -> Option<ClipmapGroundHit> {
        self.take_placement_click_result_for_identity(
            snapshot.ready,
            snapshot.revision,
            &snapshot.missing_pages,
            snapshot.authoring_height_revision(),
            map_id,
            GroundCursorTarget::PlaceAsset {
                asset_path: asset_path.to_owned(),
                asset_target,
            },
            authoring_revision,
        )
    }

    /// 已有普通摆放吸附只消费当前稳定记录的一次点击；切换选择或作者变化会拒收旧值。
    pub fn take_relocation_click_result(
        &mut self,
        snapshot: &ClipmapGroundSnapshot,
        map_id: &MapId,
        placement_id: &str,
        authoring_revision: u64,
    ) -> Option<ClipmapGroundHit> {
        self.take_placement_click_result_for_identity(
            snapshot.ready,
            snapshot.revision,
            &snapshot.missing_pages,
            snapshot.authoring_height_revision(),
            map_id,
            GroundCursorTarget::RelocatePlacement(placement_id.to_owned()),
            authoring_revision,
        )
    }

    /// 作者点放复用一次性点击门，业务目标必须与冻结射线逐项相同。
    pub fn take_author_click_result(
        &mut self,
        snapshot: &ClipmapGroundSnapshot,
        map_id: &MapId,
        target: GroundCursorTarget,
        authoring_revision: u64,
    ) -> Option<ClipmapGroundHit> {
        self.take_placement_click_result_for_identity(
            snapshot.ready,
            snapshot.revision,
            &snapshot.missing_pages,
            snapshot.authoring_height_revision(),
            map_id,
            target,
            authoring_revision,
        )
    }

    /// 把摆放点击采用门收口为纯值入口，测试无需构造场景私有分页数据。
    fn take_placement_click_result_for_identity(
        &mut self,
        surface_ready: bool,
        surface_revision: [u64; 3],
        missing_pages: &[UVec4; 16],
        surface_authoring_revision: Option<u64>,
        map_id: &MapId,
        target: GroundCursorTarget,
        authoring_revision: u64,
    ) -> Option<ClipmapGroundHit> {
        let completion = self.take_completed_author_click_for(&target)?;
        (surface_ready
            && surface_authoring_revision == Some(completion.request.context.terrain_revision)
            && completion.request.context.map_id == *map_id
            && completion.request.context.target.as_ref() == Some(&target)
            && completion.request.context.authoring_revision == Some(authoring_revision)
            && completion.request.context.surface_revision == surface_revision
            && &completion.request.context.missing_pages == missing_pages)
            .then_some(completion.result)
    }

    /// 先核对消费者所属用途再取走完成值；同域旧身份仍交由上层完整校验后丢弃。
    fn take_completed_author_click_for(
        &mut self,
        expected: &GroundCursorTarget,
    ) -> Option<GroundCompletion> {
        if self.source_fence_closed { return None; } // 索引在途时保留冻结点击，范围确定后才决定消费或取消。
        let actual = self
            .completed_author_click
            .as_ref()?
            .request
            .context
            .target
            .as_ref()?;
        let same_owner = matches!(
            (actual, expected),
            (
                GroundCursorTarget::River(_) | GroundCursorTarget::Waterfall(_),
                GroundCursorTarget::River(_) | GroundCursorTarget::Waterfall(_)
            )
        ) || std::mem::discriminant(actual) == std::mem::discriminant(expected);
        if !same_owner {
            return None; // 建筑工具与院落模式可同帧存在，错误消费者不能吞掉正确消费者的完成票据。
        }
        self.completed_author_click.take()
    }

    pub(super) fn observe_input(&mut self, held: bool, edge: bool) -> u64 {
        // 鼠标移动不增加输入代次。
        if self.held != held || edge {
            self.input_epoch = self.input_epoch.wrapping_add(1);
        } // 两次同位置点击不能混用旧完成值。
        self.held = held; // 保留真实按压状态。
        self.input_epoch // 调用方将当前代次冻结进上下文。
    } // modifier和层身份由context直接比较。

    pub(super) fn clear(&mut self) {
        // 相机无效、关闭、换工具或离开视口时立即撤销光标权威。
        self.pending_same_context = false; // 取消不能跨边界连接。
        self.paint_ready = false; // 取消落笔资格。
        self.captured = None; // 取消未提交输入。
        self.desired = None; // 不保留稍后可突然启动的旧请求。
        self.accepted = None; // 旧落点不能供笔刷或覆盖图使用。
        self.pending_author_click = None; // 换工具、换图或视口失效必须丢弃迟到作者点击。
        self.pending_author_surface = None; // 可见环带种子也属于已取消点击。
        self.completed_author_click = None;
        self.accepted_surface_point = None;
        self.accepted_authority_ready = false;
        if self.task.as_mut().and_then(check_ready).is_some() {
            self.task = None;
        } // 只回收真正完成的single-flight槽。
    } // 尚在运行的任务下一帧继续占槽，但完成值不会被采用。

    pub(super) fn source_fence_points(&self) -> [Option<Vec3>; 3] {
        [self.accepted.as_ref().and_then(|hit| hit.point),
            self.completed_author_click.as_ref().and_then(|hit| hit.point),
            self.pending_author_surface.as_ref().and_then(|hit| hit.point)]
    }
    pub(super) fn reject_source_click(&mut self) {
        self.pending_author_click = None; self.pending_author_surface = None; self.completed_author_click = None;
    }

    fn retarget(&mut self, context: GroundCursorContext, ray: Ray3d) {
        // 只在真实输入发生变化时产生下一序号。
        let origin = ray.origin.to_array().map(f32::to_bits); // 位模式保存原输入。
        let direction = ray.direction.to_array().map(f32::to_bits); // 不把相近光标误判为同一点。
        if self.desired.as_ref().is_some_and(|request| {
            request.context == context && request.origin == origin && request.direction == direction
        }) {
            return;
        } // 稳定光标复用既有完成结果。
        if self
            .desired
            .as_ref()
            .is_none_or(|request| request.context != context)
        {
            self.accepted = None;
        } // 高度、视图、层或按压换代立即清空旧可见命中。
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1); // 零保留给尚未启动状态。
        self.desired = Some(GroundRequest {
            context,
            sequence: self.next_sequence,
            origin,
            direction,
        }); // 常数空间合并最新射线。
    } // 不在输入帧复制高度页。

    fn adopt(&mut self, completion: GroundCompletion) -> bool {
        // 只接收当前上下文的有序完成值。
        let Some(desired) = self.desired.as_ref() else {
            return false;
        }; // 已取消输入拒绝任何旧完成值。
        if completion.request.context != desired.context {
            return false;
        } // 同代悬停与按压均允许原射线一帧延迟；松手改变输入代次拒绝旧按压。
        if self
            .accepted
            .as_ref()
            .is_some_and(|accepted| accepted.request.sequence >= completion.request.sequence)
        {
            return false;
        } // 同一采样不能重复或倒序采用。
        self.accepted = Some(completion); // 点与原请求序号作为完整结果一次发布。
        true // 本帧允许消费该冻结落点，不把它标成最新射线命中。
    } // 曲面Waiting/Miss/Failed也以完整无落点结果覆盖旧点。

    fn retain_author_click_scope(&mut self, context: &GroundCursorContext) {
        let matches = |request: &GroundRequest| {
            request
                .context
                .target
                .as_ref()
                .is_some_and(GroundCursorTarget::is_author_click)
                && request.context.map_id == context.map_id
                && request.context.target == context.target
                && request.context.terrain_revision == context.terrain_revision
                && request.context.surface_revision == context.surface_revision
                && request.context.missing_pages == context.missing_pages
                && request.context.authoring_revision == context.authoring_revision
                && request.context.view == context.view
                && match (request.context.placement_snap, context.placement_snap) {
                    (Some(before), Some(now)) => before.same_preferences(now)
                        && request.context.modifiers & !2 == context.modifiers & !2,
                    (None, None) => request.context.modifiers == context.modifiers,
                    _ => false,
                }
        }; // 松键、鼠标移动和相机移动不改变已经冻结的点击射线。
        if self
            .pending_author_click
            .as_ref()
            .is_some_and(|request| !matches(request))
        {
            self.pending_author_click = None;
        }
        if self
            .completed_author_click
            .as_ref()
            .is_some_and(|completion| !matches(&completion.request))
        {
            self.completed_author_click = None;
        }
        if self
            .pending_author_surface
            .as_ref()
            .is_some_and(|completion| !matches(&completion.request))
        {
            self.pending_author_surface = None;
        }
    }

    /// 用可见环带命中的世界XZ查询局部Mip 0；等待时保留种子但不发布粗LOD的Y。
    fn resolve_authoritative_completion(
        snapshot: &ClipmapGroundSnapshot,
        completion: &mut GroundCompletion,
        surface_point: Vec3,
    ) -> bool {
        Self::apply_authoritative_result(
            completion,
            if completion.terrain_mesh {
                ClipmapGroundHit::Hit(surface_point)
            } else {
                snapshot.sample_authoritative_surface(surface_point.xz())
            },
            false,
        )
    }

    /// 把基础作者三角面的结果写回同一次冻结射线；作者点击等待时必须保留可见面XZ种子。
    fn apply_authoritative_result(
        completion: &mut GroundCompletion,
        result: ClipmapGroundHit,
        preserve_surface_seed_while_waiting: bool,
    ) -> bool {
        match result {
            ClipmapGroundHit::Hit(point) => {
                completion.result = ClipmapGroundHit::Hit(point);
                completion.point = Some(point);
                true
            }
            ClipmapGroundHit::WaitingForHeight => {
                if !preserve_surface_seed_while_waiting {
                    completion.result = ClipmapGroundHit::WaitingForHeight;
                    completion.point = None;
                } // 冻结点击没有第二份XZ字段，等待期间保留种子但不会发布给业务。
                false
            }
            ClipmapGroundHit::Miss => {
                completion.result = ClipmapGroundHit::Miss;
                completion.point = None;
                true
            }
            ClipmapGroundHit::Failed => {
                completion.result = ClipmapGroundHit::Failed;
                completion.point = None;
                true
            }
        }
    }

    pub(super) fn resolve(
        &mut self,
        snapshot: &ClipmapGroundSnapshot,
        context: GroundCursorContext,
        ray: Ray3d,
    ) -> Option<Vec3> {
        // 调用方已证明属于受支持的Clipmap作者工具。
        if !snapshot.ready
            || snapshot.map_id != context.map_id
            || snapshot.authoring_height_revision() != Some(context.terrain_revision)
            || snapshot.revision != context.surface_revision
        {
            self.clear();
            return None;
        } // 未采用曲面不允许创建伪平地落点。
        self.retarget(context, ray); // 合并最新鼠标请求后再检查在途完成值。
        let current_context = self
            .desired
            .as_ref()
            .expect("retarget后必须存在当前请求")
            .context
            .clone();
        self.retain_author_click_scope(&current_context);
        let mut fresh = false; // 连续移动时旧完成点只允许在真实完成帧出现一次。
        if let Some(completion) = self.task.as_mut().and_then(check_ready) {
            // 绝不阻塞等待CPU射线。
            self.task = None; // 真实完成后释放唯一并发槽。
            if self
                .pending_author_click
                .as_ref()
                .is_some_and(|request| *request == completion.request)
            {
                self.pending_author_surface = Some(completion.clone());
            }
            fresh = self.adopt(completion); // 地形/相机/按压换代的迟到结果被拒绝。
            if fresh {
                self.accepted_surface_point = self.accepted.as_ref().and_then(|accepted| {
                    matches!(accepted.result, ClipmapGroundHit::Hit(_))
                        .then_some(accepted.point)
                        .flatten()
                });
                self.accepted_authority_ready = self.accepted_surface_point.is_none();
            }
        } // 本帧可立刻启动合并后的最新请求。
        if !self.accepted_authority_ready
            && let Some(surface_point) = self.accepted_surface_point
            && let Some(accepted) = self.accepted.as_mut()
            && Self::resolve_authoritative_completion(snapshot, accepted, surface_point)
        {
            self.accepted_authority_ready = true;
            fresh = true; // 权威页在后续帧到达时，本次才是可落笔的新完成值。
        }
        let author_ready = self
            .pending_author_surface
            .as_mut()
            .is_some_and(|completion| {
                let ClipmapGroundHit::Hit(surface_point) = completion.result else {
                    return true; // 可见环带已经明确未命中或失败，无需等待Mip 0。
                };
                Self::apply_authoritative_result(
                    completion,
                    if completion.terrain_mesh {
                        ClipmapGroundHit::Hit(surface_point)
                    } else {
                        snapshot.sample_authoritative_surface(surface_point.xz())
                    },
                    true,
                ) // 松键后仍用原可见命中的XZ等待权威页，期间不发布粗LOD的Y。
            });
        if author_ready {
            self.pending_author_click = None;
            self.completed_author_click = self.pending_author_surface.take();
        }
        let request_to_start = self
            .pending_author_click
            .as_ref()
            .filter(|request| request.sequence != self.last_started)
            .or_else(|| {
                self.desired
                    .as_ref()
                    .filter(|request| request.sequence != self.last_started)
            })
            .cloned(); // 点击优先于后续悬停射线，仍保持一个在途加一个合并请求。
        if self.task.is_none()
            && let Some(request) = request_to_start
        {
            // 最多一个在途，没有待处理队列。
            self.last_started = request.sequence; // 同一失败或未命中输入不重复计算。
            let surface = snapshot.clone(); // Arc共享已采用页，不复制整图数据。
            self.task = Some(AsyncComputeTaskPool::get().spawn(async move {
                // 可能跨多个显示单元的遍历必须离开帧关键路径。
                let origin = Vec3::from_array(request.origin.map(f32::from_bits)); // 使用本请求冻结的起点。
                let direction = Vec3::from_array(request.direction.map(f32::from_bits)); // 使用本请求冻结的方向。
                let height_only = matches!(
                    request.context.target.as_ref(),
                    Some(GroundCursorTarget::TerrainSculpt(_) | GroundCursorTarget::Puddles)
                );
                let mut result = if height_only {
                    surface.height_ray_intersection(origin, direction)
                } else {
                    surface.ray_intersection(origin, direction)
                }; // 静态三维地形由 Blender 编辑，稀疏笔刷不穿洞改写隐藏高度。
                if let Some(snap) = request.context.placement_snap {
                    result = super::snapping::surface::clipmap_seed(&surface, result, snap);
                }
                let point = match result {
                    ClipmapGroundHit::Hit(point) => Some(point), // 只作为Mip 0查询的世界XZ种子。
                    _ => None,
                }; // 不读取水、建筑或道具碰撞。
                GroundCompletion {
                    terrain_mesh: !height_only
                        && point.is_some_and(|point| surface.is_three_dimensional_surface(point)),
                    request,
                    result,
                    point,
                } // 返回同一次查询的真实落点与原序号。
            })); // 下一帧非阻塞轮询。
        } // 移动中的中间请求仅保留最后一个。
        let accepted = self.accepted.as_ref()?; // 没有新完成值时保持未命中。
        let desired = self.desired.as_ref()?; // 无交互请求不发布旧点。
        (fresh || accepted.request.sequence == desired.sequence)
            .then_some(accepted.point)
            .flatten() // 稳定光标可持续刷；移动时不重复刷前一射线的旧点。
    } // 场景自动页需求独立于光标落点，不依赖该结果去触发预取。
} // 保持GLB及其他工具的旧同步拾取路径不变。

/// 场景本帧页表与曲面发布后才采用精确落点，随后才允许各作者工具消费。
pub fn resolve_vegetation_ground_cursor(
    mut ground: ResMut<VegetationGroundCursor>,
    surface: Option<Res<feathrise_scene::DynamicClipmapGroundSurface>>,
    mut editor: ResMut<crate::resources::MapEditorState>,
) {
    // 不要求独立测试安装scene插件。
    let Some((mut context, ray, author_click)) = ground.captured.take() else {
        ground.clear();
        return;
    }; // 工具、窗口或视口失效立即取消。
    let Some(snapshot) = surface
        .as_ref()
        .and_then(|surface| surface.snapshot.as_ref())
    else {
        ground.clear();
        return;
    }; // 没有正式采用曲面就保持等待。
    if snapshot.map_id != context.map_id
        || snapshot.authoring_height_revision() != Some(context.terrain_revision)
    {
        ground.clear();
        editor.cursor_world = None;
        editor.brush_cursor_world = None;
        editor.status = "正在等待当前作者高度修订正式采用；地表操作暂停".to_owned();
        return; // 旧页即使ready也不能为新雕刻修订提供落点。
    }
    context.surface_revision = snapshot.revision;
    context.missing_pages = snapshot.missing_pages; // 完整采用身份包含缺页状态。
    if author_click {
        ground.queue_author_click(context.clone(), ray); // 点击射线先冻结，再允许后续悬停覆盖latest请求。
    }
    let point = ground.resolve(snapshot, context, ray); // 非阻塞投递/轮询唯一任务。
    ground.pending_same_context = point.is_none()
        && ground.task.is_some()
        && ground
            .accepted
            .as_ref()
            .is_some_and(|hit| hit.point.is_some()); // 同代任务等待暂停写入但不打断连续刷段。
    ground.paint_ready = point.is_some(); // 鼠标移动查询期间禁止重复刷旧射线。
    if ground.completed_status() == Some(ClipmapGroundHit::WaitingForHeight) {
        editor.status = match ground
            .desired
            .as_ref()
            .and_then(|request| request.context.target.as_ref())
        {
            Some(GroundCursorTarget::TerrainSculpt(_)) => {
                "正在等待当前地形高度页；地形雕刻暂停".to_owned()
            }
            Some(GroundCursorTarget::Surface { .. }) => {
                "正在等待当前地形高度页；地表绘制暂停".to_owned()
            }
            Some(GroundCursorTarget::Puddles) => "正在等待当前地形高度页；积水绘制暂停".to_owned(),
            Some(
                GroundCursorTarget::Domain(_),
            ) => "\u{6b63}\u{5728}\u{7b49}\u{5f85}\u{5f53}\u{524d}\u{5730}\u{5f62}\u{9ad8}\u{5ea6}\u{9875}\u{ff1b}\u{6269}\u{5c55}\u{5de5}\u{5177}\u{843d}\u{70b9}\u{6682}\u{505c}".to_owned(),
            Some(GroundCursorTarget::Portal(_)) => {
                "正在等待当前地形高度页；传送点落点暂停".to_owned()
            }
            Some(
                GroundCursorTarget::BoundaryPoint { .. } | GroundCursorTarget::BoundaryDrag { .. },
            ) => "正在等待当前地形高度页；空气墙落点暂停".to_owned(),
            Some(GroundCursorTarget::StillWater(_) | GroundCursorTarget::WaterLevel(_)) => {
                "正在等待当前地形高度页；静水笔刷暂停".to_owned()
            }
            Some(GroundCursorTarget::River(_) | GroundCursorTarget::Waterfall(_)) => {
                "正在等待当前地形高度页；样条水落点暂停".to_owned()
            }
            Some(
                GroundCursorTarget::PlaceAsset { .. } | GroundCursorTarget::RelocatePlacement(_),
            ) => "正在等待当前地形高度页；摆放物落点暂停".to_owned(),
            Some(
                GroundCursorTarget::Building { .. }
                | GroundCursorTarget::RelocateBuilding { .. }
                | GroundCursorTarget::Courtyard { .. },
            ) => "正在等待当前地形高度页；建筑与院落落点暂停".to_owned(),
            _ => "正在等待当前地形高度页；植被笔刷暂停".to_owned(),
        };
    } // 明确等待真实地表而非静默刷平地。
    let display = ground.accepted.as_ref().and_then(|hit| hit.point); // 同上下文有效命中保留显示，避免后台帧闪烁。
    editor.cursor_world = display;
    editor.brush_cursor_world = display; // 两个消费者使用同一真实地形交点。
} // 三角身份不伪造GLB triangle索引。

#[cfg(test)] // 非GPU身份回归独立于场景曲面几何测试。
mod tests; // 覆盖连续移动、松手、新笔划、地图、层和视图换代。
