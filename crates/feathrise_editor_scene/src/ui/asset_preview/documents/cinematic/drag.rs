//! 抓取键或片段时保存真实轨道与原始开始时间；移动只试播，释放才产生一笔可撤销编辑。
use super::{view::MarkerWidget, *};
use bevy::ui::RelativeCursorPosition;
use feathrise_editor_ui::UiInteraction;

pub(super) struct Gesture {
    key: Identity,
    lane: Lane,
    index: usize,
    item: Item,
    row: Entity,
    origin: f32,
    start: f32,
    length: f32,
    moved: bool,
}
pub(super) fn drag(
    mut studio: ResMut<Studio>,
    docs: Res<DocumentState>,
    mouse: Res<ButtonInput<MouseButton>>,
    markers: Query<(&MarkerWidget, &ChildOf, &UiInteraction)>,
    rows: Query<(&RelativeCursorPosition, &ComputedNode), With<view::LaneWidget>>,
    mut gesture: Local<Option<Gesture>>,
) {
    let Some(key) = active(&docs) else {
        *gesture = None;
        return;
    };
    if studio.job.is_some() || gesture.as_ref().is_some_and(|g| g.key != key) {
        *gesture = None;
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        *gesture = None;
        let Some((widget, parent, _)) = markers
            .iter()
            .find(|(w, _, i)| w.index != usize::MAX && **i == UiInteraction::Pressed)
        else {
            return;
        };
        let Some(origin) = rows
            .get(parent.parent())
            .ok()
            .and_then(|(p, _)| p.normalized)
            .map(|p| p.x)
        else {
            return;
        };
        let Some(m) = studio
            .timeline
            .as_ref()
            .and_then(|t| t.lanes[widget.lane.index()].get(widget.index))
        else {
            return;
        };
        *gesture = Some(Gesture {
            key,
            lane: widget.lane,
            index: widget.index,
            item: m.item,
            row: parent.parent(),
            origin,
            start: m.at,
            length: m.end - m.at,
            moved: false,
        });
    }
    let Some(g) = gesture.as_mut() else {
        return;
    };
    let Some(t) = studio.timeline.as_ref() else {
        *gesture = None;
        return;
    };
    let Ok((pos, node)) = rows.get(g.row) else {
        *gesture = None;
        return;
    };
    let Some(pointer) = pos.normalized else {
        *gesture = None;
        return;
    };
    let delta = pointer.x - g.origin;
    let time = (g.start + delta * t.film.duration).clamp(0., (t.film.duration - g.length).max(0.));
    if !g.moved && delta.abs() * node.size().x >= 3. {
        studio.select(g.lane, Some(g.item));
        studio.selected_marker = g.index;
        g.moved = true;
    }
    if g.moved {
        studio.seek(time);
        studio.form.at = format!("{time:.3}");
        studio.refresh();
    }
    if mouse.just_released(MouseButton::Left) || !mouse.pressed(MouseButton::Left) {
        let g = gesture.take().unwrap();
        if g.moved
            && let Err(e) = start(&mut studio, &docs, Some(Edit::Move { item: g.item, time }))
        {
            studio.notice = e;
        }
    }
}
