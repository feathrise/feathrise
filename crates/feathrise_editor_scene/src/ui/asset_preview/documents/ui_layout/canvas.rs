//! 画布命中与拖动。拖动中只更新被拖控件的 Bevy 节点，松手形成一条撤销记录。

use bevy::{
    picking::{
        events::{PointerCancel, PointerClick, PointerDrag, PointerDragEnd, PointerDragStart},
        pointer::PointerButton,
    },
    prelude::*,
    ui::UiGlobalTransform,
    window::PrimaryWindow,
};

use super::{DocumentState, Edit, UiLayouts};
use crate::resources::SceneEditorState;

#[derive(Component)]
pub(super) struct CanvasItem(pub String);

pub(super) struct Drag {
    entity: Entity,
    position: Vec2,
    screen_to_local: Mat2,
}

pub(super) fn input(
    editor: Res<SceneEditorState>,
    mut state: ResMut<UiLayouts>,
    docs: Res<DocumentState>,
    mut clicks: MessageReader<PointerClick>,
    mut starts: MessageReader<PointerDragStart>,
    mut movement: MessageReader<PointerDrag>,
    mut ends: MessageReader<PointerDragEnd>,
    mut cancelled: MessageReader<PointerCancel>,
    mut drag: Local<Option<Drag>>,
    mut items: Query<(
        &CanvasItem,
        &ComputedNode,
        &UiGlobalTransform,
        &ChildOf,
        &mut Node,
    )>,
    geometry: Query<(&ComputedNode, &UiGlobalTransform)>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    if !editor.open || editor.close_requested || state.interactive || !state.editable(&docs) {
        clicks.clear();
        starts.clear();
        movement.clear();
        ends.clear();
        cancelled.clear();
        *drag = None;
        return;
    }
    let layout = state.layout.clone().expect("画布已采用");
    for event in starts
        .read()
        .filter(|event| event.button == PointerButton::Primary)
    {
        let Ok((item, bounds, transform, parent, _)) = items.get_mut(event.entity) else {
            continue;
        };
        if item.0 == layout.root.id {
            continue;
        }
        if layout.root.find(&item.0).is_none() {
            continue;
        }
        let Ok((container, parent_transform)) = geometry.get(parent.parent()) else {
            continue;
        };
        let inv = bounds.inverse_scale_factor();
        // 用父级真实变换反算设计坐标；缩小到预览区的 1920 画布不能直接使用屏幕位移。
        let inverse = parent_transform.affine().inverse();
        let position = (inverse.transform_point2(transform.affine().translation)
            + (container.size() - bounds.size()) * 0.5) * inv;
        let screen_to_local = inverse.matrix2 * (window.single().map_or(1.0, Window::scale_factor) * inv);
        *drag = Some(Drag {
            entity: event.entity,
            position,
            screen_to_local,
        });
    }
    for event in movement
        .read()
        .filter(|event| event.button == PointerButton::Primary)
    {
        let Some(track) = drag.as_ref().filter(|track| track.entity == event.entity) else {
            continue;
        };
        let Ok((_, _, _, _, mut node)) = items.get_mut(track.entity) else {
            continue;
        };
        let position = track.position + track.screen_to_local * event.distance;
        node.position_type = PositionType::Absolute;
        node.left = Val::Px(position.x);
        node.top = Val::Px(position.y);
        node.right = Val::Auto;
        node.bottom = Val::Auto;
        node.margin = UiRect::ZERO;
    }
    for event in ends
        .read()
        .filter(|event| event.button == PointerButton::Primary)
    {
        let Some(track) = drag.take().filter(|track| track.entity == event.entity) else {
            continue;
        };
        let Ok((item, ..)) = items.get(track.entity) else {
            continue;
        };
        let id = item.0.clone();
        let position = track.position + track.screen_to_local * event.distance;
        state.submit(&docs, Edit::Position(id, position));
    }
    if cancelled.read().last().is_some() && drag.take().is_some() {
        // 丢失指针捕获时从已采用数据恢复临时位置。
        state.set_changed();
    }
    if drag.is_none()
        && let Some(id) = clicks
            .read()
            .filter(|event| event.button == PointerButton::Primary)
            .find_map(|event| items.get(event.entity).ok().map(|(item, ..)| &item.0))
        && state.selected != *id
    {
        state.selected.clone_from(id);
    }
}
