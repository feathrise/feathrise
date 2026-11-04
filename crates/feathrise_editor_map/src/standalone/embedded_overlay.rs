//! 投影尚未语义化的作者辅助线；笔刷、变换手柄与三维选框走独立语义通道，不重复捕获。

use super::embedded_snapshot::EmbeddedSnapshotState;
use bevy::{
    gizmos::{GizmoAsset, GizmoHandles, GizmoMeshSystems, gizmos::GizmoBufferView},
    prelude::*,
};
use std::any::TypeId;

const MAX_LINES: usize = 1024;
const MAX_VERTICES: usize = MAX_LINES * 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayLineData {
    pub start: [f32; 3],
    pub end: [f32; 3],
    pub start_color: [f32; 4],
    pub end_color: [f32; 4],
}

/// revision 与快照身份相同；generation 只在画线数据或诊断变化时递增，供宿主合并发送。
#[derive(Resource, Default)]
pub struct EmbeddedOverlay {
    pub revision: u64,
    pub generation: u64,
    pub lines: Vec<OverlayLineData>,
    pub error: Option<String>,
}

pub fn install(app: &mut App) {
    super::embedded_tools::install(app);
    app.init_resource::<EmbeddedOverlay>()
        .add_systems(Last, capture.after(GizmoMeshSystems));
}

fn capture(
    snapshot: Option<Res<EmbeddedSnapshotState>>,
    handles: Option<Res<GizmoHandles>>,
    assets: Option<Res<Assets<GizmoAsset>>>,
    mut overlay: ResMut<EmbeddedOverlay>,
) {
    let enabled = snapshot.as_ref().is_some_and(|state| state.enabled);
    let revision = snapshot.as_ref().map_or(0, |state| state.revision);
    let mut lines = Vec::new();
    let mut error = None;
    if enabled && let (Some(handles), Some(assets)) = (handles, assets) {
        let groups = [
            TypeId::of::<crate::viewport::MapEditorBrushGizmos>(),
            TypeId::of::<crate::viewport::spatial_brush::SpatialBrushGizmos>(),
            TypeId::of::<crate::viewport::snapping::EditorGridGizmos>(),
            TypeId::of::<crate::viewport::snapping::EditorGridOutlineGizmos>(),
        ];
        let mut vertices = 0;
        for group in groups {
            let Some(asset) = handles
                .handles()
                .get(&group)
                .and_then(Option::as_ref)
                .and_then(|handle| assets.get(handle))
            else {
                continue;
            };
            if let Err(reason) = append(asset.buffer().buffer(), &mut lines, &mut vertices) {
                lines.clear();
                error = Some(reason);
                break;
            }
        }
    }
    if overlay.revision != revision || overlay.lines != lines || overlay.error != error {
        overlay.revision = revision;
        overlay.lines = lines;
        overlay.error = error;
        overlay.generation = overlay.generation.wrapping_add(1).max(1);
    }
}

fn append(
    view: GizmoBufferView<'_>,
    lines: &mut Vec<OverlayLineData>,
    vertices: &mut usize,
) -> Result<(), String> {
    // 先检查 O(1) 长度；NaN 分隔点也消耗预算，不能靠大量空条带拖长作者帧。
    *vertices = vertices
        .saturating_add(view.list_positions.len())
        .saturating_add(view.strip_positions.len());
    if *vertices > MAX_VERTICES {
        return Err("作者工具线条超过固定顶点预算，本帧覆盖已清空".into());
    }
    if view.list_positions.len() != view.list_colors.len()
        || view.strip_positions.len() != view.strip_colors.len()
        || view.list_positions.len() % 2 != 0
    {
        return Err("作者工具线条与颜色数量不一致，本帧覆盖已清空".into());
    }
    for (positions, colors) in view
        .list_positions
        .chunks_exact(2)
        .zip(view.list_colors.chunks_exact(2))
    {
        push(lines, positions[0], positions[1], colors[0], colors[1])?;
    }
    for (positions, colors) in view
        .strip_positions
        .windows(2)
        .zip(view.strip_colors.windows(2))
    {
        // Bevy 用 NaN 顶点隔开独立线带，绝不能把两个手柄错误连起来。
        if positions[0].is_nan() || positions[1].is_nan() {
            continue;
        }
        push(lines, positions[0], positions[1], colors[0], colors[1])?;
    }
    Ok(())
}

fn push(
    lines: &mut Vec<OverlayLineData>,
    start: Vec3,
    end: Vec3,
    start_color: LinearRgba,
    end_color: LinearRgba,
) -> Result<(), String> {
    if lines.len() == MAX_LINES {
        return Err("作者工具覆盖超过1024条线段预算，本帧覆盖已清空".into());
    }
    let start_color = start_color.to_f32_array();
    let end_color = end_color.to_f32_array();
    if !start.is_finite()
        || !end.is_finite()
        || start_color
            .iter()
            .chain(&end_color)
            .any(|value| !value.is_finite())
    {
        return Err("作者工具覆盖含非法坐标或颜色，本帧覆盖已清空".into());
    }
    lines.push(OverlayLineData {
        start: start.to_array(),
        end: end.to_array(),
        start_color,
        end_color,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::gizmos::{AppGizmoBuilder, config::GizmoConfigStore};

    fn original_tool(mut gizmos: Gizmos<crate::viewport::MapEditorBrushGizmos>) {
        gizmos.line(Vec3::ZERO, Vec3::X, LinearRgba::RED);
        gizmos.linestrip([Vec3::Y, Vec3::Y + Vec3::X], LinearRgba::GREEN);
        gizmos.linestrip([Vec3::Z, Vec3::Z + Vec3::X], LinearRgba::BLUE);
    }

    #[test]
    fn real_deferred_gizmos_are_projected_after_mesh_adoption_without_connecting_strips() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<GizmoAsset>>()
            .init_resource::<GizmoConfigStore>()
            .init_gizmo_group::<crate::viewport::MapEditorBrushGizmos>()
            .init_resource::<EmbeddedSnapshotState>()
            .add_systems(Update, original_tool);
        install(&mut app);
        {
            let mut state = app.world_mut().resource_mut::<EmbeddedSnapshotState>();
            state.enabled = true;
            state.revision = 7;
        }
        app.update();
        let overlay = app.world().resource::<EmbeddedOverlay>();
        assert_eq!(overlay.revision, 7);
        assert_eq!(overlay.lines.len(), 3);
        assert_eq!(overlay.lines[0].start_color, LinearRgba::RED.to_f32_array());
        let generation = overlay.generation;
        app.update();
        assert_eq!(
            app.world().resource::<EmbeddedOverlay>().generation,
            generation
        );
        app.world_mut()
            .resource_mut::<EmbeddedSnapshotState>()
            .enabled = false;
        app.update();
        assert!(app.world().resource::<EmbeddedOverlay>().lines.is_empty());
    }

    #[test]
    fn budget_counts_separators_and_refuses_before_scanning_or_copying_large_source() {
        let positions = vec![Vec3::splat(f32::NAN); MAX_VERTICES + 1];
        let colors = vec![LinearRgba::WHITE; positions.len()];
        let empty_positions = Vec::new();
        let empty_colors = Vec::new();
        let mut lines = Vec::new();
        let result = append(
            GizmoBufferView {
                list_positions: &empty_positions,
                list_colors: &empty_colors,
                strip_positions: &positions,
                strip_colors: &colors,
            },
            &mut lines,
            &mut 0,
        );
        assert!(result.is_err());
        assert!(lines.is_empty());
    }
}
