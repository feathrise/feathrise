//! 当前传送点作者快照解析、中央世界标记与双击相机定位。

use bevy::prelude::*; // 资产、资源、UI查询和F12专属Gizmo。
use feathrise_map_content::{MapContentRegistry, MapManifestAsset}; // 默认传送名称解析目标地图显示名。
use feathrise_portal::{PORTAL_EDGE_BEAM_HEIGHT, PORTAL_VISUAL_RADIUS}; // 标记与正式底盘/光幕共用尺度。
use crate::ui::theme as colors; // 铜金选择线延续F12作者视觉语言。

use super::ui::sync_badge; // 作者快照状态增量回显到固定中央徽标。
use super::{
    PortalSelectionFrame, PortalSelectionLookupSignature, SelectedPortalIdentity,
    SelectedPortalInfo, SelectedPortalViewportState, portal_focus_extent, portal_selection_frame,
};
use crate::authoring::EditorAuthoringSession; // 当前未保存草稿是选择位置唯一权威来源。
use crate::components::{MapEditorSelectedPortalBadge, MapEditorSelectedPortalBadgeText};
use crate::resources::{MapEditorState, MapEditorViewportState}; // 双击只调整现有编辑相机会话态。
use crate::types::EditorTool; // 只有传送点工具显示本选择表现。
use crate::ui::portal_list::PortalDisplayContext; // 世界Billboard与列表共用默认/自定义名称解析。
use crate::viewport::brush_cursor::MapEditorBrushGizmos; // 选择线只进入F12离屏相机。
use crate::viewport::content_selection::{LayerCandidateState, LayerTarget}; // 正式与悬停选择共用唯一视觉所有权。

const MARKER_GROUND_OFFSET: f32 = 0.16; // 铜金环略高于底盘与地形，避免深度闪烁。
const MARKER_RADIUS_PADDING: f32 = 0.55; // 选择环包在正式触发边缘之外，不遮住边缘光。
const MARKER_TOP_EXTRA: f32 = 2.0; // 竖直标杆超过渐隐顶部，远景仍能区分选中项。

/// 从当前草稿和稳定选择建立轻量传送点快照。
fn selected_portal_info(
    portal_id: &str,             // 由当前正式或悬停目标解析，不能改写作者选择。
    author_index: Option<usize>, // 统一候选直接使用后台缓存的受检索引。
    session: &EditorAuthoringSession,
    display: &PortalDisplayContext<'_>,
) -> Option<SelectedPortalInfo> {
    let draft = session.draft.as_ref()?;
    let portal = if let Some(index) = author_index {
        draft
            .portals
            .portals
            .get(index)
            .filter(|portal| portal.id == portal_id)? // 悬停不扫描整张表。
    } else {
        draft
            .portals
            .portals
            .iter()
            .find(|portal| portal.id == portal_id)? // 旧传送列表仅在明确选择边沿查找。
    };
    let position = Vec3::from_array(portal.position);
    position.is_finite().then(|| SelectedPortalInfo {
        identity: SelectedPortalIdentity {
            map_id: draft.map_id.clone(),
            portal_id: portal.id.clone(),
        },
        label: display.portal_label(portal),
        position,
    }) // 非有限坐标不生成相机或Gizmo变换。
}

/// 同步中央徽标和世界范围，并在双击请求匹配当前稳定选择时定位现有相机。
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_selected_portal_viewport(
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    candidates: Res<LayerCandidateState>, // Inspect模式仅显示当前跨类别目标。
    registry: Option<Res<MapContentRegistry>>,
    manifests: Res<Assets<MapManifestAsset>>,
    mut selection: ResMut<SelectedPortalViewportState>,
    viewport: Option<ResMut<MapEditorViewportState>>,
    mut badges: Query<&mut Visibility, With<MapEditorSelectedPortalBadge>>,
    mut badge_texts: Query<&mut Text, With<MapEditorSelectedPortalBadgeText>>,
) {
    let inspected = editor.tool == EditorTool::Inspect; // 选择模式禁止工具状态承载临时悬停。
    let active_id = if inspected {
        let target = if selection.explicit_focus_pending() { candidates.selected.as_ref() }
            else { candidates.hover.as_ref().or(candidates.selected.as_ref()) };
        match target {
            Some(LayerTarget::Portal(id)) => Some(id.as_str()),
            _ => None, // 其他类型接管时隐藏旧传送标记。
        }
    } else if editor.tool == EditorTool::Portal {
        editor.selected_current_portal_id.as_deref()
    } else {
        None
    };
    let active_signature = editor
        .open
        .then(|| {
            let draft = session.draft.as_ref()?;
            let portal_id = active_id?;
            Some(PortalSelectionLookupSignature {
                preview: !selection.explicit_focus_pending() && inspected && matches!(candidates.hover, Some(LayerTarget::Portal(_))), // 显式取景只读取正式选择。
                map_id: draft.map_id.clone(),
                portal_id: portal_id.to_owned(),
                portal_revision: session.portal_revision,
                catalog_revision: registry.as_deref().map_or(0, |registry| registry.revision),
            })
        })
        .flatten();
    let Some(active_signature) = active_signature else {
        sync_badge(false, None, &mut selection, &mut badges, &mut badge_texts);
        selection.resolved_identity = None;
        selection.lookup_signature = None;
        selection.info = None;
        selection.frame = None;
        selection.clear_click_sequence(); // 其他工具或失效选择不能留下迟到相机请求。
        return;
    };

    let lookup_changed =
        selection.lookup_signature.as_ref() != Some(&active_signature) || manifests.is_changed(); // 目标地图显示名热更新时重新生成默认传送名称。
    if lookup_changed {
        let display = PortalDisplayContext::new(&session, registry.as_deref(), &manifests);
        let index = inspected
            .then(|| candidates.portal_index(&active_signature.portal_id))
            .flatten(); // 统一候选保证常数索引。
        let info = if inspected && index.is_none() {
            None
        } else {
            selected_portal_info(&active_signature.portal_id, index, &session, &display)
        }; // 过期候选不回退全表扫描。
        let next_identity = info.as_ref().map(|info| info.identity.clone());
        if selection.resolved_identity != next_identity {
            if selection.focus_request.as_ref() != next_identity.as_ref() {
                selection.focus_request = None; // 不同点的旧定位请求不能穿过选择变化。
            }
            selection.resolved_identity = next_identity;
        }
        selection.frame = info
            .as_ref()
            .map(|info| portal_selection_frame(info.position)); // Gizmo和相机共享同一小型范围。
        selection.info = info; // 完整解析后一次替换，稳定帧不再反查作者数组。
        selection.lookup_signature = Some(active_signature);
    }
    let desired_badge_text = (lookup_changed || selection.badge_text.is_none()).then(|| {
        selection.info.as_ref().map(|info| {
            format!(
                "{}传送点：{} · 位置 {:.1},{:.1},{:.1}",
                if !selection.explicit_focus_pending() && candidates.hover.is_some() && inspected {
                    "预览"
                } else {
                    "已选"
                },
                info.label,
                info.position.x,
                info.position.y,
                info.position.z
            )
        })
    });
    let Some(_) = selection.info.as_ref() else {
        sync_badge(false, None, &mut selection, &mut badges, &mut badge_texts);
        selection.frame = None;
        selection.clear_click_sequence(); // 悬空或损坏记录不保留旧世界位置。
        return;
    };
    sync_badge(
        true,
        desired_badge_text.flatten(),
        &mut selection,
        &mut badges,
        &mut badge_texts,
    );

    let focus_snapshot = selection.info.as_ref().and_then(|info| {
        (selection.focus_request.as_ref() == Some(&info.identity)).then(|| {
            (
                info.label.clone(), // 只有真实双击才复制短名称供状态栏使用。
                info.position,
                portal_selection_frame(info.position),
            )
        })
    });
    let Some((label, position, frame)) = focus_snapshot else {
        return; // 普通单击只显示徽标和世界标记，绝不移动相机。
    };
    let Some(mut viewport) = viewport else {
        return; // 编辑相机资源尚未建立时保留请求到下一帧。
    };
    viewport.center = frame.center; // 保留当前透视、顶、前或侧视图，只移动观察中心。
    viewport.extent = portal_focus_extent(frame).max(crate::viewport::camera_math::framing_extent(
        frame.size * 0.5, viewport.yaw, viewport.pitch, viewport.render_size, editor.orthographic)); // 窄视口也容纳完整光幕。
    selection.focus_request = None;
    editor.status = format!(
        "已定位传送点：{} · {:.1},{:.1},{:.1}",
        label, position.x, position.y, position.z
    );
}

/// 绘制包围正式触发边缘的铜金选择环和超过渐隐顶部的竖直定位标杆。
pub(crate) fn draw_selected_portal_marker(
    editor: Res<MapEditorState>,
    selection: Res<SelectedPortalViewportState>,
    mut gizmos: Gizmos<MapEditorBrushGizmos>,
) {
    if !editor.open || !matches!(editor.tool, EditorTool::Portal | EditorTool::Inspect) {
        return;
    }
    let Some(PortalSelectionFrame { position, .. }) = selection.frame else {
        return; // 没有有效当前源点时不伪造空间位置。
    };
    let ground_center = position + Vec3::Y * MARKER_GROUND_OFFSET;
    let radius = PORTAL_VISUAL_RADIUS + MARKER_RADIUS_PADDING;
    gizmos.circle(
        Isometry3d::new(
            ground_center,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        ),
        radius,
        colors::TEXT_ACCENT,
    ); // 外环不盖住正式底盘和中央箭头。
    let top = position + Vec3::Y * (PORTAL_EDGE_BEAM_HEIGHT + MARKER_TOP_EXTRA);
    gizmos.line(ground_center, top, colors::TEXT_ACCENT); // 高标杆即使光幕被草木遮住仍指出选中项。
    gizmos.circle(
        Isometry3d::new(top, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        0.65,
        colors::TEXT_ACCENT,
    ); // 顶端小环让正交和透视视角都能辨认标杆终点。
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::{IntoSystem, System}; // 初始化生产系统锁定SystemParam访问契约。

    use super::*;

    #[test] // 新增徽标查询不得重新引入Visibility的B0001冲突。
    fn portal_viewport_sync_system_initializes_without_query_conflicts() {
        let mut world = World::new();
        let mut system = IntoSystem::into_system(sync_selected_portal_viewport);
        system.initialize(&mut world);
    }
}
