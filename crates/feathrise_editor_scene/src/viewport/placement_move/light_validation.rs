//! 光域手柄专项验收的取点。由当前视口几何计算可点击的真实像素位置。

use super::*;
use super::super::input;

/// 仅查找真实前景边界的可点位置。验收仍经鼠标按下、移动和松手走原所有权、投影与历史。
pub(crate) fn validation_drag_points(world: &mut World, handle: LightHandle, target: f32) -> Option<(Vec2, Vec2)> {
    use crate::{authoring::{EditorAuthoringSession, placement_pose::placement_record_frame}, components::SceneEditorViewport,
        resources::{SceneEditorState, SceneEditorViewportState}, ui::gltf_browser::EditorGltfBrowserState,
        viewport::{LayerCandidateState, modes::ViewportModeState}};
    let dpi = world.query_filtered::<&ComputedNode, With<SceneEditorViewport>>().single(world).ok()?.inverse_scale_factor();
    let editor = world.resource::<SceneEditorState>();
    let view = world.resource::<SceneEditorViewportState>();
    let session = world.resource::<EditorAuthoringSession>();
    let camera = MoveCamera::new(editor, view, dpi)?;
    let (_, record) = input::selected_record(editor, session, world.resource::<EditorGltfBrowserState>(), world.resource::<LayerCandidateState>(), view)?;
    let frame = placement_record_frame(session.draft.as_ref()?, record).ok()?;
    let origin = frame.as_transform().transform_point(record.translation.into());
    let basis = input::placement_scale_basis(frame, record);
    let guide = LightGuide::new(origin, basis, record)?;
    let mode = world.resource::<ViewportModeState>().mode;
    let mut best = None;
    let size = camera.size * camera.inverse_scale;
    let inside = |p: Vec2| p.abs().max_element() < 0.36;
    guide.edges(|part, a, b| {
        if part != handle { return; }
        let middle = a.lerp(b, 0.65);
        let Some(start) = camera.project(middle).filter(|p| inside(*p)) else { return; };
        if input::pick_placement_handle(mode, camera, origin, basis, start, false, false, true).is_some() { return; }
        let Some((actual, grip)) = guide.pick(camera, start) else { return; };
        if actual != handle { return; }
        let offset = grip - origin;
        let desired = if handle == LightHandle::Range {
            grip + offset.normalize_or_zero() * (target - guide.range)
        } else {
            let forward = -basis.z_axis;
            let side = if guide.cone.is_some_and(|c| handle == LightHandle::Inner && c[0] == 0.0) { basis.x_axis }
                else { (offset - forward * offset.dot(forward)).try_normalize().unwrap_or(basis.x_axis) };
            let (sin, cos) = target.to_radians().sin_cos();
            origin + (forward * cos + side * sin) * offset.length()
        };
        let Some(end) = camera.project(desired).filter(|p| inside(*p)) else { return; };
        let span = ((end - start) * size).length_squared();
        if span > PICK*PICK && best.is_none_or(|(best_span, _, _)| span > best_span) { best = Some((span, start, end)); }
    });
    best.map(|(_, a, b)| (a, b))
}
