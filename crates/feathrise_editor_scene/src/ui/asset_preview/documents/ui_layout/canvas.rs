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
    scale: f32,
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
        let Some(widget) = layout.root.find(&item.0) else {
            continue;
        };
        let Ok((container, parent_transform)) = geometry.get(parent.parent()) else {
            continue;
        };
        let inv = bounds.inverse_scale_factor();
        let position = widget.position.unwrap_or_else(|| {
            (transform.affine().translation - parent_transform.affine().translation
                + (container.size() - bounds.size()) * 0.5)
                * inv
        });
        let scale = window.single().map_or(1.0, Window::scale_factor) * inv;
        *drag = Some(Drag {
            entity: event.entity,
            position,
            scale,
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
        let position = track.position + event.distance * track.scale;
        node.position_type = PositionType::Absolute;
        node.left = Val::Px(position.x);
        node.top = Val::Px(position.y);
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
        let position = track.position + event.distance * track.scale;
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
