//! 普通立方体保存验收只观察原草稿、正式接管实体和实际视口，不改任何落盘或热载结果。

use super::{action_button, require_owned};
use crate::{
    authoring::{EditorAuthoringSession, mesh_edit::MeshEditState},
    components::{EditorPlacedAsset, SceneEditorViewport},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState},
    types::EditorAction,
    viewport::shading,
};
use bevy::{camera::primitives::Aabb, prelude::*};
use feathrise_scene::{ScenePlacementInstance, ScenePlacementRuntimePhase, ScenePlacementRuntimeStatus};
use feathrise_scene_content::{SceneId, ScenePlacementRecord};
use serde_json::{Value, json};

pub fn draft_record(world: &World, scene: &SceneId) -> Option<ScenePlacementRecord> {
    let draft = world.get_resource::<EditorAuthoringSession>()?.draft.as_ref()?;
    (draft.scene_id == *scene && draft.placements.placements.len() == 1)
        .then(|| draft.placements.placements[0].clone())
}

pub fn press_save(world: &mut World) -> Result<Option<Entity>, String> {
    require_owned(world)?;
    if world.get_resource::<SceneEditorNumericScrubMouseGuard>().is_some_and(|guard| guard.blocked()) {
        return Ok(None);
    }
    let Some(entity) = action_button(world, EditorAction::Save) else {
        return Ok(None);
    };
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(Some(entity))
}

pub fn frame_cube(world: &mut World, record: &ScenePlacementRecord, after_save: bool) -> Result<(), String> {
    require_owned(world)?;
    if world.get_resource::<MeshEditState>().is_some_and(|state| state.active()) {
        return Err("验收只接受未进入编辑模式的普通立方体".into());
    }
    let orthographic = world.resource::<SceneEditorState>().orthographic;
    let mut viewport = world.resource_mut::<SceneEditorViewportState>();
    viewport.extent = 8.0;
    (viewport.yaw, viewport.pitch) = if after_save { (0.92, 0.58) } else { (0.72, 0.48) };
    let (_, pose) = crate::viewport::editor_camera_pose(orthographic, &viewport);
    // 保存后另换已知取景，新探针按实际投影采样，避免旧离屏纹理被误判为新帧。
    viewport.center = Vec3::from_array(record.translation) + pose.back().as_vec3() * (4.8 - 8.0);
    Ok(())
}

pub fn press_wireframe(world: &mut World) -> Result<Entity, String> {
    require_owned(world)?;
    shading::validation_press_mode(world, 0)
}

pub struct CubeCapture {
    pub edge_samples: Vec<(Vec2, Vec2)>,
    pub viewport: [u32; 4],
    pub camera_current: bool,
}

pub fn wireframe_capture(world: &mut World, record: &ScenePlacementRecord) -> Option<CubeCapture> {
    let (size, center) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility), With<SceneEditorViewport>>()
        .iter(world).find_map(|(node, transform, visible)| {
            (visible.get() && node.size().min_element() > 128.0).then_some((node.size(), transform.translation))
        })?;
    let viewport = world.get_resource::<SceneEditorViewportState>()?;
    let target_size = viewport.render_size;
    if target_size.min_element() < 128
        || world.get::<feathrise_scene::author_view::ShadingView>(viewport.camera)?.options.x != 0.0
    {
        return None;
    }
    let camera = world.get::<Camera>(viewport.camera)?;
    let pose = world.get::<GlobalTransform>(viewport.camera)?;
    let orthographic = world.resource::<SceneEditorState>().orthographic;
    let (_, expected) = crate::viewport::editor_camera_pose(orthographic, viewport);
    let camera_current = camera.is_active
        && pose.translation().distance_squared(expected.translation) < 0.0001
        && pose.rotation().dot(expected.rotation).abs() > 0.99999;
    let logical = camera.logical_viewport_size()?;
    let geometry = record.asset_target.editable_geometry()?;
    let transform = Transform::from_translation(Vec3::from_array(record.translation))
        .with_rotation(Quat::from_rotation_y(record.yaw_degrees.to_radians()))
        .with_scale(Vec3::from_array(record.scale));
    // 从保存的立方体配方计算像素坐标。正式实体全丢时依然能捕获同一块失败现场。
    let expected = GlobalTransform::from(expected);
    let origin = center - size * 0.5;
    let points: Vec<_> = geometry.vertices().iter().map(|point| {
        let local = Vec3::from_array(point.map(|axis| axis as f32 * 0.0005));
        let pixel = camera.world_to_viewport(&expected, transform.transform_point(local)).ok()? * size / logical;
        (pixel.cmpge(Vec2::splat(6.0)).all() && pixel.cmple(size - 6.0).all())
            .then_some(origin + pixel)
    }).collect::<Option<_>>()?;
    let mut edge_samples = Vec::with_capacity(36);
    for &[a, b] in geometry.edges() {
        let (a, b) = (points[a], points[b]);
        let normal = (b - a).perp().try_normalize()? * 4.0;
        edge_samples.extend([0.23, 0.50, 0.77].map(|t| (a.lerp(b, t), normal)));
    }
    let [x, y] = origin.ceil().as_uvec2().to_array();
    let [end_x, end_y] = (origin + size).floor().as_uvec2().to_array();
    Some(CubeCapture { viewport: [x, y, end_x, end_y], camera_current, edge_samples })
}

#[derive(Default)]
pub struct CubeState {
    pub runtime_revision: u64,
    pub runtime_ready: bool,
    pub visible_formal: bool,
    pub visible_preview: bool,
    pub placement_count: usize,
    pub details: Value,
}

pub fn cube_state(world: &mut World, scene: &SceneId, record: &ScenePlacementRecord) -> CubeState {
    let target = &record.asset_target;
    let mut roots = Vec::new();
    let mut visible_formal = false;
    let mut visible_preview = false;
    for (root, instance, editor_preview) in world.query::<(Entity, &ScenePlacementInstance, Has<EditorPlacedAsset>)>()
        .iter(world)
        .filter(|(_, instance, _)| instance.scene_id() == scene && instance.id() == record.id)
        .take(8)
    {
        let components = |entity: Entity| {
            let handle = world.get::<Mesh3d>(entity).map(|mesh| &mesh.0);
            let vertex_count = handle.and_then(|handle| world.resource::<Assets<Mesh>>().get(handle)).map(Mesh::count_vertices);
            json!({
                "entity": format!("{entity:?}"),
                "parent": world.get::<ChildOf>(entity).map(|parent| format!("{:?}", parent.parent())),
                "visibility": world.get::<Visibility>(entity).map(|state| format!("{state:?}")),
                "inherited_visibility": world.get::<InheritedVisibility>(entity).map(|visible| visible.get()),
                "view_visibility": world.get::<ViewVisibility>(entity).map(|visible| visible.get()),
                "mesh3d": handle.is_some(), "mesh_vertices": vertex_count,
                "aabb": world.get::<Aabb>(entity).is_some(),
            })
        };
        let mut visible_visual = |entity: Entity| {
            world.get::<InheritedVisibility>(entity).is_some_and(|visibility| visibility.get())
                && world.get::<Aabb>(entity).is_some()
                && world.get::<Mesh3d>(entity).and_then(|mesh| world.resource::<Assets<Mesh>>().get(&mesh.0))
                    .is_some_and(|mesh| mesh.count_vertices() == 36)
        };
        let children: Vec<_> = world.get::<Children>(root).into_iter().flatten().copied().take(8).collect();
        let ready = feathrise_scene::builtin_placement_visual_ready(world, root);
        let visible = instance.is_active() && &instance.asset_target() == target && ready
            && (visible_visual(root) || children.iter().copied().any(&mut visible_visual));
        if visible {
            if editor_preview { visible_preview = true; } else { visible_formal = true; }
        }
        roots.push(json!({
            "owner": if editor_preview { "author-preview" } else { "formal" },
            "active": instance.is_active(), "pending": instance.is_pending(), "source_matches": &instance.asset_target() == target,
            "builtin_visual_ready": ready, "visible_exact_visual": visible,
            "has_preview_suppression": world.get::<crate::authoring::placement_preview_ownership::components::PlacementPreviewSuppression>(root).is_some(),
            "root": components(root), "children": children.into_iter().map(components).collect::<Vec<_>>(),
        }));
    }
    let session = world.resource::<EditorAuthoringSession>();
    let runtime = world.resource::<ScenePlacementRuntimeStatus>();
    let editor = world.resource::<SceneEditorState>();
    let viewport = world.get_resource::<SceneEditorViewportState>();
    let camera_active = viewport.and_then(|view| world.get::<Camera>(view.camera)).map(|camera| camera.is_active);
    CubeState {
        runtime_revision: runtime.revision,
        runtime_ready: runtime.scene_id.as_ref() == Some(scene) && runtime.phase == ScenePlacementRuntimePhase::Ready,
        placement_count: runtime.placement_count,
        visible_formal,
        visible_preview,
        details: json!({
            "scene_id": scene, "placement_id": record.id,
            "draft_record": draft_record(world, scene), "save_sequence": session.successful_save_sequence,
            "dirty": editor.dirty, "saving": session.saving, "status": editor.status,
            "camera_active": camera_active, "view": viewport.map(|view| [view.yaw, view.pitch, view.extent]),
            "runtime": {"scene_id": runtime.scene_id, "phase": format!("{:?}", runtime.phase), "revision": runtime.revision,
                "attempt_revision": runtime.attempt_revision, "placement_count": runtime.placement_count, "detail": runtime.detail},
            "roots": roots, "visible_formal": visible_formal, "visible_preview": visible_preview,
        }),
    }
}
