//! 日志原生滚动条的悬停与拖动反馈；命中、捕获和尺寸继续由官方控件维护。

use super::components::{LogScrollbar, LogScrollThumb};
use bevy::{picking::hover::Hovered, prelude::*, ui_widgets::ScrollbarDragState};

pub(super) const NORMAL: Color = Color::srgb(0.27, 0.27, 0.27);
pub(super) const HOVERED: Color = Color::srgb(0.42, 0.42, 0.42);
pub(super) const DRAGGING: Color = Color::srgb(0.55, 0.55, 0.55);

pub(super) fn sync(
    tracks: Query<&Hovered, With<LogScrollbar>>,
    mut thumbs: Query<(&ChildOf, &ScrollbarDragState, &mut BackgroundColor), With<LogScrollThumb>>,
) {
    // 只有日志的两根滑块。轨道Hovered包含被拾取的滑块后代；拖出轨道后仍保留原生捕获高亮。
    // 不给ScrollbarThumb添加Node或Button，否则会覆盖官方布局或多出另一套指针行为。
    for (parent, drag, mut background) in &mut thumbs {
        let hovered = tracks.get(parent.parent()).is_ok_and(Hovered::get);
        let color = if drag.dragging { DRAGGING } else if hovered { HOVERED } else { NORMAL };
        if background.0 != color { background.0 = color; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        ecs::system::RunSystemOnce,
        picking::{
            backend::HitData,
            events::{PointerCancel, PointerDragStart, Pointer},
            hover::{HoverMap, update_is_hovered},
            pointer::{Location, PointerButton, PointerId},
        },
        ui::UiScale,
        ui_widgets::{ControlOrientation, Scrollbar, ScrollbarPlugin, ScrollbarThumb},
    };

    fn fixture() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins(ScrollbarPlugin).init_resource::<UiScale>();
        let target = app.world_mut().spawn(ScrollPosition::default()).id();
        let track = app.world_mut().spawn((
            LogScrollbar, Hovered::default(),
            Scrollbar::new(target, ControlOrientation::Vertical, 24.),
        )).id();
        let thumb = app.world_mut().spawn((
            LogScrollThumb, ScrollbarThumb::default(), BackgroundColor(NORMAL),
        )).id();
        app.world_mut().entity_mut(track).add_child(thumb);
        (app, track, thumb)
    }

    fn color(app: &App, thumb: Entity) -> Color {
        app.world().get::<BackgroundColor>(thumb).unwrap().0
    }

    fn location() -> Location {
        Location {
            target: bevy::camera::RenderTarget::Image(Handle::<Image>::default().into()).normalize(None).unwrap(),
            position: Vec2::ZERO,
        }
    }

    #[test]
    fn official_hover_on_thumb_highlights_track_and_restores_after_exit() {
        let (mut app, track, thumb) = fixture();
        let mut map = HoverMap::default();
        map.entry(PointerId::Mouse).or_default().insert(thumb, HitData::new(Entity::PLACEHOLDER, 0., None, None));
        app.insert_resource(map);
        // 只提供官方拾取来源；后代到轨道的Hovered由Bevy自身计算，不能直接写高亮结果。
        app.world_mut().run_system_once(update_is_hovered).unwrap();
        app.world_mut().run_system_once(sync).unwrap();
        assert!(app.world().get::<Hovered>(track).unwrap().get());
        assert_eq!(color(&app, thumb), HOVERED);
        assert!(app.world().get::<Node>(thumb).is_none());

        app.insert_resource(HoverMap::default());
        app.world_mut().run_system_once(update_is_hovered).unwrap();
        app.world_mut().run_system_once(sync).unwrap();
        assert!(!app.world().get::<Hovered>(track).unwrap().get());
        assert_eq!(color(&app, thumb), NORMAL);
    }

    #[test]
    fn official_drag_capture_stays_bright_outside_track_until_cancelled() {
        let (mut app, _, thumb) = fixture();
        // 触发原生拖动观察器，不自行写ScrollbarDragState或伪造捕获完成位。
        app.world_mut().trigger(PointerDragStart {
            entity: thumb,
            pointer: Pointer::new(PointerId::Mouse, location()),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0., None, None),
        });
        app.world_mut().run_system_once(sync).unwrap();
        assert!(app.world().get::<ScrollbarDragState>(thumb).unwrap().dragging);
        assert_eq!(color(&app, thumb), DRAGGING);

        app.world_mut().trigger(PointerCancel { entity: thumb, pointer: Pointer::new(PointerId::Mouse, location()), hit: HitData::new(Entity::PLACEHOLDER, 0., None, None) });
        app.world_mut().run_system_once(sync).unwrap();
        assert!(!app.world().get::<ScrollbarDragState>(thumb).unwrap().dragging);
        assert_eq!(color(&app, thumb), NORMAL);
    }
}
