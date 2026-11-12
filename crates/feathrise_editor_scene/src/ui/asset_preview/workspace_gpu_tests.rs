//! 用真实PNG/GLTF和GPU验证中央预览；夹具只写临时资产，截图保存在本地验证目录。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::*, types::*, workspace};
use crate::resources::*;
use crate::ui::{sidebars::*, vegetation_preset_editor::*};
use bevy::{
    asset::AssetPlugin,
    ecs::system::RunSystemOnce,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
    },
    window::{ExitCondition, WindowPlugin},
    winit::WinitPlugin,
};

#[test]
#[ignore = "需要真实GPU；用--ignored --test-threads=1运行"]
fn gpu_asset_center_image_and_model_real_load_expand_restore() {
    let root = std::env::temp_dir().join(format!(
        "rpg-central-media-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    image::RgbaImage::from_fn(512, 256, |x, y| {
        if (x / 64 + y / 64) % 2 == 0 {
            image::Rgba([179, 129, 61, 255])
        } else {
            image::Rgba([235, 223, 192, 255])
        }
    })
    .save(root.join("预览图片.png"))
    .unwrap();
    // 使用中央画布可辨识的真实尺寸；原生16×8图即使安全显示，也不足以通过中央亮像素验收。
    let indices: Vec<_> = (0..512 * 256)
        .flat_map(|pixel| [if pixel % 512 < 256 { 0u32 } else { 1 }, 2, 3, 0])
        .flat_map(u32::to_le_bytes)
        .collect();
    let (indices, _) = feathrise_editor_assets::ktx2::encode_surface_maps(
        &indices, &[200, 40, 15, 0].repeat(512 * 256), [512, 256], 4,
    ).unwrap();
    std::fs::write(root.join("数值索引.ktx2"), indices).unwrap();
    write_model(&root);
    let target_font = root.join(crate::ui::scene::FONT_PATH);
    std::fs::create_dir_all(target_font.parent().unwrap()).unwrap();
    std::fs::write(target_font, feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES).unwrap();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(AssetPlugin {
                file_path: root.to_string_lossy().into(),
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    )
    .init_resource::<feathrise_editor_core::EditorSettings>()
    .init_resource::<SceneBrowserIndexState>()
    .init_resource::<EditorTextInputGuard>()
    .init_resource::<SceneEditorNumericScrubMouseGuard>()
    .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
    .init_resource::<SceneBrowserDragState>()
    .init_resource::<SceneBrowserState>()
    .init_resource::<SceneDeletionTask>()
    .init_resource::<crate::authoring::TerrainExternalChangeState>()
    .init_resource::<VegetationPresetEditorState>()
    .init_resource::<SceneEditorLayoutState>()
    .init_resource::<EditorAssetCatalog>()
    .init_resource::<SceneEditorSidebarState>()
    .insert_resource(SceneEditorState {
        open: true,
        selected_asset: Some("预览图片.png".into()),
        ..default()
    })
    .configure_sets(
        Update,
        (
            crate::SceneEditorSystemSet::Input,
            crate::SceneEditorSystemSet::Tasks,
            crate::SceneEditorSystemSet::Presentation,
        )
            .chain(),
    );
    // 与正式编辑器一致装载共享控件依赖，动作编辑的数值输入也随唯一预览面板常驻。
    app.add_plugins((feathrise_editor_ui::EditorUiPlugin, feathrise_editor_documents::DocumentsEditorPlugin));
    super::install(&mut app); // 使用完整生产预览系统，包括模型异步加载、专层隔离、取景和唯一挂载。
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Assets;
    app.finish();
    app.cleanup();
    let target = app.world_mut().resource_mut::<Assets<Image>>().add({
        let mut image = Image::new_target_texture(1280, 768, TextureFormat::Bgra8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image
    });
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera::default(),
            bevy::camera::RenderTarget::from(target.clone()),
        ))
        .id();
    app.world_mut()
        .run_system_once::<_, (), _>(workspace::center_host.spawn())
        .unwrap();
    let central = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<AssetPreviewCenterHost>>();
        query.single(world).unwrap()
    };
    app.world_mut()
        .entity_mut(central)
        .insert(bevy::ui::UiTargetCamera(camera));
    {
        let mut node = app.world_mut().get_mut::<Node>(central).unwrap();
        node.position_type = PositionType::Absolute;
        node.left = px(12);
        node.top = px(56);
        node.width = px(936);
        node.height = px(700);
    }
    let sidebar = app
        .world_mut()
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(960),
                top: px(56),
                width: px(308),
                height: px(700),
                padding: UiRect::all(px(4)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            bevy::ui::UiTargetCamera(camera),
        ))
        .id();
    app.world_mut()
        .run_system_once::<_, (), _>((|| super::scene::asset_preview_panel(&feathrise_editor_documents::scenes(), &feathrise_editor_animation::scenes())).spawn())
        .unwrap();
    let panel = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<AssetPreviewPanel>>();
        query.single(world).unwrap()
    };
    app.world_mut().entity_mut(panel).insert(ChildOf(sidebar));
    app.world_mut()
        .run_system_once::<_, (), _>((|| workspace::sidebar_launch(&feathrise_editor_documents::scenes())).spawn())
        .unwrap();
    let launch = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<AssetPreviewSidebarLaunch>>();
        query.single(world).unwrap()
    };
    app.world_mut().entity_mut(launch).insert(ChildOf(sidebar));
    app.world_mut().spawn((
        crate::components::SceneEditorPresetTabScroll,
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(12),
            width: px(1256),
            height: px(36),
            ..default()
        },
        bevy::ui::UiTargetCamera(camera),
    ));
    wait_ready(&mut app, false);
    let image = {
        let state = app.world().resource::<AssetPreviewState>();
        if let AssetPreviewContent::Image { handle, .. } = &state.content {
            handle.id()
        } else {
            panic!("未加载真实PNG");
        }
    };
    app.world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view
        .zoom = 1.25;
    app.world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view
        .image_pan = Vec2::new(0.05, 0.02);
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    frames(&mut app, 6);
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), central);
    let state = app.world().resource::<AssetPreviewState>();
    assert!(
        matches!(&state.content, AssetPreviewContent::Image { handle, .. } if handle.id() == image)
    );
    assert_eq!(
        app.world().resource::<AssetPreviewNavigation>().view.zoom,
        1.25
    );
    snapshot(&mut app, target.clone(), "asset-browser-central-image.png");
    press(&mut app, AssetPreviewWorkspaceAction::RestoreSidebar);
    frames(&mut app, 4);
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), sidebar);
    assert_eq!(
        app.world().resource::<AssetPreviewNavigation>().view.zoom,
        1.25
    );
    // 实际整数KTX2必须经过相同UI绑定和截图回读，不只在CPU层检查元数据。
    app.world_mut().resource_mut::<SceneEditorState>().selected_asset = Some("数值索引.ktx2".into());
    wait_ready(&mut app, false);
    {
        let state = app.world().resource::<AssetPreviewState>();
        assert!(state.status.contains("Rgba32Uint") && state.status.contains("归一化"));
        let image = app.world().resource::<Assets<Image>>().get(state.display_image.as_ref().unwrap()).unwrap();
        assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba8UnormSrgb);
        assert!(super::compatible_texture::sample_compatible(image));
        assert!(app.world().resource::<AssetServer>().get_handle::<Image>("数值索引.ktx2").is_none());
    }
    // 与普通图片相同，通过正式操作把唯一预览挂到中央；截图后恢复侧栏再继续模型流程。
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    frames(&mut app, 6);
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), central);
    snapshot(&mut app, target.clone(), "asset-browser-integer-image.png");
    press(&mut app, AssetPreviewWorkspaceAction::RestoreSidebar);
    frames(&mut app, 4);
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), sidebar);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("预览模型.gltf".into());
    wait_ready(&mut app, true);
    let (model_root, model_camera) = {
        let state = app.world().resource::<AssetPreviewState>();
        (state.scene_root().unwrap(), state.camera.unwrap())
    };
    app.world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view
        .yaw = 1.15;
    press(&mut app, AssetPreviewWorkspaceAction::ToggleExpand);
    app.world_mut()
        .resource_mut::<SceneEditorLayoutState>()
        .inspector_visible = false;
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .right = EditorRightSidebarTab::Environment;
    frames(&mut app, 12);
    assert_eq!(
        app.world().resource::<AssetPreviewState>().scene_root(),
        Some(model_root)
    );
    assert_eq!(
        app.world().resource::<AssetPreviewState>().camera,
        Some(model_camera)
    );
    assert!(app.world().get::<Camera>(model_camera).unwrap().is_active); // 右栏收起仍渲染中央模型。
    assert_eq!(
        app.world().resource::<AssetPreviewNavigation>().view.yaw,
        1.15
    );
    let viewport_size = {
        let world = app.world_mut();
        let mut nodes = world.query_filtered::<&ComputedNode, With<AssetPreviewModelViewport>>();
        nodes.single(world).unwrap().size()
    };
    assert!(
        viewport_size.x > 850.0 && viewport_size.y > 300.0,
        "中央画布没有使用正文真实尺寸：{viewport_size:?}"
    );
    let target_image = app
        .world()
        .resource::<AssetPreviewState>()
        ._render_target
        .as_ref()
        .unwrap();
    let dimensions = app
        .world()
        .resource::<Assets<Image>>()
        .get(target_image)
        .unwrap()
        .size();
    assert_eq!(dimensions.x, viewport_size.x.round() as u32);
    assert_eq!(dimensions.y, viewport_size.y.round() as u32);
    snapshot(&mut app, target.clone(), "asset-browser-central-model.png");
    // 中央子模型入口沿用原函数和原材质；之后把唯一视图借给另一文件，验证完整恢复链。
    let button = app
        .world_mut()
        .spawn((feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, AssetPreviewAction::ShowPrimitive(0)))
        .id();
    app.world_mut()
        .run_system_once(super::interaction::handle_asset_preview_actions)
        .unwrap();
    app.world_mut().entity_mut(button).despawn();
    wait_ready(&mut app, true);
    let primitive = AssetPreviewModelTarget::Primitive(AssetPreviewPrimitiveId {
        mesh: 0,
        primitive: 0,
    });
    assert_eq!(
        app.world()
            .resource::<AssetPreviewState>()
            .model
            .as_ref()
            .unwrap()
            .target,
        primitive
    );
    app.world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view
        .yaw = 2.0;
    app.world_mut()
        .resource_mut::<AssetPreviewNavigation>()
        .view
        .zoom = 1.7;
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Scene;
    frames(&mut app, 4);
    assert!(!app.world().get::<Camera>(model_camera).unwrap().is_active); // 后台中央标签停用额外View。
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("预览图片.png".into());
    wait_ready(&mut app, false);
    let tab = app.world().resource::<AssetPreviewWorkspace>().expanded.as_ref().unwrap().id;
    press(&mut app, AssetPreviewWorkspaceAction::ActivateTab(tab));
    wait_ready(&mut app, true);
    frames(&mut app, 4);
    assert_eq!(
        app.world().resource::<AssetPreviewNavigation>().view.yaw,
        2.0
    );
    assert_eq!(
        app.world().resource::<AssetPreviewNavigation>().view.zoom,
        1.7
    );
    assert_eq!(
        app.world()
            .resource::<AssetPreviewState>()
            .model
            .as_ref()
            .unwrap()
            .target,
        primitive
    );
    assert_ne!(
        app.world().resource::<AssetPreviewState>().scene_root(),
        Some(model_root)
    );
    assert_eq!(
        {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<AssetPreviewCamera>>();
            query.iter(world).count()
        },
        1
    );
    press(&mut app, AssetPreviewWorkspaceAction::RestoreSidebar);
    frames(&mut app, 5);
    assert_eq!(app.world().get::<ChildOf>(panel).unwrap().parent(), sidebar);
    assert_eq!(
        app.world().resource::<AssetPreviewState>().camera,
        Some(model_camera)
    );
    assert!(
        app.world()
            .resource::<SceneEditorLayoutState>()
            .inspector_visible
    );
    drop(app); // 先释放后台资产IO，再回收这一唯一临时根。
    assert_eq!(
        root.parent().unwrap().canonicalize().unwrap(),
        std::env::temp_dir().canonicalize().unwrap()
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn press(app: &mut App, action: AssetPreviewWorkspaceAction) {
    let button = app.world_mut().spawn((feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed, action)).id();
    app.world_mut().run_system_once(workspace::actions).unwrap();
    app.world_mut().entity_mut(button).despawn();
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

fn wait_ready(app: &mut App, model: bool) {
    let started = std::time::Instant::now();
    loop {
        frames(app, 1);
        let state = app.world().resource::<AssetPreviewState>();
        let ready = if model {
            matches!(
                state.content,
                AssetPreviewContent::Model {
                    phase: AssetPreviewPhase::Ready,
                    ..
                }
            )
        } else {
            matches!(
                state.content,
                AssetPreviewContent::Image {
                    phase: AssetPreviewPhase::Ready,
                    ..
                }
            )
        };
        let restored = !app.world().resource::<AssetPreviewWorkspace>().restore_view;
        if ready && restored {
            break;
        }
        assert!(
            started.elapsed().as_secs() < 30,
            "中央预览加载超时：{}",
            state.status
        );
    }
}

fn snapshot(app: &mut App, target: Handle<Image>, filename: &str) {
    // AssetServer Ready不等于RenderApp已完成纹理上传/管线准备；只按真实中央像素验收，不靠固定等待帧数。
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let readback = app
        .world_mut()
        .spawn(Readback::texture(target))
        .observe(move |event: On<ReadbackComplete>| {
            let _ = sender.try_send(event.data.clone());
        })
        .id(); // 单槽非阻塞回读，未准备帧继续轮询，不能无限累积截图。
    let started = std::time::Instant::now();
    let mut bytes = loop {
        frames(app, 1);
        if let Ok(bytes) = receiver.try_recv() {
            assert_eq!(bytes.len(), 1280 * 768 * 4);
            let visible = bytes
                .chunks_exact(4)
                .enumerate()
                .filter(|(index, pixel)| {
                    let x = index % 1280;
                    let y = index / 1280;
                    (20..940).contains(&x)
                        && (200..720).contains(&y)
                        && pixel[0] > 90
                        && pixel[1] > 90
                        && pixel[2] > 90
                })
                .count(); // 只核对中央画布，标题/按钮字形不能冒充图片或模型。
            if visible > 20_000 {
                break bytes;
            }
            if started.elapsed().as_secs() >= 10 {
                save_snapshot(bytes, filename);
                panic!(
                    "中央预览没有真实图像：{filename}，亮像素{visible}，{}",
                    app.world().resource::<AssetPreviewState>().status
                );
            }
        }
        assert!(
            started.elapsed().as_secs() < 12,
            "中央预览GPU快照超时：{filename}"
        );
    };
    app.world_mut().entity_mut(readback).despawn();
    save_snapshot(std::mem::take(&mut bytes), filename);
}

fn save_snapshot(mut bytes: Vec<u8>, filename: &str) {
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let output =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/validation");
    std::fs::create_dir_all(&output).unwrap();
    image::RgbaImage::from_raw(1280, 768, bytes)
        .unwrap()
        .save(output.join(filename))
        .unwrap();
}

/// 有真实外部BIN、法线、材质与Scene0的立方体，不注入Ready或用假网格代替加载链。
fn write_model(root: &std::path::Path) {
    let faces = [
        (
            [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]],
            [0., 0., 1.],
        ),
        (
            [
                [1., -1., -1.],
                [-1., -1., -1.],
                [-1., 1., -1.],
                [1., 1., -1.],
            ],
            [0., 0., -1.],
        ),
        (
            [[1., -1., 1.], [1., -1., -1.], [1., 1., -1.], [1., 1., 1.]],
            [1., 0., 0.],
        ),
        (
            [
                [-1., -1., -1.],
                [-1., -1., 1.],
                [-1., 1., 1.],
                [-1., 1., -1.],
            ],
            [-1., 0., 0.],
        ),
        (
            [[-1., 1., 1.], [1., 1., 1.], [1., 1., -1.], [-1., 1., -1.]],
            [0., 1., 0.],
        ),
        (
            [
                [-1., -1., -1.],
                [1., -1., -1.],
                [1., -1., 1.],
                [-1., -1., 1.],
            ],
            [0., -1., 0.],
        ),
    ];
    let mut positions = Vec::<f32>::new();
    let mut normals = Vec::<f32>::new();
    for (quad, normal) in faces {
        for i in [0, 1, 2, 0, 2, 3] {
            positions.extend(quad[i]);
            normals.extend(normal);
        }
    }
    let data: Vec<u8> = positions
        .into_iter()
        .chain(normals)
        .flat_map(f32::to_le_bytes)
        .collect();
    std::fs::write(root.join("cube.bin"), &data).unwrap();
    let model = serde_json::json!({"asset":{"version":"2.0"}, "buffers":[{"uri":"cube.bin","byteLength":data.len()}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":432},{"buffer":0,"byteOffset":432,"byteLength":432}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":36,"type":"VEC3","min":[-1,-1,-1],"max":[1,1,1]},
            {"bufferView":1,"componentType":5126,"count":36,"type":"VEC3"}],
        "materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.7,0.42,0.15,1.0],"metallicFactor":0.0,"roughnessFactor":0.65}}],
        "meshes":[{"name":"铜木立方体","primitives":[{"attributes":{"POSITION":0,"NORMAL":1},"material":0}]}],
        "nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}],"scene":0});
    std::fs::write(root.join("预览模型.gltf"), model.to_string()).unwrap();
}
