//! 画布点选骨骼和三轴姿势手柄；复用编辑器投影与环拾取，松开才提交一次动画历史。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_animation_model::{host::AnimationHost, media::*};
use super::{context::AnimationContext, preview::{Pose, PreviewRuntime}, resources::*, skeleton, types::*};
use feathrise_editor_core::SceneEditorSystemSet;
use feathrise_editor_animation_model::{assembly::AnimationSystems, gizmo::{GizmoCamera, MeshEditTool, MoveConstraint, self as gizmo}};
use gizmo::{move_arrows, pick_transform_handle, rotation_delta, rotation_rings};
use bevy::{
    camera::{Projection, visibility::RenderLayers},
    ecs::system::SystemParam,
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
    transform::TransformSystems,
    ui::RelativeCursorPosition,
};
use feathrise_editor_core::constants::render_layers::SCENE_EDITOR_ASSET_PREVIEW_LAYER;

pub(super) fn install(app: &mut App) {
    install_visuals(app);
    app.add_systems(
        Update,
        input
            .after(super::timeline::input)
            .in_set(AnimationSystems::Input)
            .in_set(SceneEditorSystemSet::Input),
    );
}

pub(super) fn install_visuals(app: &mut App) {
    app.init_resource::<PoseInputState>()
        .init_gizmo_group::<AnimationGizmos>()
        .init_gizmo_group::<AnimationHighlightGizmos>()
        .add_systems(Startup, configure)
        .add_systems(
            PostUpdate,
            draw.after(TransformSystems::Propagate), // 工程动作工具必须读取当帧已发布角色的世界变换。
        );
}
fn configure(mut store: ResMut<bevy::gizmos::config::GizmoConfigStore>) {
    let (config, _) = store.config_mut::<AnimationGizmos>();
    config.render_layers = RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER);
    config.depth_bias = -1.0;
    config.line.width = 2.5;
    let (config, _) = store.config_mut::<AnimationHighlightGizmos>();
    config.render_layers = RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER);
    config.depth_bias = -1.0;
    config.line.width = 5.0;
}

/// 只采所选节点的至多三条TRS，不为数值面板复制完整姿态。
pub(super) fn local_at(s: &Session, node: usize) -> Transform {
    let n = &s.document.nodes[node];
    let mut value = Transform {
        translation: Vec3::from_array(n.translation),
        rotation: Quat::from_array(n.rotation).normalize(),
        scale: Vec3::from_array(n.scale),
    };
    if let Some(indices) = s.metadata.channels_by_node[s.clip].get(&node) {
        for &i in indices {
            let channel = &s.document.clips[s.clip].channels[i];
            let sample = channel.sample(s.position);
            match (channel.path.as_str(), sample.as_slice()) {
                ("translation", [x, y, z]) => value.translation = Vec3::new(*x, *y, *z),
                ("scale", [x, y, z]) => value.scale = Vec3::new(*x, *y, *z),
                ("rotation", [x, y, z, w]) => {
                    value.rotation = Quat::from_xyzw(*x, *y, *z, *w).normalize()
                }
                _ => {}
            }
        }
    }
    value
}

fn camera(
    canvases: &Query<
        (&ComputedNode, &RelativeCursorPosition, &UiInteraction),
        With<AssetPreviewCanvas>,
    >,
    cameras: &Query<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>,
) -> Option<(GizmoCamera, Vec2, bool)> {
    let (node, cursor, interaction) = canvases.single().ok()?;
    let (projection, transform) = cameras.single().ok()?;
    Some((
        GizmoCamera::from_projection(
            projection,
            transform,
            node.size(),
            node.inverse_scale_factor(),
        )?,
        cursor.normalized?,
        cursor.cursor_over() && *interaction != UiInteraction::None,
    ))
}

#[derive(SystemParam)]
pub(super) struct RigPoseTargets<'w, 's> {
    binding: Option<Res<'w, BindingState>>,
    joints: Query<'w, 's, (&'static GlobalTransform, Option<&'static ChildOf>)>,
}

impl RigPoseTargets<'_, '_> {
    fn frame(
        &self, state: &AnimationEditorState, runtime: &PreviewRuntime, pose: &Pose, node: usize,
    ) -> Option<(Vec3, Mat4)> {
        let session = state.current()?;
        if !state.retargeting() {
            let origin = pose.world.get(node)?.transform_point3(Vec3::ZERO);
            let parent = session.document.nodes.get(node)?.parent
                .and_then(|p| pose.world.get(p)).copied().unwrap_or(Mat4::IDENTITY);
            return Some((origin, parent));
        }
        // 外部动作原地播放只映射旋转。位置/缩放仍可在源曲线上调整，不能套用目标骨长。
        if !state.in_place_rig_preview() || state.pose_tool != PoseTool::Rotate {
            return None;
        }
        let binding = self.binding.as_ref()?;
        let identity = binding.identity.as_ref()?;
        if !binding.active || !binding.ready() || !identity.retarget
            || runtime.binding_revision != binding.revision || !runtime.rig_pose_published(state)
            || identity.revision != state.source_revision() || identity.request != state.model_binding_revision
            || state.source() != Some(identity.source.as_path())
            || state.preview_model_path() != Some(identity.model.as_str())
        {
            return None;
        }
        if !session.document.can_animate_node(node) {
            return None;
        }
        let rig = binding.mapping.as_ref()?.retarget.as_ref()?;
        let correction = rig.corrections.get(node)?.as_ref()?;
        let (world, parent) = self.joints.get(binding.node(runtime.front, node)?).ok()?;
        let parent = parent.map_or(Some(Quat::IDENTITY), |p| {
            self.joints.get(p.parent()).ok().map(|(transform, _)| transform.rotation())
        })?;
        // 目标局部旋转 = before × 源局部旋转 × after。用目标父级 × before 共轭消除显示坐标差异。
        Some((world.translation(), Mat4::from_quat((parent * correction.before).normalize())))
    }
}

pub(super) fn input(
    context: AnimationContext,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    documents: Res<AnimationHost>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keys: Option<ResMut<ButtonInput<KeyCode>>>,
    mut state: ResMut<AnimationEditorState>,
    mut interaction: ResMut<PoseInputState>,
    runtime: Res<PreviewRuntime>,
    targets: RigPoseTargets,
    canvases: Query<
        (&ComputedNode, &RelativeCursorPosition, &UiInteraction),
        With<AssetPreviewCanvas>,
    >,
    cameras: Query<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>,
) {
    state.pose_mouse_owned = false;
    let available = state.visible()
        && (!state.retargeting() || state.in_place_rig_preview() && state.editable())
        && !context.host.weights_own_viewport
        && context.available(&preview, &media)
        && context.source_matches(&state, &preview, &media, documents.asset_root());
    if !available && let Some((_, revision, _)) = &mut interaction.pick {
        *revision = None;
    }
    if let Some(result) = interaction
        .pick
        .as_mut()
        .and_then(|(_, _, task)| check_ready(task))
    {
        let (source, revision, _) = interaction.pick.take().unwrap();
        if available
            && state.source() == Some(source.as_path())
            && Some(state.revision) == revision
            && let Some(node) = result
        {
            if let Some(s) = state.current_mut() {
                s.select_node(node);
                s.bone_filter = true;
            }
            state.changed();
        }
    }
    let escape = keys
        .as_ref()
        .is_some_and(|k| k.just_pressed(KeyCode::Escape));
    let camera = camera(&canvases, &cameras);
    if let Some(gesture) = interaction.gesture.take() {
        state.pose_mouse_owned = true;
        // 编辑权限随动作和工具页变化，失效时还原按下前的姿势，禁止松手自动插帧。
        let valid = available
            && state.editable()
            && !escape
            && state.source() == Some(gesture.source.as_path())
            && state.revision == gesture.revision
            && state.source_revision == gesture.source_revision
            && state.active_clip() == gesture.clip
            && (!state.in_place_rig_preview() || runtime.published_pose.as_ref()
                .is_some_and(|pose| targets.frame(&state, &runtime, pose, gesture.node).is_some()))
            && camera.is_some();
        if !valid {
            if let Some(s) = state.sessions.get_mut(&gesture.source) {
                s.pose = gesture.previous;
            }
            state.changed();
            return;
        }
        let (_, at, _) = camera.unwrap();
        let mut value = gesture.before;
        match gesture.tool {
            PoseTool::Move => {
                let axis = gesture
                    .constraint
                    .axis()
                    .unwrap_or_else(|| *gesture.camera.ray(Vec2::ZERO).unwrap().direction);
                if gesture.constraint == MoveConstraint::View {
                    let plane = |cursor| {
                        let ray = gesture.camera.ray(cursor)?;
                        let den = ray.direction.dot(axis);
                        (den.abs() > 1e-5)
                            .then(|| ray.get_point((gesture.origin - ray.origin).dot(axis) / den))
                    };
                    if let (Some(a), Some(b)) = (plane(gesture.start), plane(at)) {
                        value.translation += gesture.parent.inverse().transform_vector3(b - a);
                    }
                } else if let (Some(a), Some(b)) = (
                    gesture.camera.project(gesture.origin),
                    gesture.camera.project(gesture.origin + axis),
                ) {
                    let direction = (b - a) * gesture.camera.size;
                    if direction.length_squared() > 1e-6 {
                        let distance = ((at - gesture.start) * gesture.camera.size).dot(direction)
                            / direction.length_squared();
                        value.translation +=
                            gesture.parent.inverse().transform_vector3(axis * distance);
                    }
                }
            }
            PoseTool::Rotate => {
                if let Ok((axis, radians)) = rotation_delta(
                    gesture.camera,
                    gesture.start,
                    at,
                    gesture.origin,
                    gesture.constraint,
                ) {
                    let (_, parent, _) = gesture.parent.to_scale_rotation_translation();
                    value.rotation = (parent.inverse()
                        * Quat::from_axis_angle(axis, radians)
                        * parent
                        * gesture.before.rotation)
                        .normalize();
                }
            }
        }
        let property = match gesture.tool {
            PoseTool::Move => PoseProperty::Translation,
            PoseTool::Rotate => PoseProperty::Rotation,
        };
        if let Some(s) = state.current_mut() {
            let changed = if value == gesture.before {
                let changed = s.pose != gesture.previous;
                if changed {
                    s.pose = gesture.previous.clone();
                }
                changed
            } else {
                s.set_pose(gesture.node, value, property)
            };
            if changed {
                state.pose_revision = state.pose_revision.wrapping_add(1);
            }
        }
        if mouse.pressed(MouseButton::Left) {
            interaction.gesture = Some(gesture);
        } else {
            if value != gesture.before {
                if state.auto_key {
                    super::control::action(&mut state, super::components::AnimationAction::InsertPose);
                } else {
                    state.status = "继续调整骨骼，或将整组姿势插入当前帧".into();
                }
            }
            state.changed();
        }
        return;
    }
    if !available
        || state.busy()
        || interaction.pick.is_some()
        || state.gesture.is_some()
        || context.guard.blocked()
        || context.numeric.as_ref().is_some_and(|n| n.entity.is_some())
        || context
            .numeric_text
            .as_ref()
            .is_some_and(|n| n.entity.is_some())
        || context.text.as_ref().is_some_and(|t| t.block_shortcuts)
        || context.host.scene_dragging
    {
        return;
    }
    let Some((camera, at, hovered)) = camera else {
        return;
    };
    let hovered = hovered && !context.host.shading_hovered;
    // 工具快捷键仅在动画画布悬停且没有鼠标手势时生效；文本、模态、只读和隐藏页沿用上面的门禁。
    if hovered
        && !context.text_input_focused()
        && !mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
        && state.editable()
        && state.current().is_some_and(|s| s.selected_node.is_some())
        && let Some(keys) = keys.as_mut()
        && !keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
        ])
    {
        let action = if keys.just_pressed(KeyCode::KeyG) {
            Some((KeyCode::KeyG, super::components::AnimationAction::MovePose))
        } else if keys.just_pressed(KeyCode::KeyR) {
            Some((
                KeyCode::KeyR,
                super::components::AnimationAction::RotatePose,
            ))
        } else {
            None
        };
        if let Some((key, action)) = action {
            super::control::action(&mut state, action);
            keys.clear_just_pressed(key);
            return;
        }
    }
    if !hovered
        || !mouse.just_pressed(MouseButton::Left)
        || [MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .any(|b| mouse.pressed(b) || mouse.just_released(b))
    {
        return;
    }
    let Some(pose) = runtime.published_pose.as_ref() else {
        return;
    };
    if !pose.can_start_edit(&state) {
        state.pose_mouse_owned = true;
        state.status = "正在准备当前姿态，请稍候再操作骨骼".into();
        return;
    }
    let Some(s) = state.current() else {
        return;
    };
    if let Some(node) = s.selected_node.filter(|&node| s.document.can_animate_node(node))
        && let Some((origin, parent)) = targets.frame(&state, &runtime, pose, node)
    {
        let tool = if state.pose_tool == PoseTool::Move {
            MeshEditTool::Move
        } else {
            MeshEditTool::Rotate
        };
        if state.editable()
            && let Some(constraint) = pick_transform_handle(tool, camera, origin, at)
        {
            interaction.gesture = Some(PoseGesture {
                source: state.source().expect("已验证当前动画源").to_owned(),
                revision: state.revision,
                source_revision: state.source_revision,
                clip: s.clip,
                node,
                before: pose.local[node],
                previous: s.pose.clone(),
                parent,
                origin,
                start: at,
                camera,
                constraint,
                tool: state.pose_tool,
            });
            // 点击播放中的角色时，以实际已经显示的完整姿态冻结时间，避免手柄起点跳到尚未显示的帧。
            state.seek(pose.time);
            state.playing = false;
            state.pose_mouse_owned = true;
            return;
        }
    }
    if state.retargeting() {
        return; // 当前角色用映射后的手柄，不能拿源骨架轮廓拾取目标骨骼。
    }
    // 相机还在重新投影时，等待与视口中菱形、骨段相同的一帧再拾取。
    let Some(view) = runtime.published_skeleton_view.filter(|view|
        view.camera == camera && view.selected == s.selected_node)
    else { return };
    state.seek(pose.time);
    let pose = pose.clone();
    let source = state.source().expect("已验证当前动画源").to_owned();
    let revision = state.revision;
    // 全骨架拾取移至唯一后台槽；大骨架不在点击帧遍历，迟到结果按源与修订拒绝。
    interaction.pick = Some((
        source,
        Some(revision),
        AsyncComputeTaskPool::get().spawn(async move { skeleton::pick(&pose, view, at) }),
    ));
    state.playing = false;
    state.pose_mouse_owned = true;
}

fn draw(
    state: Res<AnimationEditorState>,
    host: Option<Res<AnimationHost>>,
    preview: Res<AssetPreviewState>,
    interaction: Res<PoseInputState>,
    runtime: Res<PreviewRuntime>,
    targets: RigPoseTargets,
    canvases: Query<
        (&ComputedNode, &RelativeCursorPosition, &UiInteraction),
        With<AssetPreviewCanvas>,
    >,
    cameras: Query<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>,
    mut gizmos: Gizmos<AnimationGizmos>,
    mut highlight: Gizmos<AnimationHighlightGizmos>,
) {
    if !state.editable()
        || host.is_some_and(|host| host.weights_own_viewport)
        || if state.in_place_rig_preview() {
            !preview.render_visible || runtime.in_place_hidden_scene != preview.scene_root()
        } else {
            !runtime.document_ready(&state, &preview)
        }
    {
        return;
    }
    let Some(s) = state.current() else {
        return;
    };
    let Some((node, pose)) = s.selected_node.zip(runtime.published_pose.as_ref()) else {
        return;
    };
    if !pose.can_draw_tools(&state, interaction.gesture.is_some()) {
        return;
    }
    let Some((origin, _)) = targets.frame(&state, &runtime, pose, node) else {
        return;
    };
    // 绘制不依赖鼠标是否仍在画布内，移开鼠标后手柄仍可被发现。
    let Ok((canvas, cursor, canvas_interaction)) = canvases.single() else {
        return;
    };
    let Ok((projection, transform)) = cameras.single() else {
        return;
    };
    let Some(camera) = GizmoCamera::from_projection(
        projection,
        transform,
        canvas.size(),
        canvas.inverse_scale_factor(),
    ) else {
        return;
    };
    // 青色选中关节由骨架统一绘制，此处仅保留位移和旋转的交互手柄。
    let active = interaction
        .gesture
        .as_ref()
        .filter(|g| g.node == node)
        .map(|g| g.constraint);
    let hovered = if active.is_none()
        && cursor.cursor_over()
        && *canvas_interaction != UiInteraction::None
        && !state.busy()
    {
        cursor.normalized.and_then(|at| {
            pick_transform_handle(
                if state.pose_tool == PoseTool::Move {
                    MeshEditTool::Move
                } else {
                    MeshEditTool::Rotate
                },
                camera,
                origin,
                at,
            )
        })
    } else {
        None
    };
    let color = |axis| match axis {
        MoveConstraint::X => Color::srgb(1., 0.2, 0.2),
        MoveConstraint::Y => Color::srgb(0.3, 1., 0.3),
        MoveConstraint::Z => Color::srgb(0.3, 0.55, 1.),
        _ => Color::WHITE,
    };
    let feedback = |axis| {
        if active == Some(axis) {
            Some(Color::srgb(1.0, 0.85, 0.15))
        } else if hovered == Some(axis) {
            Some(Color::WHITE)
        } else {
            None
        }
    };
    if state.pose_tool == PoseTool::Move {
        for arrow in move_arrows(camera, origin).into_iter().flatten() {
            let [a, b, c, d] = arrow.world;
            for (a, b) in [(a, b), (b, c), (b, d)] {
                if let Some(color) = feedback(arrow.constraint) {
                    highlight.line(a, b, color);
                } else {
                    gizmos.line(a, b, color(arrow.constraint));
                }
            }
        }
    } else {
        for ring in rotation_rings(camera, origin).into_iter().flatten() {
            for points in ring.world.windows(2) {
                if let Some(color) = feedback(ring.constraint) {
                    highlight.line(points[0], points[1], color);
                } else {
                    gizmos.line(points[0], points[1], color(ring.constraint));
                }
            }
        }
    }
}

#[cfg(any(test, feature = "validation"))]
pub fn animation_overlay_submitted(world: &World) -> bool {
    use bevy::gizmos::GizmoHandles;
    use std::any::TypeId;

    if skeleton::submitted(world) {
        return true;
    }
    let (Some(handles), Some(assets)) = (world.get_resource::<GizmoHandles>(), world.get_resource::<Assets<GizmoAsset>>())
    else { return false };
    [TypeId::of::<AnimationGizmos>(), TypeId::of::<AnimationHighlightGizmos>()]
        .into_iter()
        .filter_map(|group| handles.handles().get(&group).and_then(Option::as_ref))
        .filter_map(|handle| assets.get(handle))
        .any(|asset| !asset.buffer().buffer().list_positions.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bone_segment_selects_its_head_and_joint_selection_has_priority() {
        let mut document = super::super::data::tests::test_document();
        document.nodes[1].translation = [2.0, 0.0, 0.0];
        let document = std::sync::Arc::new(document);
        let runtime = super::super::preview::for_input_tests(document.clone());
        let pose = runtime.published_pose.as_ref().unwrap();
        let projection = Projection::default();
        let transform =
            GlobalTransform::from(Transform::from_xyz(1., 0., 6.).looking_at(Vec3::X, Vec3::Y));
        let camera =
            GizmoCamera::from_projection(&projection, &transform, Vec2::splat(600.), 1.).unwrap();
        let at = |world| camera.project(world).unwrap();
        let view = super::super::preview::SkeletonView { camera, selected: None };
        assert_eq!(skeleton::pick(pose, view, at(Vec3::X)), Some(0));
        assert_eq!(skeleton::pick(pose, view, at(Vec3::ZERO)), Some(0));
        assert_eq!(
            skeleton::pick(pose, view, at(Vec3::X * 2.)),
            Some(1)
        );
        assert_eq!(skeleton::pick(pose, view, at(Vec3::Y * 2.)), None);
    }

    fn fixture(auto_key: bool) -> (App, Entity, Vec2) {
        let (mut app, _, _) = super::super::editor_tests::app_context();
        app.init_resource::<PoseInputState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, input);
        let document = app
            .world()
            .resource::<AnimationEditorState>()
            .document_snapshot()
            .unwrap();
        let runtime = super::super::preview::for_input_tests(document);
        let origin = runtime.published_pose.as_ref().unwrap().world[0].transform_point3(Vec3::ZERO);
        let transform =
            GlobalTransform::from(Transform::from_xyz(0., 2., 6.).looking_at(origin, Vec3::Y));
        let projection = Projection::default();
        let camera =
            GizmoCamera::from_projection(&projection, &transform, Vec2::splat(400.), 1.).unwrap();
        let arrow = move_arrows(camera, origin)
            .into_iter()
            .flatten()
            .find(|a| a.constraint == MoveConstraint::X)
            .unwrap();
        let at = arrow.screen[1] / 400.;
        let canvas = app
            .world_mut()
            .spawn((
                AssetPreviewCanvas,
                ComputedNode {
                    size: Vec2::splat(400.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(at),
                },
                UiInteraction::Hovered,
            ))
            .id();
        app.world_mut()
            .spawn((AssetPreviewCamera, projection, transform));
        app.insert_resource(runtime);
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        state.auto_key = auto_key;
        state.current_mut().unwrap().select_node(0);
        (app, canvas, at)
    }

    #[test]
    fn rig_rotation_uses_visible_target_joint_and_converts_back_to_source_axes() {
        use bevy::ecs::system::RunSystemOnce;
        use feathrise_editor_animation_model::{
            request::{PreviewRigAnimation, RigPreviewRequest},
            retarget::{JointCorrection, RigRetarget},
        };
        use std::sync::Arc;

        let (mut app, source, _) = super::super::editor_tests::app_context();
        let root = app.world().resource::<AnimationHost>().asset_root().to_owned();
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        let mut document = (*state.document_snapshot().unwrap()).clone();
        let channel = &mut document.clips[0].channels[0];
        channel.path = "rotation".into();
        for key in &mut channel.keys {
            key.value = Quat::IDENTITY.to_array().to_vec();
        }
        let node = channel.node;
        *state.current_mut().unwrap() = Session::new(document);
        let revision = state.source_revision();
        state.rig_preview = Some(RigPreviewRequest {
            ticket: revision, source: source.clone(), ready: true, failed: false,
            request: PreviewRigAnimation {
                model_path: "rig.glb".into(), animation_path: "motion.glb".into(), clip_index: 0,
                host_path: "rig.glb".into(), host_generation: 1, host_tab: 1, asset_root: root,
                recheck_source: false,
            },
        });
        state.pose_tool = PoseTool::Rotate;
        let mut runtime = super::super::preview::for_input_tests(state.document_snapshot().unwrap());
        runtime.in_place_hidden_scene = Some(Entity::PLACEHOLDER);
        let request = state.model_binding_revision;
        let parent_rotation = Quat::from_rotation_y(0.6);
        let before = Quat::from_rotation_x(0.4);
        let after = Quat::from_rotation_z(-0.3);
        let parent = app.world_mut().spawn(GlobalTransform::from(Transform::from_rotation(parent_rotation))).id();
        let origin = Vec3::new(1.0, 2.0, 3.0);
        let joint = app.world_mut().spawn((GlobalTransform::from_translation(origin), ChildOf(parent))).id();
        let mut corrections = vec![None; node + 1];
        corrections[node] = Some(JointCorrection { rest: Transform::IDENTITY, before, after });
        let mut binding = BindingState {
            active: true,
            identity: Some(BindingIdentity {
                source, revision, request, model: "rig.glb".into(), retarget: true, embedded_generation: None,
            }),
            mapping: Some(SkeletonMapping {
                names: default(), embedded_nodes: default(),
                retarget: Some(Arc::new(RigRetarget {
                    corrections, clips: vec![true],
                    bounds: AssetPreviewModelBounds { minimum: Vec3::ZERO, maximum: Vec3::ONE },
                })),
            }),
            ..default()
        };
        binding.banks[0].scanned = true;
        binding.banks[0].nodes.insert(node, joint);
        binding.banks[1].scanned = true;
        app.insert_resource(runtime).insert_resource(binding);
        let (center, basis) = app.world_mut().run_system_once(move |
            state: Res<AnimationEditorState>, runtime: Res<PreviewRuntime>, targets: RigPoseTargets,
        | targets.frame(&state, &runtime, runtime.published_pose.as_ref().unwrap(), node))
            .unwrap().expect("已发布的目标关节有且仅有旋转工具");
        assert_eq!(center, origin);
        let (_, basis, _) = basis.to_scale_rotation_translation();
        let delta_world = Quat::from_rotation_z(0.2);
        let new_source = basis.inverse() * delta_world * basis;
        let new_target = parent_rotation * before * new_source * after;
        assert!(new_target.abs_diff_eq(delta_world * parent_rotation * before * after, 1.0e-5));
    }

    #[test]
    fn canvas_tool_shortcuts_switch_and_do_not_steal_text_or_outside_keys() {
        let (mut app, canvas, at) = fixture(true);
        assert!(app.world().resource::<AnimationEditorState>().pose_tool == PoseTool::Move);
        let press = |app: &mut App, key| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            *keys = default();
            keys.press(key);
            drop(keys);
            app.update();
        };
        press(&mut app, KeyCode::KeyR);
        assert!(app.world().resource::<AnimationEditorState>().pose_tool == PoseTool::Rotate);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::KeyR)
        );
        app.world_mut()
            .get_mut::<RelativeCursorPosition>(canvas)
            .unwrap()
            .cursor_over = false;
        press(&mut app, KeyCode::KeyG);
        assert!(app.world().resource::<AnimationEditorState>().pose_tool == PoseTool::Rotate);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::KeyG)
        );
        app.world_mut()
            .get_mut::<RelativeCursorPosition>(canvas)
            .unwrap()
            .cursor_over = true;
        app.insert_resource(feathrise_editor_ui::ActiveNumericInput {
            entity: Some(canvas),
            ..default()
        });
        press(&mut app, KeyCode::KeyG);
        assert!(app.world().resource::<AnimationEditorState>().pose_tool == PoseTool::Rotate);
        app.world_mut()
            .resource_mut::<feathrise_editor_ui::ActiveNumericInput>()
            .entity = None;
        app.init_resource::<bevy::input_focus::InputFocus>();
        for native in [false, true] {
            let field = if native {
                app.world_mut().spawn(bevy::text::EditableText::default()).id()
            } else {
                app.world_mut().spawn(feathrise_editor_core::EditorTextInput).id()
            };
            app.world_mut().resource_mut::<bevy::input_focus::InputFocus>()
                .set(field, bevy::input_focus::FocusCause::Navigated);
            press(&mut app, KeyCode::KeyG);
            assert!(app.world().resource::<AnimationEditorState>().pose_tool == PoseTool::Rotate);
            assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::KeyG),
                "普通文字焦点也保有G键，不仅保护IME输入帧");
            app.world_mut().resource_mut::<bevy::input_focus::InputFocus>().clear();
        }
        press(&mut app, KeyCode::KeyG);
        assert!(app.world().resource::<AnimationEditorState>().pose_tool == PoseTool::Move);
        assert!(
            !app.world()
                .resource::<AnimationEditorState>()
                .current()
                .unwrap()
                .dirty()
        );
        begin_move(&mut app, canvas, at); // 切换后的真实箭头可直接起手拖动，而非只改按钮选中色。
        press(&mut app, KeyCode::KeyR);
        assert!(app.world().resource::<AnimationEditorState>().pose_tool == PoseTool::Move);
    }

    fn begin_move(app: &mut App, canvas: Entity, at: Vec2) {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<PoseInputState>().gesture.is_some());
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .get_mut::<RelativeCursorPosition>(canvas)
            .unwrap()
            .normalized = Some(at + Vec2::new(0.1, 0.));
        app.update();
        assert!(
            app.world()
                .resource::<AnimationEditorState>()
                .current()
                .unwrap()
                .pose
                .as_ref()
                .unwrap()
                .get(0)
                .unwrap()
                .transform
                .translation
                .x
                > 0.1
        );
        assert!(
            app.world()
                .resource::<AnimationEditorState>()
                .pending
                .is_none()
        );
    }

    fn click_published_move_handle(app: &mut App, canvas: Entity) {
        let origin = app.world().resource::<PreviewRuntime>().published_pose.as_ref().unwrap().world[0]
            .transform_point3(Vec3::ZERO);
        let camera = app.world_mut().query_filtered::<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>()
            .single(app.world()).unwrap();
        let camera = GizmoCamera::from_projection(camera.0, camera.1, Vec2::splat(400.), 1.).unwrap();
        let arrow = move_arrows(camera, origin).into_iter().flatten()
            .find(|arrow| arrow.constraint == MoveConstraint::X).unwrap();
        app.world_mut().get_mut::<RelativeCursorPosition>(canvas).unwrap().normalized = Some(arrow.screen[1] / 400.);
        *app.world_mut().resource_mut::<ButtonInput<MouseButton>>() = default();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
    }

    #[test]
    fn read_only_canvas_ignores_pose_tools_and_keeps_bone_selection() {
        let (mut app, canvas, _) = fixture(true);
        app.world_mut().resource_mut::<AnimationEditorState>().exit_animation_edit();
        for (key, tool) in [(KeyCode::KeyR, PoseTool::Move), (KeyCode::KeyG, PoseTool::Rotate)] {
            app.world_mut().resource_mut::<AnimationEditorState>().pose_tool = tool;
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            *keys = default();
            keys.press(key);
            drop(keys);
            app.update();
            assert!(app.world().resource::<AnimationEditorState>().pose_tool == tool);
        }
        app.world_mut().resource_mut::<AnimationEditorState>().pose_tool = PoseTool::Move;
        click_published_move_handle(&mut app, canvas);
        assert!(app.world().resource::<PoseInputState>().gesture.is_none());

        let camera = app.world_mut().query_filtered::<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>()
            .single(app.world()).unwrap();
        let camera = GizmoCamera::from_projection(camera.0, camera.1, Vec2::splat(400.), 1.).unwrap();
        let mut runtime = app.world_mut().resource_mut::<PreviewRuntime>();
        let target = runtime.published_pose.as_ref().unwrap().world[1].transform_point3(Vec3::ZERO);
        runtime.published_skeleton_view = Some(super::super::preview::SkeletonView { camera, selected: Some(0) });
        drop(runtime);
        app.world_mut().get_mut::<RelativeCursorPosition>(canvas).unwrap().normalized = camera.project(target);
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        *mouse = default();
        mouse.press(MouseButton::Left);
        drop(mouse);
        app.update();
        let started = std::time::Instant::now();
        while app.world().resource::<PoseInputState>().pick.is_some() {
            assert!(started.elapsed() < std::time::Duration::from_secs(5), "预览选骨任务应完成");
            app.update();
            std::thread::yield_now();
        }
        let state = app.world().resource::<AnimationEditorState>();
        assert_eq!(state.current().unwrap().selected_node, Some(1));
        assert!(state.pending.is_none() && !state.current().unwrap().dirty());
    }

    #[test]
    fn weights_taking_over_canvas_restores_pose_without_auto_keying() {
        let (mut app, canvas, at) = fixture(true);
        begin_move(&mut app, canvas, at);
        app.world_mut().resource_mut::<AnimationHost>().weights_own_viewport = true;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        let state = app.world().resource::<AnimationEditorState>();
        assert!(app.world().resource::<PoseInputState>().gesture.is_none());
        assert!(state.current().unwrap().pose.is_none());
        assert!(state.pending.is_none() && !state.current().unwrap().dirty());
    }

    #[test]
    fn gizmo_release_records_once_and_escape_restores_the_draft() {
        let (mut app, canvas, at) = fixture(true);
        begin_move(&mut app, canvas, at);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        assert!(matches!(
            app.world().resource::<AnimationEditorState>().pending,
            Some(PendingAction::Command(
                super::super::editing::Command::Pose { .. }
            ))
        ));
        let (mut app, canvas, at) = fixture(true);
        begin_move(&mut app, canvas, at);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        let state = app.world().resource::<AnimationEditorState>();
        assert!(state.current().unwrap().pose.is_none());
        assert!(state.pending.is_none());
        assert!(!state.current().unwrap().dirty());
    }
    #[test]
    fn manual_pose_blocks_interpolation_until_explicitly_reset() {
        use super::super::{components::*, control};
        let (mut app, canvas, at) = fixture(false);
        begin_move(&mut app, canvas, at);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        let pose = state.current().unwrap().pose.clone();
        let document = state.document_snapshot().unwrap();
        control::action(&mut state, AnimationAction::CycleInterpolation);
        assert!(state.pending.is_none(), "修改插值不能通过后台编辑清掉未插帧姿势");
        assert_eq!(state.current().unwrap().pose, pose);
        assert!(state.current().unwrap().dirty());
        assert!(std::sync::Arc::ptr_eq(&document, &state.document_snapshot().unwrap()));
        control::action(&mut state, AnimationAction::ResetPose);
        control::action(&mut state, AnimationAction::CycleInterpolation);
        assert!(matches!(state.pending, Some(PendingAction::Edit(Edit::Interpolation { .. }))));
    }

    #[test]
    fn pose_input_rejects_published_pose_from_another_clip_or_document() {
        use super::super::{components::AnimationAction, control};
        use std::sync::Arc;
        for change_clip in [true, false] {
            let (mut app, canvas, _) = fixture(true);
            let document = {
                let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
                let mut document = (*state.document_snapshot().unwrap()).clone();
                document.clips[1] = document.clips[0].clone();
                for key in &mut document.clips[1].channels[0].keys {
                    key.value[1] += 0.5;
                }
                *state.current_mut().unwrap() = Session::new(document);
                state.document_snapshot().unwrap()
            };
            // 保留上一份完整画面，模拟新采样和分帧发布仍未完成的真实窗口。
            app.insert_resource(super::super::preview::for_input_tests(document.clone()));
            {
                let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
                if change_clip {
                    control::action(&mut state, AnimationAction::NextClip);
                    // 模拟明确选定另一动作后进入编辑，只考察旧姿态的身份保护。
                    assert!(state.select_current_clip() && state.enter_animation_edit());
                } else {
                    let mut document = (*document).clone();
                    for key in &mut document.clips[0].channels[0].keys {
                        key.value[1] += 0.5;
                    }
                    state.current_mut().unwrap().document = Arc::new(document);
                    state.changed();
                }
            }
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            app.update();
            let interaction = app.world().resource::<PoseInputState>();
            assert!(interaction.gesture.is_none(), "旧动作或旧文档不能作为新编辑的姿势起点");
            assert!(interaction.pick.is_none(), "旧姿态也不能按新文档的节点索引启动拾取");
            let state = app.world().resource::<AnimationEditorState>();
            assert!(state.current().unwrap().pose.is_none());
            assert!(state.pending.is_none());
            let document = state.document_snapshot().unwrap();
            let clip = state.active_clip();
            app.insert_resource(super::super::preview::for_input_tests_at(document, clip, 0.0, None));
            click_published_move_handle(&mut app, canvas);
            assert!(app.world().resource::<PoseInputState>().gesture.is_some(), "当前动作完整发布后恢复手柄操作");
        }
    }

    #[test]
    fn paused_seek_waits_for_current_pose_while_playback_can_freeze_the_displayed_frame() {
        let (mut app, canvas, _) = fixture(true);
        app.world_mut().resource_mut::<AnimationEditorState>().seek(1.0);
        click_published_move_handle(&mut app, canvas);
        assert!(app.world().resource::<PoseInputState>().gesture.is_none());
        assert!(app.world().resource::<PoseInputState>().pick.is_none());
        assert_eq!(app.world().resource::<AnimationEditorState>().position(), 1.0);
        app.world_mut().resource_mut::<AnimationEditorState>().playing = true;
        click_published_move_handle(&mut app, canvas);
        assert!(app.world().resource::<PoseInputState>().gesture.is_some());
        let state = app.world().resource::<AnimationEditorState>();
        assert_eq!(state.position(), 0.0, "播放中从已经显示的完整帧开始，而非尚未发布的播放头");
        assert!(!state.playing);
    }

    #[test]
    fn reset_pose_cannot_revive_the_last_displayed_manual_transform() {
        use super::super::{components::AnimationAction, control};
        let (mut app, canvas, at) = fixture(false);
        begin_move(&mut app, canvas, at);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        let (document, draft) = {
            let state = app.world().resource::<AnimationEditorState>();
            (state.document_snapshot().unwrap(), state.current().unwrap().pose.clone())
        };
        assert!(draft.is_some());
        app.insert_resource(super::super::preview::for_input_tests_at(document.clone(), 0, 0.0, draft));
        control::action(&mut app.world_mut().resource_mut::<AnimationEditorState>(), AnimationAction::ResetPose);
        click_published_move_handle(&mut app, canvas);
        assert!(app.world().resource::<PoseInputState>().gesture.is_none());
        assert!(app.world().resource::<PoseInputState>().pick.is_none());
        assert!(!app.world().resource::<AnimationEditorState>().current().unwrap().dirty());
        app.insert_resource(super::super::preview::for_input_tests(document));
        click_published_move_handle(&mut app, canvas);
        assert!(app.world().resource::<PoseInputState>().gesture.is_some());
    }

    #[test]
    fn manual_pose_is_dirty_and_cannot_be_lost_by_scrub_or_save() {
        let (mut app, canvas, at) = fixture(false);
        begin_move(&mut app, canvas, at);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        assert!(state.current().unwrap().dirty());
        assert!(state.pending.is_none());
        state.seek(1.0);
        assert_eq!(state.position(), 0.0);
        super::super::control::action(&mut state, super::super::components::AnimationAction::Save);
        assert!(state.pending.is_none());
        super::super::control::action(
            &mut state,
            super::super::components::AnimationAction::ResetPose,
        );
        assert!(!state.current().unwrap().dirty());
    }
}
