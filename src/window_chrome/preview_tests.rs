//! 原生预览往返保持作者原三键与命中几何，隐藏后不能继续发布旧按钮坐标。
use super::*;
use crate::window_chrome::resources::HeaderParts;

fn measured_node(world: &mut World, bounds: PixelRect) -> Entity {
    let size = Vec2::new((bounds.right - bounds.left) as f32, (bounds.bottom - bounds.top) as f32);
    let center = Vec2::new((bounds.right + bounds.left) as f32 / 2., (bounds.bottom + bounds.top) as f32 / 2.);
    world.spawn((Node::default(), ComputedNode { size, ..default() },
        UiGlobalTransform::from_translation(center), InheritedVisibility::VISIBLE)).id()
}

fn fixture() -> (World, Entity, Entity, [Entity; 3]) {
    let mut world = World::new();
    world.init_resource::<ChromeParts>();
    world.init_resource::<PreviewChromeLayout>();
    world.init_resource::<ChromeGeometry>();
    world.init_resource::<crate::embedded_viewport::EmbeddedStatus>();
    let header = measured_node(&mut world, PixelRect { left: 0, top: 0, right: 800, bottom: 54 });
    let spacer = measured_node(&mut world, PixelRect { left: 0, top: 0, right: 602, bottom: 54 });
    let captions = world.spawn(Node::default()).id();
    let buttons: [Entity; 3] = [WindowAction::Minimize, WindowAction::Maximize, WindowAction::Close]
        .into_iter().enumerate().map(|(index, action)| {
            let left = 602 + index as i32 * 66;
            let entity = measured_node(&mut world, PixelRect { left, top: 0, right: left + 66, bottom: 54 });
            world.entity_mut(entity).insert((Button, ChromeButton::Window(action)));
            entity
        }).collect::<Vec<_>>().try_into().unwrap();
    world.entity_mut(header).add_children(&[spacer, captions]);
    world.entity_mut(captions).add_children(&buttons);
    world.resource_mut::<ChromeParts>().0 = Some(HeaderParts {
        header, spacer, captions, project: None, history: vec![], selection: spacer,
        settings: spacer, modes: None, profile: None, run: spacer, layout_controls: vec![],
        more_toggle: spacer, more: spacer, backdrop: spacer,
    });
    (world, header, captions, buttons)
}

#[test]
fn native_preview_keeps_original_header_caption_entities_and_physical_hit_geometry() {
    let (mut world, header, captions, buttons) = fixture();
    measure(&mut world);
    let original = *world.resource::<ChromeGeometry>();
    let count = world.entities().len();
    assert!(original.visible);
    assert_eq!(original.close, PixelRect { left: 734, top: 0, right: 800, bottom: 54 });
    for playing in [true, true, false, true, false] {
        world.insert_resource(crate::embedded_viewport::EmbeddedStatus {
            active: true, ready: true, playing, ..default()
        });
        measure(&mut world);
        assert_eq!(*world.resource::<ChromeGeometry>(), original);
        assert_eq!(*world.resource::<PreviewChromeLayout>(), PreviewChromeLayout::default());
        assert_eq!(world.get::<ChildOf>(captions).unwrap().parent(), header);
        assert_eq!(world.get::<Children>(captions).unwrap().iter().collect::<Vec<_>>(), buttons);
        assert_eq!(world.entities().len(), count, "预览不能创建替代顶栏、按钮或相机");
    }
}

#[test]
fn hidden_original_header_retracts_hit_geometry_even_during_native_preview() {
    let (mut world, header, _, _) = fixture();
    world.resource_mut::<crate::embedded_viewport::EmbeddedStatus>().playing = true;
    measure(&mut world);
    assert!(world.resource::<ChromeGeometry>().visible);
    world.entity_mut(header).insert(InheritedVisibility::HIDDEN);
    measure(&mut world);
    assert_eq!(*world.resource::<ChromeGeometry>(), ChromeGeometry::default());
}

#[cfg(feature = "validation")]
#[test]
fn native_preview_probe_keeps_original_header_and_three_button_identities() {
    let (mut world, header, _, buttons) = fixture();
    let before = probe(&world).unwrap();
    world.resource_mut::<crate::embedded_viewport::EmbeddedStatus>().playing = true;
    measure(&mut world);
    let after = probe(&world).unwrap();
    assert_eq!(before, after);
    assert_eq!(after["active_header"], header.to_bits());
    assert!(after["preview_header"].is_null() && after["camera"].is_null());
    let ids: Vec<_> = after["captions"].as_array().unwrap().iter()
        .map(|value| value["entity"].as_u64().unwrap()).collect();
    assert_eq!(ids, buttons.map(Entity::to_bits));
}
