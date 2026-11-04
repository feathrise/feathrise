//! 中央视口当前传送点的选择标记、紧凑身份徽标与双击定位。
//!
//! 【功能设计】左侧当前地图传送点行单击只改变选择并显示F12专属世界标记；快速双击同一
//! `MapId + portal_id`才调整现有编辑相机。传送点位置直接来自小型作者记录，不扫描世界实体。

use bevy::prelude::*; // Resource、Vec3和纯取景数学。
use feathrise_map_content::MapId; // 双击与选择必须隔离不同地图中的相同传送点ID。
use feathrise_portal::{PORTAL_EDGE_BEAM_HEIGHT, PORTAL_VISUAL_RADIUS}; // 定位范围覆盖正式完整视觉高度。

use super::camera_math::{MAX_EXTENT_METERS, MIN_EXTENT_METERS}; // 定位继续服从CAD视口统一上下界。

mod systems; // 作者记录解析、中央世界标记和相机定位。
mod ui; // 视口左上角紧凑身份徽标。

pub(crate) use systems::{draw_selected_portal_marker, sync_selected_portal_viewport};
pub(crate) use ui::setup_selected_portal_badge;

const DOUBLE_CLICK_MAX_SECONDS: f64 = 0.36; // 与静水和Scene0网格保持相同双击节奏。
const FOCUS_PADDING_SCALE: f32 = 1.25; // 完整十四米光幕外仍保留少量环境上下文。
const FOCUS_PADDING_METERS: f32 = 2.0; // 小型底盘不会被相机贴得过近。

/// 当前地图和传送点稳定ID共同组成唯一选择及双击身份。
#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectedPortalIdentity {
    map_id: MapId,     // 切图后同ID不得继承旧点击时间窗。
    portal_id: String, // 名称允许修改或重名，不能承担交互身份。
}

/// 当前选择用于徽标、世界标记和定位的小型作者快照。
#[derive(Clone, Debug)]
struct SelectedPortalInfo {
    identity: SelectedPortalIdentity, // 双击请求只接受完全相同的当前对象。
    label: String,                    // 世界Billboard同源的人类可读传送点名称。
    position: Vec3,                   // 当前地图中的真实触发中心。
}

/// 只有当前稳定选择、传送结构或目录句柄变化时才重新定位作者记录。
#[derive(Clone, Debug, PartialEq, Eq)]
struct PortalSelectionLookupSignature {
    preview: bool,         // 悬停切换只影响徽标和世界标记。
    map_id: MapId,         // 草稿换图即使修订数字相同也必须失效。
    portal_id: String,     // 当前列表只按稳定ID选择源点。
    portal_revision: u64,  // 增删、名称修改或撤销后重新读取记录。
    catalog_revision: u64, // 目标地图显示名句柄可能随目录热更新变化。
}

/// 双击时间窗只保留最近一次真实当前地图传送点行按下。
#[derive(Clone, Debug)]
struct TimedPortalClick {
    identity: SelectedPortalIdentity, // 目标列表和其他传送点不能组成双击。
    seconds: f64,                     // F12暂停玩法时间时仍使用真实时间。
}

/// 传送底盘与完整渐隐光幕构成相机取景和Gizmo共用范围。
#[derive(Clone, Copy, Debug, PartialEq)]
struct PortalSelectionFrame {
    position: Vec3, // 地面触发中心供选择环与竖直标杆使用。
    center: Vec3,   // 完整光幕世界中心供现有相机定位。
    size: Vec3,     // 底盘直径与光幕高度组成三维范围。
}

/// 当前传送点选择在中央视口中的有限状态。
#[derive(Resource, Default)]
pub(crate) struct SelectedPortalViewportState {
    last_click: Option<TimedPortalClick>, // 只为下一次同身份按下保留一个时间戳。
    focus_request: Option<SelectedPortalIdentity>, // 双击登记后由Cursor阶段同帧消费。
    explicit_focus: bool, // 只有F/工具条取景越过悬停预览，双击仍等待恢复正式目标。
    resolved_identity: Option<SelectedPortalIdentity>, // 世界标记当前属于哪张图的哪个点。
    lookup_signature: Option<PortalSelectionLookupSignature>, // 稳定帧禁止重复扫描传送数组。
    info: Option<SelectedPortalInfo>,     // 已解析名称、位置与稳定身份的小型完成值。
    frame: Option<PortalSelectionFrame>,  // Presentation不再反查作者数组。
    badge_text: Option<String>,           // 文案不变时不触发Text重排。
}

impl SelectedPortalViewportState {
    pub(crate) fn request_focus(&mut self, map_id: &MapId, id: &str) {
        self.focus_request = Some(SelectedPortalIdentity { map_id: map_id.clone(), portal_id: id.into() });
        self.explicit_focus = true;
    }

    fn explicit_focus_pending(&self) -> bool { self.explicit_focus && self.focus_request.is_some() }

    pub(crate) fn cancel_focus(&mut self) { self.clear_click_sequence(); }

    /// 记录当前地图传送点行按下；只有同一稳定身份的快速第二次按下才登记定位。
    pub(crate) fn register_portal_click(
        &mut self,
        map_id: &MapId,
        portal_id: &str,
        now_seconds: f64,
    ) -> bool {
        let identity = SelectedPortalIdentity {
            map_id: map_id.clone(),
            portal_id: portal_id.to_owned(),
        };
        self.focus_request = None; // 新按下先撤销尚未消费的旧对象定位请求。
        self.explicit_focus = false;
        if !now_seconds.is_finite() {
            self.last_click = None; // 异常时钟值明确按普通单击处理。
            return false;
        }
        let double_clicked = self.last_click.as_ref().is_some_and(|last| {
            last.identity == identity
                && now_seconds >= last.seconds
                && now_seconds - last.seconds <= DOUBLE_CLICK_MAX_SECONDS
        });
        if double_clicked {
            self.focus_request = Some(identity); // Cursor阶段保持当前视图类型，只改中心与量程。
            self.last_click = None; // 三连击第三下重新开始下一组点击。
        } else {
            self.last_click = Some(TimedPortalClick {
                identity,
                seconds: now_seconds,
            });
        }
        double_clicked
    }

    /// 目标列表、分页或其他工具介入时终止旧双击与迟到定位请求。
    pub(crate) fn clear_click_sequence(&mut self) {
        self.last_click = None;
        self.focus_request = None;
        self.explicit_focus = false;
    }
}

/// 从触发中心建立完整正式视觉范围，不依赖运行时实体是否已经热重载完成。
fn portal_selection_frame(position: Vec3) -> PortalSelectionFrame {
    let diameter = PORTAL_VISUAL_RADIUS * 2.0;
    PortalSelectionFrame {
        position,
        center: position + Vec3::Y * (PORTAL_EDGE_BEAM_HEIGHT * 0.5),
        size: Vec3::new(diameter, PORTAL_EDGE_BEAM_HEIGHT, diameter),
    }
}

/// 根据完整光幕范围计算透视及三种正交视图共用的安全量程。
fn portal_focus_extent(frame: PortalSelectionFrame) -> f32 {
    (frame.size.max_element() * FOCUS_PADDING_SCALE + FOCUS_PADDING_METERS)
        .clamp(MIN_EXTENT_METERS, MAX_EXTENT_METERS)
}

#[cfg(test)]
mod tests {
    use super::*; // 双击和固定范围数学无需启动渲染器。

    #[test] // 只有同地图同稳定ID的快速第二次按下才登记相机定位。
    fn portal_double_click_requires_same_complete_identity() {
        let map = MapId::parse("MAP-portal-focus").expect("测试地图ID合法");
        let other_map = MapId::parse("MAP-other-focus").expect("测试地图ID合法");
        let mut state = SelectedPortalViewportState::default();
        assert!(!state.register_portal_click(&map, "portal-a", 1.0));
        assert!(!state.register_portal_click(&map, "portal-b", 1.1)); // 不同点重新开始时间窗。
        assert!(!state.register_portal_click(&other_map, "portal-b", 1.2)); // 跨图同ID仍不是双击。
        assert!(!state.register_portal_click(&map, "portal-a", 2.0)); // 超时旧点击不能匹配。
        assert!(state.register_portal_click(&map, "portal-a", 2.2));
        assert!(state.focus_request.is_some()); // 定位请求等待Cursor阶段采用当前记录。
    }

    #[test] // 定位必须把十四米渐隐光幕整体放进现有视口，而不只对准贴地底盘。
    fn portal_frame_covers_edge_beam_height() {
        let position = Vec3::new(10.0, 3.0, -8.0);
        let frame = portal_selection_frame(position);
        assert_eq!(frame.position, position);
        assert_eq!(frame.center.y, position.y + PORTAL_EDGE_BEAM_HEIGHT * 0.5);
        assert_eq!(frame.size.y, PORTAL_EDGE_BEAM_HEIGHT);
        assert!(portal_focus_extent(frame) > PORTAL_EDGE_BEAM_HEIGHT); // 上下都保留观察余量。
    }
}
