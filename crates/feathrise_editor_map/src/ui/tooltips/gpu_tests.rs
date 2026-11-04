//! 提示浮层的真实中文字体、正式鼠标命中与离屏像素回归，不修改正式配置。

use super::components::*;
use super::types::TooltipAction;
use super::*;
use crate::authoring::EditorAuthoringSession;
use crate::components::MapEditorRoot;
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorSettingsState, MapEditorState};
use crate::types::EditorAction;
use crate::ui::theme as colors;
use bevy::ecs::system::RunSystemOnce;
use bevy::input::ButtonState;
use bevy::input::mouse::MouseButtonInput;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::text::TextLayoutInfo;
use bevy::time::TimeUpdateStrategy;
use bevy::window::{ExitCondition, PrimaryWindow};
use bevy::winit::WinitPlugin;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorColorTheme;
use feathrise_editor_core::EditorKeyBinding;
use feathrise_editor_core::EditorShortcutAction;
use std::time::Duration;

#[derive(Resource)]
struct TestSurface {
    window: Entity,
    camera: Entity,
    target: Handle<Image>,
    dpi: f32,
}

#[derive(Component, Clone, Default)]
struct TestViewport;

fn test_scene() -> impl Scene {
    bsn! {
        MapEditorRoot
        Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100),
            height: percent(100), flex_direction: FlexDirection::Column }
        BackgroundColor(colors::PANEL_BG)
        Children [
            (crate::ui::scene::header()),
            (crate::ui::viewport_toolbar::viewport_toolbar()),
            (TestViewport Node { width: percent(100), min_height: px(0), flex_grow: 1.0 }
                BackgroundColor(colors::MODEL_PREVIEW_BG)),
            (tooltip_popup()),
        ]
    }
}

/// 离屏图不创建系统窗口；仅在官方命中调用期间临时把同一UI相机指向测试窗口。
fn official_hover(world: &mut World) {
    let camera = world.resource::<TestSurface>().camera;
    let window = world.resource::<TestSurface>().window;
    let target = world
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    *world.get_mut::<bevy::camera::RenderTarget>(camera).unwrap() =
        bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    world.run_system_once(bevy::ui::ui_focus_system).unwrap();
    *world.get_mut::<bevy::camera::RenderTarget>(camera).unwrap() = target;
}

fn set_test_dpi(surface: Res<TestSurface>, mut cameras: Query<&mut Camera>) {
    if let Some(info) = cameras
        .get_mut(surface.camera)
        .unwrap()
        .computed
        .target_info
        .as_mut()
    {
        info.scale_factor = surface.dpi;
    }
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
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

fn entity<Marker: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<Marker>>()
        .single(world)
        .unwrap()
}

fn target(app: &mut App, action: TooltipAction) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &EditorTooltipTarget)>()
        .iter(world)
        .find(|(_, target)| target.0 == action)
        .unwrap()
        .0
}

fn hover(app: &mut App, target: Entity) {
    let point = bounds(app, target).center();
    let window = app.world().resource::<TestSurface>().window;
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(point.as_dvec2()));
    frames(app, 6);
    assert_eq!(
        app.world().get::<Interaction>(target),
        Some(&Interaction::Hovered)
    );
    let popup = entity::<TooltipPopup>(app);
    assert_eq!(
        app.world().get::<Node>(popup).unwrap().display,
        Display::Flex
    );
}

fn fixed_bounds(app: &mut App) -> Vec<(Entity, Rect)> {
    let world = app.world_mut();
    let mut rects: Vec<_> = world
        .query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform), Or<(
            With<EditorTooltipTarget>,
            With<TestViewport>,
            With<TooltipHeader>,
        )>>()
        .iter(world)
        .map(|(entity, node, transform)| {
            (
                entity,
                Rect::from_center_size(transform.translation, node.size()),
            )
        })
        .collect();
    rects.sort_by_key(|(entity, _)| entity.to_bits()); // 禁用标记迁移archetype可改变查询顺序，布局必须按同一实体严格对照。
    rects
}

fn snapshot(app: &mut App, name: &str) {
    let surface = app.world().resource::<TestSurface>();
    let target = surface.target.clone();
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .get(&target)
        .unwrap();
    let (width, height) = (image.width(), image.height());
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let started = std::time::Instant::now();
    let mut bytes = loop {
        app.update();
        if let Ok(bytes) = receiver.try_recv() {
            break bytes;
        }
        assert!(started.elapsed().as_secs() < 10, "提示GPU回读超时");
        std::thread::sleep(Duration::from_millis(16));
    };
    assert_eq!(bytes.len(), width as usize * height as usize * 4);
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let popup = entity::<TooltipPopup>(app);
    let rect = bounds(app, popup);
    assert!(
        rect.min.min_element() >= 0.0 && rect.max.x <= width as f32 && rect.max.y <= height as f32
    );
    let sample = rect.min + Vec2::splat(4.0);
    let offset = (sample.y as usize * width as usize + sample.x as usize) * 4;
    let expected = app
        .world()
        .resource::<feathrise_editor_ui::EditorPalette>()
        .resolve(colors::PANEL_BG_STRONG)
        .to_srgba();
    for (actual, expected) in
        bytes[offset..offset + 3]
            .iter()
            .zip([expected.red, expected.green, expected.blue])
    {
        assert!(
            (*actual as f32 - expected * 255.0).abs() <= 3.0,
            "提示必须采用当前主题：{name}"
        );
    }
    let mut bright = 0;
    for vertical in rect.min.y as usize..rect.max.y as usize {
        for horizontal in rect.min.x as usize..rect.max.x as usize {
            let offset = (vertical * width as usize + horizontal) * 4;
            if bytes[offset] > 100 && bytes[offset + 1] > 100 && bytes[offset + 2] > 100 {
                bright += 1;
            }
        }
    }
    assert!(bright > 35, "提示真实字形未绘制：{name}");
    if [
        "editor-tooltip-NeutralGray.png",
        "editor-tooltip-GraphiteBlue.png",
        "editor-tooltip-WarmCopper.png",
    ]
    .contains(&name)
    {
        let undo = self::target(app, TooltipAction::Editor(EditorAction::Undo));
        let caption = app.world().get::<Children>(undo).unwrap()[0];
        let rect = bounds(app, caption);
        let expected = app
            .world()
            .resource::<feathrise_editor_ui::EditorPalette>()
            .resolve(colors::TEXT_MUTED)
            .to_srgba();
        let mut matching = 0;
        for vertical in rect.min.y as usize..rect.max.y as usize {
            for horizontal in rect.min.x as usize..rect.max.x as usize {
                let offset = (vertical * width as usize + horizontal) * 4;
                if bytes[offset..offset + 3]
                    .iter()
                    .zip([expected.red, expected.green, expected.blue])
                    .all(|(actual, expected)| (*actual as f32 - expected * 255.0).abs() <= 5.0)
                {
                    matching += 1;
                }
            }
        }
        assert!(matching >= 2, "禁用文字必须由渲染提取采用当前主题：{name}");
    }
    let output =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&output).unwrap();
    image::RgbaImage::from_raw(width, height, bytes)
        .unwrap()
        .save(output.join(name))
        .unwrap();
}

fn resize(app: &mut App, width: u32, height: u32, scale: f32, dpi: f32) {
    let surface = app.world().resource::<TestSurface>();
    let target = surface.target.clone();
    let window = surface.window;
    let mut image = Image::new_target_texture(width, height, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .insert(target.id(), image)
        .unwrap();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(width, height);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_scale_factor_override(Some(dpi));
    app.world_mut().resource_mut::<UiScale>().0 = scale;
    app.world_mut().resource_mut::<TestSurface>().dpi = dpi;
    frames(app, 4);
}

#[test]
#[ignore = "显式执行离屏GPU提示回归；不启动原生窗口"]
fn tooltip_gpu_actual_hover_clipping_three_themes_and_scaled_roots() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                watch_for_changes_override: Some(false),
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins(feathrise_editor_ui::EditorUiPlugin)
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )))
    .init_resource::<EditorSettings>()
    .init_resource::<MapEditorSettingsState>()
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let map = draft.map_id.clone();
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
        .insert_resource(feathrise_map_content::ActiveDynamicMap { map_id: Some(map.clone()) })
        .insert_resource(MapEditorState {
            open: true,
            selected_map_id: Some(map.clone()),
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        });
    install(&mut app);
    app.add_systems(
        Update,
        crate::ui::actions::sync_editor_action_button_styles
            .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput),
    ); // 与正式F12一致：通用按钮三态先更新，作者禁用与选中态最后回显。
    crate::ui::theme::install(&mut app);
    app.add_systems(PreUpdate, official_hover.after(bevy::ui::UiSystems::Focus));
    app.add_systems(
        PostUpdate,
        set_test_dpi.before(bevy::ui::UiSystems::Prepare),
    );
    app.finish();
    app.cleanup();
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    let mut image = Image::new_target_texture(896, 720, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target_image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            bevy::camera::RenderTarget::from(target_image.clone()),
        ))
        .id();
    app.insert_resource(TestSurface {
        window,
        camera,
        target: target_image,
        dpi: 1.0,
    });
    app.world_mut()
        .run_system_once::<_, (), _>(test_scene.spawn())
        .unwrap();
    let root = entity::<MapEditorRoot>(&mut app);
    app.world_mut()
        .entity_mut(root)
        .insert(bevy::ui::UiTargetCamera(camera));
    app.insert_resource(crate::resources::MapEditorViewportState {
        _image: Handle::default(),
        camera: Entity::PLACEHOLDER,
        center: Vec3::ZERO,
        scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::ONE,
        extent: 50.0,
        yaw: 0.0,
        pitch: 0.5,
        framed_map_id: Some(map),
        render_size: UVec2::new(800, 600),
    });
    resize(&mut app, 896, 720, 1.0, 1.0);
    frames(&mut app, 12);
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    let world = app.world_mut();
    assert_eq!(
        world.query::<&EditorTooltipTarget>().iter(world).count(),
        15 // 新建地图及其提示已迁入地图浏览器；本夹具只构造顶栏与视口工具条。
    );
    assert_eq!(world.query::<&TooltipElement>().iter(world).count(), 4);
    let undo = target(&mut app, TooltipAction::Editor(EditorAction::Undo));
    // 真实空历史由生产样式系统禁用；禁用不妨碍官方悬停和穿透说明。
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(undo)
            .is_some()
    );
    for theme in [
        EditorColorTheme::NeutralGray,
        EditorColorTheme::GraphiteBlue,
        EditorColorTheme::WarmCopper,
    ] {
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .editor_color_theme = theme;
        let before = fixed_bounds(&mut app);
        hover(&mut app, undo);
        assert_eq!(before, fixed_bounds(&mut app));
        let caption = app.world().get::<Children>(undo).unwrap()[0];
        // 主世界保留规范逻辑色，实际主题只在RenderApp提取时采用；像素回读另验最终弱色。
        assert_eq!(
            app.world().get::<TextColor>(caption).unwrap().0,
            colors::TEXT_MUTED
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(undo).unwrap().0,
            colors::SLOT_BG_DISABLED
        );
        assert!(!app.world().resource::<MapEditorState>().undo_requested);
        snapshot(&mut app, &format!("editor-tooltip-{}.png", theme.as_str()));
    }
    // 用户指出启动悬停框的亮边；保存同一真实入口的中性灰画面用于直接对照。
    app.world_mut().resource_mut::<EditorSettings>().editor_color_theme = EditorColorTheme::NeutralGray;
    let launch = target(&mut app, TooltipAction::Editor(EditorAction::Close));
    hover(&mut app, launch);
    let launch_popup = entity::<TooltipPopup>(&mut app);
    assert_eq!(app.world().get::<BorderColor>(launch_popup), Some(&BorderColor::all(colors::COPPER_BORDER_WEAK)));
    snapshot(&mut app, "editor-tooltip-launch.png");
    let before = fixed_bounds(&mut app);
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .push(crate::authoring::EditorCommand::TerrainChanged(vec![
            crate::authoring::HeightChange {
                index: 0,
                before: 0.0,
                after: 1.0,
            },
        ]));
    frames(&mut app, 2);
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(undo)
            .is_none()
    );
    let caption = app.world().get::<Children>(undo).unwrap()[0];
    assert_eq!(
        app.world().get::<TextColor>(caption).unwrap().0,
        colors::TEXT_PRIMARY
    );
    assert_eq!(before, fixed_bounds(&mut app));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .clear();
    let save = target(&mut app, TooltipAction::Editor(EditorAction::Save));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    hover(&mut app, save);
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(save)
            .is_some()
    );
    snapshot(&mut app, "editor-availability-saving.png");
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = false;
    hover(&mut app, save);
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(save)
            .is_none()
    );
    snapshot(&mut app, "editor-availability-restored.png");
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Idle));
    hover(&mut app, save);
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(save)
            .is_some()
    );
    let settings = target(&mut app, TooltipAction::Settings);
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(settings)
            .is_none()
    );
    snapshot(&mut app, "editor-availability-empty.png");
    assert_eq!(before, fixed_bounds(&mut app));
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active));
    hover(&mut app, undo);
    let focus = target(&mut app, TooltipAction::Editor(EditorAction::FocusSelected));
    let popup = entity::<TooltipPopup>(&mut app);
    assert!(
        bounds(&app, popup).contains(bounds(&app, focus).center()),
        "先确认浮层确实覆盖了另一个按钮"
    );
    let focus_center = bounds(&app, focus).center().as_dvec2();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(focus_center));
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    frames(&mut app, 1);
    assert_eq!(
        app.world().get::<Interaction>(focus),
        Some(&Interaction::Pressed),
        "提示不能拦截后方按钮点击"
    );
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    frames(&mut app, 2);
    let mut input = ButtonInput::default();
    for key in [KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::AltLeft] {
        input.press(key);
    }
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_keybindings
        .assign(
            EditorShortcutAction::FocusSelected,
            Some(EditorKeyBinding::from_input(KeyCode::KeyG, &input)),
        )
        .unwrap();
    resize(&mut app, 384, 360, 1.5, 1.0);
    let scroll = entity::<crate::ui::viewport_toolbar::ViewportToolbarScroll>(&mut app);
    let overflow = bounds(&app, focus).max.x - bounds(&app, scroll).max.x + 8.0;
    let inverse = app
        .world()
        .get::<ComputedNode>(scroll)
        .unwrap()
        .inverse_scale_factor();
    app.world_mut().get_mut::<ScrollPosition>(scroll).unwrap().x = overflow.max(0.0) * inverse;
    frames(&mut app, 2);
    hover(&mut app, focus);
    snapshot(&mut app, "editor-tooltip-narrow-150.png");
    // 已裁出工具条的按钮不能靠上一帧Hovered留下提示。
    app.world_mut().get_mut::<ScrollPosition>(scroll).unwrap().x = 0.0;
    frames(&mut app, 2);
    let first = target(
        &mut app,
        TooltipAction::Editor(EditorAction::ViewPerspective),
    );
    hover(&mut app, first);
    app.world_mut().get_mut::<ScrollPosition>(scroll).unwrap().x = 100_000.0;
    frames(&mut app, 1);
    assert_eq!(
        app.world().get::<Node>(popup).unwrap().display,
        Display::None
    );
    app.world_mut().get_mut::<ScrollPosition>(scroll).unwrap().x = 0.0;
    // 辅助锚点只属于测试，覆盖真实字体在右下角、缩放、DPI及带偏移根中的避让。
    let anchor = app
        .world_mut()
        .spawn((
            Button,
            EditorTooltipTarget(TooltipAction::Editor(EditorAction::FocusSelected)),
            Node {
                position_type: PositionType::Absolute,
                right: px(8),
                bottom: px(8),
                width: px(50),
                height: px(26),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    for width in [320, 384, 640, 1280] {
        for scale in [0.6, 1.0, 1.5] {
            for dpi in [1.0, 2.0] {
                resize(
                    &mut app,
                    (width as f32 * dpi) as u32,
                    (360.0 * dpi) as u32,
                    scale,
                    dpi,
                );
                let before = fixed_bounds(&mut app);
                hover(&mut app, anchor);
                assert_eq!(before, fixed_bounds(&mut app));
                let rect = bounds(&app, popup);
                let root_rect = bounds(&app, root);
                assert!(root_rect.contains(rect.min) && root_rect.contains(rect.max));
                assert!(rect.max.y < bounds(&app, anchor).min.y);
                let world = app.world_mut();
                for (text, node, label, style) in world
                    .query_filtered::<(&TextLayoutInfo, &ComputedNode, &Text, &Node), With<TooltipText>>()
                    .iter(world)
                {
                    // Bevy把TextLayoutInfo.size回写为逻辑单位；与物理节点比较时只乘一次实际倍率。
                    let physical_size = text.size * text.scale_factor;
                    assert!(
                        physical_size.x <= node.size().x + 1.1 && physical_size.y <= node.size().y + 1.1,
                        "宽度={width} 缩放={scale} DPI={dpi} 字形={:?} 节点={:?} 字形倍率={} 显示={:?} 内容={}",
                        physical_size, node.size(), text.scale_factor, style.display, label.as_str()
                    );
                }
            }
        }
    }
    resize(&mut app, 1280, 720, 1.5, 2.0);
    {
        let mut root_node = app.world_mut().get_mut::<Node>(root).unwrap();
        root_node.left = px(12);
        root_node.top = px(8);
        root_node.width = percent(90);
        root_node.height = percent(90);
    }
    frames(&mut app, 2);
    hover(&mut app, anchor);
    let rect = bounds(&app, popup);
    let root_rect = bounds(&app, root);
    assert!(root_rect.contains(rect.min) && root_rect.contains(rect.max));
    app.world_mut()
        .get_mut::<Visibility>(anchor)
        .unwrap()
        .set_if_neq(Visibility::Hidden);
    frames(&mut app, 1);
    assert_eq!(
        app.world().get::<Node>(popup).unwrap().display,
        Display::None
    );
}
