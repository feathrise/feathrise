//! 建筑候选占地、门前净空与正门方向的 F12 专用 Gizmo 回显。

use bevy::prelude::*; // Gizmos、颜色、资源和世界向量。
use feathrise_map_content::{
    BuildingPresetDescriptor, BuildingPresetRegistry, BuildingWorldQuad,
}; // 预览模型与辅助线复用同一目录快照和正式几何公式。

use super::index::current_identity; // 索引失败时预览明确显示不可落点颜色。
use super::resources::BuildingAuthoringState; // 最近一次后台校验结果。
use super::snapping::resolve_building_pose_snapped; // 幽灵和点击请求共用唯一网格/连接姿态解析。
use super::types::{BuildingConnectionSnapState, BuildingPreviewTone}; // 模型幽灵、连接点和Gizmo共用状态语义。
use crate::authoring::EditorAuthoringSession; // 当前地图和作者修订身份。
use crate::resources::MapEditorState; // 当前工具、选择与世界光标。
use crate::types::EditorTool; // 只有专用建筑工具绘制辅助线。
use crate::ui::building_presets::BuildingPlacementControls; // 当前网格和九十度朝向。
use crate::viewport::MapEditorBrushGizmos; // 线条只进入 F12 离屏相机。

const PREVIEW_HEIGHT_OFFSET: f32 = 0.12; // 线条略高于地表，避免深度闪烁。

/// 一帧建筑模型、占地、门区和正门箭头共同使用的解析结果。
pub(super) struct BuildingPreviewFrame<'a> {
    pub(super) preset: &'a BuildingPresetDescriptor, // 模型来源与几何均来自同一已采用预设。
    pub(super) root: Vec3,                           // 模型根和全部辅助线共享世界位置。
    pub(super) yaw_degrees: f32,                     // 模型与几何共享九十度档位。
    pub(super) footprint: BuildingWorldQuad,         // 实体占地使用正式公共公式。
    pub(super) entrance_clearance: BuildingWorldQuad, // 门前净空不冒充实体碰撞。
    pub(super) tone: BuildingPreviewTone,            // 琥珀、绿色或红色共同驱动画面。
    pub(super) connection: BuildingConnectionSnapState, // 连接点高亮与失败目标回显。
}

/// 解析当前唯一建筑候选；不分配字符串、不加载资产，也不扫描任何场景实体。
pub(super) fn resolve_building_preview<'a>(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    registry: &'a BuildingPresetRegistry,
    controls: &BuildingPlacementControls,
    state: &BuildingAuthoringState,
    snap: Option<feathrise_editor_core::EditorTranslationSnap>,
) -> Option<BuildingPreviewFrame<'a>> {
    if !editor.open || editor.tool != EditorTool::Building {
        return None; // 关闭F12或切换工具后不得保留世界候选。
    }
    let cursor = editor.cursor_world?; // 光标离开视口时模型和辅助线同帧隐藏。
    let selected_id = editor.building_selected_preset_id.as_deref()?; // 没有选择时不猜首项。
    let preset = registry.resolve(selected_id)?; // 悬空选择不能借用旧模型或旧几何。
    let identity = current_identity(session, registry); // 索引与连接候选必须属于同一完整修订。
    let pose = resolve_building_pose_snapped(cursor, preset, controls, identity.as_ref(), state, None, snap); // 普通网格与显式连接模式均只从此处解析。
    let world_xz = pose.world_xz;
    let yaw_degrees = pose.yaw_degrees;
    let matching_result = state.last_validation.as_ref().filter(|result| {
        session
            .draft
            .as_ref()
            .is_some_and(|draft| draft.map_id == result.request.identity.map_id)
            && result.request.preset.id == preset.id
            && result.request.world_xz == world_xz
            && result.request.yaw_degrees == yaw_degrees
    }); // 光标或建筑变化后旧红绿结果立即退回待校验色。
    let index_failed = identity.as_ref().is_some_and(|identity| {
        state
            .index_error
            .as_ref()
            .is_some_and(|(failed, _)| failed == identity)
    });
    let snap_failed = matches!(pose.connection, BuildingConnectionSnapState::Failed { .. });
    let tone = if index_failed
        || snap_failed
        || matching_result.is_some_and(|result| result.outcome.is_err())
    {
        BuildingPreviewTone::Invalid // 明确失败时模型与线框共同变红。
    } else if matching_result.is_some_and(|result| result.outcome.is_ok()) {
        BuildingPreviewTone::Valid // 完整后台规则通过后共同变绿。
    } else {
        BuildingPreviewTone::Pending // 尚未点击或旧结果失配时保持琥珀色。
    };
    let pending_root_y = cursor.y - preset.foundation.model_base_y; // 未校验时先让模型地基基准贴住真实命中点。
    let root_y = matching_result
        .and_then(|result| result.outcome.as_ref().ok())
        .map_or(pending_root_y, |validated| validated.translation.y); // 合法结果改用五点或整平后的精确根高度。
    let root = Vec3::new(world_xz.x, root_y, world_xz.y); // 根位置同时驱动模型与门口箭头。
    let root_array = root.to_array(); // 公共建筑几何继续使用序列化兼容数组接口。
    let (footprint, entrance_clearance) = matching_result.map_or_else(
        || {
            (
                preset.world_footprint(root_array, yaw_degrees, [1.0; 3]), // 待校验候选按同一单位缩放公式显示。
                preset.world_entrance_clearance(root_array, yaw_degrees, [1.0; 3]), // 门区与模型朝向严格同步。
            )
        },
        |result| (result.footprint, result.entrance_clearance), // 红绿状态直接回显后台冻结几何。
    );
    Some(BuildingPreviewFrame {
        preset,                      // 借用当前完整目录，不克隆模型路径。
        root,                        // 世界根位置供模型和辅助线共同使用。
        yaw_degrees,                 // 唯一旋转值避免模型与箭头分叉。
        footprint,                   // 实体占地四边形。
        entrance_clearance,          // 独立门前净空四边形。
        tone,                        // 统一候选状态颜色。
        connection: pose.connection, // 连接点回显使用同一候选解析。
    })
}

/// 把统一候选状态映射为高可见度Gizmo颜色。
fn tone_color(tone: BuildingPreviewTone) -> Color {
    match tone {
        BuildingPreviewTone::Invalid => Color::srgba(1.0, 0.18, 0.12, 0.95), // 明确禁止落点。
        BuildingPreviewTone::Valid => Color::srgba(0.18, 1.0, 0.42, 0.95),   // 完整后台规则通过。
        BuildingPreviewTone::Pending => Color::srgba(1.0, 0.72, 0.12, 0.9),  // 等待作者点击校验。
    }
}

/// 每帧提交固定的十二条占地/门区线和三条正门箭头线。
pub(super) fn draw_building_preview(
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    registry: Option<Res<BuildingPresetRegistry>>,
    controls: Res<BuildingPlacementControls>,
    state: Res<BuildingAuthoringState>,
    mut gizmos: Gizmos<MapEditorBrushGizmos>,
    preferences: Option<Res<feathrise_editor_core::EditorSettings>>,
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
) {
    let Some(registry) = registry.as_deref() else {
        return; // 目录尚未形成有效快照时没有可靠几何。
    };
    let snap = preferences.as_ref().map(|settings| controls.snap_policy(settings.editor_snapping, keyboard.as_ref().is_some_and(|keys| keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])), [0.0; 3]));
    let Some(preview) = resolve_building_preview(&editor, &session, registry, &controls, &state, snap)
    else {
        return; // 工具、选择或真实地形命中不完整时辅助线与模型共同隐藏。
    };
    let color = tone_color(preview.tone); // Gizmo和模型材质读取同一三态结果。
    draw_quad(
        &mut gizmos,
        preview.footprint,
        preview.root.y + PREVIEW_HEIGHT_OFFSET,
        color,
        true,
    ); // 实体占地增加四个短标杆，与门区区分。
    draw_quad(
        &mut gizmos,
        preview.entrance_clearance,
        preview.root.y + PREVIEW_HEIGHT_OFFSET * 1.5,
        color,
        false,
    ); // 门前净空只画平面边界，不伪装成实体碰撞。
    let (door, outward) = preview.preset.world_entrance_marker(
        preview.root.to_array(), // 正门门槛使用同一模型根。
        preview.yaw_degrees,     // 正门方向使用同一模型旋转。
        [1.0; 3],                // 首期建筑固定单位缩放。
    );
    draw_arrow(
        &mut gizmos,
        door + Vec3::Y * PREVIEW_HEIGHT_OFFSET * 2.0,
        outward,
        color,
    );
    for (index, connection) in preview.preset.connections.iter().enumerate() {
        let (position, outward) = preview.preset.world_connection_marker(
            connection,
            preview.root.to_array(),
            preview.yaw_degrees,
            [1.0; 3],
        );
        let selected = matches!(
            preview.connection,
            BuildingConnectionSnapState::Snapped(snap)
                if snap.source_connection_index == index
        );
        draw_connection_marker(
            &mut gizmos,
            position,
            outward,
            if selected {
                Color::srgba(0.15, 1.0, 1.0, 1.0) // 青色表示已选源连接点。
            } else {
                Color::srgba(0.72, 0.42, 1.0, 0.95) // 紫色表示当前模块其他可用点。
            },
            selected,
        );
    }
    match preview.connection {
        BuildingConnectionSnapState::Snapped(snap) => {
            draw_connection_marker(
                &mut gizmos,
                snap.target_position,
                snap.target_outward,
                Color::srgba(0.15, 1.0, 1.0, 1.0),
                true,
            ); // 已选现有目标与源点使用相同青色。
            if let Some(source) = preview.preset.connections.get(snap.source_connection_index) {
                let (source_position, _) = preview.preset.world_connection_marker(
                    source,
                    preview.root.to_array(),
                    preview.yaw_degrees,
                    [1.0; 3],
                );
                gizmos.line(
                    source_position + Vec3::Y * PREVIEW_HEIGHT_OFFSET,
                    snap.target_position + Vec3::Y * PREVIEW_HEIGHT_OFFSET,
                    Color::srgba(0.15, 1.0, 1.0, 0.9),
                ); // 不同台地高度会以竖向差明确显示，不隐式修改连接协议。
            }
        }
        BuildingConnectionSnapState::Failed {
            nearby_target: Some((position, outward)),
            ..
        } => draw_connection_marker(
            &mut gizmos,
            position,
            outward,
            Color::srgba(1.0, 0.12, 0.08, 1.0),
            true,
        ), // 最近不兼容点用红色指出。
        BuildingConnectionSnapState::Disabled
        | BuildingConnectionSnapState::Failed {
            nearby_target: None,
            ..
        } => {}
    }
}

/// 用水平十字、短朝向线和可选竖标显示一个模块连接点。
fn draw_connection_marker(
    gizmos: &mut Gizmos<MapEditorBrushGizmos>,
    position: Vec3,
    outward: Vec3,
    color: Color,
    emphasized: bool,
) {
    let direction = Vec3::new(outward.x, 0.0, outward.z).normalize_or_zero();
    if direction == Vec3::ZERO {
        return; // 公共目录已拒绝坏方向，这里继续防御异常索引。
    }
    let side = Vec3::new(-direction.z, 0.0, direction.x);
    let center = position + Vec3::Y * PREVIEW_HEIGHT_OFFSET * 2.5;
    let radius = if emphasized { 0.48 } else { 0.32 };
    gizmos.line(
        center - direction * radius,
        center + direction * radius,
        color,
    );
    gizmos.line(center - side * radius, center + side * radius, color);
    gizmos.line(center, center + direction * (radius + 0.45), color); // 朝外短线表达对齐方向。
    if emphasized {
        gizmos.line(center, center + Vec3::Y * 0.9, color); // 被选择或拒绝的点更醒目。
    }
}

/// 绘制一个世界 X/Z 四边形，并可选在四角增加短竖线。
fn draw_quad(
    gizmos: &mut Gizmos<MapEditorBrushGizmos>,
    quad: BuildingWorldQuad,
    y: f32,
    color: Color,
    corner_ticks: bool,
) {
    for index in 0..4 {
        let start_xz = quad.corners[index];
        let end_xz = quad.corners[(index + 1) % 4];
        let start = Vec3::new(start_xz.x, y, start_xz.y);
        let end = Vec3::new(end_xz.x, y, end_xz.y);
        gizmos.line(start, end, color); // 四条固定边完整闭合候选轮廓。
        if corner_ticks {
            gizmos.line(start, start + Vec3::Y * 0.8, color); // 短标杆强调这是实体占地。
        }
    }
}

/// 用一条方向线和两条箭头翼表示正门朝外方向。
fn draw_arrow(gizmos: &mut Gizmos<MapEditorBrushGizmos>, start: Vec3, outward: Vec3, color: Color) {
    let direction = Vec3::new(outward.x, 0.0, outward.z).normalize_or_zero();
    if direction == Vec3::ZERO {
        return; // 坏方向已由目录校验阻止，这里继续防御异常热更。
    }
    let end = start + direction * 3.0; // 三米箭头清楚标出门外方向。
    let side = Vec3::new(-direction.z, 0.0, direction.x);
    gizmos.line(start, end, color); // 箭杆从门槛向外。
    gizmos.line(end, end - direction * 0.8 + side * 0.5, color); // 左翼。
    gizmos.line(end, end - direction * 0.8 - side * 0.5, color); // 右翼。
}
