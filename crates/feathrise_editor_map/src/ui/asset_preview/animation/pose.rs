//! 画布点选骨骼和三轴姿势手柄；复用编辑器投影与环拾取，松开才提交一次动画历史。

use feathrise_editor_ui::UiInteraction;
use super::super::{AssetPreviewWorkspace, types::*};
use super::{context::AnimationContext, preview::PreviewRuntime, resources::*, types::*};
use crate::{
    MapEditorSystemSet,
    authoring::mesh_edit::{MeshEditTool, MoveConstraint, transform_math::*},
    viewport::placement_move::MoveCamera,
};
use bevy::{
    camera::{Projection, visibility::RenderLayers},
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
    ui::RelativeCursorPosition,
};
use feathrise_editor_core::constants::render_layers::MAP_EDITOR_ASSET_PREVIEW_LAYER;

pub(super) fn install(app: &mut App) {
    install_visuals(app);
    app.add_systems(
        Update,
        input
            .after(super::timeline::input)
            .before(crate::authoring::mesh_edit::MeshEditInput)
            .before(crate::ui::actions::handle_editor_shortcuts)
            .before(super::super::navigation::control_asset_preview)
            .before(
                crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
            )
            .in_set(MapEditorSystemSet::Input),
    );
}

pub(super) fn install_visuals(app: &mut App) {
    app.init_resource::<PoseInputState>()
        .init_gizmo_group::<AnimationGizmos>()
        .init_gizmo_group::<AnimationHighlightGizmos>()
        .add_systems(Startup, configure)
        .add_systems(
            Update,
            draw.after(super::preview::drive)
                .in_set(MapEditorSystemSet::Presentation),
        );
}
fn configure(mut store: ResMut<bevy::gizmos::config::GizmoConfigStore>) {
    let (config, _) = store.config_mut::<AnimationGizmos>();
    config.render_layers = RenderLayers::layer(MAP_EDITOR_ASSET_PREVIEW_LAYER);
    config.depth_bias = -1.0;
    config.line.width = 2.5;
    let (config, _) = store.config_mut::<AnimationHighlightGizmos>();
    config.render_layers = RenderLayers::layer(MAP_EDITOR_ASSET_PREVIEW_LAYER);
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
) -> Option<(MoveCamera, Vec2, bool)> {
    let (node, cursor, interaction) = canvases.single().ok()?;
    let (projection, transform) = cameras.single().ok()?;
    Some((
        MoveCamera::from_projection(
            projection,
            transform,
            node.size(),
            node.inverse_scale_factor(),
        )?,
        cursor.normalized?,
        cursor.cursor_over() && *interaction != UiInteraction::None,
    ))
}

pub(super) fn input(
    context: AnimationContext,
    preview: Res<AssetPreviewState>,
    media: Res<AssetPreviewWorkspace>,
    documents: Res<super::super::DocumentState>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keys: Option<ResMut<ButtonInput<KeyCode>>>,
    mut state: ResMut<AnimationEditorState>,
    mut interaction: ResMut<PoseInputState>,
    runtime: Res<PreviewRuntime>,
    canvases: Query<
        (&ComputedNode, &RelativeCursorPosition, &UiInteraction),
        With<AssetPreviewCanvas>,
    >,
    cameras: Query<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>,
) {
    state.pose_mouse_owned = false;
    let available = state.visible()
        && !state.retargeting()
        && context.available(&preview, &media)
        && preview
            .selected_path
            .as_ref()
            .is_some_and(|p| state.source() == Some(documents.asset_root().join(p).as_path()));
    if let Some(result) = interaction
        .pick
        .as_mut()
        .and_then(|(_, _, task)| check_ready(task))
    {
        let (source, revision, _) = interaction.pick.take().unwrap();
        if available
            && state.source() == Some(source.as_path())
            && state.revision == revision
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
        let valid = available
            && !escape
            && state.source() == Some(gesture.source.as_path())
            && state.revision == gesture.revision
            && state.source_revision == gesture.source_revision
            && state.active_clip() == gesture.clip
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
        if let Some(s) = state.current_mut() {
            if s.pose != Some((gesture.node, value)) {
                s.pose = Some((gesture.node, value));
                state.pose_revision = state.pose_revision.wrapping_add(1);
            }
        }
        if mouse.pressed(MouseButton::Left) {
            interaction.gesture = Some(gesture);
        } else {
            if value == gesture.before {
                if let Some(s) = state.current_mut() {
                    s.pose = gesture.previous;
                }
            } else if state.auto_key {
                super::control::action(&mut state, super::components::AnimationAction::InsertPose);
            } else {
                state.status = "临时姿势：点击插入姿势保存到当前帧，或还原姿势".into();
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
        || context
            .map_drag
            .as_ref()
            .is_some_and(|s| s.blocks_other_mouse() || s.press.is_some())
    {
        return;
    }
    let Some((camera, at, hovered)) = camera else {
        return;
    };
    // 工具快捷键仅在动画画布悬停且没有鼠标手势时生效；文本、模态、只读和隐藏页沿用上面的门禁。
    if hovered
        && !context.text_input_focused()
        && !mouse.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle])
        && state
            .current()
            .is_some_and(|s| !s.document.read_only && s.selected_node.is_some())
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
        state.status = "正在准备当前姿态，完成后可编辑骨骼".into();
        return;
    }
    let Some(s) = state.current() else {
        return;
    };
    if let Some(node) = s.selected_node
        && let Some(world) = pose.world.get(node)
    {
        let tool = if state.pose_tool == PoseTool::Move {
            MeshEditTool::Move
        } else {
            MeshEditTool::Rotate
        };
        let origin = world.transform_point3(Vec3::ZERO);
        if !s.document.read_only
            && let Some(constraint) = pick_transform_handle(tool, camera, origin, at)
        {
            interaction.gesture = Some(PoseGesture {
                source: state.active.clone().unwrap(),
                revision: state.revision,
                source_revision: state.source_revision,
                clip: s.clip,
                node,
                before: s.pose.map_or(pose.local[node], |(_, t)| t),
                previous: s.pose,
                parent: s.document.nodes[node]
                    .parent
                    .and_then(|p| pose.world.get(p))
                    .copied()
                    .unwrap_or(Mat4::IDENTITY),
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
    if s.pose.is_some() {
        state.status = "请先插入或还原当前临时姿势，再选择其他骨骼".into();
        return;
    }
    let document = s.document.clone();
    state.seek(pose.time);
    let pose = pose.clone();
    let source = state.active.clone().unwrap();
    let revision = state.revision;
    // 全骨架拾取移至唯一后台槽；大骨架不在点击帧遍历，迟到结果按源与修订拒绝。
    interaction.pick = Some((
        source,
        revision,
        AsyncComputeTaskPool::get().spawn(async move { pick_bone(&document, &pose, camera, at) }),
    ));
    state.playing = false;
    state.pose_mouse_owned = true;
}

/// 点优先、连杆其次，线段归属于其子节点；仍在唯一后台槽遍历，不扩大点击帧工作量。
fn pick_bone(
    document: &super::data::AnimationDocument,
    pose: &super::preview::Pose,
    camera: MoveCamera,
    at: Vec2,
) -> Option<usize> {
    let logical = camera.size * camera.inverse_scale;
    let point = at * logical;
    let joint = pose
        .joints
        .iter()
        .enumerate()
        .filter(|(i, _)| pose.animated[*i])
        .filter_map(|(i, j)| {
            let distance = (camera.project(j.translation)? * logical).distance(point);
            (distance <= 14.0).then_some((i, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    joint.map(|(i, _)| i).or_else(|| {
        document
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| pose.animated[*i])
            .filter_map(|(i, node)| {
                let a = camera.project(pose.joints.get(node.parent?)?.translation)? * logical;
                let b = camera.project(pose.joints[i].translation)? * logical;
                let edge = b - a;
                if edge.length_squared() < 1e-6 {
                    return None;
                }
                let distance = point.distance(
                    a + edge * ((point - a).dot(edge) / edge.length_squared()).clamp(0.0, 1.0),
                );
                (distance <= 8.0).then_some((i, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    })
}

fn draw(
    state: Res<AnimationEditorState>,
    preview: Res<AssetPreviewState>,
    interaction: Res<PoseInputState>,
    runtime: Res<PreviewRuntime>,
    canvases: Query<
        (&ComputedNode, &RelativeCursorPosition, &UiInteraction),
        With<AssetPreviewCanvas>,
    >,
    cameras: Query<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>,
    mut gizmos: Gizmos<AnimationGizmos>,
    mut highlight: Gizmos<AnimationHighlightGizmos>,
) {
    if !runtime.document_ready(&state, &preview) {
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
    let Some(world) = pose.world.get(node) else {
        return;
    };
    // 绘制不依赖鼠标是否仍在画布内，移开鼠标后手柄仍可被发现。
    let Ok((canvas, cursor, canvas_interaction)) = canvases.single() else {
        return;
    };
    let Ok((projection, transform)) = cameras.single() else {
        return;
    };
    let Some(camera) = MoveCamera::from_projection(
        projection,
        transform,
        canvas.size(),
        canvas.inverse_scale_factor(),
    ) else {
        return;
    };
    let origin = world.transform_point3(Vec3::ZERO);
    // 每帧仅访问所选节点和父节点；覆盖来自已发布完整姿态，不随双容器换帧丢失选中反馈。
    // 青色粗连杆和固定屏幕尺寸方框独立于“橙色表示动画通道”，模型遮挡时也能辨识。
    let selected = Color::srgb(0.15, 1.0, 1.0);
    if let Some(parent) = s
        .document
        .nodes
        .get(node)
        .and_then(|n| n.parent)
        .and_then(|p| pose.world.get(p))
    {
        highlight.line(parent.transform_point3(Vec3::ZERO), origin, selected);
    }
    if let Some(corners) = camera.screen_square(origin, 8.0) {
        for i in 0..4 {
            highlight.line(corners[i], corners[(i + 1) % 4], selected);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bone_segment_selects_its_child_and_joint_selection_has_priority() {
        let mut document = super::super::data::tests::test_document();
        document.nodes[1].translation = [2.0, 0.0, 0.0];
        let document = std::sync::Arc::new(document);
        let runtime = PreviewRuntime::for_input_tests(document.clone());
        let pose = runtime.published_pose.as_ref().unwrap();
        let projection = Projection::default();
        let transform =
            GlobalTransform::from(Transform::from_xyz(1., 0., 6.).looking_at(Vec3::X, Vec3::Y));
        let camera =
            MoveCamera::from_projection(&projection, &transform, Vec2::splat(600.), 1.).unwrap();
        let at = |world| camera.project(world).unwrap();
        assert_eq!(pick_bone(&document, pose, camera, at(Vec3::X)), Some(1));
        assert_eq!(pick_bone(&document, pose, camera, at(Vec3::ZERO)), Some(0));
        assert_eq!(
            pick_bone(&document, pose, camera, at(Vec3::X * 2.)),
            Some(1)
        );
        assert_eq!(pick_bone(&document, pose, camera, at(Vec3::Y * 2.)), None);
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
        let runtime = PreviewRuntime::for_input_tests(document);
        let origin = runtime.published_pose.as_ref().unwrap().world[0].transform_point3(Vec3::ZERO);
        let transform =
            GlobalTransform::from(Transform::from_xyz(0., 2., 6.).looking_at(origin, Vec3::Y));
        let projection = Projection::default();
        let camera =
            MoveCamera::from_projection(&projection, &transform, Vec2::splat(400.), 1.).unwrap();
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
                app.world_mut().spawn(crate::components::EditorTextInput).id()
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
                .unwrap()
                .1
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
        let camera = MoveCamera::from_projection(camera.0, camera.1, Vec2::splat(400.), 1.).unwrap();
        let arrow = move_arrows(camera, origin).into_iter().flatten()
            .find(|arrow| arrow.constraint == MoveConstraint::X).unwrap();
        app.world_mut().get_mut::<RelativeCursorPosition>(canvas).unwrap().normalized = Some(arrow.screen[1] / 400.);
        *app.world_mut().resource_mut::<ButtonInput<MouseButton>>() = default();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
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
        let pose = state.current().unwrap().pose;
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
            app.insert_resource(PreviewRuntime::for_input_tests(document.clone()));
            {
                let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
                if change_clip {
                    control::action(&mut state, AnimationAction::NextClip);
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
            app.insert_resource(PreviewRuntime::for_input_tests_at(document, clip, 0.0, None));
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
            (state.document_snapshot().unwrap(), state.current().unwrap().pose)
        };
        assert!(draft.is_some());
        app.insert_resource(PreviewRuntime::for_input_tests_at(document.clone(), 0, 0.0, draft));
        control::action(&mut app.world_mut().resource_mut::<AnimationEditorState>(), AnimationAction::ResetPose);
        click_published_move_handle(&mut app, canvas);
        assert!(app.world().resource::<PoseInputState>().gesture.is_none());
        assert!(app.world().resource::<PoseInputState>().pick.is_none());
        assert!(!app.world().resource::<AnimationEditorState>().current().unwrap().dirty());
        app.insert_resource(PreviewRuntime::for_input_tests(document));
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
