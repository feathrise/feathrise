//! 画布捕获、按逻辑像素采样笔画和透明笔刷/框选提示。

use super::{
    super::{
        AssetPreviewNavigation,
        animation::{AnimationEditorState, AnimationPreviewRuntime, WeightBinding},
        orientation::PreviewAxisRoot,
        preview_shading,
        types::{AssetPreviewCamera, AssetPreviewCanvas},
    },
    edit::{Operation, Selection, Tool},
    panel::BoneToolRail,
    posed::{Bookmark, Capture, Gesture},
    resources::{Reason, Stroke, WeightEditor},
    skeleton::BoneTargets,
    systems::{Gate, Target, queue},
};
use crate::{
    resources::SceneEditorState,
    ui::sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
    viewport::placement_move::MoveCamera,
};
use bevy::{
    prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition},
    window::PrimaryWindow,
};
use feathrise_editor_ui::UiInteraction;
use std::collections::VecDeque;

pub(super) fn input(
    mut state: ResMut<WeightEditor>,
    target: Target,
    gate: Gate,
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    mut animation: Option<ResMut<AnimationEditorState>>,
    mut motion: Option<ResMut<AnimationPreviewRuntime>>,
    binding: Option<Res<WeightBinding>>,
    navigation: Res<AssetPreviewNavigation>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keys: Option<ResMut<ButtonInput<KeyCode>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    canvas: Query<
        (&ComputedNode, &RelativeCursorPosition, &UiInteraction),
        With<AssetPreviewCanvas>,
    >,
    cameras: Query<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>,
    overlays: (
        Query<(&Node, &RelativeCursorPosition), Or<(With<PreviewAxisRoot>, With<BoneToolRail>)>>,
        preview_shading::Pointer,
    ),
    bones: Res<BoneTargets>,
) {
    state.mouse = false;
    let canvas = canvas.single().ok();
    let moving = animation
        .as_ref()
        .is_some_and(|animation| animation.weight_motion_active());
    let enabled = gate.open(&editor)
        && sidebar.right == EditorRightSidebarTab::Weights
        && state.session.as_ref().is_some_and(|session| {
            target.matches(&session.key)
                && !session.stale
                && match session.tool {
                    Tool::Bone => session.show_skeleton && target.preview.render_visible,
                    Tool::Set => false,
                    tool if moving => tool == Tool::Box,
                    _ => !session.original,
                }
        })
        && windows.single().is_ok_and(|window| window.focused);
    if let Some(stroke) = state.stroke.as_mut() {
        if stroke.released || stroke.cancelled {
            return;
        }
        let escape = keys
            .as_ref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Escape));
        if escape && let Some(keys) = keys.as_deref_mut() {
            keys.clear_just_pressed(KeyCode::Escape);
        }
        let release = mouse.just_released(MouseButton::Left) || !mouse.pressed(MouseButton::Left);
        if escape
            || !enabled
            || stroke.problem.is_some_and(|gesture| {
                animation
                    .as_ref()
                    .is_none_or(|animation| !animation.matches_weight_frame(gesture.bookmark.frame))
            })
            || canvas.is_none_or(|(node, _, _)| node.size() != stroke.view.size)
            || mouse.pressed(MouseButton::Right)
            || mouse.pressed(MouseButton::Middle)
        {
            stroke.cancelled = true;
            stroke.dabs.clear();
        } else if let Some((_, cursor, _)) = canvas
            && let Some(position) = cursor.normalized.filter(|position| position.is_finite())
        {
            let normalized = position.clamp(Vec2::splat(-0.5), Vec2::splat(0.5));
            if !stroke.brush.tool.paint() || cursor.cursor_over() {
                stroke.advance(normalized, release);
            }
        }
        stroke.released = release;
        let select = !stroke.brush.tool.paint() && (stroke.released || stroke.cancelled);
        state.mouse = true; // 包括松键帧，笔画从视口拖到别处也不触发场景作者工具。
        if select {
            let stroke = state.stroke.take().expect("当前选区手势");
            if !stroke.cancelled {
                let short =
                    ((stroke.start - stroke.latest) * stroke.view.size * stroke.view.inverse_scale)
                        .length()
                        < 3.0;
                let point = stroke.brush.tool == Tool::Point || short;
                let selection = Selection {
                    view: stroke.view,
                    start: if point { stroke.latest } else { stroke.start },
                    end: stroke.latest,
                    point,
                    extend: stroke.extend,
                    visible: stroke.brush.visible_only,
                };
                if let Some(gesture) = stroke.problem {
                    if !short {
                        state.capture = Some(Capture::new(gesture, selection));
                        state.notice = "正在计算问题帧选区…".into();
                    } else {
                        state.notice = "在画布上拖出一个问题帧选区".into();
                    }
                    state.bump();
                } else {
                    queue(
                        &mut state,
                        Operation::Select(selection, None),
                        Reason::Display,
                    );
                }
            }
        }
        return;
    }
    if !enabled
        || !gate.can_press(&editor)
        || state.busy()
        || !mouse.just_pressed(MouseButton::Left)
        || mouse.pressed(MouseButton::Right)
        || mouse.pressed(MouseButton::Middle)
        || overlays
            .0
            .iter()
            .any(|(node, cursor)| node.display != Display::None && cursor.cursor_over())
        || overlays.1.cursor_over()
    {
        return;
    }
    let Some((node, cursor, interaction)) = canvas else {
        return;
    };
    if !cursor.cursor_over() || *interaction == UiInteraction::None {
        return;
    }
    let (Some(position), Ok((projection, transform))) = (cursor.normalized, cameras.single())
    else {
        return;
    };
    let Some(view) = MoveCamera::from_projection(
        projection,
        transform,
        node.size(),
        node.inverse_scale_factor(),
    ) else {
        return;
    };
    let Some(session) = state.session.as_ref() else {
        return;
    };
    if session.tool == Tool::Bone {
        // 骨骼点击只更新当前关节，播放仍使用已发布的动作姿态。
        let surface = session.surface;
        let picked = bones.pick(session, &target.preview, view, position);
        state.mouse = true;
        if let Some(joint) = picked {
            state.select_joint(surface, joint);
        }
        return;
    }
    let problem = if moving {
        let (Some(animation), Some(motion), Some(binding)) = (
            animation.as_deref_mut(),
            motion.as_deref_mut(),
            binding.as_deref(),
        ) else {
            return;
        };
        let Some((frame, root)) = motion.freeze_weight_frame(animation, binding, &session.key.path)
        else {
            state.notice = "请等待工程动作的当前姿态显示后再框选".into();
            state.bump();
            return;
        };
        Some(Gesture {
            bookmark: Bookmark {
                frame,
                view: navigation.view,
            },
            root,
        })
    } else {
        None
    };
    let brush = session.brush();
    let dabs = if brush.tool.paint() {
        VecDeque::from([position])
    } else {
        VecDeque::new()
    };
    let extend = keys
        .as_ref()
        .is_some_and(|keys| keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
    state.stroke = Some(Stroke {
        surface: session.surface,
        before: session.drafts[session.surface].weights.clone(),
        brush,
        view,
        start: position,
        latest: position,
        last_queued: position,
        dabs,
        extend,
        released: false,
        cancelled: false,
        problem,
    });
    state.mouse = true;
}

#[derive(Component)]
pub(super) struct CursorOverlay;

pub(super) fn setup_overlay(
    mut commands: Commands,
    canvases: Query<Entity, Added<AssetPreviewCanvas>>,
) {
    for canvas in &canvases {
        commands.spawn((
            CursorOverlay,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.35, 0.95, 0.89)),
            FocusPolicy::Pass,
            Pickable::IGNORE,
            ChildOf(canvas),
        ));
    }
}

pub(super) fn present_overlay(
    state: Res<WeightEditor>,
    target: Target,
    sidebar: Res<SceneEditorSidebarState>,
    animation: Option<Res<AnimationEditorState>>,
    canvas: Query<(&ComputedNode, &RelativeCursorPosition), With<AssetPreviewCanvas>>,
    mut overlays: Query<&mut Node, With<CursorOverlay>>,
) {
    let box_or_brush = (|| {
        let session = state.session.as_ref()?;
        let moving = animation
            .as_ref()
            .is_some_and(|animation| animation.weight_motion_active());
        if !target.matches(&session.key)
            || sidebar.right != EditorRightSidebarTab::Weights
            || session.stale
            || session.original && !moving
        {
            return None;
        }
        let (node, cursor) = canvas.single().ok()?;
        let size = node.size() * node.inverse_scale_factor();
        if let Some(stroke) = &state.stroke
            && stroke.brush.tool == super::edit::Tool::Box
            && !stroke.released
            && !stroke.cancelled
        {
            let min = (stroke.start.min(stroke.latest) + 0.5) * size;
            let max = (stroke.start.max(stroke.latest) + 0.5) * size;
            return Some((min, (max - min).max(Vec2::ONE), 0.0));
        }
        if !moving && session.tool.paint() && cursor.cursor_over() {
            let position = (cursor.normalized? + 0.5) * size;
            let radius = session.radius;
            return Some((position - radius, Vec2::splat(radius * 2.0), radius));
        }
        None
    })();
    for mut node in &mut overlays {
        if let Some((min, size, radius)) = box_or_brush {
            node.display = Display::Flex;
            node.left = px(min.x);
            node.top = px(min.y);
            node.width = px(size.x);
            node.height = px(size.y);
            node.border_radius = BorderRadius::all(px(radius));
        } else if node.display != Display::None {
            node.display = Display::None;
        }
    }
}
