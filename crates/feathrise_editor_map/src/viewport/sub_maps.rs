//! 子地图共用现有三轴和Y圆环数学。预览仅发布引用覆盖，松手才写草稿和一条历史。
use bevy::{prelude::*, ui::RelativeCursorPosition};
use feathrise_map_content::{MapId, MapInstanceId, MapInstancePlacement, MapInstanceTransform};
use std::sync::Arc;
use crate::{authoring::EditorAuthoringSession, components::MapEditorViewport,
    resources::{MapEditorState, MapEditorViewportState}, ui::sub_maps::SubMapEditorState};
use super::{modes::{ViewportMode, ViewportModeState}, placement_move::{MoveInputContext, MoveCamera,
    MoveHandle, handle_point, handle_translation, pick_rotation, rotation_ring, rotation_yaw, axis_colors, visible_axes},
    LayerCandidateState, LayerTarget};
use feathrise_author_tools::{AuthorTool, TransformPreview};
use crate::viewport::tool_preview::AuthorToolFrame;

struct Drag {
    map: MapId, id: MapInstanceId, index: usize, revision: u64, before: Arc<[MapInstancePlacement]>,
    preview: Arc<[MapInstancePlacement]>, camera: MoveCamera, point: Vec3, cursor: Vec2,
    transform: MapInstanceTransform, handle: MoveHandle, snap: feathrise_editor_core::EditorSnapSettings,
    valid: bool, ui_size: Vec2,
}
#[derive(Resource, Default)]
pub(crate) struct SubMapTransformState {
    drag: Option<Drag>, hover: Option<MoveHandle>, releasing: bool,
}
impl SubMapTransformState {
    /// 只暴露原手势身份；跨进程桥不重算输入，也不扫描引用列表。
    pub(crate) fn embedded_target(&self, session: &EditorAuthoringSession) -> Option<MapInstanceId> {
        self.drag.as_ref().filter(|drag| session.revision == drag.revision &&
            session.draft.as_ref().is_some_and(|draft| draft.map_id == drag.map)).map(|drag| drag.id.clone())
    }
    pub(crate) fn owns_mouse(&self) -> bool { self.drag.is_some() || self.releasing }
    /// 完整快照只等待真实变换；提交/取消后的鼠标释放期不再持有未完成数据。
    pub(crate) fn transaction_active(&self) -> bool { self.drag.is_some() }
    pub(crate) fn preview(&self, session: &EditorAuthoringSession) -> Option<&Arc<[MapInstancePlacement]>> {
        self.drag.as_ref().filter(|drag| session.revision == drag.revision &&
            session.draft.as_ref().is_some_and(|draft| draft.map_id == drag.map)).map(|drag| &drag.preview)
    }
}
pub(crate) fn install(app: &mut App) {
    app.init_resource::<SubMapTransformState>()
        .add_systems(Update, input.before(super::placement_move::handle_placement_move).in_set(crate::MapEditorSystemSet::Input))
        .add_systems(Update, draw.in_set(crate::MapEditorSystemSet::Presentation));
}
fn selected<'a>(session: &'a EditorAuthoringSession, selection: &SubMapEditorState,
    candidates: &LayerCandidateState) -> Option<(usize, &'a MapInstancePlacement)> {
    let id = selection.selected.as_ref()?;
    if !matches!(&candidates.selected, Some(LayerTarget::SubMap(target)) if target == id.as_str()) { return None; }
    session.draft.as_ref()?.manifest.sub_maps.iter().enumerate().find(|(_, record)| &record.id == id)
}
#[allow(clippy::too_many_arguments)]
fn input(mut editor: ResMut<MapEditorState>, mut session: ResMut<EditorAuthoringSession>,
    selection: Res<SubMapEditorState>, candidates: Res<LayerCandidateState>, mode: Res<ViewportModeState>,
    mut state: ResMut<SubMapTransformState>, context: MoveInputContext,
    viewport: Option<Res<MapEditorViewportState>>,
    nodes: Query<(&ComputedNode, &RelativeCursorPosition), With<MapEditorViewport>>,
    mouse: Res<ButtonInput<MouseButton>>, keys: Res<ButtonInput<KeyCode>>) {
    state.hover = None; // 本帧可用性和实际命中重新决定悬停，不能跨拖动、释放或UI遮挡残留。
    let any_mouse = mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle]);
    if state.releasing { if !any_mouse { state.releasing = false; } return; }
    let ui_size = nodes.single().map_or(Vec2::ZERO, |(node, _)| node.size());
    let camera_and_cursor = viewport.as_deref().zip(nodes.single().ok()).and_then(|(viewport, (node, cursor))|
        MoveCamera::new(&editor, viewport, node.inverse_scale_factor()).map(|camera| (camera, cursor)));
    let available = context.pointer_available(&editor) && !session.saving &&
        !selection.busy() && matches!(mode.mode, ViewportMode::Move | ViewportMode::Rotate) && editor.placement_transform_enabled;
    if let Some(mut drag) = state.drag.take() {
        let unchanged = session.revision == drag.revision && session.draft.as_ref().is_some_and(|draft|
            draft.map_id == drag.map && draft.manifest.sub_maps == drag.before);
        let camera_same = camera_and_cursor.as_ref().is_some_and(|(camera, _)| *camera == drag.camera && ui_size == drag.ui_size);
        if !available || !unchanged || !camera_same || selection.selected.as_ref() != Some(&drag.id) ||
            keys.just_pressed(KeyCode::Escape) || mouse.any_pressed([MouseButton::Right, MouseButton::Middle]) {
            state.releasing = any_mouse; editor.status = "已取消子地图整体变换".into(); return;
        }
        let (camera, cursor) = camera_and_cursor.unwrap();
        drag.valid = cursor.normalized.is_some();
        if let Some(normalized) = cursor.normalized {
            if (normalized - drag.cursor).length_squared() * (camera.size * camera.inverse_scale).min_element().powi(2) > 4.0 {
                let origin = Vec3::from_array(drag.transform.translation);
                drag.valid = false;
                if let Some(point) = camera.ray(normalized).and_then(|ray| handle_point(ray, origin, drag.handle)) {
                    let snap = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
                    let mut pose = drag.transform;
                    let valid = if drag.handle == MoveHandle::RotationY {
                        rotation_yaw(pose.yaw_degrees, origin, drag.point, point, if snap { drag.snap.rotation_degrees } else { 0.0 })
                            .map(|yaw| { pose.yaw_degrees = yaw; })
                    } else {
                        handle_translation(origin, point - drag.point, drag.handle, if snap { drag.snap.translation_meters } else { 0.0 },
                            Vec2::splat(-1_000_000.0), Vec2::splat(1_000_000.0)).map(|position| { pose.translation = position.to_array(); })
                    };
                    drag.valid = valid.is_some();
                    if drag.valid {
                        let mut references = drag.before.to_vec(); references[drag.index].transform = pose;
                        drag.preview = references.into();
                    }
                }
            }
        }
        if mouse.just_released(MouseButton::Left) && drag.valid {
            crate::ui::sub_maps::apply(&mut session, &mut editor, drag.preview);
            state.releasing = true; editor.status = "已调整子地图整体变换".into();
        } else if mouse.pressed(MouseButton::Left) { state.drag = Some(drag); }
        return;
    }
    if !available || context.pointer_over_ui() { return; }
    let Some((index, record)) = selected(&session, &selection, &candidates) else { return; };
    let Some((camera, cursor)) = camera_and_cursor.filter(|(_, cursor)| cursor.cursor_over()) else { return; };
    let Some(normalized) = cursor.normalized else { return; };
    let origin = Vec3::from_array(record.transform.translation);
    let handle = if mode.mode == ViewportMode::Rotate {
        pick_rotation(camera, origin, normalized).then_some(MoveHandle::RotationY)
    } else { camera.pick_handle(origin, normalized, false) };
    state.hover = handle;
    if !mouse.just_pressed(MouseButton::Left) || mouse.any_pressed([MouseButton::Right, MouseButton::Middle]) { return; }
    let Some(handle) = handle else { return; };
    let Some(point) = camera.ray(normalized).and_then(|ray| handle_point(ray, origin, handle)) else { return; };
    let draft = session.draft.as_ref().unwrap();
    state.drag = Some(Drag { map: draft.map_id.clone(), id: record.id.clone(), index, revision: session.revision,
        before: draft.manifest.sub_maps.clone(), preview: draft.manifest.sub_maps.clone(), camera, point, cursor: normalized,
        transform: record.transform, handle, snap: context.snap_settings(), valid: true, ui_size });
}
fn draw(editor: Res<MapEditorState>, session: Res<EditorAuthoringSession>, selection: Res<SubMapEditorState>,
    candidates: Res<LayerCandidateState>, mode: Res<ViewportModeState>, state: Res<SubMapTransformState>,
    registry: Option<Res<feathrise_map_content::MapContentRegistry>>, manifests: Res<Assets<feathrise_map_content::MapManifestAsset>>,
    viewport: Option<Res<MapEditorViewportState>>, nodes: Query<&ComputedNode, With<MapEditorViewport>>,
    mut tools: ResMut<AuthorToolFrame>) {
    if !editor.open { return; }
    let Some((index, record)) = selected(&session, &selection, &candidates) else { return; };
    let pose = state.preview(&session).map_or(record.transform, |references| references[index].transform);
    let origin = Vec3::from_array(pose.translation);
    let (Some(viewport), Ok(node)) = (viewport, nodes.single()) else { return; };
    let Some(camera) = MoveCamera::new(&editor, &viewport, node.inverse_scale_factor()) else { return; };
    let Some(length) = camera.handle_length(origin) else { return; };
    let copper = Color::srgb(0.745, 0.529, 0.243);
    let active = state.drag.as_ref().map(|drag| drag.handle).or(state.hover);
    let highlight = Color::srgb(0.92, 0.72, 0.40);
    if let Some(manifest) = registry.as_ref().and_then(|registry| registry.handles(&record.source_map_id))
        .and_then(|handles| manifests.get(&handles.manifest)) {
        let min = Vec3::new(manifest.world_min[0], manifest.terrain.base_height, manifest.world_min[1]);
        let max = Vec3::new(manifest.world_max[0], manifest.terrain.base_height + manifest.terrain.height_scale.abs().max(1.0), manifest.world_max[1]);
        let center = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        let world_center = pose.to_world(center);
        tools.push(AuthorTool::Selection(feathrise_author_tools::SelectionPreview {
            target: format!("submap:{}/{}", session.draft.as_ref().unwrap().map_id.as_str(), record.id),
            center: world_center.to_array(),
            half_axes: [Vec3::X * half.x, Vec3::Y * half.y, Vec3::Z * half.z]
                .map(|axis| (pose.to_world(center + axis) - world_center).to_array()),
            color: copper.to_linear().to_f32_array(),
        }));
    } // 完整实例轮廓只绘制十二条边，整体拖动时与正式内容使用相同预览变换。
    if mode.mode == ViewportMode::Rotate {
        if let Some(ring) = rotation_ring(camera, origin) {
            tools.push(AuthorTool::Transform(TransformPreview::Rotate {
                origin: origin.to_array(), radius: ring.world[0].distance(origin), yaw_degrees: None,
                before_yaw_degrees: None,
                color: (if active == Some(MoveHandle::RotationY) { highlight } else { copper }).to_linear().to_f32_array(),
            }));
        }
    } else if mode.mode == ViewportMode::Move {
        let axes = visible_axes(camera, origin, length, 0b111);
        let active_axes = match active { Some(MoveHandle::Axis(axis)) => 1 << axis.index(), _ => 0 } & axes;
        let plane = camera.plane_quad(origin, length).is_some();
        tools.push(AuthorTool::Transform(TransformPreview::Move {
            spatial: false,
            origin: origin.to_array(), length, axes, active_axes, plane,
            plane_active: plane && active == Some(MoveHandle::XzPlane),
            plane_cross: false, plane_color: copper.to_linear().to_f32_array(),
            color_active: highlight.to_linear().to_f32_array(), axis_colors: axis_colors(),
            anchor: None, marker_radius: None,
        }));
    }
    // 原子地图原点圈独立于当前变换模式；与旋转圆环共存，仍保持16段及原尺寸。
    tools.push(AuthorTool::Transform(TransformPreview::Move {
        spatial: false,
        origin: origin.to_array(), length, axes: 0, active_axes: 0, plane: false,
        plane_active: false, plane_cross: false, plane_color: copper.to_linear().to_f32_array(),
        color_active: copper.to_linear().to_f32_array(), axis_colors: axis_colors(),
        anchor: None, marker_radius: Some(length * 0.10),
    }));
}

#[cfg(test)]
#[path = "sub_maps/tests.rs"]
mod tests;
