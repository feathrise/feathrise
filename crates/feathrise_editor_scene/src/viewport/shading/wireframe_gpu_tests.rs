//! 真GPU验证穿地黑边、深灰背景和作者真实边；Forward/Deferred共享原显隐与恢复边界。
use super::{
    camera,
    gpu_tests::{black, black_count, dark_gray, frames, image_target, read, red_count, save},
    render::ShadingRenderPlugin,
    resources::ShadingState,
    types::ShadingMode,
};
use crate::components::SceneEditorCamera;
use bevy::{
    asset::RenderAssetUsages,
    camera::Exposure,
    core_pipeline::Skybox,
    core_pipeline::prepass::{DeferredPrepass, DepthPrepass, NormalPrepass},
    material::OpaqueRendererMethod,
    prelude::*,
    render::{
        RenderPlugin,
        error_handler::RenderErrorHandler,
        render_resource::{
            Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
            WgpuFeatures,
        },
        settings::WgpuSettings,
    },
    window::ExitCondition,
    winit::WinitPlugin,
};

const SIZE: u32 = 256; // 宽度保证GPU回读行对齐，只用两个小型网格覆盖真实渲染路径。

fn fixture(
    method: OpaqueRendererMethod,
    alpha_mode: AlphaMode,
    prepass: bool,
) -> (App, Entity, Handle<Image>, Handle<Image>) {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    features: WgpuFeatures::POLYGON_MODE_LINE | WgpuFeatures::IMMEDIATES,
                    ..default()
                }
                .into(),
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
    )
    .add_plugins(ShadingRenderPlugin)
    .init_resource::<ShadingState>()
    .insert_resource(RenderErrorHandler(|error, _, _| {
        panic!("穿地线框GPU验证失败：{error:?}")
    }))
    .add_systems(Update, camera::sync);
    app.finish();
    app.cleanup();
    let editor_target = image_target(&mut app, SIZE, SIZE);
    let game_target = image_target(&mut app, SIZE, SIZE);
    // 用六面同色的真实Skybox覆盖清屏区域。天空晚于Opaque绘制，能检出错误关闭线深度写入导致的盖线。
    let mut sky = Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        &[28, 54, 160, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    sky.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    let sky = app.world_mut().resource_mut::<Assets<Image>>().add(sky);
    let pose = Transform::from_xyz(5.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y);
    let mut editor_camera = Entity::PLACEHOLDER;
    for (order, target, editor) in [
        (0, editor_target.clone(), true),
        (1, game_target.clone(), false),
    ] {
        let mut entity = app.world_mut().spawn((
            Camera3d::default(),
            Camera {
                order,
                clear_color: bevy::camera::ClearColorConfig::Custom(Color::srgb(0.035, 0.06, 0.09)),
                ..default()
            },
            bevy::camera::RenderTarget::from(target),
            Msaa::Off,
            Exposure::default(),
            Skybox {
                image: Some(sky.clone()),
                brightness: 1.0 / Exposure::default().exposure(),
                ..default()
            },
            pose,
        ));
        // 带预通道的分支首帧就拥有附件；无预通道分支单独检查线框租约，Deferred始终走真实GBuffer。
        if prepass {
            entity.insert((DepthPrepass, NormalPrepass));
        }
        if matches!(method, OpaqueRendererMethod::Deferred) {
            entity.insert(DeferredPrepass);
        }
        if editor {
            entity.insert(SceneEditorCamera);
            editor_camera = entity.id();
        }
    }
    let cube = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(2.0, 4.0, 2.0));
    let plane = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Plane3d::default().mesh().size(10.0, 10.0));
    let red = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.035, 0.015),
            unlit: true,
            fog_enabled: false,
            opaque_render_method: method,
            alpha_mode,
            ..default()
        });
    let green = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.015, 0.8, 0.025),
            unlit: true,
            fog_enabled: false,
            opaque_render_method: method,
            alpha_mode,
            ..default()
        });
    app.world_mut().spawn((Mesh3d(cube), MeshMaterial3d(red)));
    app.world_mut()
        .spawn((Mesh3d(plane), MeshMaterial3d(green)));
    (app, editor_camera, editor_target, game_target)
}

fn projected(app: &App, camera: Entity, world: Vec3) -> Vec2 {
    app.world()
        .get::<Camera>(camera)
        .unwrap()
        .world_to_viewport(app.world().get::<GlobalTransform>(camera).unwrap(), world)
        .unwrap()
}

fn red(pixel: &[u8]) -> bool {
    pixel[0] > 100 && pixel[0] > pixel[1].saturating_mul(2) && pixel[0] > pixel[2].saturating_mul(2)
}
fn green_count(bytes: &[u8]) -> usize {
    bytes
        .chunks_exact(4)
        .filter(|p| p[1] > 80 && p[1] > p[0].saturating_mul(2) && p[1] > p[2].saturating_mul(2))
        .count()
}

/// 在完全低于绿地面的前底边上取17个点；实体检查红色、线框检查黑色，避免把填充面当作穿地证据。
fn underground_edge_samples(
    app: &App,
    camera: Entity,
    bytes: &[u8],
    matches: fn(&[u8]) -> bool,
) -> usize {
    (1..=17)
        .filter(|i| {
            let point = projected(
                app,
                camera,
                Vec3::new(-0.9 + 1.8 * (*i as f32) / 18.0, -2.0, 1.0),
            );
            let center = point.round().as_ivec2();
            (-2..=2).any(|dy| {
                (-2..=2).any(|dx| {
                    let p = center + IVec2::new(dx, dy);
                    p.min_element() >= 0
                        && p.max_element() < (SIZE as i32)
                        && matches(&bytes[((p.y as usize) * (SIZE as usize) + p.x as usize) * 4..][..4])
                })
            })
        })
        .count()
}

/// 前面上半部缩进后的区域避开边框；普通三角网格仍可能有对角线，大部分面内像素必须恢复背景。
fn face_background_fraction(app: &App, camera: Entity, bytes: &[u8]) -> f32 {
    let polygon = [
        Vec3::new(-0.7, 0.4, 1.0),
        Vec3::new(0.7, 0.4, 1.0),
        Vec3::new(0.7, 1.6, 1.0),
        Vec3::new(-0.7, 1.6, 1.0),
    ]
    .map(|point| projected(app, camera, point));
    let background = &bytes[..3];
    let (mut inside, mut clear) = (0, 0);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let sides = [0usize, 1, 2, 3]
                .map(|i| (polygon[(i + 1) % 4] - polygon[i]).perp_dot(point - polygon[i]));
            if !sides.iter().all(|value| *value >= 0.0) && !sides.iter().all(|value| *value <= 0.0)
            {
                continue;
            }
            inside += 1;
            let pixel = &bytes[((y * SIZE + x) * 4) as usize..][..3];
            if pixel
                .iter()
                .zip(background)
                .all(|(a, b)| a.abs_diff(*b) <= 3)
            {
                clear += 1;
            }
        }
    }
    assert!(inside > 250, "投影后的面内区域应有足够真实像素：{inside}");
    clear as f32 / inside as f32
}

#[test]
#[ignore = "需要真实GPU；覆盖Forward/Deferred的Opaque、AlphaMask预通道和无预通道天空盖线"]
fn wireframe_default_xray_removes_filled_prepass_occlusion_in_forward_and_deferred_views() {
    for (method, alpha_mode, prepass, label) in [
        (
            OpaqueRendererMethod::Forward,
            AlphaMode::Opaque,
            true,
            "forward",
        ),
        (
            OpaqueRendererMethod::Deferred,
            AlphaMode::Opaque,
            true,
            "deferred",
        ),
        (
            OpaqueRendererMethod::Forward,
            AlphaMode::Mask(0.5),
            true,
            "forward-mask",
        ),
        (
            OpaqueRendererMethod::Deferred,
            AlphaMode::Mask(0.5),
            true,
            "deferred-mask",
        ),
        (
            OpaqueRendererMethod::Forward,
            AlphaMode::Opaque,
            false,
            "forward-no-prepass",
        ),
    ] {
        let (mut app, editor_camera, editor_target, game_target) =
            fixture(method, alpha_mode, prepass);
        frames(&mut app, 45);
        let rendered = read(&mut app, editor_target.clone());
        let baseline = read(&mut app, game_target.clone());
        assert!(
            rendered[2] > 80 && rendered[2] > rendered[0].saturating_add(20),
            "{label}必须实际绘制蓝色Skybox，不能退化成清屏色"
        );
        assert_eq!(
            app.world().get::<DepthPrepass>(editor_camera).is_some(),
            prepass
        );
        assert_eq!(
            app.world().get::<NormalPrepass>(editor_camera).is_some(),
            prepass
        );
        assert!(
            red_count(&rendered) > 500,
            "{label}基线必须实际绘制红色实心立方体"
        );
        assert!(
            green_count(&rendered) > 1000,
            "{label}基线必须实际绘制绿色实心地面"
        );
        assert_eq!(
            underground_edge_samples(&app, editor_camera, &rendered, red),
            0,
            "{label}实体地面应遮住整条地下底边"
        );
        assert!(
            face_background_fraction(&app, editor_camera, &rendered) < 0.1,
            "{label}实体面不能误当成背景"
        );
        assert!(
            app.world().resource::<ShadingState>().xray,
            "Blender式线框默认需要穿透"
        );
        app.world_mut().resource_mut::<ShadingState>().mode = ShadingMode::Wireframe;
        frames(&mut app, 35);
        let wire = read(&mut app, editor_target.clone());
        assert!(
            app.world().get::<DepthPrepass>(editor_camera).is_some(),
            "线框后处理必须在原先没有预通道的场景中按租约安装深度"
        );
        assert!(app.world().get::<NormalPrepass>(editor_camera).is_some());
        // 先保存失败图，像素断言拒绝时也可立即审查实心深度、Deferred暗面或原生天空覆盖。
        save(
            wire.clone(),
            (SIZE, SIZE),
            &format!("viewport-wireframe-underground-{label}.png"),
        );
        assert!(
            underground_edge_samples(&app, editor_camera, &wire, black) >= 12,
            "{label}默认穿透必须显示地面以下黑色底边"
        );
        assert!(dark_gray(&wire[..4]), "{label}线框应替换蓝色天空为中性深灰背景");
        assert!(black_count(&wire) > 100, "{label}必须实际保留物体黑边");
        assert_eq!(red_count(&wire), 0, "{label}对象材质不能给线框染色");
        assert!(
            face_background_fraction(&app, editor_camera, &wire) > 0.65,
            "{label}线框面内应是背景，不能残留Deferred实心暗面"
        );
        assert!(
            green_count(&wire) < green_count(&rendered) / 4,
            "{label}地形本身也必须只剩线边"
        );
        assert_eq!(
            read(&mut app, game_target.clone()),
            baseline,
            "{label}作者线框不得改变玩法相机像素"
        );
        app.world_mut().resource_mut::<ShadingState>().mode = ShadingMode::Rendered;
        frames(&mut app, 12);
        let restored = read(&mut app, editor_target);
        save(
            restored.clone(),
            (SIZE, SIZE),
            &format!("viewport-wireframe-restored-{label}.png"),
        );
        assert_eq!(
            underground_edge_samples(&app, editor_camera, &restored, red),
            0,
            "{label}退出线框恢复地面遮挡"
        );
        assert_eq!(app.world().get::<DepthPrepass>(editor_camera).is_some(), prepass);
        assert_eq!(app.world().get::<NormalPrepass>(editor_camera).is_some(), prepass);
        assert_eq!(restored, rendered, "{label}退出线框应恢复原始实体像素");
        assert_eq!(read(&mut app, game_target), baseline);
    }
}

fn black_near(bytes: &[u8], point: Vec2) -> bool {
    let center = point.round().as_ivec2();
    (-2..=2).any(|dy| {
        (-2..=2).any(|dx| {
            let pixel = center + IVec2::new(dx, dy);
            pixel.min_element() >= 0
                && pixel.max_element() < SIZE as i32
                && black(&bytes[((pixel.y as usize) * (SIZE as usize) + pixel.x as usize) * 4..][..4])
        })
    })
}

fn segment_distance(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let edge = end - start;
    let fraction = ((point - start).dot(edge) / edge.length_squared().max(0.0001)).clamp(0.0, 1.0);
    point.distance(start + edge * fraction)
}

#[test]
#[ignore = "需要真实GPU；验证内置立方体、已编辑网格和幽灵只显示作者边，且隐藏树不泄露线框"]
fn authoring_wire_metadata_keeps_twelve_real_edges_and_hides_quad_diagonals() {
    use bevy::ecs::system::RunSystemOnce;
    use feathrise_scene_content::{SceneId, ScenePlacementAssetTarget, ScenePlacementRecord};
    use feathrise_scene::{ATTRIBUTE_AUTHORING_WIRE, ScenePlacementInstance};

    for (method, method_name) in [
        (OpaqueRendererMethod::Forward, "forward"),
        (OpaqueRendererMethod::Deferred, "deferred"),
    ] {
        for source in ["cuboid", "editable", "ghost"] {
            let (mut app, editor_camera, editor_target, game_target) =
                fixture(method, AlphaMode::Opaque, true);
            {
                let world = app.world_mut();
                let old_meshes = world.query_filtered::<Entity, With<Mesh3d>>()
                    .iter(world).collect::<Vec<_>>();
                for entity in old_meshes {
                    world.despawn(entity);
                }
                let mut cameras = world.query_filtered::<(&mut Projection, &mut Transform), With<Camera3d>>();
                for (mut projection, mut transform) in cameras.iter_mut(world) {
                    *projection = Projection::Orthographic(OrthographicProjection {
                        scaling_mode: bevy::camera::ScalingMode::FixedVertical { viewport_height: 4.5 },
                        ..OrthographicProjection::default_3d()
                    });
                    *transform = Transform::from_xyz(5.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y);
                }
            }
            let cuboid = ScenePlacementAssetTarget::Cuboid {
                size_mm: [2_000; 3], color_srgb: [230, 30, 15], roughness_milli: 700, collision: false,
            };
            let geometry = cuboid.editable_geometry().unwrap();
            assert_eq!(geometry.edges().len(), 12, "夹具必须是六个作者四边面组成的十二边闭壳");
            let container = app.world_mut().spawn((Transform::IDENTITY, Visibility::Hidden)).id();
            let surface = if source == "ghost" {
                // 与编辑手势幽灵使用同一显示生成器和标记；没有第二份边实体或独立的显隐通路。
                let mesh = app.world_mut().resource_mut::<Assets<Mesh>>()
                    .add(feathrise_scene::editable_mesh_mesh(geometry));
                let material = app.world_mut().resource_mut::<Assets<StandardMaterial>>()
                    .add(StandardMaterial { base_color: Color::srgb(0.95, 0.035, 0.015), ..default() });
                app.world_mut().spawn((
                    Mesh3d(mesh), MeshMaterial3d(material),
                    Transform::IDENTITY, Visibility::Inherited, ChildOf(container),
                )).id()
            } else {
                let target = if source == "cuboid" { cuboid } else {
                    ScenePlacementAssetTarget::EditableMesh {
                        geometry, color_srgb: [230, 30, 15], roughness_milli: 700, collision: false,
                    }
                };
                let record: ScenePlacementRecord = serde_json::from_value(serde_json::json!({
                    "id":"wire-metadata-cube", "asset_path":"", "asset_target":target,
                    "translation":[0,0,0], "yaw_degrees":0, "scale":[1,1,1]
                })).unwrap();
                let root = app.world_mut().spawn((
                    ScenePlacementInstance::new(&SceneId::parse("MAP-wire-metadata").unwrap(), &record),
                    Transform::IDENTITY, Visibility::Inherited, ChildOf(container),
                )).id();
                app.world_mut().run_system_once(move |mut commands: Commands| {
                    assert!(feathrise_scene::spawn_builtin_placement_visual(&mut commands, root, target));
                }).unwrap();
                app.world().get::<Children>(root).unwrap().iter()
                    .find(|child| app.world().get::<Mesh3d>(*child).is_some()).unwrap()
            };
            let mesh = app.world().get::<Mesh3d>(surface).unwrap().0.clone();
            assert!(app.world().resource::<Assets<Mesh>>().get(&mesh).unwrap()
                .attribute(ATTRIBUTE_AUTHORING_WIRE).is_some(), "实际可视网格必须带作者边布局身份");
            assert!(app.world().resource::<Assets<Mesh>>().get(&mesh).unwrap()
                .attribute(Mesh::ATTRIBUTE_UV_1).is_some(), "真实显示生成器必须提供每三角作者边掩码");
            let material = app.world().get::<MeshMaterial3d<StandardMaterial>>(surface).unwrap().0.clone();
            {
                let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
                let mut material = materials.get_mut(&material).unwrap();
                material.unlit = true;
                material.fog_enabled = false;
                material.opaque_render_method = method;
            }
            app.world_mut().resource_mut::<ShadingState>().mode = ShadingMode::Wireframe;
            frames(&mut app, 40);
            let hidden = read(&mut app, editor_target.clone());
            assert!(dark_gray(&hidden[..4]));
            assert_eq!(black_count(&hidden), 0, "{method_name}/{source}隐藏候选祖先不能泄露黑边");
            app.world_mut().entity_mut(container).insert(Visibility::Inherited);
            frames(&mut app, 18);
            let wire = read(&mut app, editor_target.clone());
            let game_baseline = read(&mut app, game_target.clone());
            assert!(red_count(&game_baseline) > 500, "玩法相机仍应绘制原实心材质");
            save(wire.clone(), (SIZE, SIZE), &format!("viewport-authoring-wire-{method_name}-{source}.png"));

            let vertices = geometry.vertices().iter().map(|point| {
                projected(&app, editor_camera, Vec3::from_array(point.map(|coordinate| coordinate as f32 * 0.0005)))
            }).collect::<Vec<_>>();
            let edges = geometry.edges().into_iter().map(|[a, b]| [vertices[a], vertices[b]]).collect::<Vec<_>>();
            for (index, [start, end]) in edges.iter().copied().enumerate() {
                let hits = (1..=9).filter(|step| black_near(&wire, start.lerp(end, *step as f32 / 10.0))).count();
                assert!(hits >= 6, "{method_name}/{source}真实边{index}没有完整显示：{hits}/9");
            }
            // 所有对角线样本都离十二条真实边至少6.5像素；避免把背面的真实边误报成残留剖分线。
            let diagonal_samples = geometry.faces().iter().flat_map(|face| {
                let face = face.indices();
                let [start, end] = [vertices[usize::from(face[0])], vertices[usize::from(face[2])]];
                (1..20).map(move |step| start.lerp(end, step as f32 / 20.0))
            }).filter(|point| edges.iter().all(|[start, end]| segment_distance(*point, *start, *end) > 6.5))
                .collect::<Vec<_>>();
            assert!(diagonal_samples.len() >= 12, "斜视投影必须留下足够不与真实边重合的对角线样本");
            let diagonal_hits = diagonal_samples.iter().filter(|point| black_near(&wire, **point)).count();
            assert!(diagonal_hits <= 1, "{method_name}/{source}面内三角对角线不应可见：{diagonal_hits}/{}", diagonal_samples.len());

            // 仅移除布局身份，保留同一三角、UV1和材质；普通网格回退必须恢复剖分线，不能按UV1误认作者源。
            let fallback_mesh = {
                let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
                let mut plain = meshes.get(&mesh).unwrap().clone();
                plain.remove_attribute(ATTRIBUTE_AUTHORING_WIRE).unwrap();
                assert!(plain.attribute(Mesh::ATTRIBUTE_UV_1).is_some());
                meshes.add(plain)
            };
            app.world_mut().entity_mut(surface).insert(Mesh3d(fallback_mesh));
            frames(&mut app, 15);
            let fallback = read(&mut app, editor_target.clone());
            let fallback_hits = diagonal_samples.iter().filter(|point| black_near(&fallback, **point)).count();
            assert!(fallback_hits * 3 >= diagonal_samples.len() * 2,
                "{method_name}/{source}普通几何回退必须继续显示三角边：{fallback_hits}/{}", diagonal_samples.len());
            assert_eq!(read(&mut app, game_target), game_baseline, "标记和作者片元不得改变玩法相机");
            app.world_mut().entity_mut(surface).insert((Mesh3d(mesh), Visibility::Hidden));
            frames(&mut app, 10);
            assert_eq!(black_count(&read(&mut app, editor_target.clone())), 0,
                "{method_name}/{source}直接隐藏或取消幽灵不能留下黑边");
            app.world_mut().entity_mut(surface).insert(Visibility::Inherited);
            app.world_mut().entity_mut(container).insert(Visibility::Hidden);
            frames(&mut app, 8);
            assert_eq!(black_count(&read(&mut app, editor_target)), 0,
                "{method_name}/{source}隐藏或退役祖先后不能重新画出作者边");
        }
    }
}
