//! 冻结空间笔刷端点与地图身份；长路径由有界后台批次完整处理。
use super::types::*;
use crate::{authoring::EditorAuthoringSession, resources::MapEditorState, types::EditorTool};
use bevy::prelude::*;
use feathrise_map_terrain::volume::{VolumeBrush, VolumeBrushKind, sparse::SPARSE_CELL_SIZE};

/// 只校验参数和地图边界，内部刷印/块预算不能变成用户的路径长度限制。
pub(super) fn build_request(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    center: Option<Vec3>,
    mode: ApplyMode,
    start: Option<Vec3>,
    end: Option<Vec3>,
) -> Result<SpatialBrushRequest, String> {
    let identity = Identity::capture(editor, session)
        .ok_or("请在Clipmap地图的三维雕刻编辑模式使用空间笔刷")?;
    let (from, to) = match mode {
        ApplyMode::Point => {
            let p = center.ok_or("请先放置空间笔刷")?;
            (p, p)
        }
        ApplyMode::Line => (start.ok_or("请先设置起点")?, end.ok_or("请先设置终点")?),
        ApplyMode::Path => return Err("多点路径必须冻结已就绪的完整路径版本".into()),
    };
    let kind = match editor.tool {
        EditorTool::VolumeDig => VolumeBrushKind::Dig,
        EditorTool::VolumeFill => VolumeBrushKind::Fill,
        EditorTool::VolumeSmooth => VolumeBrushKind::Smooth,
        EditorTool::VolumeFlatten => VolumeBrushKind::Flatten,
        _ => return Err("当前工具不是三维雕刻工具".into()),
    };
    let brush = VolumeBrush {
        kind,
        from: from.to_array(),
        to: to.to_array(),
        radius: editor.brush_radius,
        strength: editor.brush_strength,
        shape: editor.volume_brush_shape,
        half_height: editor.volume_brush_half_height,
        plane_height: from.y,
        ..default()
    };
    if !from.is_finite()
        || !to.is_finite()
        || from.abs().max_element() > 100_000.
        || to.abs().max_element() > 100_000.
        || !brush.radius.is_finite()
        || !(0.5..=8.).contains(&brush.radius)
        || !brush.half_height.is_finite()
        || !(0.5..=8.).contains(&brush.half_height)
        || !brush.strength.is_finite()
        || !(0.01..=1.).contains(&brush.strength)
    {
        return Err("空间笔刷坐标、尺寸或强度无效".into());
    }
    let draft = session.draft.as_ref().unwrap();
    let local=brush.in_source_frame(draft.terrain_documents.active.as_ref().map_or(Mat4::IDENTITY,|active|
        active.definition.transform.as_transform().to_matrix()));
    let domain = local.sample_half_extents(SPARSE_CELL_SIZE) + Vec3::splat(SPARSE_CELL_SIZE);
    let min = Vec2::from_array(draft.terrain_manifest().world_min);
    let max = Vec2::from_array(draft.terrain_manifest().world_max);
    let local_from=Vec3::from_array(local.from);let local_to=Vec3::from_array(local.to);
    if (local_from.min(local_to).xz() - domain.xz()).cmplt(min).any()
        || (local_from.max(local_to).xz() + domain.xz()).cmpgt(max).any()
    {
        return Err("空间笔刷范围越出地图边界".into());
    }
    Ok(SpatialBrushRequest {
        identity,
        brush,
        path: None,
    })
}
