//! 用真实窗口、中文字体、UI布局和GPU截图验证外壳倍率及关闭后的菜单承载，不读取场景草稿。

use feathrise_editor_ui::InteractionButton;
use super::*;
use bevy::core_pipeline::upscaling::ViewUpscalingPipeline;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::render::{
    Render, RenderApp, RenderSystems,
    camera::ExtractedCamera,
    render_resource::{PipelineCache, TextureFormat},
    sync_world::MainEntity,
    view::ViewTarget,
};
use bevy::ui::{ComputedUiRenderTargetInfo, ComputedUiTargetCamera};
use bevy::winit::WinitPlugin;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

#[derive(Resource)]
struct UiCameraProof {
    frame: usize,
    phase: usize,
    since: usize,
    root: Entity,
    menu_root: Entity,
    button: Entity,
    label: Entity,
    source: Entity,
    target: Entity,
    menu_camera: Entity,
    ui_camera: Option<Entity>,
    physical_size: UVec2,
    scale_factor: f32,
    button_size: Vec2,
    label_size: Vec2,
    captured: Arc<AtomicBool>,
    output: Arc<WindowOutputProof>,
}

#[derive(Default)]
struct WindowOutputProof {
    camera: OnceLock<Entity>,
    idle_format: OnceLock<TextureFormat>,
    first_scene_ready: AtomicBool,
}

#[derive(Resource)]
struct OutputProbe(Arc<WindowOutputProof>);

fn verify_first_scene_output(
    probe: Res<OutputProbe>,
    cache: Res<PipelineCache>,
    views: Query<(
        &MainEntity,
        &ExtractedCamera,
        Option<&ViewTarget>,
        Option<Ref<ViewUpscalingPipeline>>,
    )>,
) {
    let proof = &probe.0;
    if proof.first_scene_ready.load(Ordering::Acquire) {
        return;
    }
    let Some((_, camera, target, pipeline)) = views
        .iter()
        .find(|(entity, ..)| proof.camera.get() == Some(&entity.id()))
    else {
        return;
    };
    let format = target.and_then(ViewTarget::out_texture_view_format);
    if camera.sorted_camera_index_for_target == 0 {
        if let Some(format) = format
            && target.is_some_and(ViewTarget::needs_present)
            && pipeline.is_some_and(|pipeline| pipeline.is_ready(&cache))
        {
            let _ = proof.idle_format.set(format);
        }
        return;
    }
    let idle_format = proof.idle_format.get().expect("先呈现没有场景相机的窗口");
    let pipeline = pipeline.expect("首次叠加时作者窗口输出必须有管线");
    assert_eq!(camera.sorted_camera_index_for_target, 1);
    assert_eq!(format, Some(*idle_format));
    assert!(target.is_some_and(ViewTarget::needs_present));
    assert!(pipeline.is_ready(&cache), "首次叠加帧的作者输出管线必须就绪");
    // 组件仅在最终输出规格变化时重新插入。即使 GPU 很快编完新管线，也必须检查本帧沿用原变体。
    assert!(!pipeline.is_changed(), "场景相机加入时必须沿用已就绪的作者输出管线");
    proof.first_scene_ready.store(true, Ordering::Release);
}

fn assert_geometry(world: &World, proof: &UiCameraProof) {
    assert_eq!(
        world
            .get::<ComputedUiTargetCamera>(proof.root)
            .unwrap()
            .get(),
        proof.ui_camera
    );
    let info = world.get::<ComputedUiRenderTargetInfo>(proof.root).unwrap();
    assert_eq!(info.physical_size(), proof.physical_size);
    assert_eq!(info.scale_factor(), proof.scale_factor);
    assert_eq!(
        world.get::<ComputedNode>(proof.button).unwrap().size(),
        proof.button_size
    );
    assert_eq!(
        world.get::<ComputedNode>(proof.label).unwrap().size(),
        proof.label_size
    );
}

fn drive_window_test(world: &mut World) {
    world.resource_scope(|world, mut proof: Mut<UiCameraProof>| {
        proof.frame += 1;
        assert!(proof.frame < 300, "窗口UI必须在有限帧内完成切换与截图");
        if proof.frame <= proof.since + 12 {
            return;
        }
        match proof.phase {
            0 => {
                let camera = world
                    .get::<ComputedUiTargetCamera>(proof.root)
                    .unwrap()
                    .get()
                    .unwrap();
                proof.ui_camera = Some(camera);
                let _ = proof.output.camera.set(camera);
                if proof.output.idle_format.get().is_none() {
                    return;
                }
                // 实机DPI由显示器决定；先验证真实窗口换算，再检查切换场景全过程保持同一结果。
                let window = world
                    .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
                    .single(world)
                    .unwrap();
                proof.physical_size = window.resolution.physical_size();
                proof.scale_factor = window.resolution.scale_factor() * 0.85;
                proof.button_size = world.get::<ComputedNode>(proof.button).unwrap().size();
                proof.label_size = world.get::<ComputedNode>(proof.label).unwrap().size();
                assert!(proof.button_size.x >= 127.0 && proof.button_size.y >= 35.0);
                assert!(proof.label_size.x > 40.0 && proof.label_size.y > 5.0);
                assert_geometry(world, &proof);
                // 让 RenderApp 在窗口已提交过 UI 之后首次接入玩法相机。
                world.get_mut::<Camera>(proof.source).unwrap().is_active = true;
            }
            1 => {
                if !proof.output.first_scene_ready.load(Ordering::Acquire) {
                    return;
                }
                assert_geometry(world, &proof);
                world.entity_mut(proof.source).insert(Disabled);
            }
            2 => {
                assert_geometry(world, &proof);
                world.entity_mut(proof.target).remove::<Disabled>();
            }
            3 => {
                assert_geometry(world, &proof);
                world.entity_mut(proof.target).insert(Disabled);
                world.entity_mut(proof.source).remove::<Disabled>();
            }
            4 => {
                assert_geometry(world, &proof);
                world.resource_mut::<SceneEditorState>().open = false;
                world.get_mut::<Node>(proof.root).unwrap().display = Display::None;
                world.get_mut::<Node>(proof.menu_root).unwrap().display = Display::Flex;
                world.entity_mut(proof.source).insert(Disabled);
                world.entity_mut(proof.menu_camera).remove::<Disabled>();
            }
            5 => {
                assert!(world.get::<Disabled>(proof.ui_camera.unwrap()).is_some());
                assert_eq!(
                    world
                        .get::<ComputedUiTargetCamera>(proof.menu_root)
                        .unwrap()
                        .get(),
                    Some(proof.menu_camera)
                );
                world.resource_mut::<SceneEditorState>().open = true;
                world.get_mut::<Node>(proof.root).unwrap().display = Display::Flex;
                world.get_mut::<Node>(proof.menu_root).unwrap().display = Display::None;
                world.entity_mut(proof.source).remove::<Disabled>();
                world.entity_mut(proof.menu_camera).insert(Disabled);
            }
            6 => {
                assert_geometry(world, &proof);
                let captured = proof.captured.clone();
                let path = std::env::var("RPG_EDITOR_UI_CAMERA_GPU_PATH").expect("指定UI截图路径");
                let sample = world
                    .get::<UiGlobalTransform>(proof.button)
                    .unwrap()
                    .affine()
                    .transform_point2(Vec2::new(proof.button_size.x * 0.4, 0.0))
                    .as_uvec2();
                let physical_size = proof.physical_size;
                world
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path))
                    .observe(
                        move |shot: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                            // 检查实际GPU按钮像素，确保空RenderLayers只排除世界Sprite，没有屏蔽UI。
                            assert_eq!(shot.image.size(), physical_size);
                            let pixels = shot.image.data.as_ref().unwrap();
                            let offset = ((sample.y * physical_size.x + sample.x) * 4) as usize;
                            assert!(
                                u16::from(pixels[offset + 1]) > u16::from(pixels[offset]) + 40
                                    && u16::from(pixels[offset + 1])
                                        > u16::from(pixels[offset + 2]) + 40,
                                "窗口UI按钮应已提交到GPU截图"
                            );
                            captured.store(true, Ordering::Relaxed);
                            exit.write(AppExit::Success);
                        },
                    );
            }
            _ => return,
        }
        proof.since = proof.frame;
        proof.phase += 1;
    });
}

#[test]
#[ignore = "需要真实GPU与独立窗口；RPG_EDITOR_UI_CAMERA_GPU_PATH指定截图，单项运行"]
fn gpu_editor_ui_keeps_chinese_button_geometry_through_scene_camera_handoff_and_reopen() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WinitPlugin {
                run_on_any_thread: true,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "编辑器窗口UI缩放验收".into(),
                    resolution: (640, 480).into(),
                    focused: false,
                    ..default()
                }),
                ..default()
            }),
    )
    .insert_resource(SceneEditorState {
        open: true,
        ..default()
    })
    .insert_resource(UiScale(0.85));
    install(&mut app);
    let output = Arc::new(WindowOutputProof::default());
    app.get_sub_app_mut(RenderApp)
        .unwrap()
        .insert_resource(OutputProbe(output.clone()))
        .add_systems(Render, verify_first_scene_output.in_set(RenderSystems::Cleanup));
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    let source = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Camera {
                is_active: false,
                ..default()
            },
            IsDefaultUiCamera,
        ))
        .id();
    let target = app
        .world_mut()
        .spawn((Camera3d::default(), IsDefaultUiCamera, Disabled))
        .id();
    let menu_camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                order: 10,
                ..default()
            },
            RenderLayers::none(),
            Disabled,
        ))
        .id();
    let root = app
        .world_mut()
        .spawn((
            SceneEditorRoot,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.09, 0.07)),
        ))
        .id();
    let button = app
        .world_mut()
        .spawn((
            InteractionButton,
            Node {
                position_type: PositionType::Absolute,
                left: px(20),
                top: px(20),
                width: px(150),
                height: px(42),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.04, 0.65, 0.12)),
            ChildOf(root),
        ))
        .id();
    let label = app
        .world_mut()
        .spawn((
            Text::new("编辑器缩放"),
            TextFont {
                font: font.into(),
                font_size: px(13).into(),
                ..default()
            },
            TextColor(Color::WHITE),
            ChildOf(button),
        ))
        .id();
    let menu_root = app
        .world_mut()
        .spawn((
            Node {
                display: Display::None,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgb(0.2, 0.1, 0.3)),
        ))
        .id();
    let captured = Arc::new(AtomicBool::new(false));
    app.insert_resource(UiCameraProof {
        frame: 0,
        phase: 0,
        since: 0,
        root,
        menu_root,
        button,
        label,
        source,
        target,
        menu_camera,
        ui_camera: None,
        physical_size: UVec2::ZERO,
        scale_factor: 0.0,
        button_size: Vec2::ZERO,
        label_size: Vec2::ZERO,
        captured: captured.clone(),
        output,
    })
    .add_systems(Last, drive_window_test);
    app.run();
    assert!(captured.load(Ordering::Relaxed));
}
