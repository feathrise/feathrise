//! 公共专业数值框的真实鼠标命中、中文字体和离屏反馈检查，不创建系统窗口或写设置。

#[path = "support/render.rs"]
#[allow(dead_code)]
mod render;

use bevy::ecs::{system::RunSystemOnce, world::CommandQueue};
use bevy::input::{ButtonState, mouse::MouseButtonInput};
use bevy::prelude::*;
use bevy::text::TextLayoutInfo;
use bevy::window::PrimaryWindow;
use feathrise_editor_ui::*;

#[derive(Resource)]
struct TestPointer {
    window: Entity,
    camera: Entity,
}

/// 正式UI命中暂时读取ECS窗口光标，渲染仍使用原独立图像目标。
fn official_pointer(world: &mut World) {
    let pointer = world.resource::<TestPointer>();
    let (window, camera) = (pointer.window, pointer.camera);
    let target = world
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    *world.get_mut::<bevy::camera::RenderTarget>(camera).unwrap() =
        bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    world.run_system_once(bevy::ui::ui_focus_system).unwrap();
    *world.get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = target;
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
    }
}

fn bounds(app: &App, entity: Entity) -> Rect {
    Rect::from_center_size(
        app.world()
            .get::<UiGlobalTransform>(entity)
            .unwrap()
            .translation,
        app.world().get::<ComputedNode>(entity).unwrap().size(),
    )
}

fn pointer(app: &mut App, position: Vec2, state: Option<ButtonState>) {
    let window = app.world().resource::<TestPointer>().window;
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.as_dvec2()));
    if let Some(state) = state {
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Right,
            state,
            window,
        });
    }
    app.update();
}

fn assert_feedback_layout(app: &mut App, field: Entity, expected: &str) {
    let world = app.world_mut();
    let feedback = world
        .query::<(Entity, &NumericInputScrubSensitivityText)>()
        .iter(world)
        .find_map(|(entity, part)| (part.root == field).then_some(entity))
        .unwrap();
    assert_eq!(world.get::<Node>(feedback).unwrap().display, Display::Flex);
    assert_eq!(world.get::<Text>(feedback).unwrap().0, expected);
    let layout_size = world.get::<TextLayoutInfo>(feedback).unwrap().size;
    let feedback_bounds = bounds(app, feedback);
    let field_bounds = bounds(app, field);
    let pointer = app.world().resource::<TestPointer>();
    let window = app.world().get::<Window>(pointer.window).unwrap();
    let cursor = window.physical_cursor_position().unwrap();
    let viewport = Vec2::new(window.physical_width() as f32, window.physical_height() as f32);
    assert!(feedback_bounds.min.cmpge(Vec2::ZERO).all() && feedback_bounds.max.cmple(viewport).all());
    let scale = app.world().resource::<UiScale>().0;
    assert!(cursor.distance(cursor.clamp(feedback_bounds.min, feedback_bounds.max)) <= 24.0 * scale + 1.0,
        "拖出原字段后倍率仍应贴近当前鼠标");
    assert!(app.world().get::<bevy::ui::CalculatedClip>(feedback).is_none(), "倍率不得继承字段或滚动祖先的裁切");
    assert!(
        layout_size.x <= feedback_bounds.width() + 1.0,
        "倍率文字不应被原字段裁切"
    );
    assert!(
        layout_size.y <= feedback_bounds.height() + 1.0,
        "倍率文字不应换行遮住相邻字段"
    );
    let label = app
        .world()
        .get::<Children>(field)
        .unwrap()
        .iter()
        .find(|child| {
            app.world()
                .get::<Text>(*child)
                .is_some_and(|text| text.0 == "X")
        })
        .unwrap();
    assert_eq!(
        app.world().get::<Node>(label).unwrap().display,
        Display::Flex
    );
    let label_bounds = bounds(app, label);
    assert!(
        label_bounds.min.x >= field_bounds.min.x && label_bounds.max.x <= field_bounds.max.x
    );
    assert!(
        app.world().get::<TextLayoutInfo>(label).unwrap().size.x <= label_bounds.width() + 1.0,
        "原轴标签不能被倍率文字挤掉"
    );
    let world = app.world_mut();
    assert!(world.query::<(&NumericInputScrubValueText, &Node)>().iter(world)
        .filter(|(part, _)| part.root == field).all(|(_, node)| node.display == Display::Flex));
}

#[test]
#[ignore = "需要GPU；通过正式UI命中和真实中文字体检查专业数字右拖反馈"]
fn numeric_scrub_gpu_right_drag_feedback_keeps_xyz_readable() {
    // 48px模拟LOD等窄输入；同一浮层也必须在高缩放下保持完整并越过滚动裁切。
    for (scale, width, precision) in [(1.0, 104.0, 6), (1.5, 104.0, 6), (1.5, 48.0, 1)] {
        let (mut app, root, font) = render::app(900, 300);
        app.insert_resource(UiScale(scale));
        let window = app
            .world_mut()
            .spawn((
                PrimaryWindow,
                Window {
                    resolution: (900, 300).into(),
                    focused: true,
                    ..default()
                },
            ))
            .id();
        let camera = app
            .world_mut()
            .query_filtered::<Entity, With<Camera2d>>()
            .single(app.world())
            .unwrap();
        app.insert_resource(TestPointer { window, camera });
        app.add_systems(
            PreUpdate,
            official_pointer.after(bevy::ui::UiSystems::Focus),
        );
        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, app.world());
        let mut x_field = Entity::PLACEHOLDER;
        commands.entity(root).with_children(|parent| {
            parent.spawn((
                Text::new("变换"),
                TextFont {
                    font: font.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
            ));
            parent
                .spawn(Node {
                    column_gap: px(4),
                    width: px(width * 3.0 + 8.0),
                    height: px(26),
                    ..default()
                })
                .with_children(|row| {
                    for (axis, value) in [("X", 1.234567), ("Y", 1.0), ("Z", 1.0)] {
                        let field = spawn_numeric_scrub_field(
                            row,
                            Name::new(axis),
                            font.clone(),
                            NumericScrubFieldOptions::new(
                                axis,
                                "×",
                                NumericValue::F32(value),
                                NumericInputSpec::f32(precision),
                                NumericInputScrubConfig::linear(0.01, 0.000001, 0.001, 1000.0, 1.0),
                            )
                            .with_style(NumericScrubFieldStyle {
                                width,
                                height: 26.0,
                                ..default()
                            }),
                        );
                        if axis == "X" {
                            x_field = field;
                        }
                    }
                });
        });
        queue.apply(app.world_mut());
        frames(&mut app, 8);
        let original_bounds = bounds(&app, x_field);
        let start = original_bounds.center();
        pointer(&mut app, start, Some(ButtonState::Pressed));
        assert_eq!(
            app.world().resource::<ActiveNumericInputScrub>().entity,
            Some(x_field)
        );
        pointer(&mut app, start + Vec2::X * 48.0, None);
        assert_feedback_layout(&mut app, x_field, "灵敏度 2.00×");
        assert_eq!(bounds(&app, x_field), original_bounds);
        render::snapshot(&mut app, &format!("numeric-right-feedback-{scale}-{width}"));
        pointer(&mut app, start + Vec2::X * 480.0, None);
        assert_feedback_layout(&mut app, x_field, "灵敏度 100.00×");
        render::snapshot(&mut app, &format!("numeric-right-feedback-maximum-{scale}-{width}"));
        let edge = Vec2::new(894.0, 294.0);
        pointer(&mut app, edge, None);
        assert_feedback_layout(&mut app, x_field, "灵敏度 100.00×"); // 倍率已钳制不再变化时，提示仍须追随鼠标并换侧。
        render::snapshot(&mut app, &format!("numeric-right-feedback-edge-{scale}-{width}"));
        pointer(
            &mut app,
            edge,
            Some(ButtonState::Released),
        );
        assert_eq!(bounds(&app, x_field), original_bounds);
        let world = app.world_mut();
        let visible_feedback = world
            .query::<(&NumericInputScrubSensitivityText, &Node)>()
            .iter(world)
            .filter(|(_, node)| node.display != Display::None)
            .count();
        assert_eq!(visible_feedback, 0);
        let value_text = world
            .query::<(&NumericInputDisplayText, &Text)>()
            .iter(world)
            .find_map(|(part, text)| (part.root == x_field).then_some(text.0.clone()))
            .unwrap();
        let expected = if precision == 6 { ("1.234567", 1.234567) } else { ("1.2", 1.2) };
        assert_eq!(value_text, expected.0);
        assert_eq!(
            world.get::<NumericInputValue>(x_field).unwrap().0,
            NumericValue::F32(expected.1)
        );
        render::snapshot(
            &mut app,
            &format!("numeric-right-feedback-restored-{scale}-{width}"),
        );
    }
}
