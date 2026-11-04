//! 静水仅以当前水位平面接收鼠标射线；地形缺页、显示LOD和网格是否存在均不授权或阻止R8绘制。

use bevy::{prelude::*, ui::RelativeCursorPosition, window::PrimaryWindow};
use crate::{
    authoring::{EditorAuthoringSession, water_canvas::{self, WaterCanvasIdentity}},
    components::MapEditorViewport,
    resources::{MapEditorState, MapEditorViewportState},
    types::{EditorTool, EditorView},
};
use super::placement_move::MoveCamera;

#[derive(Clone, PartialEq)]
pub(crate) struct WaterPlaneIdentity {
    pub canvas: WaterCanvasIdentity,
    pub layer: (usize, String, u32),
    bounds: [[u32; 2]; 2],
    resolution: [u32; 2],
    camera: MoveCamera,
    camera_entity: Entity,
}

#[derive(Resource, Default)]
pub(crate) struct WaterPlaneCursor {
    pub identity: Option<WaterPlaneIdentity>,
    pub point: Option<Vec3>, // 源画布坐标；世界显示点另由editor.brush_cursor_world发布。
    pub world: Option<Vec3>,
    pub authority: u64,
    pub epoch: u64,
    pub available: bool,
    pub segment: u64, // 离开视口/画布后重新命中会换段，不能跨界面把两端补成直线。
    held: bool,
}

/// 先逆变换射线再与源Y水位求交，兼容平移、三轴旋转和非均匀缩放，绝不读取高度页。
fn intersect_water_plane(ray: Ray3d, frame: Mat4, level: f32) -> Option<(Vec3, Vec3)> {
    let inverse = frame.inverse();
    let origin = inverse.transform_point3(ray.origin);
    let direction = inverse.transform_vector3(*ray.direction);
    if !inverse.is_finite() || !level.is_finite() || direction.y.abs() <= 0.000001 { return None; }
    let distance = (level - origin.y) / direction.y;
    if !distance.is_finite() || !(0.0..=1_000_000.0).contains(&distance) { return None; }
    let source = origin + direction * distance;
    let world = ray.get_point(distance);
    (source.is_finite() && world.is_finite()).then_some((source, world))
}

/// 所有权取消代次每帧推进；恢复焦点/模态后仍按住鼠标，不能复活已经废止的笔划。
pub(crate) fn sync_water_plane_cursor(
    mut cursor: ResMut<WaterPlaneCursor>, mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>, viewport: Option<Res<MapEditorViewportState>>,
    nodes: Query<&RelativeCursorPosition, With<MapEditorViewport>>,
    mouse: Res<ButtonInput<MouseButton>>, keys: Res<ButtonInput<KeyCode>>,
    ownership: (
        Option<Res<crate::resources::MapEditorNumericScrubMouseGuard>>,
        Option<Res<crate::resources::MapBrowserState>>,
        Option<Res<crate::resources::MapBrowserDragState>>,
        Option<Res<crate::authoring::TerrainExternalChangeState>>,
        Option<Res<super::placement_move::PlacementMoveState>>,
        Option<Res<crate::resources::MapEditorSettingsState>>,
        Option<Res<super::modes::ViewportModeState>>,
        Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
        Option<Res<feathrise_editor_ui::ActiveNumericInput>>,
        Option<Res<feathrise_editor_ui::ActiveNumericInputScrub>>,
        Option<Res<crate::ui::text_input::EditorTextInputGuard>>,
    ),
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let held = mouse.pressed(MouseButton::Left);
    if (!cursor.held && held) || mouse.just_pressed(MouseButton::Left) {
        cursor.epoch = cursor.epoch.wrapping_add(1);
    }
    cursor.held = held;
    let previous_valid = cursor.point.is_some();
    cursor.point = None;
    cursor.world = None;
    cursor.identity = None;
    cursor.available = false;
    if editor.tool != EditorTool::Water || editor.water_level_from_cursor_requested.is_some() {
        cursor.authority = cursor.authority.wrapping_add(1);
        return; // 显式“取地形水位”继续走独立严格查询，不把水平面伪装为地形。
    }
    editor.cursor_world = None; editor.cursor_triangle = None;
    editor.brush_cursor_world = None; editor.brush_cursor_triangle = None;
    editor.brush_terrain_id = None;
    let available = editor.open && !editor.close_requested && !editor.save_requested && !session.saving
        && !editor.new_map_dialog_open && !editor.suppress_world_pick_this_frame
        && !editor.undo_requested && !editor.redo_requested && editor.map_switch_confirmation.is_none()
        && !editor.map_transition_in_progress && editor.pending_map_transition.is_none()
        && matches!(editor.view, EditorView::Perspective | EditorView::Top)
        && water_canvas::writable(&session, &editor)
        && session.draft.as_ref().is_some_and(|draft| editor.selected_map_id.as_ref() == Some(&draft.map_id))
        && !ownership.0.as_ref().is_some_and(|guard| guard.blocked())
        && !ownership.1.as_ref().is_some_and(|browser| browser.dialog.is_some())
        && !ownership.2.as_ref().is_some_and(|drag| drag.blocks_other_mouse())
        && !ownership.3.as_ref().is_some_and(|conflict| conflict.modal_open())
        && !ownership.4.as_ref().is_some_and(|movement| movement.owns_mouse())
        && !ownership.5.as_ref().is_some_and(|settings| settings.open || settings.owns_mouse_this_frame)
        && !ownership.6.as_ref().is_some_and(|mode| mode.pointer_over_controls)
        && !ownership.7.as_ref().is_some_and(|workspace| workspace.active_tab != crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Map)
        && !ownership.8.as_ref().is_some_and(|numeric| numeric.entity.is_some())
        && !ownership.9.as_ref().is_some_and(|scrub| scrub.entity.is_some())
        && !ownership.10.as_ref().is_some_and(|text| text.block_shortcuts)
        && !keys.just_pressed(KeyCode::Escape)
        && !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
        && !mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
        && windows.iter().all(|window| window.focused);
    if !available { cursor.authority = cursor.authority.wrapping_add(1); return; }
    let Some(viewport) = viewport else { return; };
    let Some(camera) = MoveCamera::new(&editor, &viewport, 1.0) else { return; };
    let draft = session.draft.as_ref().unwrap();
    let Some(layer) = draft.water.as_ref().and_then(|water| water.layers.get(editor.selected_water_layer)) else { return; };
    let (min, max) = water_canvas::layer_bounds(draft, layer);
    cursor.identity = Some(WaterPlaneIdentity {
        canvas: water_canvas::identity(draft),
        layer: (editor.selected_water_layer, layer.id.clone(), layer.water_level.to_bits()),
        bounds: [min.map(f32::to_bits), max.map(f32::to_bits)], resolution: layer.resolution,
        camera, camera_entity: viewport.camera,
    });
    cursor.available = true; // 离开视口只暂停同一次按压；模态和失焦才撤销所有权。
    let Some(normalized) = nodes.single().ok().filter(|node| node.cursor_over()).and_then(|node| node.normalized) else { return; };
    let Some((source, world)) = camera.ray(normalized)
        .and_then(|ray| intersect_water_plane(ray, water_canvas::source_frame(draft), layer.water_level)) else { return; };
    let metric = crate::authoring::terrain_documents::PaintMetricSpace::from_draft(draft);
    if !crate::authoring::brush_overlaps_world_bounds(metric.point(source), editor.brush_radius,
        metric.bounds(min), metric.bounds(max)) { return; }
    if !previous_valid { cursor.segment = cursor.segment.wrapping_add(1); }
    cursor.point = Some(source);
    cursor.world = Some(world);
    editor.cursor_world = Some(world);
    editor.brush_cursor_world = Some(world); // 后续子地图只读门与模式浮层仍可撤回本帧写权限。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_plane_uses_its_level_and_full_source_frame_without_any_ground() {
        let frame = Transform::from_xyz(17.0, 9.0, -4.0)
            .with_rotation(Quat::from_euler(EulerRot::YXZ, 0.4, 0.3, -0.2))
            .with_scale(Vec3::new(2.0, 3.0, 0.5)).to_matrix();
        let expected = Vec3::new(4.0, 7.0, -2.0);
        let world = frame.transform_point3(expected);
        let normal = frame.transform_vector3(Vec3::Y).normalize();
        let ray = Ray3d::new(world + normal * 20.0, Dir3::new(-normal).unwrap());
        let (source, actual) = intersect_water_plane(ray, frame, 7.0).unwrap();
        assert!(source.abs_diff_eq(expected, 0.0001));
        assert!(actual.abs_diff_eq(world, 0.0001));
        assert!(intersect_water_plane(Ray3d::new(Vec3::Y, Dir3::X), Mat4::IDENTITY, 0.0).is_none());
        assert!(intersect_water_plane(Ray3d::new(Vec3::Y, Dir3::Y), Mat4::IDENTITY, 0.0).is_none());
    }
}
