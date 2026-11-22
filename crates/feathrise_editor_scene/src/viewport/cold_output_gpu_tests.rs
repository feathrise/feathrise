//! 回读正式作者相机的冷输出，验证上采样管线未就绪时仍使用作者底色。

use super::*;
use bevy::{
    core_pipeline::{
        Core3d, Core3dSystems,
        blit::BlitPipeline,
        upscaling::{ViewUpscalingPipeline, upscaling},
    },
    ecs::system::RunSystemOnce,
    render::{
        RenderApp,
        gpu_readback::{Readback, ReadbackComplete},
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::PipelineCache,
        renderer::ViewQuery,
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

const SIZE: UVec2 = UVec2::new(64, 32); // BGRA 每行 256 字节，与 GPU 回读对齐。

#[derive(Resource, Default)]
struct ColdOutputFrames(usize);

fn keep_output_cold(
    view: ViewQuery<&ViewUpscalingPipeline>,
    blit: Res<BlitPipeline>,
    mut cache: ResMut<PipelineCache>,
    mut frames: ResMut<ColdOutputFrames>,
) {
    // 渲染队列已推进。本次视图只移除输出着色器，让原输出节点确定性走冷分支。
    cache.remove_shader(blit.fragment_shader.id());
    assert!(!view.into_inner().is_ready(&cache));
    frames.0 += 1;
}

fn cold_frame(app: &mut App, target: &Handle<Image>) -> Vec<u8> {
    let frame_count = |app: &App| {
        app.get_sub_app(RenderApp)
            .unwrap()
            .world()
            .resource::<ColdOutputFrames>()
            .0
    };
    let before = frame_count(app);
    let (send, receive) = mpsc::channel();
    let readback = app
        .world_mut()
        .spawn(Readback::texture(target.clone()))
        .observe(move |event: On<ReadbackComplete>| {
            let _ = send.send(event.data.clone());
        })
        .id();
    // 读取同帧真实目标纹理。保持观察实体至异步回执到达，只在这一帧提交读取。
    app.update();
    assert_eq!(frame_count(app), before + 1, "回读请求必须与待测冷输出同帧");
    app.world_mut().entity_mut(readback).remove::<Readback>();
    let started = Instant::now();
    loop {
        if let Ok(bytes) = receive.try_recv() {
            app.world_mut().despawn(readback);
            return bytes;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "冷输出 GPU 回读超时"
        );
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn assert_uniform_color(pixels: &[u8], expected: [u8; 4]) {
    assert_eq!(pixels.len(), (SIZE.x * SIZE.y * 4) as usize);
    for pixel in pixels.chunks_exact(4) {
        assert!(
            pixel
                .iter()
                .zip(expected)
                .all(|(actual, expected)| actual.abs_diff(expected) <= 1),
            "实际冷输出 BGRA {pixel:?}，期望 {expected:?}"
        );
    }
}

#[test]
#[ignore = "需要真实 GPU。使用独立离屏世界，不启动窗口或读取作者场景"]
fn gpu_cold_scene_output_keeps_the_editor_background() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<PipelinedRenderingPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    )
    .init_resource::<bevy::picking::pointer::PointerMap>()
    .init_resource::<SceneEditorViewMemory>()
    .insert_resource(SceneEditorState {
        open: true,
        ..default()
    })
    .insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
    .insert_resource(feathrise_scene_content::ActiveDynamicScene {
        scene_id: Some(feathrise_scene_content::SceneId::parse("SCENE-cold-viewport").unwrap()),
    })
    .insert_resource(ClearColor(Color::srgb(0.5, 0.7, 1.0))); // SDK 玩法天空色作为阳性对照。
    app.get_sub_app_mut(RenderApp)
        .unwrap()
        .init_resource::<ColdOutputFrames>()
        .add_systems(
            Core3d,
            keep_output_cold
                .after(Core3dSystems::PostProcess)
                .before(upscaling),
        );
    app.finish();
    app.cleanup();

    app.world_mut().spawn(SceneEditorViewport);
    app.world_mut()
        .run_system_once(setup_editor_viewport)
        .unwrap();
    let viewport = app.world().resource::<SceneEditorViewportState>();
    let (camera, target) = (viewport.camera, viewport._image.clone());
    {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let mut image = images.get_mut(&target).unwrap();
        image.resize(Extent3d {
            width: SIZE.x,
            height: SIZE.y,
            depth_or_array_layers: 1,
        });
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC; // 仅读回原 RTT，保留输出附件与颜色格式。
    }
    app.world_mut()
        .run_system_once(crate::viewport::sync_editor_camera_activity)
        .unwrap();
    let editor = cold_frame(&mut app, &target);

    // 以旧默认输出模式重复同一路径，确认能在同一纹理上实际测到蓝色。
    app.world_mut()
        .get_mut::<Camera>(camera)
        .unwrap()
        .output_mode = CameraOutputMode::default();
    let legacy = cold_frame(&mut app, &target);
    assert_uniform_color(&legacy, [255, 179, 128, 255]);
    assert_uniform_color(&editor, [43, 46, 41, 255]);
}
