//! XYZ 几何回归：物理缩放、斜线变换、空心圆及固定预算都通过正式轮廓提取实现。

use super::*;
use bevy::math::Affine2;

#[derive(Resource, Default)]
struct Captured(Option<Result<Vec<ViewportControlShape>, String>>);

fn capture(axes: AxisOcclusion, mut result: ResMut<Captured>) {
    result.0 = Some(axes.capture(Rect::new(0., 0., 500., 500.)));
}

fn fixture() -> (App, Entity) {
    let mut app = App::new();
    app.init_resource::<Captured>().add_systems(Update, capture);
    let root = app.world_mut().spawn((MapEditorOrientationAxisRoot, Node::default(),
        InheritedVisibility::VISIBLE)).id();
    (app, root)
}

#[test]
fn xyz_contours_transform_the_actual_line_and_keep_negative_endpoint_hollow() {
    let computed = ComputedNode { size: Vec2::new(20., 2.), ..default() };
    let transform = UiGlobalTransform::from(Affine2::from_scale_angle_translation(
        Vec2::splat(1.5), std::f32::consts::FRAC_PI_4, Vec2::new(100., 60.)));
    let ViewportControlShape::Quad { points } = node_shape(&computed, &transform, None).unwrap() else {
        panic!("斜线必须保留四个实际角，不能退回外接矩形");
    };
    assert!((points[0].distance(points[1]) - 30.).abs() < 0.001);
    assert!((points[1].distance(points[2]) - 3.).abs() < 0.001);
    assert!((points[0].y - points[1].y).abs() > 20.);

    let mut circle = ComputedNode { size: Vec2::splat(14.), ..default() };
    circle.border.min_inset = Vec2::splat(2.);
    circle.border.max_inset = Vec2::splat(2.);
    let transform = UiGlobalTransform::from(Affine2::from_scale_angle_translation(
        Vec2::splat(1.5), 0., Vec2::new(100., 60.)));
    assert_eq!(node_shape(&circle, &transform, Some(true)).unwrap(), ViewportControlShape::Ellipse {
        outer: Rect::from_center_size(Vec2::new(100., 60.), Vec2::splat(21.)),
        inner: Some(Rect::from_center_size(Vec2::new(100., 60.), Vec2::splat(15.))),
    });
    assert!(matches!(node_shape(&circle, &transform, Some(false)).unwrap(),
        ViewportControlShape::Ellipse { inner: None, .. }));
}

#[test]
fn xyz_capture_reads_only_visible_direct_children_and_is_stable_when_order_changes() {
    let (mut app, root) = fixture();
    let mut endpoints = Vec::new();
    for (axis, x) in [(Vec3::X, 110.), (Vec3::Y, 140.), (Vec3::Z, 170.)] {
        let mut circle = ComputedNode { size: Vec2::splat(14.), ..default() };
        circle.border.min_inset = Vec2::splat(2.);
        circle.border.max_inset = Vec2::splat(2.);
        let child = app.world_mut().spawn((ChildOf(root), Node::default(), circle,
            UiGlobalTransform::from_xy(x, 100.), InheritedVisibility::VISIBLE,
            MapEditorOrientationAxisEndpoint { world_axis: axis, positive: false }, Button)).id();
        endpoints.push(child);
    }
    app.update();
    let first = app.world().resource::<Captured>().0.as_ref().unwrap().as_ref().unwrap().clone();
    assert_eq!(first.len(), 3);
    assert!(first.iter().all(|shape| matches!(shape, ViewportControlShape::Ellipse { inner: Some(_), .. })));
    app.world_mut().get_mut::<Children>(root).unwrap().swap(0, 2);
    app.update();
    assert_eq!(app.world().resource::<Captured>().0.as_ref().unwrap().as_ref().unwrap(), &first);
    app.world_mut().entity_mut(endpoints[0]).insert(InheritedVisibility::HIDDEN);
    app.update();
    assert_eq!(app.world().resource::<Captured>().0.as_ref().unwrap().as_ref().unwrap().len(), 2);
}

#[test]
fn xyz_capture_rejects_duplicate_roots_or_more_than_ten_fixed_nodes_without_truncating() {
    let (mut app, root) = fixture();
    for index in 0..11 {
        app.world_mut().spawn((ChildOf(root), Node::default(),
            ComputedNode { size: Vec2::new(10., 2.), ..default() },
            UiGlobalTransform::from_xy(100., 100. + index as f32), InheritedVisibility::VISIBLE,
            MapEditorOrientationAxisLine { world_axis: Vec3::X }));
    }
    app.update();
    assert!(app.world().resource::<Captured>().0.as_ref().unwrap().is_err());

    app.world_mut().entity_mut(root).despawn();
    for _ in 0..2 {
        let root = app.world_mut().spawn((MapEditorOrientationAxisRoot, Node::default(),
            InheritedVisibility::VISIBLE)).id();
        app.world_mut().spawn((ChildOf(root), Node::default(),
            ComputedNode { size: Vec2::splat(10.), ..default() },
            UiGlobalTransform::from_xy(100., 100.), InheritedVisibility::VISIBLE, Button));
    }
    app.update();
    assert!(app.world().resource::<Captured>().0.as_ref().unwrap().is_err());
}
