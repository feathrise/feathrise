//! 地形手柄复用通用移动、旋转和倍率数学；拖动只发布姿态，松手在后台物化集合并登记一次历史。
use super::{
    LayerCandidateState, LayerTarget,
    modes::{ViewportMode, ViewportModeState},
    placement_move::{
        MoveCamera, MoveHandle, MoveInputContext, group_scale_factor, handle_point,
        handle_translation_mode, pick_rotation, pick_scale, rotation_ring, rotation_yaw, axis_colors, visible_axes,
    },
};
use crate::{
    authoring::EditorAuthoringSession,
    components::SceneEditorViewport,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState},
    ui::terrain_objects::{Mutation, TerrainObjectsEditorState},
};
use bevy::{prelude::*, ui::RelativeCursorPosition};
use feathrise_scene_content::{SceneId, TerrainId, TerrainObjectTransform};
use feathrise_scene::TerrainObjectTransformPreview;
use feathrise_author_tools::{AuthorTool, TransformPreview};
use crate::viewport::tool_preview::AuthorToolFrame;

struct Drag {
    scene_id: SceneId,
    id: TerrainId,
    revision: u64,
    before: TerrainObjectTransform,
    after: TerrainObjectTransform,
    camera: MoveCamera,
    point: Vec3,
    cursor: Vec2,
    size: Vec2,
    handle: MoveHandle,
    snap: feathrise_editor_core::EditorSnapSettings,
    control: bool,
}
#[derive(Resource, Default)]
pub(crate) struct TransformState {
    drag: Option<Drag>,
    hover: Option<MoveHandle>, // 只记录本帧原拾取命中；悬停不占有鼠标、不创建姿态预览或历史。
    releasing: bool,
}
impl TransformState {
    pub(super) fn grid_reference(&self) -> Option<(Vec3, feathrise_editor_core::EditorTranslationSnap)> {
        let drag = self.drag.as_ref().filter(|drag| drag.handle.mode() == ViewportMode::Move)?;
        Some((Vec3::from_array(drag.after.translation), drag.snap.translation_snap(drag.control, drag.before.translation)))
    }
}
/// 作者快照只读真实拖动，不把完成后的输入释放期当作数据事务。
pub(crate) fn transaction_active(world: &World) -> bool {
    world.get_resource::<TransformState>().is_some_and(|state| state.drag.is_some())
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<TransformState>()
        .init_resource::<TerrainObjectTransformPreview>()
        .add_systems(
            Update,
            input
                .in_set(crate::SceneEditorSystemSet::Input)
                .after(crate::authoring::placement_batch::sync_batch_selection)
                .before(super::control_editor_viewport)
                .before(super::placement_move::handle_placement_move),
        )
        .add_systems(Update, draw.in_set(crate::SceneEditorSystemSet::Presentation));
}
#[allow(clippy::too_many_arguments)]
fn input(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut selection: ResMut<TerrainObjectsEditorState>,
    candidates: Res<LayerCandidateState>,
    mode: Res<ViewportModeState>,
    context: MoveInputContext,
    mut state: ResMut<TransformState>,
    mut preview: ResMut<TerrainObjectTransformPreview>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<(&ComputedNode, &RelativeCursorPosition), With<SceneEditorViewport>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
) {
    state.hover = None; // 模态、失焦、离开视口及目标失效都必须清掉上一帧的高亮。
    let down = mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle]);
    let released = [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
        .into_iter().any(|button| mouse.just_released(button));
    if state.releasing {
        if !down && !released {
            state.releasing = false;
            selection.transforming = false;
        }
        return;
    }
    let camera_cursor =
        viewport
            .as_deref()
            .zip(nodes.single().ok())
            .and_then(|(viewport, (node, cursor))| {
                MoveCamera::new(&editor, viewport, node.inverse_scale_factor())
                    .map(|camera| (camera, cursor, node.size()))
            });
    let available = editor.open
        && !session.vegetation_cross_busy
        && !session.saving
        && !editor.save_requested
        && (!guard.blocked() || state.drag.is_some())
        && context.pointer_available(&editor)
        && matches!(
            mode.mode,
            ViewportMode::Move | ViewportMode::Rotate | ViewportMode::Scale
        );
    if let Some(mut drag) = state.drag.take() {
        let valid = available
            && mode.mode == drag.handle.mode()
            && session.revision == drag.revision
            && selection.selected.as_ref() == Some(&drag.id)
            && !selection.locked.contains(&drag.id)
            && !selection.hidden.contains(&drag.id)
            && crate::ui::terrain_objects::selected(&session, &selection)
                .is_some_and(|object| object.enabled)
            && session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.scene_id == drag.scene_id)
            && camera_cursor
                .as_ref()
                .is_some_and(|(camera, _, size)| *camera == drag.camera && *size == drag.size);
        if !valid
            || keys.just_pressed(KeyCode::Escape)
            || mouse.any_pressed([MouseButton::Right, MouseButton::Middle])
        {
            preview.pose = None;
            state.releasing = down || released;
            selection.transforming = state.releasing;
            editor.status = "已取消地形对象变换".into();
            return;
        }
        let (camera, cursor, _) = camera_cursor.unwrap();
        if cursor.normalized.is_some_and(|normalized|
            ((normalized - drag.cursor) * camera.size * camera.inverse_scale).length() <= 2.0)
        {
            // 拖回起点与未拖动同义：撤回旧预览，不能把离开时的姿态登记成一次作者操作。
            drag.after = drag.before;
            preview.pose = None;
        }
        if let Some(normalized) = cursor.normalized
            && ((normalized - drag.cursor) * camera.size * camera.inverse_scale).length() > 2.0
        {
            let origin = Vec3::from_array(drag.before.translation);
            drag.control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
            let snap = drag.snap.active(drag.control); // 常开时Ctrl临时关闭；本笔步长与相位沿用按下时偏好。
            let mut pose = drag.before;
            let result = if drag.handle == MoveHandle::UniformScale {
                let before = Vec3::from_array(pose.scale);
                group_scale_factor(
                    (normalized - drag.cursor) * camera.size * camera.inverse_scale,
                    if snap { drag.snap.scale_factor } else { 0.0 },
                    0.001 / before.min_element(),
                    1_000_000.0 / before.max_element(),
                )
                .map(|factor| pose.scale = (before * factor).to_array())
            } else {
                camera
                    .ray(normalized)
                    .and_then(|ray| handle_point(ray, origin, drag.handle))
                    .and_then(|point| {
                        if drag.handle == MoveHandle::RotationY {
                            rotation_yaw(
                                pose.yaw_degrees,
                                origin,
                                drag.point,
                                point,
                                if snap {
                                    drag.snap.rotation_degrees
                                } else {
                                    0.0
                                },
                            )
                            .map(|yaw| pose.yaw_degrees = yaw)
                        } else {
                            handle_translation_mode(
                                origin,
                                point - drag.point,
                                drag.handle,
                                if snap {
                                    drag.snap.translation_meters
                                } else {
                                    0.0
                                },
                                drag.snap.mode,
                                Vec2::splat(-1_000_000.0),
                                Vec2::splat(1_000_000.0),
                            )
                            .map(|position| pose.translation = position.to_array())
                        }
                    })
            };
            if result.is_some() && pose.validate().is_ok() {
                drag.after = pose;
                preview.pose = Some((drag.scene_id.clone(), drag.id.clone(), pose));
            }
        }
        if mouse.just_released(MouseButton::Left) {
            selection.transforming = false;
            if drag.before != drag.after {
                crate::ui::terrain_objects::start(
                    &mut session,
                    &mut editor,
                    &mut selection,
                    Mutation::Transform(drag.id, drag.after),
                );
            }
            state.releasing = true;
            selection.transforming = true; // 松手和和弦收尾仍拥有鼠标，静默帧才归还导航与选择。
        } else if mouse.pressed(MouseButton::Left) {
            state.drag = Some(drag);
        }
        return;
    }
    if !available || selection.busy() || context.pointer_over_ui()
        || mouse.any_pressed([MouseButton::Right, MouseButton::Middle]) {
        return;
    }
    let Some(object) = crate::ui::terrain_objects::selected(&session, &selection) else {
        return;
    };
    if !object.enabled
        || selection.locked.contains(&object.id)
        || selection.hidden.contains(&object.id)
    {
        return;
    }
    if !matches!(&candidates.selected, Some(LayerTarget::TerrainObject(id)) if id == object.id.as_str())
    {
        return;
    }
    let Some((camera, cursor, size)) = camera_cursor.filter(|(_, cursor, _)| cursor.cursor_over())
    else {
        return;
    };
    let Some(normalized) = cursor.normalized else {
        return;
    };
    let origin = Vec3::from_array(object.transform.translation);
    let handle = match mode.mode {
        ViewportMode::Rotate => {
            pick_rotation(camera, origin, normalized).then_some(MoveHandle::RotationY)
        }
        ViewportMode::Scale => {
            pick_scale(camera, origin, normalized).then_some(MoveHandle::UniformScale)
        }
        _ => camera.pick_handle(origin, normalized, false),
    };
    let Some(handle) = handle else {
        return;
    };
    state.hover = Some(handle);
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(point) = camera
        .ray(normalized)
        .and_then(|ray| handle_point(ray, origin, handle))
    else {
        return;
    };
    state.drag = Some(Drag {
        scene_id: session.draft.as_ref().unwrap().scene_id.clone(),
        id: object.id.clone(),
        revision: session.revision,
        before: object.transform,
        after: object.transform,
        camera,
        point,
        cursor: normalized,
        size,
        handle,
        snap: context.snap_settings(),
        control: keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
    });
    selection.transforming = true;
}

#[cfg(test)]
#[path = "terrain_transform_tests.rs"]
mod tests;
fn draw(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    selection: Res<TerrainObjectsEditorState>,
    candidates: Res<LayerCandidateState>,
    mode: Res<ViewportModeState>,
    state: Res<TransformState>,
    preview: Res<TerrainObjectTransformPreview>,
    viewport: Option<Res<SceneEditorViewportState>>,
    nodes: Query<&ComputedNode, With<SceneEditorViewport>>,
    mut tools: ResMut<AuthorToolFrame>,
) {
    if !editor.open {
        return;
    }
    let Some(object) = crate::ui::terrain_objects::selected(&session, &selection) else {
        return;
    };
    if !object.enabled
        || selection.locked.contains(&object.id)
        || selection.hidden.contains(&object.id)
    {
        return;
    }
    if !matches!(&candidates.selected, Some(LayerTarget::TerrainObject(id)) if id == object.id.as_str())
    {
        return;
    }
    let pose = preview
        .for_object(&session.draft.as_ref().unwrap().scene_id, &object.id)
        .unwrap_or(object.transform);
    let origin = Vec3::from_array(pose.translation);
    let (Some(viewport), Ok(node)) = (viewport, nodes.single()) else {
        return;
    };
    let Some(camera) = MoveCamera::new(&editor, &viewport, node.inverse_scale_factor()) else {
        return;
    };
    let Some(length) = camera.handle_length(origin) else {
        return;
    };
    let copper = Color::srgb(0.745, 0.529, 0.243);
    let highlight = Color::srgb(0.92, 0.72, 0.40);
    // 继续由原作者输入判定命中，语义合同只传显示状态，两种3D视口自然使用同一高亮。
    let active = |handle| state.drag.as_ref().map(|drag| drag.handle).or(state.hover) == Some(handle);
    match mode.mode {
        ViewportMode::Move => {
            let axes = visible_axes(camera, origin, length, 0b111);
            let active_axes = super::placement_move::MoveAxis::ALL.into_iter().fold(0, |mask, axis|
                mask | if active(MoveHandle::Axis(axis)) { 1 << axis.index() } else { 0 }) & axes;
            tools.push(AuthorTool::Transform(TransformPreview::Move {
                spatial: false,
                origin: origin.to_array(), length, axes,
                active_axes, plane: camera.plane_quad(origin, length).is_some(), plane_active: active(MoveHandle::XzPlane),
                plane_cross: false, plane_color: copper.to_linear().to_f32_array(),
                color_active: highlight.to_linear().to_f32_array(), axis_colors: axis_colors(),
                anchor: None, marker_radius: None,
            }));
        }
        ViewportMode::Rotate => {
            if let Some(ring) = rotation_ring(camera, origin) {
                tools.push(AuthorTool::Transform(TransformPreview::Rotate {
                    origin: origin.to_array(), radius: ring.world[0].distance(origin), yaw_degrees: None,
                    before_yaw_degrees: None,
                    color: if active(MoveHandle::RotationY) { highlight } else { copper }.to_linear().to_f32_array(),
                }));
            }
        }
        ViewportMode::Scale => {
            tools.push(AuthorTool::Transform(TransformPreview::Scale {
                origin: origin.to_array(), right: Vec3::X.to_array(), up: Vec3::Y.to_array(),
                half_size: length * 0.075,
                color: if active(MoveHandle::UniformScale) { highlight } else { copper }.to_linear().to_f32_array(), cube: true,
            })); // cube仍为原length*0.15总边长，不能把半边长当成缩放系数。
        }
        _ => {}
    }
}
