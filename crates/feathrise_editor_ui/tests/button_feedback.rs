//! 真实GPU背景像素回归：每帧业务覆写、透明按钮、语义底色、禁用和游戏UI隔离。

#[path = "support/render.rs"]
#[allow(dead_code)]
mod render;

use feathrise_editor_ui::InteractionButton;
use bevy::{
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    },
    ui::{InteractionDisabled, RelativeCursorPosition},
};
use feathrise_editor_ui::{EditorPalette, colors as c};
use feathrise_ui_widgets::{NumericInputDisabled, NumericInputFieldButton};
use std::time::{Duration, Instant};

#[derive(Resource)]
struct FinalInteraction(Interaction, bool);

#[derive(Component)]
struct BusinessBackground(Color);

/// 模拟LOD等业务持续覆写背景，确保最终反馈不依赖业务系统是否读取Interaction。
fn business_rewrites(
    state: Res<FinalInteraction>,
    mut buttons: Query<(
        &BusinessBackground,
        &mut Interaction,
        &mut BackgroundColor,
        Option<&mut RelativeCursorPosition>,
    )>,
) {
    for (base, mut interaction, mut background, cursor) in &mut buttons {
        *interaction = state.0;
        background.0 = base.0;
        if let Some(mut cursor) = cursor {
            cursor.cursor_over = state.1;
            cursor.normalized = Some(if state.1 {
                Vec2::splat(0.5)
            } else {
                Vec2::splat(1.5)
            });
        }
    }
}

fn pixels(app: &mut App, name: &str) -> (UVec2, Vec<u8>) {
    for _ in 0..20 {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
    let (target, size) = {
        let world = app.world_mut();
        let target = world
            .query::<&bevy::camera::RenderTarget>()
            .single(world)
            .unwrap();
        let bevy::camera::RenderTarget::Image(target) = target else {
            panic!("应使用离屏目标")
        };
        (
            target.handle.clone(),
            world
                .resource::<Assets<Image>>()
                .get(&target.handle)
                .unwrap()
                .size(),
        )
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let started = Instant::now();
    let bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(started.elapsed().as_secs() < 10, "按钮GPU回归读回超时");
        std::thread::sleep(Duration::from_millis(16));
    };
    let stride = size.x as usize * 4;
    let pixels: Vec<u8> = bytes
        .chunks_exact(stride.div_ceil(256) * 256)
        .take(size.y as usize)
        .flat_map(|row| row[..stride].iter().copied())
        .collect();
    let image = Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.clone(),
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    let output =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&output).unwrap();
    image
        .try_into_dynamic()
        .unwrap()
        .save(output.join(format!("button-feedback-{name}.png")))
        .unwrap();
    (size, pixels)
}

fn sample(app: &App, frame: &(UVec2, Vec<u8>), entity: Entity, border: bool) -> [u8; 4] {
    let center = app
        .world()
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .translation;
    let position = if border {
        center
            - Vec2::new(
                app.world().get::<ComputedNode>(entity).unwrap().size().x / 2.0 - 1.0,
                0.0,
            )
    } else {
        center
    };
    let index = (position.y as usize * frame.0.x as usize + position.x as usize) * 4;
    frame.1[index..index + 4].try_into().unwrap()
}

#[test]
#[ignore = "需要真实GPU，根任务单独运行并检查离屏读回像素"]
fn editor_button_feedback_is_visible_after_business_rewrites_and_preserves_semantics() {
    let (mut app, root, _) = render::app(512, 120);
    {
        let mut layout = app.world_mut().get_mut::<Node>(root).unwrap();
        layout.flex_direction = FlexDirection::Row;
        layout.column_gap = px(8);
    }
    app.insert_resource(FinalInteraction(Interaction::None, true));
    app.add_systems(Last, business_rewrites);
    let mut buttons = vec![];
    for base in [
        c::BUTTON_DEV_SMALL_NORMAL,
        c::SELECT_BG,
        c::DANGER_BG,
        c::SLOT_BG_DISABLED,
        Color::NONE,
        c::SLOT_BG_DISABLED,
    ] {
        buttons.push(
            app.world_mut()
                .spawn((
                    InteractionButton,
                    BusinessBackground(base),
                    BackgroundColor(base),
                    ChildOf(root),
                    Node {
                        width: px(64),
                        height: px(48),
                        flex_shrink: 0.0,
                        border: UiRect::all(px(2)),
                        ..default()
                    },
                    BorderColor::all(c::COPPER_BORDER_WEAK.with_alpha(1.0)),
                    RelativeCursorPosition::default(),
                ))
                .id(),
        );
    }
    app.world_mut()
        .entity_mut(buttons[3])
        .insert(InteractionDisabled);
    let disabled_root = app.world_mut().spawn(NumericInputDisabled).id();
    app.world_mut()
        .entity_mut(buttons[5])
        .insert(NumericInputFieldButton {
            root: disabled_root,
        });
    let camera = app.world().get::<bevy::ui::UiTargetCamera>(root).unwrap().0;
    let game = app
        .world_mut()
        .spawn((
            InteractionButton,
            BusinessBackground(c::BUTTON_DEV_SMALL_NORMAL),
            BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL),
            bevy::ui::UiTargetCamera(camera),
            Node {
                position_type: PositionType::Absolute,
                left: px(8),
                top: px(76),
                width: px(64),
                height: px(32),
                ..default()
            },
        ))
        .id();
    for palette in [
        EditorPalette::neutral_gray(),
        EditorPalette::graphite_blue(),
        EditorPalette::warm_copper(),
    ] {
        let name = palette.name();
        app.insert_resource(palette);
        let mut frames = vec![];
        for (state, label) in [
            (Interaction::None, "normal"),
            (Interaction::Hovered, "hovered"),
            (Interaction::Pressed, "pressed"),
        ] {
            app.world_mut().resource_mut::<FinalInteraction>().0 = state;
            frames.push(pixels(&mut app, &format!("{name}-{label}")));
        }
        for &button in &buttons[..3] {
            let samples: Vec<_> = frames
                .iter()
                .map(|frame| sample(&app, frame, button, false))
                .collect();
            assert!(
                samples[0][0] + 4 < samples[1][0] && samples[1][0] + 4 < samples[2][0],
                "三态必须有可辨别亮度：{name} {samples:?}"
            );
        }
        let selected = sample(&app, &frames[2], buttons[1], false);
        if name != "WarmCopper" {
            assert!(selected[2] > selected[0], "选中项应保留蓝灰底色");
        }
        let danger = sample(&app, &frames[2], buttons[2], false);
        assert!(danger[0] > danger[1] + 10, "危险按钮必须保留红色");
        for button in [buttons[3], buttons[5], game] {
            let first = sample(&app, &frames[0], button, false);
            assert!(
                frames
                    .iter()
                    .all(|frame| sample(&app, frame, button, false) == first),
                "禁用字段和游戏UI不应附加反馈"
            );
        }
        assert_eq!(
            sample(&app, &frames[0], buttons[4], false),
            sample(&app, &frames[0], root, false),
            "透明按钮普通态保持透明"
        );
        for frame in &frames[1..] {
            assert_ne!(
                sample(&app, frame, buttons[4], false),
                sample(&app, &frames[0], buttons[4], false),
                "透明按钮交互时应补背景"
            );
        }
        let border = sample(&app, &frames[0], buttons[0], true);
        assert!(
            frames
                .iter()
                .all(|frame| sample(&app, frame, buttons[0], true) == border),
            "背景反馈不能增亮边框"
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(buttons[4]).unwrap().0,
            Color::NONE
        );
        app.world_mut().resource_mut::<FinalInteraction>().1 = false;
        let outside = pixels(&mut app, &format!("{name}-pressed-outside"));
        for &button in &buttons {
            assert_eq!(
                sample(&app, &outside, button, false),
                sample(&app, &frames[0], button, false),
                "移出按钮应取消按压视觉"
            );
        }
        app.world_mut().resource_mut::<FinalInteraction>().1 = true;
    }
}

/// 使用共享构造函数画出地图、文件与关闭确认标签，防止只检查Node而漏掉主题最终绘制的亮底线或关闭方块。
#[test]
#[ignore = "需要真实GPU，根任务单独运行并检查多文档标签离屏图"]
fn workspace_tabs_use_one_fill_without_active_underline_or_static_close_box() {
    #[derive(Component, Clone, Copy, PartialEq, Eq)]
    enum Action { Activate(usize), Close(usize) }

    let (mut app, root, font) = render::app(640, 80);
    let mut queue = bevy::ecs::world::CommandQueue::default();
    Commands::new(&mut queue, app.world()).entity(root).with_children(|parent| {
        let mut node = feathrise_editor_ui::workspace_tabs::tab_list_node();
        node.height = px(28);
        parent.spawn(node).with_children(|tabs| {
            for (index, label) in ["1", "clipmap2*", "glb地图", "地表·草地*"].iter().enumerate() {
                feathrise_editor_ui::workspace_tabs::spawn_preset_tab(
                    tabs, &font, label, Action::Activate(index), Action::Close(index),
                    index == 1, index == 3,
                );
            }
        });
    });
    queue.apply(app.world_mut());
    let actions: Vec<_> = app.world_mut().query::<(Entity, &Action)>()
        .iter(app.world()).map(|(entity, action)| (entity, *action)).collect();
    let titles: Vec<_> = (0..4).map(|index| actions.iter()
        .find(|(_, action)| *action == Action::Activate(index)).unwrap().0).collect();
    let closes: Vec<_> = (0..4).map(|index| actions.iter()
        .find(|(_, action)| *action == Action::Close(index)).unwrap().0).collect();
    let frames: Vec<_> = titles.iter().map(|title|
        app.world().get::<ChildOf>(*title).unwrap().parent()).collect();
    // 只改变活动标签的关闭键交互；其余标题和标签外框必须保留原语义填色。
    app.world_mut().entity_mut(closes[1]).insert((
        BusinessBackground(c::SELECT_BG), RelativeCursorPosition::default(),
    ));
    app.insert_resource(FinalInteraction(Interaction::None, true));
    app.add_systems(Last, business_rewrites);
    let pixel = |app: &App, image: &(UVec2, Vec<u8>), entity: Entity, inset: Vec2| {
        let size = app.world().get::<ComputedNode>(entity).unwrap().size();
        let center = app.world().get::<UiGlobalTransform>(entity).unwrap().translation;
        let point = center - size / 2.0 + inset;
        let offset = (point.y as usize * image.0.x as usize + point.x as usize) * 4;
        <[u8; 4]>::try_from(&image.1[offset..offset + 4]).unwrap()
    };
    for palette in [EditorPalette::neutral_gray(), EditorPalette::graphite_blue(), EditorPalette::warm_copper()] {
        let name = palette.name();
        app.insert_resource(palette);
        app.world_mut().resource_mut::<FinalInteraction>().0 = Interaction::None;
        let normal = pixels(&mut app, &format!("tabs-{name}-normal"));
        for index in 0..4 {
            let frame = frames[index];
            let size = app.world().get::<ComputedNode>(frame).unwrap().size();
            let fill = pixel(&app, &normal, frame, Vec2::new(3.0, 2.0));
            assert_eq!(pixel(&app, &normal, titles[index], Vec2::new(3.0, 2.0)), fill,
                "{name}：标题应融入整标签背景");
            for x in [3.0, size.x / 2.0, size.x - 12.0] {
                assert_eq!(pixel(&app, &normal, frame, Vec2::new(x, size.y - 1.0)), fill,
                    "{name}：整个标签底边不得恢复白色或亮色活动横线");
            }
            if index < 3 {
                assert_eq!(pixel(&app, &normal, closes[index], Vec2::splat(3.0)), fill,
                    "{name}：普通关闭键不得有常驻独立底块");
            }
        }
        assert_ne!(pixel(&app, &normal, frames[0], Vec2::new(3.0, 2.0)),
            pixel(&app, &normal, frames[1], Vec2::new(3.0, 2.0)),
            "{name}：活动标签仍须有独立填色");
        let danger = pixel(&app, &normal, closes[3], Vec2::splat(3.0));
        assert!(danger[0] > danger[1] + 10, "{name}：未保存关闭确认保持危险语义");
        let mut samples = vec![pixel(&app, &normal, closes[1], Vec2::splat(3.0))];
        for (interaction, label) in [(Interaction::Hovered, "hover"), (Interaction::Pressed, "press")] {
            app.world_mut().resource_mut::<FinalInteraction>().0 = interaction;
            let image = pixels(&mut app, &format!("tabs-{name}-{label}"));
            samples.push(pixel(&app, &image, closes[1], Vec2::splat(3.0)));
        }
        assert!(samples[0][0] + 4 < samples[1][0] && samples[1][0] + 4 < samples[2][0],
            "{name}：融合普通态后仍需保留可辨别的关闭悬停及按压反馈 {samples:?}");
    }
}
