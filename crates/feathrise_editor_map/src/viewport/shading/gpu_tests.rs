//! 真GPU验证四球、方向球正式鼠标松开输入，以及实体/线框只改变作者视口像素。
use super::*;
use crate::{
    components::{MapEditorCamera, MapEditorOrientationAxisEndpoint, MapEditorViewport},
    resources::{MapEditorNumericScrubMouseGuard, MapEditorState, MapEditorViewportState},
};
use bevy::{
    ecs::system::RunSystemOnce,
    input::{ButtonState, mouse::MouseButtonInput},
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
    },
    window::{ExitCondition, PrimaryWindow},
    winit::WinitPlugin,
};
use components::*;
use types::*;

#[derive(Resource)]
struct Pointer {
    window: Entity,
    camera: Entity,
}
fn official_pointer(world: &mut World) {
    let pointer = world.resource::<Pointer>();
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
fn click(app: &mut App, entity: Entity, offset: Vec2) {
    let center = app
        .world()
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .translation
        + offset;
    let window = app.world().resource::<Pointer>().window;
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(center.as_dvec2()));
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    app.update();
    assert_eq!(
        app.world().get::<Interaction>(entity),
        Some(&Interaction::Pressed)
    );
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    app.update();
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .suppress_world_pick_this_frame,
        "正式释放点击必须阻断世界拾取"
    );
}
pub(super) fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
pub(super) fn image_target(app: &mut App, width: u32, height: u32) -> Handle<Image> {
    let mut image = Image::new_target_texture(width, height, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    app.world_mut().resource_mut::<Assets<Image>>().add(image)
}
pub(super) fn read(app: &mut App, target: Handle<Image>) -> Vec<u8> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().spawn(Readback::texture(target)).observe(
        move |event: On<ReadbackComplete>, mut commands: Commands| {
            let _ = sender.send(event.data.clone());
            commands.entity(event.entity).try_despawn();
        },
    );
    let started = std::time::Instant::now();
    loop {
        app.update();
        if let Ok(mut bytes) = receiver.try_recv() {
            for p in bytes.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            return bytes;
        }
        assert!(started.elapsed().as_secs() < 10, "着色视口GPU回读超时");
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
pub(super) fn save(bytes: Vec<u8>, size: (u32, u32), name: &str) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&path).unwrap();
    image::RgbaImage::from_raw(size.0, size.1, bytes)
        .unwrap()
        .save(path.join(name))
        .unwrap();
}
fn pose(
    editor: Res<MapEditorState>,
    viewport: Res<MapEditorViewportState>,
    mut camera: Query<(&mut Projection, &mut Transform), With<MapEditorCamera>>,
) {
    let Ok((mut projection, mut transform)) = camera.single_mut() else {
        return;
    };
    let (next_projection, next_transform) =
        super::super::systems::editor_camera_pose(editor.orthographic, &viewport);
    *projection = next_projection;
    *transform = next_transform;
}
fn button(app: &mut App, mode: ShadingMode) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ShadingButton)>()
        .iter(world)
        .find(|(_, button)| matches!(button.0,ShadingAction::Mode(value)if value==mode))
        .unwrap()
        .0
}
pub(super) fn red_count(bytes: &[u8]) -> usize {
    bytes
        .chunks_exact(4)
        .filter(|p| p[0] > 100 && p[0] > p[1].saturating_mul(2) && p[0] > p[2].saturating_mul(2))
        .count()
}

pub(super) fn black(pixel: &[u8]) -> bool {
    pixel[..3].iter().all(|value| *value <= 28) && pixel[3] > 200
}

pub(super) fn black_count(bytes: &[u8]) -> usize {
    bytes.chunks_exact(4).filter(|pixel| black(pixel)).count()
}

pub(super) fn dark_gray(pixel: &[u8]) -> bool {
    let minimum = *pixel[..3].iter().min().unwrap();
    let maximum = *pixel[..3].iter().max().unwrap();
    minimum >= 30 && maximum <= 120 && maximum - minimum <= 12
}

#[test]
#[ignore = "需要真实GPU；验证着色与可点击方向球，输出离屏图像且不启动原生窗口"]
fn shading_modes_and_axis_use_real_release_clicks_and_keep_game_camera_material_pixels() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(bevy::render::RenderPlugin {
                render_creation: bevy::render::settings::WgpuSettings {
                    features: bevy::render::render_resource::WgpuFeatures::POLYGON_MODE_LINE
                        | bevy::render::render_resource::WgpuFeatures::IMMEDIATES,
                    ..default()
                }
                .into(),
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
    .add_plugins((
        feathrise_editor_ui::EditorThemePlugin,
        feathrise_editor_ui::EditorButtonInputPlugin,
        render::ShadingRenderPlugin,
    ))
    .init_resource::<resources::ShadingState>()
    .insert_resource(MapEditorState {
        open: true,
        ..default()
    })
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .insert_resource(UiScale(1.7))
    .insert_resource(bevy::render::error_handler::RenderErrorHandler(
        |error, _, _| panic!("着色视口GPU验证失败：{error:?}"),
    ))
    .add_systems(
        PreUpdate,
        official_pointer
            .after(bevy::ui::UiSystems::Focus)
            .before(feathrise_editor_ui::update_editor_button_clicks),
    )
    .add_systems(
        Update,
        (
            (|mut editor: ResMut<MapEditorState>| editor.suppress_world_pick_this_frame = false),
            ui::setup,
            super::super::orientation_axis::setup_editor_orientation_axis,
            ui::input,
            super::super::orientation_axis::orientation_axis_input,
            ui::style,
            camera::sync,
            pose,
            super::super::orientation_axis::sync_editor_orientation_axis,
            ui::place_overlays,
        )
            .chain(),
    );
    app.finish();
    app.cleanup();
    let ui_target = image_target(&mut app, 640, 360);
    let editor_target = image_target(&mut app, 256, 256);
    let game_target = image_target(&mut app, 256, 256);
    let ui_camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                order: 5,
                ..default()
            },
            bevy::camera::RenderTarget::from(ui_target.clone()),
        ))
        .id();
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: (640, 360).into(),
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    app.insert_resource(Pointer {
        window,
        camera: ui_camera,
    });
    let yaw: f32 = 0.5;
    let pitch: f32 = 0.4;
    let center = Vec3::ZERO;
    let extent = 8.0;
    let initial = super::super::camera_math::perspective_transform(center, extent, yaw, pitch);
    let editor_camera = app
        .world_mut()
        .spawn((
            MapEditorCamera,
            Camera3d::default(),
            Msaa::Off,
            Camera {
                order: 0,
                clear_color: bevy::camera::ClearColorConfig::Custom(Color::srgb(0.04, 0.05, 0.06)),
                ..default()
            },
            bevy::camera::RenderTarget::from(editor_target.clone()),
            initial,
        ))
        .id();
    app.world_mut().spawn((
        Camera3d::default(),
        Msaa::Off,
        Camera {
            order: 1,
            clear_color: bevy::camera::ClearColorConfig::Custom(Color::srgb(0.04, 0.05, 0.06)),
            ..default()
        },
        bevy::camera::RenderTarget::from(game_target.clone()),
        Projection::Perspective(PerspectiveProjection {
            fov: 50.0_f32.to_radians(),
            near: 0.2,
            far: 10000.0,
            ..default()
        }),
        initial,
    ));
    app.insert_resource(MapEditorViewportState {
        _image: editor_target.clone(),
        camera: editor_camera,
        center,
        scene_center: center,
        scene_half_extents: Vec3::splat(2.0),
        extent,
        yaw,
        pitch,
        framed_map_id: None,
        render_size: UVec2::splat(256),
    });
    app.world_mut().spawn((
        MapEditorViewport,
        Node {
            width: percent(100),
            height: percent(100),
            ..default()
        },
        UiTargetCamera(ui_camera),
    ));
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(2.0, 2.0, 2.0));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.1, 0.05),
            unlit: true,
            ..default()
        });
    let primitive = app
        .world_mut()
        .spawn((Mesh3d(mesh), MeshMaterial3d(material.clone())))
        .id();
    frames(&mut app, 40);
    let baseline = read(&mut app, game_target.clone());
    let rendered = read(&mut app, editor_target.clone());
    assert!(red_count(&rendered) > 1000);
    let solid_button = button(&mut app, ShadingMode::Solid);
    click(&mut app, solid_button, Vec2::ZERO);
    frames(&mut app, 20);
    let solid = read(&mut app, editor_target.clone());
    assert!(red_count(&solid) < 100, "实体视图必须实际移除红色材质表现");
    assert_eq!(
        read(&mut app, game_target.clone()),
        baseline,
        "作者实体着色不能改变玩法相机像素"
    );
    save(solid, (256, 256), "viewport-shading-solid.png");
    let menu_button = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ShadingButton)>()
            .iter(world)
            .find(|(_, button)| matches!(button.0, ShadingAction::Menu))
            .unwrap()
            .0
    };
    click(&mut app, menu_button, Vec2::ZERO);
    frames(&mut app, 3);
    let menu = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<ShadingMenu>>()
            .single(world)
            .unwrap()
    };
    let position = app
        .world()
        .get::<UiGlobalTransform>(menu)
        .unwrap()
        .translation;
    let size = app.world().get::<ComputedNode>(menu).unwrap().size();
    assert!(
        position.x - size.x * 0.5 >= 0.0
            && position.y - size.y * 0.5 >= 0.0
            && position.x + size.x * 0.5 <= 640.5
            && position.y + size.y * 0.5 <= 360.5,
        "170%窄视口下拉必须完整处于画布内：{position:?} {size:?}"
    );
    save(
        read(&mut app, ui_target.clone()),
        (640, 360),
        "viewport-shading-axis-controls-170.png",
    );
    let wire_button = button(&mut app, ShadingMode::Wireframe);
    click(&mut app, wire_button, Vec2::ZERO);
    frames(&mut app, 25);
    let wire = read(&mut app, editor_target.clone());
    let wire_black = black_count(&wire);
    assert!(
        wire_black > 10 && wire_black < red_count(&rendered) / 2,
        "线框必须实际画黑色边而非继续显示填充面：{wire_black}"
    );
    assert!(dark_gray(&wire[..4]), "线框空白处应显示中性深灰背景");
    assert_eq!(red_count(&wire), 0, "线框不能继续沿用对象红色材质");
    assert_eq!(read(&mut app, game_target.clone()), baseline);
    save(wire, (256, 256), "viewport-shading-wireframe.png");
    let preview_button = button(&mut app, ShadingMode::MaterialPreview);
    click(&mut app, preview_button, Vec2::ZERO);
    frames(&mut app, 10);
    assert_eq!(
        app.world()
            .get::<AmbientLight>(editor_camera)
            .unwrap()
            .brightness,
        800.0
    );
    assert_eq!(read(&mut app, game_target.clone()), baseline);
    let render_button = button(&mut app, ShadingMode::Rendered);
    click(&mut app, render_button, Vec2::ZERO);
    frames(&mut app, 10);
    assert_eq!(
        app.world()
            .get::<MeshMaterial3d<StandardMaterial>>(primitive)
            .unwrap()
            .0,
        material
    );
    assert_eq!(
        app.world()
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .unwrap()
            .base_color,
        Color::srgb(0.8, 0.1, 0.05)
    );
    let center_before = app.world().resource::<MapEditorViewportState>().center;
    let axis = {
        let world = app.world_mut();
        world
            .query::<(Entity, &MapEditorOrientationAxisEndpoint)>()
            .iter(world)
            .find(|(_, axis)| axis.world_axis == Vec3::X && !axis.positive)
            .unwrap()
            .0
    };
    click(&mut app, axis, Vec2::ZERO);
    frames(&mut app, 4);
    assert!(app.world().resource::<MapEditorState>().orthographic);
    assert_eq!(
        app.world().resource::<MapEditorViewportState>().center,
        center_before
    );
    assert!(
        (app.world().resource::<MapEditorViewportState>().yaw + std::f32::consts::FRAC_PI_2).abs()
            < 0.001
    );
    let projection = {
        let world = app.world_mut();
        world.query_filtered::<Entity,With<super::super::orientation_axis::OrientationProjectionToggle>>().single(world).unwrap()
    };
    click(&mut app, projection, Vec2::ZERO);
    assert!(!app.world().resource::<MapEditorState>().orthographic);
    save(
        read(&mut app, ui_target),
        (640, 360),
        "viewport-shading-axis-controls.png",
    );
}
