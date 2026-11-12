//! 以同一预览相机投影关节点，在按下时冻结视平面，旋转视角后可修正深度。

use bevy::{camera::Projection, prelude::*, ui::RelativeCursorPosition};
use feathrise_editor_ui::UiInteraction;

use feathrise_editor_glb_model::rigging::Landmarks;

use super::{
    super::{AssetPreviewNavigation, types::{AssetPreviewCamera, AssetPreviewCanvas}},
    context::Context,
    resources::RiggingState,
    types::*,
};
use crate::{ui::theme as colors, viewport::placement_move::MoveCamera};

fn plane_point(camera: MoveCamera, cursor: Vec2, anchor: Vec3, normal: Dir3) -> Option<Vec3> {
    let ray = camera.ray(cursor)?;
    let denominator = ray.direction.dot(*normal);
    if denominator.abs() < 1e-5 {
        return None;
    }
    let position = ray.get_point((anchor - ray.origin).dot(*normal) / denominator);
    position.is_finite().then_some(position)
}

fn opposite(index: usize) -> Option<usize> {
    // 两侧七组点按同一顺序登记，头、颈、骨盆没有镜像配对。
    match index {
        3..=9 => Some(index + 7),
        10..=16 => Some(index - 7),
        _ => None,
    }
}

fn edit_markers(points: &mut Landmarks, drag: &LandmarkDrag, wanted: Vec3) {
    if let Some(point) = points.get_mut(LANDMARKS[drag.index].0) {
        *point = wanted.to_array();
    }
    if let Some(mirror) = &drag.mirror
        && let Some(point) = points.get_mut(LANDMARKS[mirror.index].0)
    {
        // 单击、回到起点和取消都保留原估计，真实位移才按冻结的人体中线镜像。
        *point = if wanted == drag.before { mirror.before } else { mirror.frame.reflect(wanted) }.to_array();
    }
}

pub(super) fn input(
    context: Context,
    mut state: ResMut<RiggingState>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keys: Option<ResMut<ButtonInput<KeyCode>>>,
    navigation: Res<AssetPreviewNavigation>,
    canvas: Query<
        (&ComputedNode, &RelativeCursorPosition, &UiInteraction),
        With<AssetPreviewCanvas>,
    >,
    camera: Query<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>,
    axes: Query<(&Node, &RelativeCursorPosition), With<super::super::orientation::PreviewAxisRoot>>,
    shading: super::super::preview_shading::Pointer,
) {
    if state.mouse_owned {
        state.mouse_owned = false;
    }
    let source = context.candidate();
    let enabled = state.panel_open
        && state.work.is_none()
        && context.can_input()
        && state
            .session
            .as_ref()
            .is_some_and(|session| source.is_some_and(|source| session.matches(source)));
    let canvas = canvas.single().ok();

    if state.drag.is_some() {
        let drag = state.drag.take().expect("已核对关节点拖动存在");
        // 鼠标拖出或换源后，释放帧仍归原手势，不能落进场景放置或相机操作。
        state.mouse_owned = true;
        let escape = keys
            .as_ref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Escape));
        if escape && let Some(keys) = &mut keys {
            keys.clear_just_pressed(KeyCode::Escape);
        }
        let Some(draft) = state
            .session
            .as_mut()
            .and_then(|session| session.draft.as_mut())
        else {
            return;
        };
        if escape || !enabled || canvas.is_none_or(|(node, _, _)| node.size() != drag.camera.size) {
            edit_markers(&mut draft.points, &drag, drag.before);
            return;
        }
        if let Some((_, cursor, _)) = canvas
            && let Some(cursor) = cursor.normalized
            && let Some(position) = plane_point(drag.camera, cursor, drag.before, drag.normal)
        {
            let mut wanted = drag.before + (position - drag.start);
            if let Some((snap, horizontal, vertical)) = drag.snap && wanted != drag.before {
                // 只吸附冻结视平面的两轴，保持关节深度。镜像在吸附之后一起更新。
                let u = wanted.dot(horizontal);
                let v = wanted.dot(vertical);
                if let (Some(snapped_u), Some(snapped_v)) = (snap.axis(u, 0), snap.axis(v, 1)) {
                    wanted += horizontal * (snapped_u - u) + vertical * (snapped_v - v);
                }
            }
            if let Some(bounds) = context.preview.model_bounds {
                let padding = ((bounds.maximum - bounds.minimum).max_element() * 0.15).max(0.001);
                wanted = wanted.clamp(
                    bounds.minimum - Vec3::splat(padding),
                    bounds.maximum + Vec3::splat(padding),
                );
            }
            edit_markers(&mut draft.points, &drag, wanted);
        }
        if mouse.pressed(MouseButton::Left) {
            state.drag = Some(drag);
        }
        return;
    }

    if !enabled
        || axes.single().is_ok_and(|(node, cursor)| node.display != Display::None && cursor.cursor_over())
        || shading.cursor_over()
        || context
            .mouse_guard
            .as_ref()
            .is_some_and(|guard| guard.blocked())
        || !mouse.just_pressed(MouseButton::Left)
        || [MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .any(|button| mouse.pressed(button) || mouse.just_released(button))
    {
        return;
    }
    let Some((node, cursor, interaction)) = canvas else {
        return;
    };
    if !cursor.cursor_over() || *interaction == UiInteraction::None {
        return;
    }
    let (Some(cursor), Ok((projection, transform))) = (cursor.normalized, camera.single()) else {
        return;
    };
    let Some(camera) = MoveCamera::from_projection(
        projection,
        transform,
        node.size(),
        node.inverse_scale_factor(),
    ) else {
        return;
    };
    let Some(draft) = state
        .session
        .as_ref()
        .and_then(|session| session.draft.as_ref())
    else {
        return;
    };
    let nearest = LANDMARKS
        .iter()
        .enumerate()
        .filter_map(|(index, (key, _))| {
            let world = Vec3::from_array(*draft.points.get(*key)?);
            let point = camera.project(world)?;
            let pixels = (point - cursor) * camera.size * camera.inverse_scale;
            (pixels.length_squared() <= 100.0).then_some((index, world, pixels.length_squared()))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2));
    let (Some((index, before, _)), Some(center)) = (nearest, camera.ray(Vec2::ZERO)) else {
        return;
    };
    let Some(start) = plane_point(camera, cursor, before, center.direction) else {
        return;
    };
    let mirror = if state.independent_markers {
        None
    } else {
        opposite(index).zip(draft.frame).and_then(|(index, frame)| {
            Some(MirrorDrag {
                index,
                before: Vec3::from_array(*draft.points.get(LANDMARKS[index].0)?),
                frame,
            })
        })
    };
    if let Some(draft) = state
        .session
        .as_mut()
        .and_then(|session| session.draft.as_mut())
    {
        draft.selected = Some(index);
    }
    state.mouse_owned = true;
    if mouse.pressed(MouseButton::Left) {
        let snap = context.central().then(|| {
            let (_, rotation, _) = transform.to_scale_rotation_translation();
            let (horizontal, vertical) = if center.direction.y.abs() > 0.999_999 {
                (Vec3::X, Vec3::Z) // 顶视和水平世界网格使用同一组轴。
            } else {
                (rotation * Vec3::X, rotation * Vec3::Y)
            };
            let control = keys.as_ref().is_some_and(|keys| {
                keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
            });
            let snap = navigation.snapping().translation_snap(
                control,
                [before.dot(horizontal), before.dot(vertical), 0.0],
            );
            snap.active().then_some((snap, horizontal, vertical))
        }).flatten();
        state.drag = Some(LandmarkDrag {
            index,
            before,
            start,
            normal: center.direction,
            camera,
            snap,
            mirror,
        });
    }
}

pub(super) fn present(
    context: Context,
    state: Res<RiggingState>,
    canvas: Query<&ComputedNode, With<AssetPreviewCanvas>>,
    camera: Query<(&Projection, &Transform), With<AssetPreviewCamera>>,
    mut dots: Query<(&RigLandmarkDot, &mut Node, &mut BackgroundColor)>,
) {
    let view = (|| {
        let source = context.candidate()?;
        let session = state
            .session
            .as_ref()
            .filter(|session| state.panel_open && session.matches(source))?;
        let draft = session.draft.as_ref()?;
        let node = canvas.single().ok()?;
        let (projection, transform) = camera.single().ok()?;
        // 预览相机是无父节点的根，表现阶段用本帧相机位姿而非上一帧传播值。
        let transform = GlobalTransform::from(*transform);
        let camera = MoveCamera::from_projection(
            projection,
            &transform,
            node.size(),
            node.inverse_scale_factor(),
        )?;
        Some((draft, camera))
    })();
    for (dot, mut node, mut color) in &mut dots {
        // 普通资产 Scene0 根保持单位变换，默认 glTF 装载与后台标记共用原场景 Y-up 坐标。
        let position = view
            .and_then(|(draft, camera)| {
                camera.project(Vec3::from_array(*draft.points.get(LANDMARKS[dot.0].0)?))
            })
            .filter(|position| position.abs().max_element() <= 0.5);
        let display = if position.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let Some(position) = position else {
            continue;
        };
        let position = (position + Vec2::splat(0.5)) * 100.0;
        let (left, top) = (percent(position.x), percent(position.y));
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
        let selected = view.is_some_and(|(draft, _)| draft.selected == Some(dot.0));
        let shade = if selected {
            Color::WHITE
        } else if (3..10).contains(&dot.0) {
            Color::srgb(0.25, 0.82, 0.92)
        } else if dot.0 >= 10 {
            Color::srgb(0.99, 0.53, 0.24)
        } else {
            colors::TEXT_ACCENT
        };
        if color.0 != shade {
            color.0 = shade;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rig_marker_pick_and_drag_share_the_visible_camera_plane() {
        let projection = Projection::Perspective(PerspectiveProjection::default());
        let transform =
            GlobalTransform::from(Transform::from_xyz(3.0, 2.0, 4.0).looking_at(Vec3::Y, Vec3::Y));
        let camera =
            MoveCamera::from_projection(&projection, &transform, Vec2::new(960.0, 640.0), 0.75)
                .unwrap();
        let initial = Vec3::new(0.4, 1.25, -0.05);
        let cursor = camera.project(initial).unwrap();
        let normal = camera.ray(Vec2::ZERO).unwrap().direction;
        let same = plane_point(camera, cursor, initial, normal).unwrap();
        assert!(same.abs_diff_eq(initial, 0.0001));

        let next_cursor = cursor + Vec2::new(0.04, -0.025);
        let moved = plane_point(camera, next_cursor, initial, normal).unwrap();
        assert!((moved - initial).dot(*normal).abs() < 0.0001);
        assert!(
            camera
                .project(moved)
                .unwrap()
                .abs_diff_eq(next_cursor, 0.0001)
        );
    }
}
